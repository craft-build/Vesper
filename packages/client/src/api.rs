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
    /// Live thread reply lists published by thread-focused timelines
    /// opened for the thread panel, keyed by the thread root's event id
    /// (checkpoint 05 follow-up: live thread panel). Same "one map signal""
    /// lifecycle as `messages`. Snap-shot-only backends (mock) never
    /// publish here; callers fall back to one-shot data.
    pub threads: Signal<BTreeMap<String, Vec<ThreadReply>>, SyncStorage>,
    /// Per-room typing indicators (checkpoint 06): resolved display names of
    /// the users currently typing in each room, keyed by room id. Written by
    /// the backend's typing task from `m.typing` ephemeral events; read by the
    /// conversation's typing row. Same one-map lifecycle as `messages` — a
    /// single map signal created in the App scope, entries upserted/cleared
    /// by the typing task. Snap-shot-only backends (mock) never publish here.
    pub typing: Signal<BTreeMap<String, Vec<String>>, SyncStorage>,
    /// Per-user presence (checkpoint 06): the latest presence for each user
    /// keyed by MXID, written by the backend's presence task from
    /// `m.presence` events and read by the profile panel and the nav-drawer
    /// status dots (via the `Convo.status` mapping done in the sync task).
    /// Same one-map lifecycle as `messages`.
    pub presence: Signal<BTreeMap<String, Presence>, SyncStorage>,
    /// Whether the app window is focused (checkpoint 06): UI-written (the
    /// shell's focus/visibility listener), backend-read by the desktop
    /// notification task to suppress notifications while the user is looking
    /// at the app. The notification task lives behind the `matrix` feature
    /// and only reads `peek()`, so a plain UI-side signal is safe.
    pub focused: Signal<bool, SyncStorage>,
    /// The room id of the currently open conversation (checkpoint 06):
    /// UI-written (the chat view's mount/drop effect), backend-read by the
    /// desktop notification task to suppress notifications for the room the
    /// user is already in. `None` when no room is open.
    pub active_room: Signal<Option<String>, SyncStorage>,
    /// In-memory cache of resolved media (checkpoint 07): `data:` URI strings
    /// keyed by `"{mxc}|{w}x{h}"` for thumbnails or `"{mxc}"` for full
    /// content. UI-written by the media-resolution hook after each
    /// [`VesperClient::media_uri`] fetch, read-first by every resolver so
    /// repeated avatars/images of the same MXC never refetch (no flicker).
    /// MXC media is content-addressed, so entries never need invalidation;
    /// on-disk persistence across restarts is the SqliteMediaStore's job.
    pub media: Signal<BTreeMap<String, String>, SyncStorage>,
    /// Joined Matrix spaces (checkpoint 09), recomputed by the room-list
    /// sync task whenever the room list or a space's children change. The
    /// nav drawer groups rooms under these; the ⌘K switcher stays flat.
    /// Same App-scope lifecycle as the maps above (see the lifecycle note
    /// on this struct).
    pub spaces: Signal<Vec<Space>, SyncStorage>,
    /// The active interactive verification session, if any (checkpoint 08).
    /// Written by the backend's verification driver from the tokio thread,
    /// read by the verify dialog; `None` when no session is running. One
    /// slot — one dialog at a time. Same App-scope lifecycle as the maps
    /// above (see the lifecycle note on this struct).
    pub verification: Signal<Option<VerificationSession>, SyncStorage>,
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

    /// The signed-in account's joined spaces (checkpoint 09), latest
    /// snapshot. Components that want live updates should read
    /// [`ClientState::spaces`] instead — this is the one-shot fallback for
    /// snapshot-only backends.
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
    /// Fetch the replies of the thread rooted at `message_id` (a mapped row
    /// id) within `convo_id` — the room id is required on the real backend
    /// (the mock ignores it). One-shot: use [`Self::open_thread`] for a
    /// live-updating list.
    async fn thread(&self, convo_id: &str, message_id: &str) -> Vec<ThreadReply>;
    /// Open (refcounted) the live thread rooted at `message_id` in
    /// `convo_id`: publishes reply rows into `ClientState::threads[message_id]`
    /// on every diff batch. Mock: no-op.
    fn open_thread(&self, convo_id: &str, message_id: &str);
    /// Release one reference to `message_id`'s thread; disposes its
    /// thread-focused timeline at zero. Mock: no-op.
    fn close_thread(&self, message_id: &str);

    /// Send a text message into `convo_id`, optionally as an in-reply-to on
    /// `reply_to` (the message row id). The returned `Message` is only a
    /// bookkeeping stub — the painted row comes from the backend's live
    /// timeline (or, for the mock, from its own store).
    async fn send_message(
        &self,
        convo_id: &str,
        text: String,
        attachment: Option<Attachment>,
        reply_to: Option<String>,
    ) -> Message;
    /// Send a text reply in the thread rooted at `message_id` within
    /// `convo_id` (the room id is required to build the `m.thread` relation
    /// with the real backend).
    async fn send_thread_reply(
        &self,
        convo_id: &str,
        message_id: &str,
        text: String,
    ) -> ThreadReply;
    /// Add the user's `emoji` reaction to `message_id`, or retract it when
    /// already present (toggle semantics). The returned aggregate may lag
    /// the toggle by one echo round-trip on the real backend; live rows
    /// repaint from the timeline diff stream instead.
    async fn react(&self, convo_id: &str, message_id: &str, emoji: &str) -> Vec<Reaction>;

    /// Retry a failed/queued send identified by its message row id
    /// (no-op `Ok` in the mock).
    async fn retry_send(&self, convo_id: &str, message_id: &str) -> Result<(), ClientError>;
    /// Discard a pending/failed send identified by its message row id,
    /// removing its local echo (no-op `Ok` in the mock).
    async fn discard_send(&self, convo_id: &str, message_id: &str) -> Result<(), ClientError>;

    /// Tell the backend the user is (or is not) typing in `convo_id`
    /// (checkpoint 06). The composer calls this on input / send. The real
    /// backend debounces a 4s idle reset and stops on send; the mock no-ops.
    /// Fire-and-forget: never blocks the composer.
    fn set_typing(&self, _convo_id: &str, _typing: bool) {}

    /// Mark `convo_id` as read up to its latest event (checkpoint 06): the
    /// conversation calls this on mount and when new items arrive while it's
    /// focused. The real backend sends a throttled `m.read` receipt (>=1s
    /// between sends, only when the latest event advances); the mock no-ops.
    /// Fire-and-forget.
    fn mark_read(&self, _convo_id: &str) {}

    async fn devices(&self) -> Vec<Device>;

    // ------------------------------------------------------------------
    // Account console (checkpoint 10): profile, sessions, notification
    // rules, and device-local prefs. Every method has a mock counterpart
    // so the settings UI stays demoable offline.
    // ------------------------------------------------------------------

    /// Set the account display name server-side. Returns the fresh identity
    /// snapshot so the caller can rewrite its `Me` signal and repaint the
    /// shell. Empty names are rejected (they would *remove* the name).
    async fn set_display_name(&self, name: String) -> Result<Me, ClientError> {
        let _ = name;
        Err(ClientError("Not supported by this backend.".into()))
    }

    /// Upload `path`'s bytes as the account avatar (the file is picked on
    /// the UI side; the backend reads bytes + sniffs the mime type). Returns
    /// the fresh identity snapshot with the new `avatar` mxc.
    async fn set_avatar(&self, path: String) -> Result<Me, ClientError> {
        let _ = path;
        Err(ClientError("Not supported by this backend.".into()))
    }

    /// Rename a session by device id (no re-auth needed).
    async fn rename_device(&self, device_id: String, name: String) -> Result<(), ClientError> {
        let _ = (device_id, name);
        Err(ClientError("Not supported by this backend.".into()))
    }

    /// Delete another session, re-authenticating with `password` if the
    /// homeserver demands it (UIAA `m.login.password`; the backend owns the
    /// stage-completion dance). Deleting the current device is rejected —
    /// use [`Self::logout`]. The password never appears in logs or errors.
    async fn delete_device(&self, device_id: String, password: String) -> Result<(), ClientError> {
        let _ = (device_id, password);
        Err(ClientError("Not supported by this backend.".into()))
    }

    /// The push-rule toggles of the notification settings (see
    /// `client::notifications::RULE_TABLE` for the toggle↔rule mapping).
    async fn notification_rules(&self) -> Result<Vec<NotifToggle>, ClientError> {
        Err(ClientError("Not supported by this backend.".into()))
    }

    /// Flip one toggle (writing every Matrix rule behind it) and return the
    /// full refreshed list — the caller replaces its state wholesale, no
    /// optimistic patch to reconcile.
    async fn set_notification_rule(
        &self,
        toggle_id: String,
        enabled: bool,
    ) -> Result<Vec<NotifToggle>, ClientError> {
        let _ = (toggle_id, enabled);
        Err(ClientError("Not supported by this backend.".into()))
    }

    /// Device-local application preferences (theme, receipt/typing opt-outs)
    /// persisted next to the session. Snapshot-only: reads are cheap and
    /// local, no signal plumbing needed.
    async fn prefs(&self) -> Prefs {
        Prefs::default()
    }
    /// Persist preferences (versioned `prefs.json`).
    async fn set_prefs(&self, prefs: Prefs) -> Result<(), ClientError> {
        let _ = prefs;
        Err(ClientError("Not supported by this backend.".into()))
    }

    /// Start (or replace) an interactive verification session against
    /// `target` (checkpoint 08). Progress arrives through
    /// [`ClientState::verification`]; errors surface there as
    /// [`VerificationState::Failed`] rather than through this return (the
    /// session outlives the call). Fire-and-forget semantics.
    fn start_verification(&self, target: VerificationTarget) {
        let _ = target;
    }

    /// Act on the active verification session (confirm / mismatch /
    /// cancel). No-op when none is running.
    fn verification_action(&self, action: VerificationAction) {
        let _ = action;
    }

    /// One page of the homeserver's public room directory (checkpoint 09)
    /// matching `query` (server-side search; empty = browse all), continuing
    /// after `batch_token` from a previous page. `next` on the returned page
    /// feeds the next call; `None` means the end.
    async fn public_rooms(
        &self,
        query: String,
        batch_token: Option<String>,
    ) -> Result<PublicRoomPage, ClientError>;
    /// Same as [`Self::public_rooms`] but restricted to spaces
    /// (`room_type: m.space` server-side where supported, client-filtered
    /// otherwise).
    async fn public_spaces(
        &self,
        query: String,
        batch_token: Option<String>,
    ) -> Result<PublicSpacePage, ClientError>;
    /// Join a public room by id or alias. "Already joined" is a success, not
    /// an error; rate-limit failures (`M_LIMIT_EXCEEDED`) surface as an
    /// [`ClientError`] whose message carries a retry-after hint. On success
    /// the room arrives through the room-list stream — no manual merge.
    async fn join_room(&self, room_id_or_alias: &str) -> Result<(), ClientError>;
    /// Leave `room_id`; the room-list stream drops it from the list.
    async fn leave_room(&self, room_id: &str) -> Result<(), ClientError>;

    /// Resolve media to a renderable URL (checkpoint 07): returns a `data:`
    /// URI (base64) the webview can place directly in `img { src }`.
    ///
    /// `encrypted` carries a serialized ruma `EncryptedFile` JSON blob for
    /// E2EE media (see [`Attachment::encrypted`]); the backend decrypts
    /// transparently. `thumb` requests a server-side thumbnail of up to
    /// `w×h` pixels, falling back to the full content when the server can't
    /// thumbnail the type (e.g. SVG).
    ///
    /// A `data:` URI — not a file path as docs/07 first sketched — because
    /// dioxus-desktop's asset protocol serves only bundled assets and
    /// registered asset-handler routes; bare filesystem paths don't resolve
    /// in the webview. Data URIs also work identically on the (future
    /// wasm) web target.
    async fn media_uri(
        &self,
        _mxc: &str,
        _encrypted: Option<String>,
        _thumb: Option<(u32, u32)>,
    ) -> Result<String, ClientError> {
        Err(ClientError(
            "Media is not supported by this backend.".into(),
        ))
    }

    /// Save `attachment`'s full-resolution content to `dest_path`
    /// (checkpoint 07). The destination is picked on the UI side (native
    /// save dialog must run on the UI thread); the backend fetches bytes
    /// (media-cache aware, transparent decryption) and writes the file.
    async fn save_attachment(
        &self,
        _convo_id: &str,
        _attachment: Attachment,
        _dest_path: String,
    ) -> Result<(), ClientError> {
        Err(ClientError(
            "Media is not supported by this backend.".into(),
        ))
    }
}
