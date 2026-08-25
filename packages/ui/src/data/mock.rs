//! In-memory client seeded with the same dataset as the prototype's `data.js`.
//!
//! The real backend lives in the `client` crate (`MatrixClient`); which one the app
//! uses is chosen in [`crate::data::backend`].

use std::cell::RefCell;
use std::collections::HashMap;

use dioxus::prelude::{ReadableExt, WritableExt};

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
    notif: Vec<NotifToggle>,
    prefs: Prefs,
    next_id: u64,
}

/// Mock directory page size: small on purpose so search + "load more" can be
/// exercised offline without a wall of fixtures.
const MOCK_PAGE_SIZE: usize = 4;

pub struct MockClient {
    state: RefCell<MockState>,
    /// Live UI state handed over by `bind_state` (checkpoint 08): the fake
    /// verification session publishes into its `verification` signal.
    bound: RefCell<Option<ClientState>>,
    /// Device id the active fake session is verifying, so `Confirm` can
    /// flip its verified flag in the mock store.
    target_device: RefCell<Option<String>>,
}

/// The mock's scripted SAS short-auth string: same 7 emojis every time,
/// so UI iteration is deterministic.
const MOCK_EMOJIS: [(&str, &str); 7] = [
    ("🐱", "Cat"),
    ("🚀", "Rocket"),
    ("🎩", "Top hat"),
    ("🔑", "Key"),
    ("🍕", "Pizza"),
    ("🌋", "Volcano"),
    ("🌊", "Wave"),
];

fn dm(id: &str, name: &str, mxid: &str, status: Presence, last: &str, unread: u32) -> Convo {
    Convo {
        id: id.into(),
        kind: ConvoKind::Dm,
        name: name.into(),
        last: last.into(),
        unread,
        encrypted: true,
        avatar: None,
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
        avatar: None,
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
    m.attachment = Some(Attachment::new(kind, name.into(), size.into()));
    m
}

impl Default for MockClient {
    fn default() -> Self {
        let me = Me {
            name: "You".into(),
            id: "@you:vesper.chat".into(),
            avatar: None,
        };

        // Children mirror the seeded convos' `space` fields so the drawer's
        // grouping has data to chew on offline.
        let spaces = vec![
            Space {
                id: "vesper-team".into(),
                name: "Vesper Team".into(),
                avatar: None,
                members: Some(7),
                children: vec!["general".into(), "design".into(), "ops".into()],
            },
            Space {
                id: "matrix-hq".into(),
                name: "Matrix HQ".into(),
                avatar: None,
                members: Some(523),
                children: vec!["matrix-hq".into(), "random".into()],
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
                current: true,
            },
            Device {
                id: "d2".into(),
                name: "Vesper · iOS".into(),
                last_seen: "2h ago".into(),
                verified: false,
                current: false,
            },
            Device {
                id: "d3".into(),
                name: "Element · Web".into(),
                last_seen: "3d ago".into(),
                verified: true,
                current: false,
            },
        ];

        // Checkpoint 10: notification toggles + prefs seeded from the rule
        // table defaults, so the settings tabs render real shapes offline.
        let notif = client::notifications::default_toggles();
        let prefs = Prefs::default();

        // Directory fixtures (checkpoint 09): enough rows to overflow
        // [`MOCK_PAGE_SIZE`] so "load more" has something to load. Joining
        // the "forbidden" room exercises the modal's inline error state
        // offline.
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
            PublicRoom {
                id: "rust".into(),
                name: "rust:matrix.org".into(),
                members: 3100,
                topic: "The Rust programming language".into(),
            },
            PublicRoom {
                id: "thisweekinmatrix".into(),
                name: "twim:matrix.org".into(),
                members: 480,
                topic: "This Week in Matrix".into(),
            },
            PublicRoom {
                id: "matrix-spec".into(),
                name: "matrix-spec:matrix.org".into(),
                members: 260,
                topic: "Spec authoring and process".into(),
            },
            PublicRoom {
                id: "triage".into(),
                name: "traversal:matrix.org".into(),
                members: 120,
                topic: "Wednesday triage party".into(),
            },
            PublicRoom {
                id: "forbidden-room".into(),
                name: "invite-only:example.org".into(),
                members: 42,
                topic: "Joining this one fails (mock error fixture)".into(),
            },
        ];

        let public_spaces = vec![
            PublicSpace {
                id: "foss-collective".into(),
                name: "FOSS Collective".into(),
                members: 2412,
                topic: "Free software communities under one roof".into(),
            },
            PublicSpace {
                id: "indie-devs".into(),
                name: "Indie Devs".into(),
                members: 611,
                topic: "Solo gamedev and appdev chatter".into(),
            },
            PublicSpace {
                id: "matrix-live".into(),
                name: "Matrix Live".into(),
                members: 890,
                topic: "Talks, streams, and office hours".into(),
            },
            PublicSpace {
                id: "privacy-tools".into(),
                name: "Privacy Tools".into(),
                members: 137,
                topic: "E2EE everything".into(),
            },
            PublicSpace {
                id: "web-dev-hub".into(),
                name: "Web Dev Hub".into(),
                members: 533,
                topic: "Frontend, backend, and the misery between".into(),
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
                notif,
                prefs,
                next_id: 1,
            }),
            bound: RefCell::new(None),
            target_device: RefCell::new(None),
        }
    }
}

