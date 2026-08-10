use super::model::*;

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
/// Today the only implementation is [`super::mock::MockClient`], which serves an in-memory
/// copy of the prototype's seed data. A real integration (e.g. a `matrix-sdk`-backed
/// `MatrixClient`) only has to implement this trait — no component in `ui::chat` or
/// `ui::screens` talks to mock data or Matrix directly, they only ever go through
/// `Rc<dyn VesperClient>` pulled from context.
///
/// `?Send` because this trait must be usable from the `web` (wasm32) target, where
/// futures are not `Send`.
#[async_trait::async_trait(?Send)]
pub trait VesperClient {
    async fn login(&self, user_id: String, password: String) -> Result<Me, ClientError>;
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
