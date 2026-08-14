# 07 — Media & Attachments

## Goal

Real images everywhere Matrix has them: user/room avatars rendered from
`mxc://` URIs, image messages displayed inline (with thumbnails), and the
composer's attachment flow sending real uploads (images + arbitrary files)
with progress and correct `m.image`/`m.file` events.

## Deliverable / how to test

1. Room list / message rows / profile panel show real avatars (initials
   fallback remains when none set).
2. A room with image history (send images from Element first) renders them
   inline; thumbnail loads fast, click shows full-size (lightbox can be
   minimal or out-of-scope — document the choice).
3. Drag/paste or pick an image in Vesper's composer → caption + send →
   Element sees a proper caption+image message with correct dimensions.
4. Send a non-image file → Element shows a downloadable `m.file`; Vesper row
   shows filename/size with a working Download action.
5. Relaunch: avatars/images come from the media cache, not network.

## Context

- `design_system/avatar.rs` currently takes placeholder/initials props —
  extend it with an optional `mxc` source without breaking existing uses.
- `model::Attachment { kind: AttachmentKind, ... }` gains what it needs:
  mxc uri, mime, size, width/height, thumbnail_uri.
- Composer already has `send_message(..., attachment: Option<Attachment>, ...)`.
- SDK: `client.media()` — `get_media_content`/`get_media_thumbnail`
  (`MediaRequestParameters`); uploads via `client.media().upload(mime,
  reader/data)` returning MXC; send with
  `RoomMessageEventContent::new(MessageType::Image(ImageMessageEventContent`
  with `info` incl. `ThumbnailInfo`)). Use the SDK's `SqliteMediaStore`
  (feature `sqlite`, already on) via
  `ClientBuilder` media-store configuration for caching.

## Design decisions

- **Media resolution lives in the client crate**: trait addition
  `async fn media_uri(&self, mxc: &str, thumb: Option<(u32,u32)>) -> Result<String, ClientError>`
  returning a **local cache file path** (from the media store / temp dir) that
  Dioxus can render via `img { src: path }` on desktop. (Web would need blob
  URLs instead — hide that difference behind this method when web revives in
  checkpoint 11.)
- **Avatar pipeline**: room/member mapping extensions: `room.avatar_url()` /
  sender profile `avatar_url` are MXC strings → components call `media_uri
  (mxc, Some(48,48))` through a small `use_resource`-based hook; cache
  resolved paths in a `HashMap<OwnedMxcUri, String>` in `ClientState` to
  avoid refetch flicker.
- **Upload flow**: composer picks a file (native dialog: `rfd` crate on
  desktop), reads bytes + infers mime (`infer` crate), computes thumbnail for
  images (`image` crate, max 800px JPEG/PNG), uploads media, builds event
  with `info` (w/h, size, mime, thumbnail), sends via existing send path so
  local echo/progress ride checkpoint 05's rails. Progress: SDK upload is
  one-shot; show indeterminate progress, fine for v1.
- **Encrypted-room media**: attachment encryption (`mxc` in `file` field with
  JWK key) is a matrix-sdk `AttachmentEncryptor` concern — implement now if
  the room is encrypted using the SDK's attachment helpers, since a client
  that can't send images into encrypted rooms fails most real usage. If SDK
  helper ergonomics are poor, scope-limit to unencrypted uploads and mark
  encrypted-media as the top follow-up under this doc.
- **Download**: `save_file` dialog (`rfd`) → media content → write to disk.

## Implementation steps

1. Wire `SqliteMediaStore`/media cache in `ClientBuilder` (client/src/session.rs).
2. Trait: `media_uri`, `send_attachment(...)` (or extend `Attachment` +
   reuse send_message), `download_attachment(...)`.
3. `Avatar` component: accept `mxc: Option<String>` prop + hook; update the 3
   call sites (nav rows, message row, profile panel).
4. Timeline mapping: `m.image`/`m.file`/`m.video`/`m.audio` → `Attachment` on
   `Message`; MessageRow renders inline image / file card.
5. Composer: file picker + caption + send; file-card rendering for non-image.
6. Download button on file cards (desktop, `rfd` save dialog).
7. Cache-bust correctness: MXC URIs are content-addressed — safe to cache
   forever; document that assumption.

## Acceptance criteria

- [ ] Avatars render for users and rooms that have them; initials fallback OK.
- [ ] Inline image messages with correct aspect; thumbnails via homeserver
      thumbnail API.
- [ ] Image send shows correctly in Element (caption, dimensions, thumbnail).
- [ ] File send/download round-trips bytes exactly (hash-check a 5MB file).
- [ ] Media cache persists across restarts.
- [ ] Encrypted-room image send either works (AttachmentEncryptor) or is
      explicitly documented as scoped out with a tracking note in this doc.

## AI implementation prompt

