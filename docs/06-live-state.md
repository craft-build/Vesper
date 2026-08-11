# 06 — Live State: Typing, Receipts, Presence, Notifications

## Goal

The "alive" layer: incoming typing indicators, our typing notices from the
composer, read receipts that clear unread badges when you view a room,
presence dots in the profile panel, and OS-level desktop notifications for
new messages in background rooms.

## Deliverable / how to test

1. Type in Element (same room, other account) → "Alice is typing…" appears
   under Vesper's room within ~1s, disappears ~5s after they stop.
2. Type in Vesper's composer → Element shows Vesper typing; stops when the
   composer is empty or sent.
3. Open a room with unread messages → badge clears, and the other account's
   "read by" receipts advance (check via Element member-info or devtools).
4. Profile panel for a contact shows Online/Offline/Idle matching Element.
5. App unfocused, message lands in another room → OS notification (sender +
   body preview); clicking focuses the app.

## Context

- Consumers: `chat/conversation.rs` (typing row), `chat/composer.rs`,
  `chat/profile_panel.rs` (`Presence` in `model.rs:19`), nav badges (03).
- SDK: typing notices `room.typing_notice(bool)`; incoming typing via
  `Timeline` `TypingNotification` virtual items or
  `client.add_event_handler`/`room` ephemeral handlers; receipts via
  `room.send_single_receipt(ReceiptType::Read, ...)` and room
  `latest_read_receipt` streams / account-data — use
  `Room::subscribe_typing` helpers where offered by 0.18 (check API surface
  at implementation time, SDK refactors here are frequent); presence via
  `room_member.presence()`/`presence_events` handlers.
- Desktop notifications: `notify-rust` crate (macOS/Win/Linux); no daemon
  needed.

## Design decisions

- **Typing send**: debounced in composer — on input, if not already
  "typing" for this room: send `typing_notice(true)`; reset a 4s idle timer
  to send `(false)`; also send `(false)` on message send. Implement in the
  client crate so the composer stays thin.
- **Typing display**: per-room `Signal<Vec<String>>` of typing MXIDs→names in
  `ClientState`, updated from ephemeral typing events (30s timeout pruning
  handled by the SDK/homeserver; still apply client-side 10s safety prune).
- **Receipts**: when `Conversation` mounts a room (and when new items arrive
  while it's focused), call `send_receipt` for the latest fully-read event,
  throttled to ≥1s between sends and only when advancing. This clears server
  unread counters — keeping nav badges honest after room visits (badges
  themselves continue to flow from 03's room-list stream).
- **Presence**: subscribe to presence events; maintain
  `Signal<BTreeMap<String, Presence>>` in `ClientState`; ProfilePanel maps
  MXID → dot. Presence on Matrix.org can be sparse — if a contact shows
  "unknown", render Offline but log `debug` (documented known limitation,
  not a bug).
- **Notifications**: background task listens to the same room-list/latest-
  events stream; notify only when (a) app window unfocused (`document::eval`
  visibility/focus check cached in a signal), (b) room not currently open,
  (c) event is a message (not own). Respects `room.notification_mode` later;
  for now skip muted (`Rules` check optional/stretch). Body preview truncated
  to 80 chars; never log body.

## Implementation steps

1. `client/src/live.rs`: typing-display task + presence task + their signals.
2. Composer: wire `typing_notice` via a small `client.set_typing(room, bool)`
   trait addition (non-breaking default impl so MockClient compiles).
3. `Conversation`: focus/mount effect sends read receipt (trait addition,
   default no-op).
4. ProfilePanel: consume presence map.
5. `client/Cargo.toml` += `notify-rust`; notification task gated to desktop
   (`cfg!(not(target_arch = "wasm32"))` guard + `ui` desktop feature flag).
6. Settings toggle for notifications out of scope (checkpoint 10 adds it);
   hard-code on, note the TODO.

## Acceptance criteria

- [ ] Typing indicators work in both directions with sensible timeout UX.
- [ ] Viewing a room clears its unread badge and advances receipts.
- [ ] Presence dots match reality for online contacts.
- [ ] Background notifications fire once per message, not for own messages,
      not for the open room.
- [ ] No receipt/typing spam (throttles verified in logs).
- [ ] Mock mode untouched; new trait methods have defaults so MockClient
      compiles unmodified.

## AI implementation prompt

> Implement live state per docs/00 and docs/06. Add client modules for
> incoming typing (per-room Signal<Vec<String>> in ClientState), outgoing
> debounced typing_notice from the composer (4s idle reset, stop on send),
> throttled m.read receipts when a room is viewed/advanced (>=1s throttle),
> presence map signal consumed by ProfilePanel, and desktop notifications via
> notify-rust for messages arriving while the window is unfocused and the
> room isn't open (skip own events; truncate body previews; cfg-gate
> non-wasm). Add narrow trait methods with default no-op impls so MockClient
> still compiles. Verify everything two-way against an Element session.
