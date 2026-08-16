# 09 — Room Discovery & Spaces

## Goal

Complete the navigation model: the existing `DiscoveryModal` browses the real
public room directory (search + pagination) with working Join buttons, and
Matrix Spaces appear in the nav sidebar with their child rooms grouped
underneath.

## Deliverable / how to test

1. Open Discovery (existing modal) → "Rooms" tab lists the homeserver's
   public directory with names/topics/member counts; search filters
   server-side; scrolling loads the next page.
2. Click Join on a public room (use a known busy one, e.g. a Matrix HQ test
   space room) → modal shows joined state, room appears in the nav within
   seconds and is openable.
3. "Spaces" tab lists public spaces similarly.
4. An account belonging to spaces sees them as nav entries; expanding a space
   shows its child rooms; the flat room list gains a section/grouping without
   breaking the existing flat view (toggle or default grouping — pick one,
   document).
5. Leaving a room (right-click or future affordance — minimal UI ok) removes
   it from the list.

## Context

- `chat/discovery_modal.rs` (`DiscoveryTab::Rooms|Spaces`),
  `chat/nav_drawer.rs` (spaces rail exists in the shell per
  `model::Space`), trait methods `public_rooms()`, `public_spaces()`,
  `join_room()`, `spaces()` (stubbed empty since 03).
- SDK: `client.public_rooms(filter, server, limit, since)` paged API (and/or
  `room_directory_search` module in 0.18), join via
  `client.join_room_by_id_or_alias(alias_or_id, &[])`, space hierarchy via
  `room.space_parents()`/`space children state events` (`m.space.child`) —
  traverse from rooms whose `room_type() == RoomType::Space`.
- `model::Space` matches a shallow summary: id, name, avatar(07 media),
  member/child counts.

## Design decisions

- **Directory search becomes paged + query-driven**: change the trait from
  `public_rooms() -> Vec<PublicRoom>` to
  `public_rooms(query: String, batch_token: Option<String>) -> Result<PublicRoomPage, ClientError>`
  (new `PublicRoomPage { rooms: Vec<PublicRoom>, next: Option<String> }`).
  Modal drives it with `use_resource` keyed on query + "load more" button.
  Same for spaces (filter `room_type: m.space` in the directory request where
  supported; else client-filter).
- **Join flow**: `join_room(room_id_or_alias)` → on success the room-list
  stream (03) delivers the room automatically — no manual list merge. Handle
  "already joined" as success. Rate-limit errors (M_LIMIT_EXCEEDED on big
  rooms) surface as a modal inline error with retry-after hint.
- **Spaces in nav**: build space list from synced rooms where
  `room_type == Space`: `Space { id, name, avatar mxc }` + children =
  joined rooms whose ids appear in the space's `m.space.child` state events
  (ordered by the `order` key). `spaces()` returns this snapshot; nav drawer
  renders sections. Space-less rooms fall into an "Ungrouped" bucket so
  nothing disappears.
- **Recursive spaces**: one level only for v1 (space → rooms); nested
  spaces render at top level with a note. Document as limitation.
- **Room preview before join**: if cheap, use `RoomPreview`/directory info to
  show member count & topic in the modal (they come with the directory
  response) — no extra API call needed unless world-readable history preview
  is wanted (defer).

## Implementation steps

1. Trait refactor (rooms/spaces paging + `PublicRoomPage`); update MockClient
   fixtures to honor query/pagination so modal UX can be iterated offline.
2. `client::directory` module wrapping the SDK calls + mapping to
   `PublicRoom/PublicSpace` (member counts, topic, joinability flags).
3. `join_room()` impl (+ leave_room as a thin addition since it's nearly free
   and needed for a sane demo).
4. Spaces computation in the room-list task (03's stream already sees space
   rooms) → `Signal<Vec<Space>>` in `ClientState`; `spaces()` returns it.
5. Nav drawer: spaces section(s) + ungrouped bucket; keep ⌘K switcher flat
   over all rooms.
6. Discovery modal: search field (debounced), paged results, join button with
   pending/error states.

## Acceptance criteria

- [ ] Directory browse/search/paginate works against matrix.org.
- [ ] Join from modal → room appears + opens; already-joined and rate-limit
      errors handled gracefully.
- [ ] Spaces render with grouped children; ungrouped rooms never hidden.
- [ ] Leave flow removes the room.
- [ ] Mock mode exercises search + pagination + spaces layout.

## AI implementation prompt

> Implement discovery + spaces per docs/00 and docs/09. Refactor
> VesperClient public_rooms/public_spaces to paged, query-driven APIs using
> matrix-sdk's public_rooms (filter/room_type for spaces) returning
> PublicRoomPage { items, next_batch }. Implement join_room via
> join_room_by_id_or_alias (already-joined = success, M_LIMIT_EXCEEDED →
> inline retryable error) plus leave_room. Compute spaces in the room-list
> task: rooms with room_type == m.space, children via m.space.child state
> honoring order keys, one level deep; expose as Signal<Vec<Space>>;nav
> groups rooms under spaces with an Ungrouped bucket; switcher stays flat.
> Update DiscoveryModal with debounced search, load-more paging, and join
> button states. Keep MockClient fixtures query/pagination-aware for offline
> UI work.

## Implemented / Deviations (retrospective footer)

**Implemented**: public room directory browse/search (paginated), public
space directory, room join by id/alias with rate-limit + invite-only
error mapping, leave-room, spaces rail grouping rooms by `m.space.child`
order (one level).

**Deviations**:
- Space summaries are cached per room in the sync task; a diff carrying a
  space re-maps only that space, and room→space grouping refreshes only
  rooms whose membership actually moved (checkpoint 11 §C).
- The directory's server-side `m.space` filter falls back to client-side
  filtering on older servers exactly as planned.
