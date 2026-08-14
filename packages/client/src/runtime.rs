//! Runtime bridge between Dioxus (any thread) and matrix-sdk (tokio).
//!
//! matrix-sdk is async and needs a tokio runtime; the UI is not. `ClientRuntime`
//! owns a tokio runtime on a dedicated background thread and accepts commands
//! over an [`UnboundedSender`]. Each command carries a oneshot so the caller
//! can await the result from wherever it lives.
//!
//! Thread ownership: the `matrix_sdk::Client` is constructed inside this task
//! in response to `Login`/`Restore` and is owned here forever. Nothing
//! non-`Send` ever crosses the bridge — the channel payloads are plain data by
//! construction (see docs/00-roadmap.md §3).
//!
//! Checkpoint 02/03 commands: `Login`, `Restore`, `Logout` (auth + session
//! persistence), `Ping` for the round-trip sanity check, and `BindState`
//! which hands the UI's live signals in so room-list sync can publish into
//! them.

use std::sync::{Arc, RwLock};

use anyhow::Result;
use dioxus_signals::WritableExt;
use matrix_sdk::Client;
use tokio::{
    runtime::Runtime,
    sync::{mpsc::UnboundedSender, oneshot},
};

use crate::{
    api::{ClientError, ClientState},
    live::{LiveHandles, TypingManager},
    model::{Attachment, Convo, Me, Reaction, ThreadReply},
    session, sync,
    timeline::TimelineRegistry,
};

