//! The seam between Vesper's UI and whatever serves it data.
//!
//! This module is deliberately free of `matrix-sdk`: the trait, the error
//! type, and every [`crate::model`] type are plain Rust, so the `ui` crate
//! (including its wasm target) can depend on this crate with default features
//! off and never link matrix-sdk.

use crate::model::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientError(pub String);

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ClientError {}

/// Everything a Vesper screen needs from "the backend".
///
/// Two implementations exist: `MockClient` (in the `ui` crate) serving the
/// prototype's seed data, and [`crate::MatrixClient`] talking to a real
/// homeserver. No component talks to a backend directly — they only ever go
/// through `Rc<dyn VesperClient>` pulled from context.
///
/// `?Send` because this trait must be usable from the `web` (wasm32) target,
/// where futures are not `Send`.
#[async_trait::async_trait(?Send)]
pub trait VesperClient {
    /// Log in with a password against `homeserver` (a server name like
    /// `matrix.org`; well-known discovery finds the client API URL).
    async fn login(
        &self,
        homeserver: String,
        user_id: String,
        password: String,
    ) -> Result<Me, ClientError>;

    /// Attempt to restore a persisted session. `Ok(None)` means "no stored
    /// session, show the login screen" — not an error. `Err` means a session
    /// file existed but could not be used; the UI should still show the login
    /// screen, having logged the reason.
    async fn restore(&self) -> Result<Option<Me>, ClientError>;

    /// End the current session and remove its on-disk artifacts.
    async fn logout(&self) -> Result<(), ClientError>;

    /// The signed-in account snapshot, if any.
    async fn me(&self) -> Option<Me>;

    async fn spaces(&self) -> Vec<Space>;
    /// Direct messages and rooms combined, mirroring the prototype's `[...dms, ...rooms]`.
    async fn conversations(&self) -> Vec<Convo>;

    async fn messages(&self, convo_id: &str) -> Vec<Message>;
    async fn thread(&self, message_id: &str) -> Vec<ThreadReply>;

    async fn send_message(
        &self,
        convo_id: &str,
        text: String,
        attachment: Option<Attachment>,
        reply_to: Option<String>,
    ) -> Message;
    async fn send_thread_reply(&self, message_id: &str, text: String) -> ThreadReply;
    async fn react(&self, convo_id: &str, message_id: &str, emoji: &str) -> Vec<Reaction>;

    async fn devices(&self) -> Vec<Device>;
    async fn verify_device(&self, device_id: &str) -> Result<(), ClientError>;
    async fn verify_user(&self, mxid: &str) -> Result<(), ClientError>;

    async fn public_rooms(&self) -> Vec<PublicRoom>;
    async fn public_spaces(&self) -> Vec<PublicSpace>;
    async fn join_room(&self, room_id: &str) -> Result<(), ClientError>;
}
