//! Matrix bridge for Vesper.
//!
//! This crate owns everything that talks to a Matrix homeserver via
//! `matrix-sdk`. The trait seam ([`api`]) and domain types ([`model`]) are
//! feature-free so wasm builds of the `ui` crate can consume them without
//! linking matrix-sdk; everything SDK-touching sits behind the `matrix`
//! feature (on by default for native builds).

pub mod api;
pub mod model;

#[cfg(feature = "matrix")]
pub mod runtime;
#[cfg(feature = "matrix")]
mod session;
#[cfg(feature = "matrix")]
pub mod sync;

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
        async fn messages(&self, _convo_id: &str) -> Vec<Message> {
            Self::empty_vec("messages")
        }
        async fn thread(&self, _message_id: &str) -> Vec<ThreadReply> {
            Self::empty_vec("thread")
        }

        async fn send_message(
            &self,
            convo_id: &str,
            _text: String,
            _attachment: Option<Attachment>,
            _reply_to: Option<String>,
        ) -> Message {
            tracing::warn!("send_message: not implemented yet (checkpoint 05)");
            Message::new(format!("local-{}", convo_id), "", "", "")
        }
        async fn send_thread_reply(&self, _message_id: &str, text: String) -> ThreadReply {
            tracing::warn!("send_thread_reply: not implemented yet (checkpoint 05)");
            ThreadReply {
                from: String::new(),
                time: String::new(),
                mine: true,
                text,
            }
        }
        async fn react(&self, _convo_id: &str, _message_id: &str, _emoji: &str) -> Vec<Reaction> {
            Self::empty_vec("react")
        }

        async fn devices(&self) -> Vec<Device> {
            Self::empty_vec("devices")
        }
        async fn verify_device(&self, _device_id: &str) -> Result<(), ClientError> {
            Err(Self::unimplemented("verify_device"))
        }
        async fn verify_user(&self, _mxid: &str) -> Result<(), ClientError> {
            Err(Self::unimplemented("verify_user"))
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
    }
}

#[cfg(feature = "matrix")]
pub use matrix_impl::MatrixClient;
