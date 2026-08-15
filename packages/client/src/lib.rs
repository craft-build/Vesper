//! Matrix bridge for Vesper.
//!
//! This crate owns everything that talks to a Matrix homeserver via
//! `matrix-sdk`. The trait seam ([`api`]) and domain types ([`model`]) are
//! feature-free so wasm builds of the `ui` crate can consume them without
//! linking matrix-sdk; everything SDK-touching sits behind the `matrix`
//! feature (on by default for native builds).

#![recursion_limit = "512"]
pub mod api;
pub mod model;

#[cfg(feature = "matrix")]
pub mod live;
#[cfg(feature = "matrix")]
pub mod media;
#[cfg(feature = "matrix")]
pub mod runtime;
#[cfg(feature = "matrix")]
mod session;
#[cfg(feature = "matrix")]
pub mod sync;
#[cfg(feature = "matrix")]
pub mod timeline;
#[cfg(feature = "matrix")]
pub mod verification;

#[cfg(feature = "matrix")]
mod matrix_impl {
    use std::sync::{Arc, RwLock};

    use tokio::sync::{mpsc::UnboundedSender, oneshot};

    use crate::{
        api::{ClientError, ClientState, VesperClient},
        model::*,
        runtime::{ClientRuntime, Command},
    };

    /// The `matrix-sdk`-backed implementation of the backend seam.
    ///
    /// Holds only the command channel into the runtime thread (plus the
    /// runtime itself for lifecycle) — never an SDK handle. All SDK state
    /// lives on the runtime thread; see docs/00-roadmap.md §3.
    pub struct MatrixClient {
        tx: UnboundedSender<Command>,
        // Latest synced conversations; the runtime's sync task writes it and
        // `conversations()` clones it. Kept identical to the bound
        // `ClientState::convos` signal.
        snapshot: Arc<RwLock<Vec<Convo>>>,
        // Keep the runtime alive for as long as we might send commands.
        _runtime: ClientRuntime,
    }

    impl Default for MatrixClient {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MatrixClient {
        #[must_use]
        pub fn new() -> Self {
            let (runtime, tx) = ClientRuntime::spawn();
            Self {
                tx,
                snapshot: Arc::new(RwLock::new(Vec::new())),
                _runtime: runtime,
            }
        }

        async fn ask<T>(
            &self,
            build: impl FnOnce(oneshot::Sender<Result<T, ClientError>>) -> Command,
        ) -> Result<T, ClientError> {
            let (reply_tx, reply_rx) = oneshot::channel();
            self.tx
                .send(build(reply_tx))
                .map_err(|_| ClientError("The Matrix runtime is not running.".into()))?;
            reply_rx
                .await
                .map_err(|_| ClientError("The Matrix runtime stopped responding.".into()))?
        }

        fn unimplemented(error_hint: &'static str) -> ClientError {
            tracing::warn!("{error_hint}: not implemented yet (later checkpoint)");
            ClientError("Not implemented yet.".into())
        }

        fn empty_vec<T>(what: &'static str) -> Vec<T> {
            tracing::warn!("{what}: returning empty until later checkpoint");
            Vec::new()
        }
    }

    #[async_trait::async_trait(?Send)]
    impl VesperClient for MatrixClient {
        async fn login(
            &self,
            homeserver: String,
            user_id: String,
            password: String,
        ) -> Result<Me, ClientError> {
            self.ask(move |reply| Command::Login {
                homeserver,
                user_id,
                password,
                reply,
            })
            .await
        }

        async fn restore(&self) -> Result<Option<Me>, ClientError> {
            self.ask(move |reply| Command::Restore { reply }).await
        }

        async fn logout(&self) -> Result<(), ClientError> {
            self.ask(move |reply| Command::Logout { reply }).await
        }

        async fn me(&self) -> Option<Me> {
            let (reply_tx, reply_rx) = oneshot::channel();
            self.tx.send(Command::WhoAmI { reply: reply_tx }).ok()?;
            reply_rx.await.ok().flatten()
        }

        fn bind_state(&self, state: ClientState) {
            // The runtime's sync task will publish directly into `state`'s
            // sync-storage signals from the tokio thread — legal because
            // they're Send+Sync handles (see `api::ClientState` docs).
            let _ = self.tx.send(Command::BindState {
                state,
                snapshot: self.snapshot.clone(),
            });
        }

        async fn spaces(&self) -> Vec<Space> {
            // Spaces are checkpoint 09.
            Vec::new()
        }
        async fn conversations(&self) -> Vec<Convo> {
            self.snapshot
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }
        // Snapshot read-back for first paint comes from the live map; a room
        // that was never opened yields an empty history until `open_timeline`
        // (called by the Conversation component) publishes into it.
        async fn messages(&self, _convo_id: &str) -> Vec<Message> {
            Vec::new()
        }
        fn open_timeline(&self, convo_id: &str) {
            let _ = self.tx.send(Command::OpenTimeline {
                room_id: convo_id.to_string(),
            });
        }
        fn close_timeline(&self, convo_id: &str) {
            let _ = self.tx.send(Command::CloseTimeline {
                room_id: convo_id.to_string(),
            });
        }
        async fn load_older(&self, convo_id: &str) -> Result<usize, ClientError> {
            self.ask(move |reply| Command::LoadOlder {
                room_id: convo_id.to_string(),
                reply,
            })
            .await
        }
        // One-shot thread read: builds a thread-focused timeline on the
        // runtime side (cached events + /relations backfill).
        async fn thread(&self, convo_id: &str, message_id: &str) -> Vec<ThreadReply> {
            self.ask(move |reply| Command::FetchThread {
                room_id: convo_id.to_string(),
                root_id: message_id.to_string(),
                reply,
            })
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("thread: {}", e.0);
                Vec::new()
            })
        }

