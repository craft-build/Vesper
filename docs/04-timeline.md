# 04 — Room Timeline (Read Path)

## Goal

Open any room/DM from the real room list and see its actual message history:
messages, sender names, timestamps/day separators, membership/system events,
"load older" back-pagination, and live messages appended while the room is
open. Encrypted events render as placeholders until checkpoint 08.

## Deliverable / how to test

1. Click a busy room from the list → history appears (newest first, scroll
   back loads more in pages with a spinner/threshold trigger).
2. Same room open in Element on a second device, send messages → they appear
   in Vesper live, correct order, correct sender.
3. Room with membership churn shows "X joined/left" system rows instead of
   crashing or blank gaps.
4. Redacted messages show as "message removed" placeholders.
5. Relaunch and reopen the same room → cached events from SQLite appear
   instantly before network.

## Context

- Trait methods: `messages(convo_id)`, `thread(message_id)` (threads are
  read-only until checkpoint 05; listing thread replies belongs here if cheap).
- Consumers: `chat/conversation.rs`, `chat/message_row.rs`, `chat/chat_view.rs`.
- Existing `ui::markdown::render_markdown` renders message bodies — feed it
  the plain or markdown body per `msgtype` (`m.text` markdown unless
  `formatted_body` present; prefer SDK-parsed `m.html→` handling: use
  `formatted_body` HTML through the existing markdown/HTML renderer only if
  it's compatible, else render plain body. Decide and document).
- SDK: `matrix_sdk_ui::timeline::Timeline` built from a room
  (`TimelineBuilder`; with event cache on, `room.timeline()`-family APIs).
  Subscribe to its `VectorDiff` stream like the room list.

## Design decisions

- **One live timeline per open room.** `MatrixClient` keeps
  `HashMap<RoomId, TimelineHandle>` where `TimelineHandle` wraps the SDK
  `Timeline`, its diff subscription task, and a `Signal<Vec<Message>>`
  per-room (book-ID'd by room id; store in `ClientState` context map of
  signals, NOT per-component, so leaving/re-entering instantaneously paints
  from snapshot).
- **Mapping `TimelineItem` → `model::Message`**:
  - `Event` items: sender (MXID + display name via `sender_profile()`),
    timestamp (`origin_server_ts`), event id as `Message.id`, body by
    `msgtype` (`m.text`, `m.emote` → italic/system styling flag,
    `m.notice`), attachments → `<attachment placeholder>` until 07,
    reactions aggregated from the SDK timeline's reaction map, reply-to id
    from the in-reply-to relation, redacted → `system "removed"`,
    encrypted & undecryptable → `system "🔒 unable to decrypt"` flag.
  - `Virtual` items: DayDivider and ReadMarker map to the model's
    system/separator rows; TypingNotification left to checkpoint 06.
  - Local echoes: include with a pending flag if cheap; else defer to 05.
- **Back-pagination**: trait gains
  `async fn load_older(&self, convo_id: &str) -> Result<usize, ClientError>`
  calling `timeline.paginate_backwards(num_events = 30)` until a page is
  added or the start is reached. `Conversation` already has a scroll-top
  hook/infinite scroll (verify in `chat/conversation.rs`; wire the trigger to
  this method).
- **Sender display names**: batch-resolve via `timeline` item sender profile;
  lazy-fetch missing ones; don't block diff application.
- Keep `messages()` (sync snapshot read) for tests/first paint, but the live
  path is the signal.

## Implementation steps

1. `client/src/timeline.rs`: `TimelineRegistry` (map, spawn task per room,
   dispose on last-reader drop or app exit — simple refcount enough).
2. Extend the runtime `Command` set: `OpenTimeline { room_id }`,
   `CloseTimeline { room_id }`, `LoadOlder { room_id, respond }`.
3. Mapping functions `map_item(&TimelineItem) -> Option<Message>` (pure,
   unit-testable with `matrix-sdk-test` timeline items).
4. `ClientState` (context, created in `App`): add
   `Signal<BTreeMap<String, Signal<Vec<Message>>>>` or a small struct with
   interior map; `ChatView` looks up/creates the signal for its `room_id` and
   calls `load_older` from its scroll trigger.
5. Update `Conversation`/`MessageRow` only where flag-rendering differs
   (pending, redacted, encrypted placeholder).
6. Unread marker: skip; checkpoint 06 handles receipts/read-marker display.

## Acceptance criteria

- [ ] History loads for real rooms, ≥3 back-pagination pages without gaps.
- [ ] Live messages append in real time while the room is open.
- [ ] Day dividers, join/leave, redaction placeholders render sensibly.
- [ ] Warm reopen paints from cache before sync completes.
- [ ] Encrypted rooms show placeholders, not blank screens or panics.
- [ ] No unbounded memory: registries drop timelines for unopened rooms
      (verify by opening 20 rooms and checking map size).

## AI implementation prompt

> Implement the read path of room timelines per docs/00 and docs/04. In
> packages/client add a TimelineRegistry: on open, build a
> matrix_sdk_ui::timeline::Timeline for the room (event cache already
> subscribed from checkpoint 03), subscribe to its VectorDiff stream on the
> ClientRuntime, map TimelineItems to ui::data::model::Message (text/emote/
> notice, senders via sender_profile, timestamps, reactions aggregate,
> in-reply-to id, redacted and undecryptable placeholders, DayDivider virtual
> items), and write a per-room Signal<Vec<Message>> stored in ClientState
> context. Add load_older to the VesperClient trait (timeline.paginate_
> backwards, 30/event pages) and wire Conversation's top-of-scroll trigger to
> it. Live messages must append without reopening the room. Test with a real
> busy room plus an encrypted placeholder room. Update the trait + MockClient
> to keep mock mode compiling.