impl MockClient {
    /// The mock's stand-in for the backend's sync stream (checkpoint 09):
    /// after join/leave mutate the store, republish both lists into the
    /// bound signals so the nav drawer re-renders. No-ops before
    /// `bind_state`.
    fn publish_lists(&self) {
        let Some(mut state) = *self.bound.borrow() else {
            return;
        };
        state.convos.set(self.state.borrow().convos.clone());
        state.spaces.set(self.state.borrow().spaces.clone());
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
        // "connecting". Mutations (send, react, join/leave) touch messages
        // and the convo/space lists, republished below.
        state.convos.set(self.state.borrow().convos.clone());
        state.spaces.set(self.state.borrow().spaces.clone());
        state.connecting.set(false);
        *self.bound.borrow_mut() = Some(state);
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

    // Mock has no live timelines (checkpoint 04): the snapshot `messages()`
    // path stays authoritative and nothing is published into
    // `ClientState::messages`.
    fn open_timeline(&self, _convo_id: &str) {}
    fn close_timeline(&self, _convo_id: &str) {}
    async fn load_older(&self, _convo_id: &str) -> Result<usize, ClientError> {
        Ok(0)
    }

    async fn thread(&self, _convo_id: &str, message_id: &str) -> Vec<ThreadReply> {
        self.state
            .borrow()
            .threads
            .get(message_id)
            .cloned()
            .unwrap_or_default()
    }

    // Mock has no live threads: the snapshot `thread()` data plus the
    // panel's local optimistic pushes stay authoritative.
    fn open_thread(&self, _convo_id: &str, _message_id: &str) {}
    fn close_thread(&self, _message_id: &str) {}

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
            send_state: SendState::Sent,
            avatar: None,
        };
        state
            .messages
            .entry(convo_id.to_string())
            .or_default()
            .push(message.clone());
        message
    }

    async fn send_thread_reply(
        &self,
        _convo_id: &str,
        message_id: &str,
        text: String,
    ) -> Result<ThreadReply, ClientError> {
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
        Ok(reply)
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

    async fn retry_send(&self, _convo_id: &str, _message_id: &str) -> Result<(), ClientError> {
        Ok(())
    }

    async fn discard_send(&self, _convo_id: &str, _message_id: &str) -> Result<(), ClientError> {
        Ok(())
    }

    async fn devices(&self) -> Vec<Device> {
        self.state.borrow().devices.clone()
    }

    // Checkpoint 10: in-memory account console so the settings tabs stay
    // demoable offline. Profile saves rewrite `state.me` — `me()` clones it,
    // so callers (nav footer, settings header) repaint.
    async fn set_display_name(&self, name: String) -> Result<Me, ClientError> {
        if name.trim().is_empty() {
            return Err(ClientError::invalid("Display name cannot be empty."));
        }
        let mut state = self.state.borrow_mut();
        state.me.name = name;
        Ok(state.me.clone())
    }

    async fn set_avatar(&self, _path: String) -> Result<Me, ClientError> {
        // The mock has no media backend: mint a fresh fake avatar id so the
        // "changed" state is observable (initials swap is the visible effect
        // since nothing resolves mock:// urls).
        let mut state = self.state.borrow_mut();
        let n = state.next_id;
        state.next_id += 1;
        state.me.avatar = Some(format!("mock://avatar-{n}"));
        Ok(state.me.clone())
    }

    async fn rename_device(&self, device_id: String, name: String) -> Result<(), ClientError> {
        let mut state = self.state.borrow_mut();
        let Some(device) = state.devices.iter_mut().find(|d| d.id == device_id) else {
            return Err(ClientError::invalid("No such session."));
        };
        device.name = name;
        Ok(())
    }

    async fn delete_device(&self, device_id: String, _password: String) -> Result<(), ClientError> {
        let mut state = self.state.borrow_mut();
        if state.devices.iter().any(|d| d.id == device_id && d.current) {
            return Err(ClientError::invalid(
                "Sign out instead of deleting this session.",
            ));
        }
        let before = state.devices.len();
        state.devices.retain(|d| d.id != device_id);
        if state.devices.len() == before {
            return Err(ClientError::invalid("No such session."));
        }
        Ok(())
    }

    async fn notification_rules(&self) -> Result<Vec<NotifToggle>, ClientError> {
        Ok(self.state.borrow().notif.clone())
    }

    async fn set_notification_rule(
        &self,
        toggle_id: String,
        enabled: bool,
    ) -> Result<Vec<NotifToggle>, ClientError> {
        let mut state = self.state.borrow_mut();
        let Some(toggle) = state.notif.iter_mut().find(|t| t.id == toggle_id) else {
            return Err(ClientError::invalid("Unknown notification setting."));
        };
        toggle.enabled = enabled;
        Ok(state.notif.clone())
    }

    async fn prefs(&self) -> Prefs {
        self.state.borrow().prefs.clone()
    }

    async fn set_prefs(&self, prefs: Prefs) -> Result<(), ClientError> {
        self.state.borrow_mut().prefs = prefs;
        Ok(())
    }

    // Checkpoint 08: scripted happy-path session. `EmojisShown` immediately
    // (deterministic 7 emojis), confirm → `Done` and the target device
    // flips verified in the mock store, mismatch/cancel → `Cancelled`.
    fn start_verification(&self, target: VerificationTarget) {
        let Some(state) = *self.bound.borrow() else {
            return;
        };
        let subject = match &target {
            VerificationTarget::Device(id) => self
                .state
                .borrow()
                .devices
                .iter()
                .find(|d| &d.id == id)
                .map(|d| d.name.clone())
                .unwrap_or_default(),
            VerificationTarget::User(_) => String::new(),
        };
        if let VerificationTarget::Device(id) = &target {
            *self.target_device.borrow_mut() = Some(id.clone());
        }
        let mut verification = state.verification;
        verification.set(Some(VerificationSession {
            subject,
            target,
            state: VerificationState::EmojisShown,
            emojis: MOCK_EMOJIS
                .iter()
                .map(|(symbol, description)| SasEmoji {
                    symbol: (*symbol).into(),
                    description: (*description).into(),
                })
                .collect(),
        }));
    }

    fn verification_action(&self, action: VerificationAction) {
        let Some(state) = *self.bound.borrow() else {
            return;
        };
        let Some(mut session) = state.verification.read().clone() else {
            return;
        };
        match action {
            VerificationAction::Confirm => {
                session.state = VerificationState::Done;
                if let Some(id) = self.target_device.borrow().clone() {
                    let mut store = self.state.borrow_mut();
                    if let Some(device) = store.devices.iter_mut().find(|d| d.id == id) {
                        device.verified = true;
                    }
                }
            }
            VerificationAction::Mismatch | VerificationAction::Cancel => {
                session.state = VerificationState::Cancelled;
            }
        }
        let mut verification = state.verification;
        if verification.read().clone() != Some(session.clone()) {
            verification.set(Some(session));
        }
    }

    // Checkpoint 09: query-aware, paginated directory over the fixtures.
    // The batch token is the next offset into the query-filtered list
    // (clamped — a stale token from a superseded query yields an empty page,
    // never a panic).
    async fn public_rooms(
        &self,
        query: String,
        batch_token: Option<String>,
    ) -> Result<PublicRoomPage, ClientError> {
        let state = self.state.borrow();
        let filtered = filter_directory(&state.public_rooms, &query, |r| (&r.name, &r.topic));
        let start = batch_token
            .and_then(|t| t.parse::<usize>().ok())
            .unwrap_or(0)
            .min(filtered.len());
        let end = (start + MOCK_PAGE_SIZE).min(filtered.len());
        Ok(PublicRoomPage {
            rooms: filtered[start..end].to_vec(),
            next: (end < filtered.len()).then(|| end.to_string()),
        })
    }

    async fn public_spaces(
        &self,
        query: String,
        batch_token: Option<String>,
    ) -> Result<PublicSpacePage, ClientError> {
        let state = self.state.borrow();
        let filtered = filter_directory(&state.public_spaces, &query, |s| (&s.name, &s.topic));
        let start = batch_token
            .and_then(|t| t.parse::<usize>().ok())
            .unwrap_or(0)
            .min(filtered.len());
        let end = (start + MOCK_PAGE_SIZE).min(filtered.len());
        Ok(PublicSpacePage {
            spaces: filtered[start..end].to_vec(),
            next: (end < filtered.len()).then(|| end.to_string()),
        })
    }

    // Joining (checkpoint 09): rooms join as ungrouped convos with a
    // system-style last line; spaces join into the spaces list. The
    // "forbidden" fixture exercises the modal's inline error state.
    async fn join_room(&self, room_id_or_alias: &str) -> Result<(), ClientError> {
        if room_id_or_alias.contains("forbidden") {
            return Err(ClientError::auth(
                "You can't join that room (invite-only?).",
            ));
        }
        let mut state = self.state.borrow_mut();
        if let Some(space) = state
            .public_spaces
            .iter()
            .find(|s| s.id == room_id_or_alias)
            .cloned()
        {
            if state.spaces.iter().any(|s| s.id == space.id) {
                return Ok(());
            }
            state.spaces.push(Space {
                id: space.id,
                name: space.name,
                avatar: None,
                members: Some(space.members),
                children: Vec::new(),
            });
            drop(state);
            self.publish_lists();
            return Ok(());
        }
        if state.convos.iter().any(|c| c.id == room_id_or_alias) {
            return Ok(());
        }
        let entry = state.public_rooms.iter().find(|r| r.id == room_id_or_alias);
        let (name, members, topic) = match entry {
            Some(r) => (r.name.clone(), Some(r.members), Some(r.topic.clone())),
            None => (room_id_or_alias.to_string(), None, None),
        };
        let convo = Convo {
            id: room_id_or_alias.to_string(),
            kind: ConvoKind::Room,
            name,
            last: "You joined this room".into(),
            unread: 0,
            encrypted: false,
            avatar: None,
            mxid: None,
            status: None,
            topic,
            // Ungrouped until the (mock-absent) space lists it.
            space: None,
            members,
        };
        state.convos.push(convo);
        drop(state);
        self.publish_lists();
        Ok(())
    }

    async fn leave_room(&self, room_id: &str) -> Result<(), ClientError> {
        let mut state = self.state.borrow_mut();
        let had_room = state.convos.iter().any(|c| c.id == room_id);
        let had_space = state.spaces.iter().any(|s| s.id == room_id);
        if !had_room && !had_space {
            return Err(ClientError::invalid(
                "That room isn't in your list anymore.",
            ));
        }
        state.convos.retain(|c| c.id != room_id);
        state.spaces.retain(|s| s.id != room_id);
        // A space's children list can keep ids we just left — rooms are only
        // grouped when they're in the convo list, so no extra cleanup needed.
        drop(state);
        self.publish_lists();
        Ok(())
    }
}

/// Mock search: case-insensitive substring over name and topic, mirroring
/// the server-side `generic_search_term` contract well enough for offline
/// UI iteration.
fn filter_directory<T>(
    items: &[T],
    query: &str,
    fields: impl Fn(&T) -> (&String, &String),
) -> Vec<T>
where
    T: Clone,
{
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return items.to_vec();
    }
    items
        .iter()
        .filter(|item| {
            let (name, topic) = fields(item);
            name.to_lowercase().contains(&q) || topic.to_lowercase().contains(&q)
        })
        .cloned()
        .collect()
}