        // Checkpoint 05: real sends go through the open timeline's send
        // queue; the painted row is the local echo in the diff stream. The
        // returned `Message` is a placeholder — callers ignore it and rely
        // on the live `ClientState::messages` entry instead.
        async fn send_message(
            &self,
            convo_id: &str,
            text: String,
            attachment: Option<Attachment>,
            reply_to: Option<String>,
        ) -> Message {
            let send_state = self
                .ask(move |reply| Command::SendMessage {
                    room_id: convo_id.to_string(),
                    text,
                    attachment,
                    reply_to,
                    reply,
                })
                .await
                .map(|_| SendState::Sending)
                .unwrap_or(SendState::Failed);
            let mut stub = Message::new(String::new(), "", "", "");
            stub.mine = true;
            stub.send_state = send_state;
            stub
        }
        async fn send_thread_reply(
            &self,
            convo_id: &str,
            message_id: &str,
            text: String,
        ) -> ThreadReply {
            let body = text.clone();
            if let Err(e) = self
                .ask(move |reply| Command::SendThreadReply {
                    room_id: convo_id.to_string(),
                    root_id: message_id.to_string(),
                    text: body,
                    reply,
                })
                .await
            {
                tracing::warn!("send_thread_reply: {}", e.0);
            }
            ThreadReply {
                from: String::new(),
                time: String::new(),
                mine: true,
                text,
            }
        }
        async fn react(&self, convo_id: &str, message_id: &str, emoji: &str) -> Vec<Reaction> {
            self.ask(move |reply| Command::ToggleReaction {
                room_id: convo_id.to_string(),
                event_id: message_id.to_string(),
                emoji: emoji.to_string(),
                reply,
            })
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("react: {}", e.0);
                Vec::new()
            })
        }
        async fn retry_send(&self, convo_id: &str, message_id: &str) -> Result<(), ClientError> {
            self.ask(move |reply| Command::RetrySend {
                room_id: convo_id.to_string(),
                message_id: message_id.to_string(),
                reply,
            })
            .await
        }
        async fn discard_send(&self, convo_id: &str, message_id: &str) -> Result<(), ClientError> {
            self.ask(move |reply| Command::DiscardSend {
                room_id: convo_id.to_string(),
                message_id: message_id.to_string(),
                reply,
            })
            .await
        }

        fn open_thread(&self, convo_id: &str, message_id: &str) {
            let _ = self.tx.send(Command::OpenThread {
                room_id: convo_id.to_string(),
                root_id: message_id.to_string(),
            });
        }
        fn close_thread(&self, message_id: &str) {
            let _ = self.tx.send(Command::CloseThread {
                root_id: message_id.to_string(),
            });
        }
        fn set_typing(&self, convo_id: &str, typing: bool) {
            let _ = self.tx.send(Command::SetTyping {
                room_id: convo_id.to_string(),
                typing,
            });
        }
        fn mark_read(&self, convo_id: &str) {
            let _ = self.tx.send(Command::MarkRead {
                room_id: convo_id.to_string(),
            });
        }

        // Checkpoint 08: interactive verification session (see
        // `verification::Session`); state flows back through
        // `ClientState::verification`.
        fn start_verification(&self, target: VerificationTarget) {
            let _ = self.tx.send(Command::StartVerification { target });
        }
        fn verification_action(&self, action: VerificationAction) {
            let _ = self.tx.send(Command::VerificationAction { action });
        }

        async fn devices(&self) -> Vec<Device> {
            self.ask(|reply| Command::FetchDevices { reply })
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!("devices: {}", e.0);
                    Vec::new()
                })
        }

        async fn public_rooms(&self) -> Vec<PublicRoom> {
            Self::empty_vec("public_rooms")
        }
        async fn public_spaces(&self) -> Vec<PublicSpace> {
            Self::empty_vec("public_spaces")
        }
        async fn join_room(&self, _room_id: &str) -> Result<(), ClientError> {
            Err(Self::unimplemented("join_room"))
        }

        // Checkpoint 07: media resolve (data URI) and file-card download.
        async fn media_uri(
            &self,
            mxc: &str,
            encrypted: Option<String>,
            thumb: Option<(u32, u32)>,
        ) -> Result<String, ClientError> {
            let mxc = mxc.to_string();
            self.ask(move |reply| Command::MediaUri {
                mxc,
                encrypted,
                thumb,
                reply,
            })
            .await
        }

        async fn save_attachment(
            &self,
            _convo_id: &str,
            attachment: Attachment,
            dest_path: String,
        ) -> Result<(), ClientError> {
            self.ask(move |reply| Command::SaveAttachment {
                attachment,
                dest_path,
                reply,
            })
            .await
        }
    }
}

#[cfg(feature = "matrix")]
pub use matrix_impl::MatrixClient;
