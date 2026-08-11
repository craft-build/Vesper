//! In-memory client seeded with the same dataset as the prototype's `data.js`.
//!
//! The real backend lives in the `client` crate (`MatrixClient`); which one the app
//! uses is chosen in [`crate::data::backend`].

use std::cell::RefCell;
use std::collections::HashMap;

use dioxus::prelude::WritableExt;

use crate::data::*;

struct MockState {
    me: Me,
    spaces: Vec<Space>,
    convos: Vec<Convo>,
    messages: HashMap<String, Vec<Message>>,
    threads: HashMap<String, Vec<ThreadReply>>,
    devices: Vec<Device>,
    public_rooms: Vec<PublicRoom>,
    public_spaces: Vec<PublicSpace>,
    next_id: u64,
}

pub struct MockClient {
    state: RefCell<MockState>,
}

fn dm(id: &str, name: &str, mxid: &str, status: Presence, last: &str, unread: u32) -> Convo {
    Convo {
        id: id.into(),
        kind: ConvoKind::Dm,
        name: name.into(),
        last: last.into(),
        unread,
        encrypted: true,
        mxid: Some(mxid.into()),
        status: Some(status),
        topic: None,
        space: None,
        members: None,
    }
}

fn room(
    id: &str,
    space: &str,
    name: &str,
    topic: &str,
    members: u32,
    last: &str,
    unread: u32,
    encrypted: bool,
) -> Convo {
    Convo {
        id: id.into(),
        kind: ConvoKind::Room,
        name: name.into(),
        last: last.into(),
        unread,
        encrypted,
        mxid: None,
        status: None,
        topic: Some(topic.into()),
        space: Some(space.into()),
        members: Some(members),
    }
}

fn msg(id: &str, from: &str, time: &str, text: &str) -> Message {
    Message::new(id, from, time, text)
}

fn mine(mut m: Message) -> Message {
    m.mine = true;
    m
}

fn reacted(mut m: Message, emoji: &str, count: u32, me: bool) -> Message {
    m.reactions.push(Reaction {
        emoji: emoji.into(),
        count,
        me,
    });
    m
}

fn replying_to(mut m: Message, id: &str) -> Message {
    m.reply_to = Some(id.into());
    m
}

fn threaded(mut m: Message, count: u32) -> Message {
    m.thread_count = count;
    m
}

fn read(mut m: Message, by: &[&str]) -> Message {
    m.read_by = by.iter().map(|s| s.to_string()).collect();
    m
}

fn system(time: &str, text: &str) -> Message {
    let mut m = Message::new(format!("sys-{time}"), "", time, text);
    m.system = true;
    m
}

fn with_attachment(mut m: Message, kind: AttachmentKind, name: &str, size: &str) -> Message {
    m.attachment = Some(Attachment {
        kind,
        name: name.into(),
        size: size.into(),
    });
    m
}

