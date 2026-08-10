//! Domain types shared by every `VesperClient` implementation.
//!
//! These are plain data — no Matrix-specific or mock-specific concepts leak in here,
//! so a real `matrix-sdk`-backed client can produce the exact same shapes.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Me {
    pub name: String,
    pub id: String,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub kind: AttachmentKind,
    pub name: String,
    pub size: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reaction {
    pub emoji: String,
    pub count: u32,
    pub me: bool,
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
