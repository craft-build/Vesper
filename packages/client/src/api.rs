//! The seam between Vesper's UI and whatever serves it data.
//!
//! This module is deliberately free of `matrix-sdk`: the trait, the error
//! type, and every [`crate::model`] type are plain Rust, so the `ui` crate
//! (including its wasm target) can depend on this crate with default features
//! off and never link matrix-sdk.

use std::collections::BTreeMap;

use crate::model::*;
use dioxus_signals::{Signal, SyncStorage};

/// Live UI-visible state written by backend sync tasks and read reactively by
/// components.
///
/// The signals use [`SyncStorage`] so they can safely be written from the
/// backend's dedicated tokio runtime thread: Dioxus' default `Signal<T>`
/// storage is thread-local and would fail off-thread, while sync-signal
/// writes are direct in-place writes that mark subscribers dirty through the
/// (cross-thread-safe) Dioxus scheduler channel. The signals must be created
/// in a component scope that outlives any sync task — the root `App` scope,
/// which lives for the app lifetime — otherwise a write after scope disposal
/// panics the writer (docs/03-sync-roomlist.md, "Signal ownership problem").
/// If that panic ever surfaces, fall back to an mpsc channel drained by a
/// `use_coroutine` on the UI side.
#[derive(Clone, Copy)]
pub struct ClientState {
    /// The unified DM + room list, ordered by recency as produced by sync.
    pub convos: Signal<Vec<Convo>, SyncStorage>,
    /// True while the sync is catching up, reconnecting, or offline; the
    /// shell shows a subtle hint.
    pub connecting: Signal<bool, SyncStorage>,
    /// Per-room message history published by the timeline tasks introduced in
    /// checkpoint 04, keyed by room id, oldest first. One map signal (not
    /// nested per-room signals) so new entries never need a new signal to be
    /// created outside this struct's owner scope — see the lifecycle note
    /// above. Reading from a component subscribes it to the whole map; the
    /// timeline tasks replace the entry value on every diff batch.
    pub messages: Signal<BTreeMap<String, Vec<Message>>, SyncStorage>,
}

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

    /// Hand the UI's live state handles to the backend. Called once from the
    /// root `App` scope after the signals exist; must be idempotent
    /// (component bodies can re-run). Background sync tasks write into these
    /// signals so components re-render live.
    fn bind_state(&self, state: ClientState);

    async fn spaces(&self) -> Vec<Space>;
    /// Direct messages and rooms combined, mirroring the prototype's `[...dms, ...rooms]`.
    ///
    /// With the real backend this is the latest synced snapshot (empty until
    /// the first sync batch lands); components that want live updates should
    /// read [`ClientState::convos`] instead.
    async fn conversations(&self) -> Vec<Convo>;

    async fn messages(&self, convo_id: &str) -> Vec<Message>;

    /// Open (refcounted) the live timeline for `convo_id`. The backend
    /// publishes mapped messages into [`ClientState::messages`]; reopening an
    /// already-open room is cheap (refcount only). Mock: no-op.
    fn open_timeline(&self, convo_id: &str);
    /// Close one reference to `convo_id`'s timeline. When the count reaches
    /// zero the backend disposes the timeline task. Mock: no-op.
    fn close_timeline(&self, convo_id: &str);
    /// Back-paginate `convo_id`'s open timeline by one ~30-event page.
    /// Returns the number of new messages actually added (0 when the start of
    /// the timeline is reached or the timeline isn't open).
    async fn load_older(&self, convo_id: &str) -> Result<usize, ClientError>;
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
