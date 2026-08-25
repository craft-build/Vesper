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
    directory,
    live::{LiveHandles, TypingManager},
    model::{
        Attachment, Convo, Me, PublicRoomPage, PublicSpacePage, Reaction, Space, ThreadReply,
        VerificationSession, VerificationState,
    },
    session, sync,
    timeline::TimelineRegistry,
};

/// The UI-bound handles cached from `BindState`: the live signal struct plus
/// the synchronous snapshots backing `conversations()` / `spaces()`. Both
/// halves are kept identical to their signals by the sync task.
struct Bound {
    state: ClientState,
    snapshot: Arc<RwLock<Vec<Convo>>>,
    spaces: Arc<RwLock<Vec<Space>>>,
}

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
    /// it. The snapshots belong to `MatrixClient` and back
    /// `conversations()` / `spaces()`; the sync task keeps them identical to
    /// `state.convos` / `state.spaces`. Arrives once after the Dioxus scope
    /// exists — before or after login, either is fine.
    BindState {
        state: ClientState,
        snapshot: Arc<RwLock<Vec<Convo>>>,
        spaces: Arc<RwLock<Vec<Space>>>,
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
    /// Start (or replace) an interactive SAS verification session
    /// (checkpoint 08). Progress publishes into `state.verification`.
    /// Fire-and-forget: the driver runs in its own task.
    StartVerification {
        target: crate::model::VerificationTarget,
    },
    /// Act on the active verification session (checkpoint 08).
    /// Fire-and-forget.
    VerificationAction {
        action: crate::model::VerificationAction,
    },
    /// List the signed-in user's devices with trust flags (checkpoint 08).
    /// Answered from a spawned task (network key query).
    FetchDevices {
        reply: oneshot::Sender<Result<Vec<crate::model::Device>, ClientError>>,
    },
    /// One page of the public room directory matching `query`, continuing
    /// after `batch_token` (checkpoint 09). Answered from a spawned task.
    PublicRooms {
        query: String,
        batch_token: Option<String>,
        reply: oneshot::Sender<Result<PublicRoomPage, ClientError>>,
    },
    /// One page of the public space directory (checkpoint 09). Answered
    /// from a spawned task.
    PublicSpaces {
        query: String,
        batch_token: Option<String>,
        reply: oneshot::Sender<Result<PublicSpacePage, ClientError>>,
    },
    /// Join a public room by id or alias (checkpoint 09). Answered from a
    /// spawned task; on success the room arrives via the room-list stream.
    JoinRoom {
        id_or_alias: String,
        reply: oneshot::Sender<Result<(), ClientError>>,
    },
    /// Leave a joined room (checkpoint 09). Answered from a spawned task.
    LeaveRoom {
        room_id: String,
        reply: oneshot::Sender<Result<(), ClientError>>,
    },
    /// Set the account display name; replies with the fresh identity
    /// snapshot (checkpoint 10). Answered from a spawned task.
    SetDisplayName {
        name: String,
        reply: oneshot::Sender<Result<Me, ClientError>>,
    },
    /// Read `path`, upload its bytes as the account avatar, and reply with
    /// the fresh identity snapshot (checkpoint 10). Answered from a spawned
    /// task so a slow upload never stalls the command loop.
    SetAvatar {
        path: String,
        reply: oneshot::Sender<Result<Me, ClientError>>,
    },
    /// Rename a session by device id (checkpoint 10). Answered from a
    /// spawned task.
    RenameDevice {
        device_id: String,
        name: String,
        reply: oneshot::Sender<Result<(), ClientError>>,
    },
    /// Delete another session, completing a UIAA password stage with
    /// `password` when the server demands one (checkpoint 10). Answered from
    /// a spawned task.
    DeleteDevice {
        device_id: String,
        password: String,
        reply: oneshot::Sender<Result<(), ClientError>>,
    },
    /// Read the notification push-rule toggles (checkpoint 10). Answered
    /// from a spawned task.
    NotificationRules {
        reply: oneshot::Sender<Result<Vec<crate::model::NotifToggle>, ClientError>>,
    },
    /// Flip one toggle (writing every Matrix rule behind it); replies with
    /// the full refreshed list (checkpoint 10). Answered from a spawned task.
    SetNotificationRule {
        toggle_id: String,
        enabled: bool,
        reply: oneshot::Sender<Result<Vec<crate::model::NotifToggle>, ClientError>>,
    },
    /// Read device-local preferences (checkpoint 10). Local file, cheap:
    /// answered inline on the loop.
    GetPrefs {
        reply: oneshot::Sender<crate::model::Prefs>,
    },
    /// Persist device-local preferences (checkpoint 10). Local file, cheap:
    /// answered inline.
    SetPrefs {
        prefs: crate::model::Prefs,
        reply: oneshot::Sender<Result<(), ClientError>>,
    },
    /// Bytes used by the on-disk media cache (checkpoint 11 §C). Local
    /// files, cheap: answered from a spawned task (the dir walk).
    MediaCacheBytes { reply: oneshot::Sender<u64> },
    /// Delete every cached media entry; replies with the bytes freed
    /// (checkpoint 11 §C settings action).
    ClearMediaCache {
        reply: oneshot::Sender<Result<u64, ClientError>>,
    },
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

/// Post-login E2EE setup (checkpoint 08): cross-signing bootstrap + key
/// backup/recovery pull, run in its own task so neither the command loop
/// nor the login reply ever waits on it. Every step logs under one span so
/// a startup hang is diagnosable from the logs.
///
/// UIAA note: `bootstrap_cross_signing_if_needed(None)` can require
/// interactive auth on accounts that already have cross-signing but lost
/// their local private keys. That's a warn + targeted error here; a
/// password-based UIAA flow is a checkpoint-08 stretch goal, checkpoint-11
/// otherwise.
fn spawn_crypto_setup(client: &Client) {
    let client = client.clone();
    tokio::spawn(async move {
        let span = tracing::info_span!("e2ee_setup");
        let _enter = span.enter();
        // Returns `()`; failures inside the setup task are logged by the SDK.
        client
            .encryption()
            .wait_for_e2ee_initialization_tasks()
            .await;
        match client
            .encryption()
            .bootstrap_cross_signing_if_needed(None)
            .await
        {
            Ok(()) => tracing::debug!("cross-signing bootstrap ok"),
            Err(e) => {
                tracing::warn!("cross-signing bootstrap failed (interactive auth?): {e:?}");
            }
        }
        let status = client.encryption().cross_signing_status().await;
        tracing::debug!(?status, "cross-signing status");
        tracing::debug!(
            backup = ?client.encryption().backups().state(),
            recovery = ?client.encryption().recovery().state(),
            "key backup / recovery state"
        );
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
    bound: &Option<Bound>,
    current: &mut Option<sync::SyncHandles>,
    live: &mut Option<LiveHandles>,
    typing: &TypingManager,
) {
    if let Some(handles) = current.take() {
        handles.stop().await;
    }
    // Drop previous live handlers (unregisters them) before re-registering.
    *live = None;
    let (Some(client), Some(bound)) = (sdk_client, bound) else {
        return;
    };
    match sync::start_room_list(client.clone(), bound.state, &bound.snapshot, &bound.spaces).await {
        Ok(handles) => *current = Some(handles),
        Err(e) => {
            tracing::warn!("room list sync failed to start: {}", e);
            // Don't strand the user at a permanently empty list: leave the
            // "connecting…" pill on so a non-delivering sync reads as one.
            let mut state = bound.state;
            state.connecting.set(true);
        }
    }
    // Typing resets per session (no stale timers across a re-login).
    typing.abort_all();
    *live = Some(crate::live::start_live(client, bound.state));
}

/// Map the account's devices into the UI model (checkpoint 10 upgrade).
///
/// Two sources merged: the `/devices` HTTP endpoint is authoritative for
/// id/name/last-seen (it carries `last_seen_ts`/`last_seen_ip`, which the
/// crypto device list lacks), and the crypto `get_user_devices` query
/// supplies the verified flag. A crypto-query failure degrades to unverified
/// flags rather than failing the whole list — verification badges disappearing
/// offline is better than the settings screen going blank.
async fn fetch_devices(client: &Client) -> Result<Vec<crate::model::Device>, matrix_sdk::Error> {
    use std::collections::BTreeMap;

    use matrix_sdk::ruma::OwnedDeviceId;

    let own = client
        .user_id()
        .ok_or_else(|| crate::verification::unknown_error("not signed in".into()))?;
    let listed = client.devices().await?.devices;
    let current_id = client.device_id().map(|d| d.to_string());

    let verified: BTreeMap<OwnedDeviceId, bool> = client
        .encryption()
        .get_user_devices(own)
        .await
        .map(|all| {
            all.devices()
                .map(|d| (d.device_id().to_owned(), d.is_verified()))
                .collect()
        })
        .unwrap_or_else(|e| {
            tracing::warn!("device trust query failed, showing all as unverified: {e:?}");
            BTreeMap::new()
        });

    Ok(listed
        .into_iter()
        .map(|d| crate::model::Device {
            current: Some(d.device_id.to_string()) == current_id,
            id: d.device_id.to_string(),
            name: d.display_name.unwrap_or_else(|| "Unnamed session".into()),
            last_seen: format_last_seen(
                d.last_seen_ts.map(|ts| u64::from(ts.get())),
                d.last_seen_ip.as_deref(),
            ),
            verified: verified.get(&d.device_id).copied().unwrap_or(false),
        })
        .collect())
}

/// Human-friendly "last seen" for a device row: relative age from unix-ms,
/// optionally with the IP ("3 d ago · 1.2.3.4"). Mirrors the timeline's
/// no-chrono date formatting (civil-days math).
fn format_last_seen(ts: Option<u64>, ip: Option<&str>) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let Some(ms) = ts else {
        return "Unknown".into();
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(ms);
    let age = now.saturating_sub(ms) / 1000; // seconds
    let rel = if age < 60 {
        "just now".to_string()
    } else if age < 3600 {
        format!("{} m ago", age / 60)
    } else if age < 86_400 {
        format!("{} h ago", age / 3600)
    } else {
        format!("{} d ago", age / 86_400)
    };
    match ip {
        Some(ip) if !ip.is_empty() => format!("{rel} · {ip}"),
        _ => rel,
    }
}

// ----------------------------------------------------------------------
// Account console helpers (checkpoint 10). All run inside spawned tasks,
// never on the sequential command loop.
// ----------------------------------------------------------------------

/// Set the display name server-side and return the fresh identity snapshot.
/// The runtime's cached `me` is refreshed by the caller (it owns the slot).
async fn set_display_name(client: &Client, name: &str) -> Result<Me, ClientError> {
    client
        .account()
        .set_display_name(Some(name))
        .await
        .map_err(|_| ClientError::server("Could not save your display name."))?;
    Ok(session::me_snapshot(client).await)
}

/// Read the picked avatar file, upload it, and return the fresh identity
/// snapshot. `upload_avatar` uploads *and* sets the account avatar url in
/// one call. Mime sniffing via `infer` (already a dep for checkpoint 07).
async fn set_avatar(client: &Client, path: &str) -> Result<Me, ClientError> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| ClientError::storage("Could not read the chosen image."))?;
    let kind = infer::get(&bytes)
        .ok_or_else(|| ClientError::invalid("That file does not look like an image."))?;
    let mime: mime::Mime = kind
        .mime_type()
        .parse()
        .map_err(|_| ClientError::invalid("Unsupported image type."))?;
    if mime.type_() != mime::IMAGE {
        return Err(ClientError::invalid("Avatars must be images."));
    }
    client
        .account()
        .upload_avatar(&mime, bytes)
        .await
        .map_err(|_| ClientError::server("Could not upload the image."))?;
    Ok(session::me_snapshot(client).await)
}

/// Read the rule table's toggles from the server push ruleset (checkpoint
/// 10). A toggle reports enabled only when *all* its rules are enabled;
/// missing rules (fresh login, ruleset not synced yet) read as the table
/// default.
async fn notification_toggles(
    client: &Client,
) -> Result<Vec<crate::model::NotifToggle>, ClientError> {
    let settings = client.notification_settings().await;
    let mut out = Vec::with_capacity(crate::notifications::RULE_TABLE.len());
    for def in crate::notifications::RULE_TABLE {
        // ALL-rule semantics: any disabled rule disables the toggle; no
        // later rule may resurrect it (the first `false`/error wins).
        let mut enabled = def.default;
        for rule in def.rules {
            let kind = map_rule_kind(rule.kind);
            match settings.is_push_rule_enabled(kind, rule.rule_id).await {
                Ok(true) => enabled = true,
                Ok(false) => {
                    enabled = false;
                    break;
                }
                Err(_) => {
                    enabled = def.default;
                    break;
                }
            }
        }
        out.push(crate::model::NotifToggle {
            id: def.id.to_string(),
            label: def.label.to_string(),
            enabled,
        });
    }
    Ok(out)
}

/// Flip one toggle, writing every Matrix rule behind it, then re-read.
async fn set_notification_toggle(
    client: &Client,
    toggle_id: &str,
    enabled: bool,
) -> Result<Vec<crate::model::NotifToggle>, ClientError> {
    let Some(def) = crate::notifications::toggle_def(toggle_id) else {
        return Err(ClientError::invalid("Unknown notification setting."));
    };
    let settings = client.notification_settings().await;
    for rule in def.rules {
        settings
            .set_push_rule_enabled(map_rule_kind(rule.kind), rule.rule_id, enabled)
            .await
            .map_err(|_| ClientError::server("Could not save that notification setting."))?;
    }
    notification_toggles(client).await
}

fn map_rule_kind(kind: crate::notifications::RuleKind) -> matrix_sdk::ruma::push::RuleKind {
    match kind {
        crate::notifications::RuleKind::Override => matrix_sdk::ruma::push::RuleKind::Override,
        crate::notifications::RuleKind::Underride => matrix_sdk::ruma::push::RuleKind::Underride,
    }
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
                    // Checkpoint 10: same identity slot, shared so spawned
                    // profile-save tasks (display name / avatar) can refresh
                    // it without bouncing through the sequential loop.
                    let me_cache = Arc::new(std::sync::RwLock::new(Option::<Me>::None));
                    // UI-bound live state (arrives via `BindState`) and the
                    // running room-list sync built from it + the client.
                    let mut bound: Option<Bound> = None;
                    let mut sync: Option<sync::SyncHandles> = None;
                    // Checkpoint 06: live-state tasks (presence + notifications)
                    // and the outgoing typing debounce manager. `typing_mgr`
                    // (not `typing`) to avoid shadowing the command's bool.
                    let mut live: Option<LiveHandles> = None;
                    let typing_mgr = TypingManager::default();
                    // Checkpoint 04: live timelines keyed by room id, disposed
                    // when the last reader closes (or on logout below).
                    let mut timelines = TimelineRegistry::default();
                    // Checkpoint 08: the active interactive verification
                    // session, if any. One at a time; replaced by a new
                    // `StartVerification` and aborted on logout. Shared
                    // slot: StartVerification resolves the target in a
                    // spawned task (network key query) and stores the
                    // session from there.
                    let verification = Arc::new(std::sync::Mutex::new(
                        Option::<crate::verification::Session>::None,
                    ));

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
                                        *me_cache.write().unwrap_or_else(|e| e.into_inner()) =
                                            Some(snapshot.clone());
                                        wire_send_queue(sdk_client.as_ref().expect("just set"))
                                            .await;
                                        let _ = reply.send(Ok(snapshot));
                                        spawn_crypto_setup(sdk_client.as_ref().expect("just set"));
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
                                    *me_cache.write().unwrap_or_else(|e| e.into_inner()) =
                                        Some(snapshot.clone());
                                    wire_send_queue(sdk_client.as_ref().expect("just set")).await;
                                    let _ = reply.send(Ok(Some(snapshot)));
                                    spawn_crypto_setup(sdk_client.as_ref().expect("just set"));
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
                                if let Some(bound) = &bound {
                                    *bound.snapshot.write().unwrap_or_else(|e| e.into_inner()) =
                                        Vec::new();
                                    *bound.spaces.write().unwrap_or_else(|e| e.into_inner()) =
                                        Vec::new();
                                    let mut state = bound.state;
                                    state.convos.set(Vec::new());
                                    state.spaces.set(Vec::new());
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
                                // Checkpoint 08: the verification session dies
                                // with the account; cancel it server-side and
                                // blank the slot so a later login's dialog
                                // never resumes stale state.
                                if let Some(session) = verification
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .take()
                                {
                                    session.abort();
                                }
                                if let Some(bound) = &bound {
                                    let mut state = bound.state;
                                    state.messages.set(Default::default());
                                    state.threads.set(Default::default());
                                    state.typing.set(Default::default());
                                    state.presence.set(Default::default());
                                    state.verification.set(None);
                                } // Take the client (by value) so the session
                                  // module can drop it — closing sqlite handles —
                                  // before deleting the store dir.
                                let result = session::logout(sdk_client.take()).await;
                                me = None;
                                *me_cache.write().unwrap_or_else(|e| e.into_inner()) = None;
                                let _ = reply.send(result);
                            }
                            Command::WhoAmI { reply } => {
                                // Read the shared slot: profile saves refresh
                                // it from spawned tasks (checkpoint 10).
                                let fresh =
                                    me_cache.read().unwrap_or_else(|e| e.into_inner()).clone();
                                let _ = reply.send(fresh.or_else(|| me.clone()));
                            }
                            Command::BindState {
                                state,
                                snapshot,
                                spaces,
                            } => {
                                bound = Some(Bound {
                                    state,
                                    snapshot,
                                    spaces,
                                });
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
                                if let (Some(client), Some(bound)) = (&sdk_client, &bound) {
                                    timelines.open(client, &room_id, bound.state).await;
                                }
                            }
                            Command::CloseTimeline { room_id } => {
                                timelines.close(&room_id);
                            }
                            Command::LoadOlder { room_id, reply } => {
                                // Answered from a spawned task: pagination + its
                                // delivery wait are network I/O, the command
                                // loop is sequential (checkpoint-06 lesson).
                                match timelines.pagination_handles(&room_id) {
                                    Some((timeline, inner)) => {
                                        tokio::spawn(async move {
                                            let result =
                                                crate::timeline::load_older_page(&timeline, &inner)
                                                    .await;
                                            let _ = reply.send(result);
                                        });
                                    }
                                    None => {
                                        let _ = reply.send(Ok(0));
                                    }
                                }
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
                                            let _ = reply.send(Err(ClientError::invalid(
                                                "That conversation is not open.",
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
                                    let _ = reply.send(Err(ClientError::auth("Not signed in.")));
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
                                    let _ = reply.send(Err(ClientError::auth("Not signed in.")));
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
                                // Answered from a spawned task: the thread
                                // build + /relations backfill are network I/O
                                // (mirrors the directory commands).
                                match timelines.thread_request(&room_id, &root_id) {
                                    Err(e) => {
                                        let _ = reply.send(Err(e));
                                    }
                                    Ok((room, root)) => {
                                        tokio::spawn(async move {
                                            let result =
                                                crate::timeline::thread_replies(&room, root).await;
                                            let _ = reply.send(result);
                                        });
                                    }
                                }
                            }
                            Command::OpenThread { room_id, root_id } => {
                                if let Some(bound) = &bound {
                                    timelines.open_thread(&room_id, &root_id, bound.state).await;
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
                                if let (Some(client), Some(bound)) = (&sdk_client, &bound) {
                                    timelines.mark_read(client, &room_id, bound.state);
                                }
                            }
                            Command::StartVerification { target } => {
                                let (Some(client), Some(bound)) = (&sdk_client, &bound) else {
                                    // Tell the UI instead of a silent no-op
                                    // (review P3): the dialog would otherwise
                                    // sit blank forever.
                                    if let Some(bound) = &bound {
                                        let mut verification = bound.state.verification;
                                        verification.set(Some(VerificationSession {
                                            subject: String::new(),
                                            target: target.clone(),
                                            state: VerificationState::Failed(
                                                "Not signed in.".into(),
                                            ),
                                            emojis: Vec::new(),
                                        }));
                                    }
                                    continue;
                                };
                                // Replace any running session: one dialog.
                                if let Some(old) = verification
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .take()
                                {
                                    old.abort();
                                }
                                let subject = match &target {
                                    crate::model::VerificationTarget::Device(id) => {
                                        // Best-effort label; the dialog falls
                                        // back to "the other device".
                                        id.clone()
                                    }
                                    crate::model::VerificationTarget::User(_) => String::new(),
                                };
                                // The one target-resolution key query is a
                                // network round-trip: it runs in a spawned
                                // task so the command loop keeps moving
                                // (checkpoint-06 lesson). `Session::start`
                                // publishes `Requested` before its await, so
                                // the dialog has feedback immediately; the
                                // driver task then owns everything else.
                                let client = client.clone();
                                let state = bound.state;
                                let slot = verification.clone();
                                tokio::spawn(async move {
                                    let session = crate::verification::Session::start(
                                        &client, target, subject, state,
                                    )
                                    .await;
                                    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
                                    if let Some(old) = guard.replace(session) {
                                        // A newer StartVerification raced us
                                        // to the slot; cancel the loser.
                                        old.abort();
                                    }
                                });
                            }
                            Command::VerificationAction { action } => {
                                let guard = verification.lock().unwrap_or_else(|e| e.into_inner());
                                if let Some(session) = guard.as_ref() {
                                    session.act(action);
                                }
                            }
                            Command::FetchDevices { reply } => {
                                let Some(client) = sdk_client.clone() else {
                                    let _ = reply.send(Err(ClientError::auth("Not signed in.")));
                                    continue;
                                };
                                tokio::spawn(async move {
                                    let result = fetch_devices(&client).await.map_err(|e| {
                                        tracing::warn!("devices fetch failed: {e:?}");
                                        ClientError::server("Could not load sessions.")
                                    });
                                    let _ = reply.send(result);
                                });
                            }
                            Command::PublicRooms {
                                query,
                                batch_token,
                                reply,
                            } => {
                                // Spawned: directory queries are network
                                // round-trips and the command loop is
                                // sequential (checkpoint-06 lesson).
                                let Some(client) = sdk_client.clone() else {
                                    let _ = reply.send(Err(ClientError::auth("Not signed in.")));
                                    continue;
                                };
                                tokio::spawn(async move {
                                    let result =
                                        directory::public_rooms(&client, &query, batch_token).await;
                                    let _ = reply.send(result);
                                });
                            }
                            Command::PublicSpaces {
                                query,
                                batch_token,
                                reply,
                            } => {
                                let Some(client) = sdk_client.clone() else {
                                    let _ = reply.send(Err(ClientError::auth("Not signed in.")));
                                    continue;
                                };
                                tokio::spawn(async move {
                                    let result =
                                        directory::public_spaces(&client, &query, batch_token)
                                            .await;
                                    let _ = reply.send(result);
                                });
                            }
                            Command::JoinRoom { id_or_alias, reply } => {
                                let Some(client) = sdk_client.clone() else {
                                    let _ = reply.send(Err(ClientError::auth("Not signed in.")));
                                    continue;
                                };
                                tokio::spawn(async move {
                                    let result = directory::join_room(&client, &id_or_alias).await;
                                    let _ = reply.send(result);
                                });
                            }
                            Command::LeaveRoom { room_id, reply } => {
                                let Some(client) = sdk_client.clone() else {
                                    let _ = reply.send(Err(ClientError::auth("Not signed in.")));
                                    continue;
                                };
                                tokio::spawn(async move {
                                    let result = directory::leave_room(&client, &room_id).await;
                                    let _ = reply.send(result);
                                });
                            }
                            Command::SetDisplayName { name, reply } => {
                                // Empty would *remove* the name server-side;
                                // the UI also guards, the backend enforces.
                                if name.trim().is_empty() {
                                    let _ = reply.send(Err(ClientError::invalid(
                                        "Display name cannot be empty.",
                                    )));
                                    continue;
                                }
                                let Some(client) = sdk_client.clone() else {
                                    let _ = reply.send(Err(ClientError::auth("Not signed in.")));
                                    continue;
                                };
                                let me_slot = me_cache.clone();
                                tokio::spawn(async move {
                                    let result = set_display_name(&client, &name).await;
                                    if let Ok(fresh) = &result {
                                        *me_slot.write().unwrap_or_else(|e| e.into_inner()) =
                                            Some(fresh.clone());
                                    }
                                    let _ = reply.send(result);
                                });
                            }
                            Command::SetAvatar { path, reply } => {
                                let Some(client) = sdk_client.clone() else {
                                    let _ = reply.send(Err(ClientError::auth("Not signed in.")));
                                    continue;
                                };
                                let me_slot = me_cache.clone();
                                tokio::spawn(async move {
                                    let result = set_avatar(&client, &path).await;
                                    if let Ok(fresh) = &result {
                                        *me_slot.write().unwrap_or_else(|e| e.into_inner()) =
                                            Some(fresh.clone());
                                    }
                                    let _ = reply.send(result);
                                });
                            }
                            Command::RenameDevice {
                                device_id,
                                name,
                                reply,
                            } => {
                                let Some(client) = sdk_client.clone() else {
                                    let _ = reply.send(Err(ClientError::auth("Not signed in.")));
                                    continue;
                                };
                                tokio::spawn(async move {
                                    let device_id =
                                        matrix_sdk::ruma::OwnedDeviceId::from(device_id);
                                    let result = client
                                        .rename_device(&device_id, &name)
                                        .await
                                        .map(|_| ())
                                        .map_err(|_| {
                                            ClientError::server("Could not rename that session.")
                                        });
                                    let _ = reply.send(result);
                                });
                            }
                            Command::DeleteDevice {
                                device_id,
                                password,
                                reply,
                            } => {
                                let Some(client) = sdk_client.clone() else {
                                    let _ = reply.send(Err(ClientError::auth("Not signed in.")));
                                    continue;
                                };
                                // Deleting the current device is refused —
                                // that's logout's job (UI blocks it too; the
                                // backend guard keeps other callers honest).
                                if Some(&device_id)
                                    == client.device_id().map(|d| d.to_string()).as_ref()
                                {
                                    let _ = reply.send(Err(ClientError::invalid(
                                        "Sign out instead of deleting this session.",
                                    )));
                                    continue;
                                }
                                tokio::spawn(async move {
                                    let result = crate::uiaa::delete_device_with_password(
                                        &client, device_id, password,
                                    )
                                    .await;
                                    let _ = reply.send(result);
                                });
                            }
                            Command::NotificationRules { reply } => {
                                let Some(client) = sdk_client.clone() else {
                                    let _ = reply.send(Err(ClientError::auth("Not signed in.")));
                                    continue;
                                };
                                tokio::spawn(async move {
                                    let result = notification_toggles(&client).await;
                                    let _ = reply.send(result);
                                });
                            }
                            Command::SetNotificationRule {
                                toggle_id,
                                enabled,
                                reply,
                            } => {
                                let Some(client) = sdk_client.clone() else {
                                    let _ = reply.send(Err(ClientError::auth("Not signed in.")));
                                    continue;
                                };
                                tokio::spawn(async move {
                                    let result =
                                        set_notification_toggle(&client, &toggle_id, enabled).await;
                                    let _ = reply.send(result);
                                });
                            }
                            Command::GetPrefs { reply } => {
                                // Local file, cheap: answered inline.
                                let _ = reply.send(session::load_prefs());
                            }
                            Command::SetPrefs { prefs, reply } => {
                                let _ = reply.send(session::save_prefs(&prefs));
                            }
                            Command::MediaCacheBytes { reply } => {
                                // Local dir walk: off the loop, like every
                                // other filesystem-heavy op.
                                tokio::task::spawn_blocking(move || {
                                    let _ = reply.send(crate::media_cache::size_bytes());
                                });
                            }
                            Command::ClearMediaCache { reply } => {
                                tokio::task::spawn_blocking(move || {
                                    let _ = reply.send(crate::media_cache::clear());
                                });
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

    // MutexGuard held across the login await on purpose: env vars are global
    // and the session tests touch the same variable.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test(flavor = "multi_thread")]
    async fn login_with_garbage_homeserver_fails_safely() {
        // No network dependency: an invalid server name fails client build with
        // ClientBuildError::InvalidServerName before any request is attempted.
        // Keep everything out of the real profile directory. The env lock is
        // shared with the session tests: env vars are global.
        let tmp = tempfile::tempdir().expect("tempdir");
        let _guard = crate::session::tests::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("VESPER_DATA_DIR", tmp.path());
        // Never touch the real keychain from tests (checkpoint 11).
        std::env::set_var("VESPER_SECRET_STORE", "file");

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
            err.message.contains("homeserver") || err.message.contains("client"),
            "unexpected error text: {}",
            err.message
        );
        drop(tx);
        runtime.join().expect("runtime thread exits cleanly");
        drop(_guard);
        std::env::remove_var("VESPER_SECRET_STORE");
        std::env::remove_var("VESPER_DATA_DIR");
    }

    // Checkpoint 10: prefs commands round-trip through the runtime loop and
    // land in the data dir (the runtime answers them inline; no login needed
    // because prefs are device-local, not session-bound).
    #[tokio::test(flavor = "multi_thread")]
    async fn prefs_round_trip_through_runtime() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let _guard = crate::session::tests::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("VESPER_DATA_DIR", tmp.path());
        std::env::set_var("VESPER_SECRET_STORE", "file");

        let (runtime, tx) = ClientRuntime::spawn();
        let fresh = crate::model::Prefs {
            theme: "light".into(),
            typing_indicators: false,
            ..Default::default()
        };
        let (set_tx, set_rx) = oneshot::channel();
        tx.send(Command::SetPrefs {
            prefs: fresh.clone(),
            reply: set_tx,
        })
        .expect("send set prefs");
        set_rx.await.expect("set reply").expect("set ok");

        let (get_tx, get_rx) = oneshot::channel();
        tx.send(Command::GetPrefs { reply: get_tx })
            .expect("send get prefs");
        let loaded = get_rx.await.expect("get reply");
        assert_eq!(loaded, fresh);

        drop(tx);
        runtime.join().expect("runtime thread exits cleanly");
        drop(_guard);
        std::env::remove_var("VESPER_SECRET_STORE");
        std::env::remove_var("VESPER_DATA_DIR");
    }
}