impl Default for MockClient {
    fn default() -> Self {
        let me = Me {
            name: "You".into(),
            id: "@you:vesper.chat".into(),
        };

        let spaces = vec![
            Space {
                id: "vesper-team".into(),
                name: "Vesper Team".into(),
            },
            Space {
                id: "matrix-hq".into(),
                name: "Matrix HQ".into(),
            },
        ];

        let convos = vec![
            dm(
                "dm-akari",
                "Akari Fur",
                "@akari:vesper.chat",
                Presence::Online,
                "want me to open a PR for the avatar fallback color?",
                1,
            ),
            dm(
                "dm-mira",
                "Mira Solheim",
                "@mira:matrix.org",
                Presence::Away,
                "see you at 3",
                0,
            ),
            dm(
                "dm-jonas",
                "Jonas Reyk",
                "@jonas:envs.net",
                Presence::Offline,
                "thanks for the review",
                0,
            ),
            room(
                "general",
                "vesper-team",
                "general",
                "Team-wide chatter",
                6,
                "Akari: pushed the new fox icon set",
                3,
                true,
            ),
            room(
                "design",
                "vesper-team",
                "design",
                "Design reviews & assets",
                4,
                "You: sounds good, shipping today",
                0,
                true,
            ),
            room(
                "ops",
                "vesper-team",
                "ops",
                "Deploys & infra",
                5,
                "Deploy complete — v2.3.1",
                0,
                false,
            ),
            room(
                "matrix-hq",
                "matrix-hq",
                "matrix-hq",
                "Federation & protocol talk",
                341,
                "sync federation issue resolved",
                12,
                false,
            ),
            room(
                "random",
                "matrix-hq",
                "random",
                "Off-topic",
                182,
                "anyone else on the new build?",
                1,
                false,
            ),
        ];

        let mut messages = HashMap::new();
        messages.insert(
            "general".to_string(),
            vec![
                reacted(
                    threaded(
                        msg(
                            "g1",
                            "Akari Fur",
                            "13:58",
                            "pushed the new fox icon set to the shared drive",
                        ),
                        3,
                    ),
                    "👍",
                    2,
                    false,
                ),
                replying_to(
                    mine(msg(
                        "g2",
                        "You",
                        "14:00",
                        "looks great, the stroke weight matches our **lucide** setup",
                    )),
                    "g1",
                ),
                msg(
                    "g3",
                    "Akari Fur",
                    "14:01",
                    "want me to open a PR for the avatar fallback color?",
                ),
                read(
                    reacted(mine(msg("g4", "You", "14:02", "yes please")), "✅", 1, true),
                    &["Akari Fur"],
                ),
            ],
        );
        messages.insert(
            "design".to_string(),
            vec![
                with_attachment(
                    msg(
                        "d1",
                        "Mira Solheim",
                        "11:20",
                        "new fox mark variants are in the drive, check the roundel version",
                    ),
                    AttachmentKind::Image,
                    "vesper-mark-variants.fig",
                    "2.1 MB",
                ),
                mine(msg(
                    "d2",
                    "You",
                    "11:32",
                    "the roundel is the one, matches the login screen nicely",
                )),
                reacted(
                    msg("d3", "Mira Solheim", "11:33", "sounds good, shipping today"),
                    "🎉",
                    2,
                    false,
                ),
            ],
        );
        messages.insert(
            "ops".to_string(),
            vec![
                with_attachment(
                    msg(
                        "o1",
                        "Jonas Reyk",
                        "09:02",
                        "rolling out v2.3.1, should be quick",
                    ),
                    AttachmentKind::File,
                    "deploy-notes.pdf",
                    "184 KB",
                ),
                system("09:14", "Room upgraded to v11"),
                msg("o3", "Jonas Reyk", "09:20", "Deploy complete — v2.3.1"),
            ],
        );
        messages.insert(
            "matrix-hq".to_string(),
            vec![
                msg(
                    "h1",
                    "Elin Voss",
                    "08:40",
                    "anyone seeing sync delays on synapse 1.99?",
                ),
                msg(
                    "h2",
                    "Ravi Kant",
                    "08:44",
                    "yeah, federation queue backed up on our side too",
                ),
                system("08:55", "Encryption verified for this session"),
                reacted(
                    msg("h4", "Elin Voss", "09:10", "sync federation issue resolved"),
                    "👍",
                    4,
                    false,
                ),
            ],
        );
        messages.insert(
            "random".to_string(),
            vec![
                msg("r1", "Ravi Kant", "15:02", "anyone else on the new build?"),
                mine(msg("r2", "You", "15:10", "yep, running it now")),
            ],
        );
        messages.insert(
            "dm-akari".to_string(),
            vec![
                msg(
                    "a1",
                    "Akari Fur",
                    "13:58",
                    "pushed the new fox icon set to the shared drive",
                ),
                mine(msg(
                    "a2",
                    "You",
                    "14:00",
                    "looks great, the stroke weight matches our lucide setup",
                )),
                msg(
                    "a3",
                    "Akari Fur",
                    "14:01",
                    "want me to open a PR for the avatar fallback color?",
                ),
            ],
        );
        messages.insert(
            "dm-mira".to_string(),
            vec![
                msg("m1", "Mira Solheim", "12:50", "lunch at 1?"),
                mine(msg(
                    "m2",
                    "You",
                    "12:55",
                    "see you at 3 works better for me",
                )),
                msg("m3", "Mira Solheim", "12:56", "see you at 3"),
            ],
        );
        messages.insert(
            "dm-jonas".to_string(),
            vec![read(
                msg("j1", "Jonas Reyk", "Mon", "thanks for the review"),
                &[],
            )],
        );

        let mut threads = HashMap::new();
        threads.insert(
            "g1".to_string(),
            vec![
                ThreadReply {
                    from: "Mira Solheim".into(),
                    time: "14:05".into(),
                    mine: false,
                    text: "love the new tail shape".into(),
                },
                ThreadReply {
                    from: "Akari Fur".into(),
                    time: "14:06".into(),
                    mine: false,
                    text: "thanks, iterated on it a bunch".into(),
                },
                ThreadReply {
                    from: "You".into(),
                    time: "14:10".into(),
                    mine: true,
                    text: "shipping in the next release".into(),
                },
            ],
        );

        let devices = vec![
            Device {
                id: "d1".into(),
                name: "Vesper · macOS".into(),
                last_seen: "active now".into(),
                verified: true,
            },
            Device {
                id: "d2".into(),
                name: "Vesper · iOS".into(),
                last_seen: "2h ago".into(),
                verified: false,
            },
            Device {
                id: "d3".into(),
                name: "Element · Web".into(),
                last_seen: "3d ago".into(),
                verified: true,
            },
        ];

        let public_rooms = vec![
            PublicRoom {
                id: "fosdem".into(),
                name: "fosdem:matrix.org".into(),
                members: 2400,
                topic: "FOSDEM conference chat".into(),
            },
            PublicRoom {
                id: "synapse-dev".into(),
                name: "synapse-dev:matrix.org".into(),
                members: 890,
                topic: "Synapse homeserver development".into(),
            },
            PublicRoom {
                id: "matrix-clients".into(),
                name: "matrix-clients:matrix.org".into(),
                members: 610,
                topic: "Client development discussion".into(),
            },
        ];

        let public_spaces = vec![
            PublicSpace {
                id: "foss-collective".into(),
                name: "FOSS Collective".into(),
                rooms: 24,
            },
            PublicSpace {
                id: "indie-devs".into(),
                name: "Indie Devs".into(),
                rooms: 11,
            },
        ];

        Self {
            state: RefCell::new(MockState {
                me,
                spaces,
                convos,
                messages,
                threads,
                devices,
                public_rooms,
                public_spaces,
                next_id: 1,
            }),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl VesperClient for MockClient {
    async fn login(
        &self,
        _homeserver: String,
        _user_id: String,
        _password: String,
    ) -> Result<Me, ClientError> {
        Ok(self.state.borrow().me.clone())
    }

    async fn restore(&self) -> Result<Option<Me>, ClientError> {
        // The mock keeps today's UX: every launch starts at the login screen.
        Ok(None)
    }

    async fn logout(&self) -> Result<(), ClientError> {
        Ok(())
    }

    async fn me(&self) -> Option<Me> {
        Some(self.state.borrow().me.clone())
    }

    fn bind_state(&self, mut state: ClientState) {
        // Mock has no live sync: seed the list once and never show
        // "connecting". Mutations (send, react) touch messages, not convos.
        state.convos.set(self.state.borrow().convos.clone());
        state.connecting.set(false);
    }

    async fn spaces(&self) -> Vec<Space> {
        self.state.borrow().spaces.clone()
    }

    async fn conversations(&self) -> Vec<Convo> {
        self.state.borrow().convos.clone()
    }

    async fn messages(&self, convo_id: &str) -> Vec<Message> {
        self.state
            .borrow()
            .messages
            .get(convo_id)
            .cloned()
            .unwrap_or_default()
    }

    async fn thread(&self, message_id: &str) -> Vec<ThreadReply> {
        self.state
            .borrow()
            .threads
            .get(message_id)
            .cloned()
            .unwrap_or_default()
    }

    async fn send_message(
        &self,
        convo_id: &str,
        text: String,
        attachment: Option<Attachment>,
        reply_to: Option<String>,
    ) -> Message {
        let mut state = self.state.borrow_mut();
        let id = state.next_id;
        state.next_id += 1;
        let from = state.me.name.clone();
        let message = Message {
            id: format!("new-{id}"),
            from,
            time: "now".into(),
            mine: true,
            system: false,
            text,
            reply_to,
            reactions: Vec::new(),
            thread_count: 0,
            attachment,
            read_by: Vec::new(),
        };
        state
            .messages
            .entry(convo_id.to_string())
            .or_default()
            .push(message.clone());
        message
    }

    async fn send_thread_reply(&self, message_id: &str, text: String) -> ThreadReply {
        let mut state = self.state.borrow_mut();
        let from = state.me.name.clone();
        let reply = ThreadReply {
            from,
            time: "now".into(),
            mine: true,
            text,
        };
        state
            .threads
            .entry(message_id.to_string())
            .or_default()
            .push(reply.clone());
        for messages in state.messages.values_mut() {
            if let Some(root) = messages.iter_mut().find(|m| m.id == message_id) {
                root.thread_count += 1;
                break;
            }
        }
        reply
    }

    async fn react(&self, convo_id: &str, message_id: &str, emoji: &str) -> Vec<Reaction> {
        let mut state = self.state.borrow_mut();
        let Some(messages) = state.messages.get_mut(convo_id) else {
            return Vec::new();
        };
        let Some(message) = messages.iter_mut().find(|m| m.id == message_id) else {
            return Vec::new();
        };
        match message.reactions.iter().position(|r| r.emoji == emoji) {
            Some(idx) => {
                let r = &mut message.reactions[idx];
                if r.me {
                    r.count -= 1;
                    r.me = false;
                } else {
                    r.count += 1;
                    r.me = true;
                }
                if message.reactions[idx].count == 0 {
                    message.reactions.remove(idx);
                }
            }
            None => message.reactions.push(Reaction {
                emoji: emoji.into(),
                count: 1,
                me: true,
            }),
        }
        message.reactions.clone()
    }

    async fn devices(&self) -> Vec<Device> {
        self.state.borrow().devices.clone()
    }

    async fn verify_device(&self, device_id: &str) -> Result<(), ClientError> {
        let mut state = self.state.borrow_mut();
        match state.devices.iter_mut().find(|d| d.id == device_id) {
            Some(device) => {
                device.verified = true;
                Ok(())
            }
            None => Err(ClientError(format!("unknown device {device_id}"))),
        }
    }

    async fn verify_user(&self, _mxid: &str) -> Result<(), ClientError> {
        // Verification state for people (as opposed to devices) is UI-local in the
        // prototype (VerifyDialog's confirmation just flips ProfilePanel's own signal) —
        // there's nothing to persist server-side in the mock either.
        Ok(())
    }

    async fn public_rooms(&self) -> Vec<PublicRoom> {
        self.state.borrow().public_rooms.clone()
    }

    async fn public_spaces(&self) -> Vec<PublicSpace> {
        self.state.borrow().public_spaces.clone()
    }

    async fn join_room(&self, _room_id: &str) -> Result<(), ClientError> {
        Ok(())
    }
}
