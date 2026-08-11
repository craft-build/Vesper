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