/// Commands the UI can send into the Matrix runtime.
///
/// Every variant answers through its oneshot; never `unwrap`/panic between
/// receiving a command and answering it, or the Dioxus side hangs waiting on
/// a dropped sender.
pub enum Command {
    /// Connectivity sanity check. Responds with the echoed payload.
    Ping {
        payload: String,
        reply: oneshot::Sender<String>,
    },
    /// Password login against `homeserver` (e.g. `matrix.org`; well-known
    /// discovery resolves the client API URL). On success the session is
    /// persisted to disk and `reply` carries the identity snapshot.
    Login {
        homeserver: String,
        user_id: String,
        password: String,
        reply: oneshot::Sender<Result<Me, ClientError>>,
    },
    /// Attempt to restore the persisted session. `Ok(None)` means "no stored
    /// session" — the normal first-run state.
    Restore {
        reply: oneshot::Sender<Result<Option<Me>, ClientError>>,
    },
    /// End the current session (remote + local cleanup).
    Logout {
        reply: oneshot::Sender<Result<(), ClientError>>,
    },
    /// The cached identity snapshot, if a session is active.
    WhoAmI { reply: oneshot::Sender<Option<Me>> },
    /// Hand the UI-created live state to the runtime so sync can publish into
    /// it. `snapshot` belongs to `MatrixClient` and backs `conversations()`;
    /// the sync task keeps it identical to `state.convos`. Arrives once after
    /// the Dioxus scope exists — before or after login, either is fine.
    BindState {
        state: ClientState,
        snapshot: Arc<RwLock<Vec<Convo>>>,
    },
    /// Open (refcounted) the live timeline for `room_id`; publishes mapped
    /// messages into `state.messages`. Fire-and-forget: failures are logged,
    /// the UI just sees an empty history (checkpoint 04).
    OpenTimeline { room_id: String },
    /// Release one reference to `room_id`'s timeline (disposing at zero).
    CloseTimeline { room_id: String },
    /// Back-paginate `room_id`'s open timeline by one page; replies with the
    /// number of messages actually added.
    LoadOlder {
        room_id: String,
        reply: oneshot::Sender<Result<usize, ClientError>>,
    },
    /// Send a markdown text message into `room_id`'s open timeline,
    /// optionally as an in-reply-to on `reply_to` (checkpoint 05).
    /// When `attachment` is present (checkpoint 07, a composer-picked file
    /// staging `local_path`), the text becomes the caption and the file is
    /// uploaded + sent as `m.image`/`m.file`/… in a spawned task — the
    /// reply returns immediately so a slow upload never stalls the
    /// sequential command loop (checkpoint-06 lesson).
    SendMessage {
        room_id: String,
        text: String,
        attachment: Option<Attachment>,
        reply_to: Option<String>,
        reply: oneshot::Sender<Result<(), ClientError>>,
    },
    /// Resolve media to a `data:` URI string (checkpoint 07). `encrypted`
    /// carries serialized `EncryptedFile` JSON for E2EE media. Answered
    /// from a spawned task once the (cache-aware) fetch completes — the
    /// command loop keeps processing peers meanwhile.
    MediaUri {
        mxc: String,
        encrypted: Option<String>,
        thumb: Option<(u32, u32)>,
        reply: oneshot::Sender<Result<String, ClientError>>,
    },
    /// Save an attachment's full-resolution bytes to `dest_path`
    /// (checkpoint 07 Download action). Answered from a spawned task.
    SaveAttachment {
        attachment: Attachment,
        dest_path: String,
        reply: oneshot::Sender<Result<(), ClientError>>,
    },
    /// Send a markdown text message as an `m.thread` reply rooted at
    /// `root_id` in `room_id` (checkpoint 05).
    SendThreadReply {
        room_id: String,
        root_id: String,
        text: String,
        reply: oneshot::Sender<Result<(), ClientError>>,
    },
    /// Toggle the user's `emoji` reaction on `event_id` in `room_id`:
    /// sends an annotation or redacts their matching reaction (checkpoint 05).
    ToggleReaction {
        room_id: String,
        event_id: String,
        emoji: String,
        reply: oneshot::Sender<Result<Vec<Reaction>, ClientError>>,
    },
    /// Retry a wedged/queued local echo (checkpoint 05).
    RetrySend {
        room_id: String,
        message_id: String,
        reply: oneshot::Sender<Result<(), ClientError>>,
    },
    /// Abort and remove a pending local echo (checkpoint 05).
    DiscardSend {
        room_id: String,
        message_id: String,
        reply: oneshot::Sender<Result<(), ClientError>>,
    },
    /// One-shot read of the thread rooted at `root_id` in `room_id`
    /// (thread panel, checkpoint 05).
    FetchThread {
        room_id: String,
        root_id: String,
        reply: oneshot::Sender<Result<Vec<ThreadReply>, ClientError>>,
    },
    /// Open (refcounted) the live thread rooted at `root_id` in `room_id` —
    /// the thread panel keeps it open for live replies. Fire-and-forget.
    OpenThread { room_id: String, root_id: String },
    /// Release one reference to `root_id`'s thread.
    CloseThread { root_id: String },
    /// Tell the backend the user is (or is not) typing in `room_id`
    /// (checkpoint 06). Fire-and-forget: debounced + spawned off the loop.
    SetTyping { room_id: String, typing: bool },
    /// Mark `room_id` as read up to its latest event (checkpoint 06).
    /// Fire-and-forget: throttled + spawned off the loop.
    MarkRead { room_id: String },
}