> Implement media per docs/00 and docs/07. Enable SqliteMediaStore in the
> client builder. Add VesperClient methods media_uri (mxc + optional
> thumbnail size → cached local file path via client.media()), attachment
> sending (read file via rfd picker, infer mime, generate ≤800px thumbnail
> with the image crate, upload via media().upload, send m.image/m.file with
> full info incl. dimensions/thumbnail), and download_attachment (rfd save
> dialog). Extend model::Attachment with mxc/mime/size/dims/thumbnail_uri,
> map m.image/m.file/\* msgtypes in the timeline mapper, update Avatar to
> take optional mxc and be consumed by nav rows, message rows, and profile
> panel via a shared cached lookup. For encrypted rooms use the SDK
> attachment encryption helpers; if blocked, scope-limit and document. Verify
> byte-exact round trip and Element interop.

## Implementation notes (as built)

- **Store wiring needed no code**: `ClientBuilder::sqlite_store` already
  opens a `SqliteMediaStore` alongside the state/event-cache stores
  (builder/mod.rs in matrix-sdk 0.18). All fetches go through
  `Media::get_media_content(.., use_cache = true)`, so the persistence
  criterion ("relaunch → cache, not network") is satisfied by construction.
- **Data-URI, not file paths (deviation)**: dioxus-desktop 0.7's asset
  protocol only serves bundled assets and component-registered asset
  handlers — a bare filesystem path in `img { src }` never loads. Every
  resolver therefore returns `data:{mime};base64,…`, sized small by using
  the homeserver thumbnail API at display size (avatars 128px, inline
  images ≤800px). This also collapses the planned native/web divergence:
  data URIs work identically on the wasm target, so the checkpoint-11
  blob-URL work is unnecessary unless payload size becomes an issue.
  Thumbnail fetch failures fall back to full content (e.g. SVG — the
  thumbnail API can't scale it server-side).
- **No local echo for attachments**: matrix-sdk-ui 0.18's
  `send_attachment` "does not currently support local echoes" — sent media
  appears in the conversation only when the remote echo syncs back.
  Checkpoint-05's send-state/retry rails deliberately do not apply. So a
  silent spawned-task failure would swallow a whole message: the runtime
  therefore runs a synchronous preflight before answering the send command
  (picked file must open, `reply_to` must parse an event id — replies to
  pending `txn-` echoes fail *visibly* instead of vanishing). Failures
  after preflight (network, room errors) remain `tracing::warn`ed, as a
  media send has no failed-row to repaint; surfacing those is a follow-up
  (e.g. sdk-ui's experimental send-queue media echo). Uploads run in a
  spawned task off the sequential runtime command loop (the checkpoint-06
  blocking lesson).
- **Encryption is transparent**: `Room::send_attachment(..).store_in_cache()`
  encrypts upload+thumbnail automatically in encrypted rooms; downloads and
  thumbnail fetches decrypt via `MediaSource::Encrypted`. The encrypted
  `EncryptedFile` blob crosses the trait seam as serialized JSON
  (`Attachment.encrypted` / `thumb_encrypted`) to keep ruma types out of
  the UI. So the "encrypted media as top follow-up" escape hatch was not
  needed — it works by construction.
- **Compose flow**: picker only (rfd native dialog, sync, on the UI thread
  — a dialog on the backend's tokio thread would be wrong; drag/paste
  remains a follow-up). Kind (image vs file) is a filename-extension
  heuristic at pick time; the real mime is sniffed from bytes with `infer`
  at send. Thumbnails: ≤800px JPEG via the `image` crate (features jpeg /
  png / webp only); animated GIFs intentionally get no static thumbnail
  (servers can thumb them). Caption = composer text, sent through
  `AttachmentConfig.caption` (body differs from filename → Element
  renders it as a caption).
- **Rendering**: `ClientState::media` (App-scope `Signal<BTreeMap>`) memoizes
  resolved data URIs; a small `use_media_src` hook reads the cache at
  render time (subscribing each consumer) and kicks off resolution once on
  mount. MXC content is content-addressed, so the map never invalidates —
  the memory bound is a checkpoint-11 candidate (`Media::clean()`/retention
  policy exists). Avatars resolve at 128px. `MessageRow` renders inline
  images inside an aspect-ratio box from `info.{w,h}` (no layout jump) with
  a placeholder icon while loading; clicking the image downloads full-size
  — **no lightbox in v1** (scoped out deliberately).
- **Downloads**: `MessageRow` file cards (and the image click) → rfd save
  dialog on the UI thread → `save_attachment(convo_id, attachment, dest)`
  writes the full-resolution bytes unchanged. Byte-exactness holds: no
  re-encode on the download path.
- **Mapped but not interactive**: `m.video`/`m.audio` render as file cards
  (kinds `Video`/`Audio` exist for icons/labels; no players). Stickers,
  galleries (msc4274), and thread-panel reply avatars stay unmapped.
