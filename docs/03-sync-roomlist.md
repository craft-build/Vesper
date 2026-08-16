# 03 — Sync & Room List

## Goal

Bring the nav drawer and DM/room lists to life against the real account:
initial sync, live updates while running, unread badges, DM vs room
distinction, recency ordering. No message bodies yet.

## Deliverable / how to test

1. Log in with a test account that has several rooms/DMs → nav drawer lists
   them with names, unread counts, and recency ordering matching Element on
   the same account.
2. From a second session (Element mobile/web), send a message into one of
   those rooms → Vesper's list reorders and the unread badge increments
   within ~2s without any interaction.
3. Switcher (⌘K) searches the real room list.
4. Relaunch: room list appears from the SQLite store before network finishes
   (cached, then refreshes).

## Context

- Trait methods: `spaces()` (return `vec![]` until checkpoint 09),
  `conversations()`.
- `ui::data::model::{Convo, ConvoKind, Presence}` are the target shapes; note
  `Convo::is_room`.
- Consumers: `chat/nav_drawer.rs`, `chat/switcher.rs`, `chat/app_shell.rs`.
- SDK 0.18 notes: use `matrix_sdk_ui::room_list_service::RoomListService`
  over sliding sync (default sync; do not hand-roll `/sync`); unread counts
  require the **event cache**: call `client.event_cache().subscribe()` before
  syncing.

## Design decisions

- **RoomListService + all-rooms list**: build via
  `RoomListService::new(client.clone())`, use the `"all_rooms"` list with a
  visible-ranges dynamic filter (start with the default loading state
  machine the service provides). Subscribe to its `entries_stream()`
  (stream of `VectorDiff`s) inside the ClientRuntime.
- **Signal-backed snapshot**: `MatrixClient` owns
  `Signal<Vec<Convo>>` (created in `App`'s scope via a
  `use_context_provider`-compatible handle — see "signal ownership" below) and
  a plain `RwLock<Vec<room_list_service::Room>>` snapshot. A background task
  applies each `VectorDiff` batch to the snapshot, remaps to `Convo`, and
  `.set()`s the signal. `conversations()` clones the snapshot.
- **Signal ownership problem**: signals are scoped to a Dioxus component tree,
  but the sync task lives on the tokio thread. Solution: signals are `Copy`
  handles backed by the generational-box runtime; as long as the owning scope
  (`App`) is alive, other threads may write them via `Signal::set` (dioxus
  signals are thread-safe moveable handles; the write schedules a UI update).
  Pass the signal handles into `MatrixClient` at construction after
  `use_context_provider` runs. Add a code comment documenting this; if a
  panic ever surfaces ("droppable disposed"), fall back to an
  `std::sync::mpsc` → `use_coroutine` drain on the UI side.
- **Mapping**: `Room` → `Convo`:
  - id = `room_id()`, name = `room.compute_display_name()` (falls back to
    heroes/cannonical alias handling the SDK already implements)
  - `ConvoKind::Dm` if `room.is_direct().await` / direct targets nonzero, else
    `Room`
  - unread = `room.num_unread_messages()`, mention flag from
    `num_unread_notifications`/`num_unread_mentions`
  - last activity for sort: latest event timestamp from the event-cache
    latest-events API (0.18: `matrix_sdk::latest_events`), not room state
    iteration.
  - avatar: keep the model's placeholder; real avatars in checkpoint 07.
- **Invites**: add `ConvoKind` mapping for invited rooms if cheap (list filter
  in RoomListService supports invites); if UI has nowhere to show them, log
  count and defer.
- Start sync right after successful login/restore; stop on logout. Handle
  network-loss: RoomListService retries internally; surface a "connecting…"
  flag in `ClientState` shown subtly in the app shell.

## Implementation steps

1. `client/src/sync.rs`: `spawn_room_list(client, convos_signal, status_signal)`
   — builds RoomListService, subscribes diffs, maps, writes signal. Unit-test
   the pure `map_room(&RoomListRoomSnapshot) -> Convo` function with
   `matrix-sdk-test` where feasible.
2. Wire into commands: after `Login`/`Restore` succeeds, call `spawn_room_list`.
3. `VesperClient::conversations()` → snapshot clone. `spaces()` → empty.
4. `ChatView`/nav drawer already re-render off the signal (verify; if they
   currently call `conversations()` once, switch them to a `use_resource`
   reading the signal so updates propagate — this touching of UI is
   acceptable and expected here).
5. Connection status indicator in `AppShell` from `status_signal`.
6. Log one `info!` line per sync batch diff size for debugging.

## Acceptance criteria

- [ ] Real rooms + DMs appear, correctly categorized, ordered by recency.
- [ ] Unread badges match Element; sending from another device updates Vesper live.
- [ ] ⌘K switcher searches real rooms.
- [ ] Cold relaunch shows cached list in <1s (SQLite store warm start).
- [ ] Logging out and back in does not duplicate/stale the list.
- [ ] Mock backend unaffected.

## AI implementation prompt

> Implement live room-list sync in Vesper per docs/00 and docs/03. Use
> matrix-sdk-ui 0.18 RoomListService ("all_rooms" list) with the event cache
> enabled (client.event_cache().subscribe(), required for unread counts in
> 0.18). In packages/client, spawn a sync task on the ClientRuntime that
> applies VectorDiffs to a snapshot and writes a Signal<Vec<Convo>> passed in
> from the Dioxus App scope. Map rooms to ui::data::model::Convo (DM via
> room.is_direct, unread via num_unread_messages, recency via the
> latest-events API). Implement conversations()/spaces() on MatrixClient,
> start sync after login/restore, add a connecting-status signal surfaced in
> AppShell, and make nav drawer + switcher reactive to the signal. Verify
> live updates by messaging from a second session. Keep mock mode working.

## Implemented / Deviations (retrospective footer)

**Implemented**: `SyncService` (MSC4186 simplified sliding sync) driving a
`RoomListService`, `VectorDiff` stream → `Vec<Convo>` published into
sync-storage Dioxus signals from the runtime thread, offline mode with
"connecting…" surfaced via the `connecting` signal.

**Deviations**:
- **No local sort**: docs/03 predicted a latest-event sort; the diff API
  in 0.18 doesn't expose timestamps per diff, so ordering is sliding
  sync's bump order applied faithfully (documented in `sync.rs`).
- **Diff application became O(batch) (checkpoint 11 §C).** Rows now pair
  each `RoomListItem` with its mapped `Convo`; a batch maps only the
  values it carries, and DM presence dots refresh via a presence-change
  dirty set instead of full-list remaps. Original design remapped the
  entire list every batch.
- Invites are still not rendered (no public accessor in 0.18; unchanged
  from the deferral noted above).
- The signal-ownership concern (off-thread writes needing App-scope,
  SyncStorage signals) landed exactly as the doc's "Signal ownership
  problem" section prescribed.