/// Post-login send-queue setup (checkpoint 05): make sure the global send
/// queue is enabled (it persists per room, so an earlier recoverable send
/// failure may have disabled it) and forward queue errors to the log at
/// `debug`, per docs/05 step 1.
async fn wire_send_queue(client: &Client) {
    client.send_queue().set_enabled(true).await;
    tracing::debug!(
        enabled = client.send_queue().is_enabled(),
        "send queue status"
    );
    let mut errors = client.send_queue().subscribe_errors();
    tokio::spawn(async move {
        loop {
            match errors.recv().await {
                Ok(err) => tracing::debug!(?err, "send queue error"),
                // Lagged behind the broadcast: missing a transient error is
                // fine, the message state still reflects it.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// (Re)start the room-list sync once both halves exist: an authenticated
/// client and the UI-bound state. A previously running sync is stopped first,
/// so a login/rebind never stacks two of them.
///
/// Also (re)starts the live-state tasks (presence + notifications, checkpoint
/// 06): the handlers are re-registered per client, so a previous set is
/// dropped (unregistered) first.
async fn restart_sync(
    sdk_client: &Option<Client>,
    bound: &Option<(ClientState, Arc<RwLock<Vec<Convo>>>)>,
    current: &mut Option<sync::SyncHandles>,
    live: &mut Option<LiveHandles>,
    typing: &TypingManager,
) {
    if let Some(handles) = current.take() {
        handles.stop().await;
    }
    // Drop previous live handlers (unregisters them) before re-registering.
    *live = None;
    let (Some(client), Some((state, snapshot))) = (sdk_client, bound) else {
        return;
    };
    match sync::start_room_list(client.clone(), *state, snapshot.clone()).await {
        Ok(handles) => *current = Some(handles),
        Err(e) => {
            tracing::warn!("room list sync failed to start: {}", e.0);
            // Don't strand the user at a permanently empty list: leave the
            // "connecting…" pill on so a non-delivering sync reads as one.
            let mut state = *state;
            state.connecting.set(true);
        }
    }
    // Typing resets per session (no stale timers across a re-login).
    typing.abort_all();
    *live = Some(crate::live::start_live(client, *state));
}

/// Owns the tokio runtime that matrix-sdk code runs on.
pub struct ClientRuntime {
    handle: std::thread::JoinHandle<()>,
}

impl ClientRuntime {
    /// Spawn the dedicated thread hosting the tokio runtime. Returns the
    /// runtime owner and the command sender.
    pub fn spawn() -> (Self, UnboundedSender<Command>) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let handle = std::thread::Builder::new()
            .name("vesper-matrix-runtime".into())
            .spawn(move || {
                let runtime = Runtime::new().expect("failed to build tokio runtime");
                runtime.block_on(async move {
                    // The SDK client lives and dies inside this task.
                    let mut sdk_client: Option<Client> = None;
                    let mut me: Option<Me> = None;
                    // UI-bound live state (arrives via `BindState`) and the
                    // running room-list sync built from it + the client.
                    let mut bound: Option<(ClientState, Arc<RwLock<Vec<Convo>>>)> = None;
                    let mut sync: Option<sync::SyncHandles> = None;
                    // Checkpoint 06: live-state tasks (presence + notifications)
                    // and the outgoing typing debounce manager. `typing_mgr`
                    // (not `typing`) to avoid shadowing the command's bool.
                    let mut live: Option<LiveHandles> = None;
                    let typing_mgr = TypingManager::default();
                    // Checkpoint 04: live timelines keyed by room id, disposed
                    // when the last reader closes (or on logout below).
                    let mut timelines = TimelineRegistry::default();

                    while let Some(cmd) = rx.recv().await {
                        match cmd {
                            Command::Ping { payload, reply } => {
                                tracing::debug!(payload = %payload, "ping");
                                let _ = reply.send(format!("pong:{payload}"));
                            }
                            Command::Login {
                                homeserver,
                                user_id,
                                password,
                                reply,
                            } => {
                                tracing::info!(%user_id, "login attempt");
                                let result =
                                    session::connect_login(homeserver, user_id, password).await;
                                match result {
                                    Ok((client, snapshot)) => {
                                        // Reply first, then start sync: the
                                        // caller's await shouldn't wait on it.
                                        sdk_client = Some(client);
                                        me = Some(snapshot.clone());
                                        wire_send_queue(sdk_client.as_ref().expect("just set"))
                                            .await;
                                        let _ = reply.send(Ok(snapshot));
                                        restart_sync(
                                            &sdk_client,
                                            &bound,
                                            &mut sync,
                                            &mut live,
                                            &typing_mgr,
                                        )
                                        .await;
                                    }
                                    Err(e) => {
                                        let _ = reply.send(Err(e));
                                    }
                                }
                            }
                            Command::Restore { reply } => match session::connect_restore().await {
                                Ok(Some((client, snapshot))) => {
                                    sdk_client = Some(client);
                                    me = Some(snapshot.clone());
                                    wire_send_queue(sdk_client.as_ref().expect("just set")).await;
                                    let _ = reply.send(Ok(Some(snapshot)));
                                    restart_sync(
                                        &sdk_client,
                                        &bound,
                                        &mut sync,
                                        &mut live,
                                        &typing_mgr,
                                    )
                                    .await;
                                }
                                Ok(None) => {
                                    let _ = reply.send(Ok(None));
                                }
                                Err(e) => {
                                    let _ = reply.send(Err(e));
                                }
                            },
                            Command::Logout { reply } => {
                                // Stop sync before the session teardown drops
                                // the client; then blank the UI's live state
                                // so a later login never sees a stale list.
                                if let Some(handles) = sync.take() {
                                    handles.stop().await;
                                }
                                if let Some((state, snapshot)) = &bound {
                                    *snapshot.write().unwrap_or_else(|e| e.into_inner()) =
                                        Vec::new();
                                    let mut state = *state;
                                    state.convos.set(Vec::new());
                                    state.connecting.set(false);
                                }
                                // Timelines die with the session; blank their
                                // published messages alongside the convo list
                                // so a later login never paints stale history.
                                timelines.abort_all();
                                // Live-state tasks die with the session too:
                                // drop the handlers (unregister) and cancel any
                                // pending typing timers, and blank the
                                // typing/presence maps so a later login never
                                // shows stale indicators.
                                live = None;
                                typing_mgr.abort_all();
                                if let Some((state, _)) = &bound {
                                    let mut state = *state;
                                    state.messages.set(Default::default());
                                    state.threads.set(Default::default());
                                    state.typing.set(Default::default());
                                    state.presence.set(Default::default());
                                }
                                // Take the client (by value) so the session
                                // module can drop it — closing sqlite handles —
                                // before deleting the store dir.
                                let result = session::logout(sdk_client.take()).await;
                                me = None;
                                let _ = reply.send(result);
                            }
                            Command::WhoAmI { reply } => {
                                let _ = reply.send(me.clone());
                            }
                            Command::BindState { state, snapshot } => {
                                bound = Some((state, snapshot));
                                restart_sync(
                                    &sdk_client,
                                    &bound,
                                    &mut sync,
                                    &mut live,
                                    &typing_mgr,
                                )
                                .await;
                            }
                            Command::OpenTimeline { room_id } => {
                                if let (Some(client), Some((state, _))) = (&sdk_client, &bound) {
                                    timelines.open(client, &room_id, *state).await;
                                }
                            }
                            Command::CloseTimeline { room_id } => {
                                timelines.close(&room_id);
                            }
                            Command::LoadOlder { room_id, reply } => {
                                let _ = reply.send(timelines.load_older(&room_id).await);
                            }
                            Command::SendMessage {
                                room_id,
                                text,
                                attachment,
                                reply_to,
                                reply,
                            } => match attachment {
                                None => {
                                    let _ = reply.send(
                                        timelines.send_message(&room_id, text, reply_to).await,
                                    );
                                }
                                Some(att) => {
                                    // Validate before answering: media sends
                                    // have no local echo, so an opaque
                                    // spawned-task failure would silently
                                    // swallow the message (review P2).
                                    if let Err(e) = crate::media::preflight_attachment(
                                        &att,
                                        reply_to.as_deref(),
                                    ) {
                                        let _ = reply.send(Err(e));
                                        continue;
                                    }
                                    match timelines.room_for_send(&room_id) {
                                        Some(room) => {
                                            tokio::spawn(crate::media::send_attachment(
                                                room, att, text, reply_to,
                                            ));
                                            let _ = reply.send(Ok(()));
                                        }
                                        None => {
                                            let _ = reply.send(Err(ClientError(
                                                "That conversation is not open.".into(),
                                            )));
                                        }
                                    }
                                }
                            },
                            Command::MediaUri {
                                mxc,
                                encrypted,
                                thumb,
                                reply,
                            } => {
                                // Answered from a spawned task: cache misses
                                // are network fetches and the command loop is
                                // sequential (checkpoint-06 lesson).
                                let Some(client) = sdk_client.clone() else {
                                    let _ = reply.send(Err(ClientError("Not signed in.".into())));
                                    continue;
                                };
                                tokio::spawn(async move {
                                    let result = crate::media::resolve(
                                        &client,
                                        &mxc,
                                        encrypted.as_deref(),
                                        thumb,
                                    )
                                    .await;
                                    let _ = reply.send(result);
                                });
                            }
                            Command::SaveAttachment {
                                attachment,
                                dest_path,
                                reply,
                            } => {
                                let Some(client) = sdk_client.clone() else {
                                    let _ = reply.send(Err(ClientError("Not signed in.".into())));
                                    continue;
                                };
                                tokio::spawn(async move {
                                    let result =
                                        crate::media::save_to(&client, &attachment, &dest_path)
                                            .await;
                                    let _ = reply.send(result);
                                });
                            }
                            Command::SendThreadReply {
                                room_id,
                                root_id,
                                text,
                                reply,
                            } => {
                                let _ = reply.send(
                                    timelines.send_thread_reply(&room_id, &root_id, text).await,
                                );
                            }
                            Command::ToggleReaction {
                                room_id,
                                event_id,
                                emoji,
                                reply,
                            } => {
                                let _ = reply.send(
                                    timelines.toggle_reaction(&room_id, &event_id, &emoji).await,
                                );
                            }
                            Command::RetrySend {
                                room_id,
                                message_id,
                                reply,
                            } => {
                                let _ =
                                    reply.send(timelines.retry_send(&room_id, &message_id).await);
                            }
                            Command::DiscardSend {
                                room_id,
                                message_id,
                                reply,
                            } => {
                                let _ =
                                    reply.send(timelines.discard_send(&room_id, &message_id).await);
                            }
                            Command::FetchThread {
                                room_id,
                                root_id,
                                reply,
                            } => {
                                let _ =
                                    reply.send(timelines.thread_replies(&room_id, &root_id).await);
                            }
                            Command::OpenThread { room_id, root_id } => {
                                if let Some((state, _)) = &bound {
                                    timelines.open_thread(&room_id, &root_id, *state).await;
                                }
                            }
                            Command::CloseThread { root_id } => {
                                timelines.close_thread(&root_id);
                            }
                            Command::SetTyping { room_id, typing } => {
                                if let Some(client) = &sdk_client {
                                    typing_mgr.set(client, &room_id, typing);
                                }
                            }
                            Command::MarkRead { room_id } => {
                                if let (Some(client), Some((state, _))) = (&sdk_client, &bound) {
                                    timelines.mark_read(client, &room_id, *state);
                                }
                            }
                        }
                    }
                });
            })
            .expect("failed to spawn matrix runtime thread");
        (ClientRuntime { handle }, tx)
    }

    /// Wait for the runtime thread to finish (e.g. after the sender is dropped).
    pub fn join(self) -> Result<()> {
        self.handle
            .join()
            .map_err(|_| anyhow::anyhow!("matrix runtime thread panicked"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn ping_round_trip() {
        let (runtime, tx) = ClientRuntime::spawn();
        let (reply_tx, reply_rx) = oneshot::channel();
        tx.send(Command::Ping {
            payload: "hello".into(),
            reply: reply_tx,
        })
        .expect("send ping");
        let pong = reply_rx.await.expect("receive pong");
        assert_eq!(pong, "pong:hello");
        drop(tx);
        runtime.join().expect("runtime thread exits cleanly");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn whoami_is_none_before_login() {
        let (runtime, tx) = ClientRuntime::spawn();
        let (reply_tx, reply_rx) = oneshot::channel();
        tx.send(Command::WhoAmI { reply: reply_tx })
            .expect("send whoami");
        assert_eq!(reply_rx.await.expect("whoami reply"), None);
        drop(tx);
        runtime.join().expect("runtime thread exits cleanly");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn login_with_garbage_homeserver_fails_safely() {
        // No network dependency: an invalid server name fails client build with
        // ClientBuildError::InvalidServerName before any request is attempted.
        // Keep everything out of the real profile directory.
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("VESPER_DATA_DIR", tmp.path());

        let (runtime, tx) = ClientRuntime::spawn();
        let (reply_tx, reply_rx) = oneshot::channel();
        tx.send(Command::Login {
            homeserver: "not a server name at all!!!".into(),
            user_id: "@x:y".into(),
            password: "pw".into(),
            reply: reply_tx,
        })
        .expect("send login");
        let err = reply_rx
            .await
            .expect("login reply")
            .expect_err("login must fail");
        assert!(
            err.0.contains("homeserver") || err.0.contains("client"),
            "unexpected error text: {}",
            err.0
        );
        drop(tx);
        runtime.join().expect("runtime thread exits cleanly");
    }
}
