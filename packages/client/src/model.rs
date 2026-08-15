//! Domain types shared by every `VesperClient` implementation.
//!
//! These are plain data — no Matrix-specific or mock-specific concepts leak in here,
//! so both the mock backend and the `matrix-sdk`-backed client produce the exact
//! same shapes. (Moved here from `ui::data::model` in checkpoint 02 so that the
//! `client` crate can implement the trait without depending on the UI.)

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Me {
    pub name: String,
    pub id: String,
    /// Own account avatar (`mxc://`) from the profile lookup (checkpoint 07);
    /// initials fallback when unset/unresolved.
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Space {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Online,
    Away,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvoKind {
    Dm,
    Room,
}

/// A direct message or a room — the prototype's `[...dms, ...rooms]` unified list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Convo {
    pub id: String,
    pub kind: ConvoKind,
    pub name: String,
    pub last: String,
    pub unread: u32,
    pub encrypted: bool,
    /// Room avatar (`mxc://`) for rooms, counterpart member avatar for DMs
    /// (checkpoint 07). `None` → initials fallback.
    pub avatar: Option<String>,

    // DM-only
    pub mxid: Option<String>,
    pub status: Option<Presence>,

    // Room-only
    pub topic: Option<String>,
    pub space: Option<String>,
    pub members: Option<u32>,
}

impl Convo {
    pub fn is_room(&self) -> bool {
        matches!(self.kind, ConvoKind::Room)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Image,
    File,
    /// Video/audio are rendered as file cards in the UI (no player in
    /// checkpoint 07); the distinct kinds keep icons and stats honest.
    Video,
    Audio,
}

/// An attachment on a message, or one staged for upload from the composer
/// (checkpoint 07). MXC media metadata is mapped from `m.image`/`m.file`
/// event content; `local_path` travels the other way — set when a file is
/// picked for sending, consumed by the backend's upload path, and never
/// appears on a row published by the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub kind: AttachmentKind,
    pub name: String,
    /// Human-readable size for file cards (e.g. `340 KB`).
    pub size: String,
    /// `mxc://` URI of the media (plain) or of the encrypted blob that
    /// `encrypted` describes.
    pub mxc: Option<String>,
    pub mime: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// `mxc://` URI of a server-generated thumbnail (avatars are thumbed
    /// server-side from `mxc` directly; this is the event's declared
    /// thumbnail).
    pub thumb_mxc: Option<String>,
    /// Serialized ruma `EncryptedFile` JSON when the media is encrypted
    /// (E2EE rooms). Plain JSON keeps the trait seam free of ruma types.
    pub encrypted: Option<String>,
    /// Serialized ruma `EncryptedFile` JSON for `thumb_mxc`, same idea.
    pub thumb_encrypted: Option<String>,
    /// Composer staging only: path of a picked local file pending upload.
    pub local_path: Option<String>,
}

impl Attachment {
    /// A received-message attachment with no media metadata resolved yet.
    pub fn new(kind: AttachmentKind, name: String, size: String) -> Self {
        Self {
            kind,
            name,
            size,
            mxc: None,
            mime: None,
            width: None,
            height: None,
            thumb_mxc: None,
            encrypted: None,
            thumb_encrypted: None,
            local_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reaction {
    pub emoji: String,
    pub count: u32,
    pub me: bool,
}

/// Delivery state of a message the local user sent (checkpoint 05).
///
/// `Sent` is also the state for everything that didn't originate here —
/// received messages carry no send state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SendState {
    /// Server-confirmed (or a received remote message): the default.
    #[default]
    Sent,
    /// Local echo in the send queue or in flight.
    Sending,
    /// Sending failed; the UI offers retry/discard.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub from: String,
    pub time: String,
    pub mine: bool,
    pub system: bool,
    pub text: String,
    pub reply_to: Option<String>,
    pub reactions: Vec<Reaction>,
    pub thread_count: u32,
    pub attachment: Option<Attachment>,
    pub read_by: Vec<String>,
    pub send_state: SendState,
    /// Sender avatar (`mxc://`) from the event's sender profile
    /// (checkpoint 07). `None` → initials fallback.
    pub avatar: Option<String>,
}

impl Message {
    pub fn new(
        id: impl Into<String>,
        from: impl Into<String>,
        time: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            from: from.into(),
            time: time.into(),
            mine: false,
            system: false,
            text: text.into(),
            reply_to: None,
            reactions: Vec::new(),
            thread_count: 0,
            attachment: None,
            read_by: Vec::new(),
            send_state: SendState::Sent,
            avatar: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadReply {
    pub from: String,
    pub time: String,
    pub mine: bool,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub last_seen: String,
    pub verified: bool,
}

/// Who an interactive verification session runs against (checkpoint 08).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationTarget {
    /// One of the signed-in user's other sessions, by device id.
    Device(String),
    /// A conversation counterpart, by MXID.
    User(String),
}

/// One emoji of a SAS short-auth string: `("🐱", "Cat")`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SasEmoji {
    pub symbol: String,
    pub description: String,
}

/// Lifecycle of an interactive verification session, mirrored from the
/// backend's SAS state machine into [`ClientState::verification`] so the
/// dialog can render it live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationState {
    /// Outgoing request sent, waiting for the other side to accept.
    Requested,
    /// Both sides accepted; short-auth string not yet available.
    Ready,
    /// Emojis are available for the user to compare.
    EmojisShown,
    /// We confirmed; waiting for the other side to confirm too.
    Confirmed,
    /// Both sides confirmed — verification complete.
    Done,
    /// Cancelled by either side (includes mismatch).
    Cancelled,
    /// The session failed to start (no SAS support, network error…).
    Failed(String),
}

/// The active verification session published through
/// [`ClientState::verification`]. One at a time — the UI shows one dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationSession {
    /// Display label for the dialog ("the other device" fallback when the
    /// UI passes none).
    pub subject: String,
    /// Who this session verifies — lets surfaces (profile panel, device
    /// rows) correlate a `Done` with *their* target instead of any session
    /// completing (review P2).
    pub target: VerificationTarget,
    pub state: VerificationState,
    /// The 7-emoji short-auth string; empty until `EmojisShown`.
    pub emojis: Vec<SasEmoji>,
}

/// User decisions driving an active verification session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationAction {
    /// "They match" — confirm.
    Confirm,
    /// "They don't match" — mismatch + cancel.
    Mismatch,
    /// Dialog closed — cancel.
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicRoom {
    pub id: String,
    pub name: String,
    pub members: u32,
    pub topic: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicSpace {
    pub id: String,
    pub name: String,
    pub rooms: u32,
}
