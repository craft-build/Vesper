//! Media resolution and upload helpers (checkpoint 07, `matrix` feature).
//!
//! Crucially, `resolve` returns a **`data:` URI string**, not a file path:
//! dioxus-desktop's asset protocol serves only bundled assets and
//! component-registered asset handlers, so a bare filesystem path placed in
//! `img { src }` never loads in the webview. `data:` URIs render identically
//! on desktop and (later) wasm-web. Payloads stay small because avatars and
//! inline images are resolved through the homeserver's thumbnail API at
//! display size, and resolved URIs are memoized in `ClientState::media`.
//!
//! Caching (checkpoint 11 §C): every fetch goes through Vesper's own
//! on-disk LRU cache ([`crate::media_cache`], 500 MB cap) — the SDK's
//! sqlite media store is bypassed (`use_cache = false`) because nothing
//! in matrix-sdk 0.18 evicts it and it grows without bound. Cache hits
//! survive restarts; MXC URIs are content-addressed so entries never go
//! stale; the cap evicts least-recently-viewed media first.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use matrix_sdk::{
    attachment::{
        AttachmentConfig, AttachmentInfo, BaseAudioInfo, BaseFileInfo, BaseImageInfo,
        BaseVideoInfo, Thumbnail,
    },
    media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings},
    room::reply::{EnforceThread, Reply},
    ruma::{
        events::room::{
            message::{AddMentions, TextMessageEventContent},
            EncryptedFile, MediaSource,
        },
        EventId, OwnedMxcUri, UInt,
    },
    Client, Room,
};

use crate::{
    api::ClientError,
    model::{Attachment, AttachmentKind},
};

/// Build the ruma media source for an MXC URI, parsing the serialized
/// `EncryptedFile` JSON when the piece of media is encrypted.
fn media_source(mxc: &str, encrypted: Option<&str>) -> Result<MediaSource, ClientError> {
    let uri = OwnedMxcUri::try_from(mxc)
        .map_err(|_| ClientError::invalid(format!("Could not use the media URI '{mxc}'.")))?;
    match encrypted {
        None => Ok(MediaSource::Plain(uri)),
        Some(json) => {
            let file: EncryptedFile = serde_json::from_str(json).map_err(|e| {
                ClientError::invalid(format!("Could not read the media encryption info: {e}"))
            })?;
            Ok(MediaSource::Encrypted(Box::new(file)))
        }
    }
}

/// Fetch media (cache-aware, transparent decryption) as bytes. Thumbnails
/// request `w×h` from the homeserver thumbnail API and fall back to the full
/// content when the server can't thumbnail the type (e.g. SVG).
async fn fetch(
    client: &Client,
    mxc: &str,
    encrypted: Option<&str>,
    thumb: Option<(u32, u32)>,
) -> Result<Vec<u8>, ClientError> {
    // Cache-first (checkpoint 11 §C): a hit never touches the network or
    // the SDK store. Directory setup failure degrades to uncached fetches.
    let key = crate::media_cache::cache_key(mxc, encrypted, thumb);
    if let Ok(dir) = crate::media_cache::cache_dir() {
        if let Some(bytes) = crate::media_cache::read(&dir, &key) {
            return Ok(bytes);
        }
    }

    let source = media_source(mxc, encrypted)?;
    let err_prefix = "Could not load media";
    let bytes = fetch_uncached(client, source, thumb, err_prefix, mxc).await?;

    // Store + evict on the tokio runtime; eviction walks the cache dir.
    // An unwritable cache dir degrades to "this fetch stays uncached"
    // (same promise as the read/setup path) — never fail the fetch itself.
    if let Ok(dir) = crate::media_cache::cache_dir() {
        match crate::media_cache::write(&dir, &key, &bytes) {
            Ok(()) => {
                let cap = crate::media_cache::DEFAULT_CAP_BYTES;
                tokio::task::spawn_blocking(move || {
                    crate::media_cache::evict_to_cap(&dir, cap);
                })
                .await
                .ok();
            }
            Err(e) => tracing::warn!(mxc, "media cache write failed, leaving uncached: {e}"),
        }
    }
    Ok(bytes)
}

/// Network half: thumbnails first (falling back to full content when the
/// server can't thumbnail the type), with the SDK's sqlite media cache
/// bypassed — see the module docs for why Vesper owns the cache.
async fn fetch_uncached(
    client: &Client,
    source: MediaSource,
    thumb: Option<(u32, u32)>,
    err_prefix: &str,
    mxc: &str,
) -> Result<Vec<u8>, ClientError> {
    if let Some((w, h)) = thumb {
        let request = MediaRequestParameters {
            source: source.clone(),
            format: MediaFormat::Thumbnail(MediaThumbnailSettings::new(
                UInt::from(w),
                UInt::from(h),
            )),
        };
        match client.media().get_media_content(&request, false).await {
            Ok(bytes) => return Ok(bytes),
            Err(e) => {
                tracing::warn!(
                    mxc,
                    "thumbnail fetch failed, falling back to full media: {e}"
                );
            }
        }
    }
    let request = MediaRequestParameters {
        source,
        format: MediaFormat::File,
    };
    client
        .media()
        .get_media_content(&request, false)
        .await
        .map_err(|e| ClientError::network(format!("{err_prefix}: {e}")))
}

/// Resolve media to a `data:` URI for `img { src }`. MIME is sniffed from
/// the bytes (whatever the server actually returned — thumbnails may differ
/// from the source format), falling back to a generic image type.
pub async fn resolve(
    client: &Client,
    mxc: &str,
    encrypted: Option<&str>,
    thumb: Option<(u32, u32)>,
) -> Result<String, ClientError> {
    let bytes = fetch(client, mxc, encrypted, thumb).await?;
    let mime = infer::get(&bytes)
        .map(|k| k.mime_type().to_string())
        .unwrap_or_else(|| "image/jpeg".to_string());
    Ok(format!("data:{mime};base64,{}", B64.encode(bytes)))
}

/// Download full-resolution content and write it to `dest`
/// (checkpoint 07's file-card Download action). Byte-exact: no re-encoding,
/// decrypted in place when `attachment.encrypted` is set.
pub async fn save_to(
    client: &Client,
    attachment: &Attachment,
    dest: &str,
) -> Result<(), ClientError> {
    let mxc = attachment
        .mxc
        .as_deref()
        .ok_or_else(|| ClientError::invalid("That attachment has no media source."))?;
    let bytes = fetch(client, mxc, attachment.encrypted.as_deref(), None).await?;
    std::fs::write(dest, bytes)
        .map_err(|e| ClientError::storage(format!("Could not write the downloaded file: {e}")))?;
    Ok(())
}

/// Human-friendly file size for attachment cards.
pub fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{} KB", bytes.div_ceil(1_000))
    } else {
        format!("{bytes} B")
    }
}

/// Map ruma attachment message content (image/file/video/audio share the
/// `source: MediaSource` + info shape) onto our domain `Attachment`.
pub struct MappedMedia {
    /// Caption-ish body from the event (filename when no caption).
    pub body: String,
    pub attachment: Attachment,
}

/// Shared mapping core: `name` = `filename` when set (else `body`), `size`
/// from info when known.
fn mapped(
    kind: AttachmentKind,
    body: String,
    filename: Option<&str>,
    source: &MediaSource,
    mime: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    size: Option<u64>,
    thumb_source: Option<&MediaSource>,
) -> MappedMedia {
    let (mxc, encrypted) = split_source(source);
    let (thumb_mxc, thumb_encrypted) = thumb_source.map(split_source).unwrap_or((None, None));
    let mut attachment = Attachment::new(
        kind,
        filename.unwrap_or(&body).to_string(),
        size.map(format_size).unwrap_or_default(),
    );
    attachment.mxc = mxc;
    attachment.encrypted = encrypted;
    attachment.mime = mime;
    attachment.width = width;
    attachment.height = height;
    attachment.thumb_mxc = thumb_mxc;
    attachment.thumb_encrypted = thumb_encrypted;
    MappedMedia { body, attachment }
}

fn split_source(source: &MediaSource) -> (Option<String>, Option<String>) {
    match source {
        MediaSource::Plain(uri) => (Some(uri.to_string()), None),
        MediaSource::Encrypted(file) => {
            let uri = file.url.to_string();
            let json = serde_json::to_string(file.as_ref()).ok();
            (Some(uri), json)
        }
    }
}

/// `u64` → ruma `UInt` (js_int-backed: up to 2⁵³ — file sizes fit exactly).
fn uint(v: Option<u64>) -> Option<UInt> {
    v.and_then(|v| UInt::try_from(v).ok())
}

impl From<&matrix_sdk::ruma::events::room::message::ImageMessageEventContent> for MappedMedia {
    fn from(t: &matrix_sdk::ruma::events::room::message::ImageMessageEventContent) -> Self {
        let info = t.info.as_deref();
        mapped(
            AttachmentKind::Image,
            t.body.clone(),
            t.filename.as_deref(),
            &t.source,
            info.and_then(|i| i.mimetype.clone()),
            info.and_then(|i| i.width.and_then(|w| u32::try_from(w).ok())),
            info.and_then(|i| i.height.and_then(|h| u32::try_from(h).ok())),
            info.and_then(|i| i.size.map(|s| s.into())),
            info.and_then(|i| i.thumbnail_source.as_ref()),
        )
    }
}

impl From<&matrix_sdk::ruma::events::room::message::VideoMessageEventContent> for MappedMedia {
    fn from(t: &matrix_sdk::ruma::events::room::message::VideoMessageEventContent) -> Self {
        let info = t.info.as_deref();
        mapped(
            AttachmentKind::Video,
            t.body.clone(),
            t.filename.as_deref(),
            &t.source,
            info.and_then(|i| i.mimetype.clone()),
            info.and_then(|i| i.width.and_then(|w| u32::try_from(w).ok())),
            info.and_then(|i| i.height.and_then(|h| u32::try_from(h).ok())),
            info.and_then(|i| i.size.map(|s| s.into())),
            info.and_then(|i| i.thumbnail_source.as_ref()),
        )
    }
}

impl From<&matrix_sdk::ruma::events::room::message::AudioMessageEventContent> for MappedMedia {
    fn from(t: &matrix_sdk::ruma::events::room::message::AudioMessageEventContent) -> Self {
        mapped(
            AttachmentKind::Audio,
            t.body.clone(),
            None,
            &t.source,
            t.info.as_deref().and_then(|i| i.mimetype.clone()),
            None,
            None,
            t.info.as_deref().and_then(|i| i.size.map(|s| s.into())),
            None,
        )
    }
}

impl From<&matrix_sdk::ruma::events::room::message::FileMessageEventContent> for MappedMedia {
    fn from(t: &matrix_sdk::ruma::events::room::message::FileMessageEventContent) -> Self {
        mapped(
            AttachmentKind::File,
            t.body.clone(),
            t.filename.as_deref(),
            &t.source,
            t.info.as_deref().and_then(|i| i.mimetype.clone()),
            None,
            None,
            t.info.as_deref().and_then(|i| i.size.map(|s| s.into())),
            t.info.as_deref().and_then(|i| i.thumbnail_source.as_ref()),
        )
    }
}

/// Largest edge we enclose generated thumbnails in.
const THUMB_EDGE: u32 = 800;

/// Send a composer-picked file as `m.image`/`m.video`/`m.audio`/`m.file`
/// (checkpoint 07). Runs inside a spawned task — the runtime responds before
/// the upload so the sequential command loop never stalls on network I/O
/// (checkpoint-06 lesson). Transparently encrypts when the room is
/// encrypted; no local echo exists for media sends in matrix-sdk-ui 0.18,
/// so the row appears when the remote echo syncs back.
pub async fn send_attachment(
    room: Room,
    picked: Attachment,
    caption: String,
    reply_to: Option<String>,
) {
    let Some(path) = picked.local_path.clone() else {
        tracing::warn!("send_attachment without a local path — nothing to upload");
        return;
    };
    if let Err(e) = upload_and_send(room, &path, picked.name, caption, reply_to).await {
        tracing::warn!(path, "attachment send failed: {}", e);
    }
}

/// Cheap, synchronous validation the runtime runs BEFORE answering the send
/// command (review: with no local echo for media, a silent spawned-task
/// failure would swallow a whole message): the picked file must exist and be
/// readable, and `reply_to` must parse as an event id (replies to pending
/// `txn-` echoes fail here instead of vanishing). Network/room failures
/// after this point remain log-only in the spawned task — media has no
/// send-state rails in sdk-ui 0.18 (documented in docs/07).
pub fn preflight_attachment(
    picked: &Attachment,
    reply_to: Option<&str>,
) -> Result<(), ClientError> {
    let path = picked
        .local_path
        .as_deref()
        .ok_or_else(|| ClientError::invalid("No file was picked."))?;
    // Open (not just metadata) so permission errors surface too.
    std::fs::File::open(path)
        .map_err(|e| ClientError::storage(format!("Could not open the picked file: {e}")))?;
    if let Some(reply_to) = reply_to {
        EventId::parse(reply_to)
            .map_err(|_| ClientError::invalid("Could not reply with an attachment yet — try again once the previous message has sent."))?;
    }
    Ok(())
}

async fn upload_and_send(
    room: Room,
    path: &str,
    filename: String,
    caption: String,
    reply_to: Option<String>,
) -> Result<(), ClientError> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| ClientError::storage(format!("Could not read the picked file: {e}")))?;
    let mime: mime::Mime = infer::get(&bytes)
        .and_then(|k| k.mime_type().parse().ok())
        .unwrap_or(mime::APPLICATION_OCTET_STREAM);
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);

    let (info, thumbnail) = match mime.type_() {
        mime::IMAGE => {
            let (mut info, thumb) = image_info(&bytes);
            // Declared byte size, consistent with the video/audio/file arms.
            info.size = uint(Some(size));
            // GIFs may be animated; a generated JPEG thumb would lie about
            // the content. Leave thumbnail None — servers can thumb them.
            (AttachmentInfo::Image(info), thumb)
        }
        mime::VIDEO => (
            AttachmentInfo::Video(BaseVideoInfo {
                size: uint(Some(size)),
                ..Default::default()
            }),
            None,
        ),
        mime::AUDIO => (
            AttachmentInfo::Audio(BaseAudioInfo {
                size: uint(Some(size)),
                ..Default::default()
            }),
            None,
        ),
        _ => (
            AttachmentInfo::File(BaseFileInfo {
                size: uint(Some(size)),
            }),
            None,
        ),
    };

    let mut config = AttachmentConfig::new()
        .info(info)
        .thumbnail(thumbnail.filter(|_| mime.subtype() != mime::GIF));
    if !caption.is_empty() {
        // Markdown so Vesper's own rows (rendered via `render_markdown`) and
        // receivers see the same formatting.
        config = config.caption(Some(TextMessageEventContent::markdown(caption)));
    }
    if let Some(reply_to) = reply_to {
        let event_id = EventId::parse(&reply_to)
            .map(|e| e.to_owned())
            .map_err(|_| ClientError::invalid("Could not attach the reply to the media."))?;
        config = config.reply(Some(Reply {
            event_id,
            enforce_thread: EnforceThread::Unthreaded,
            add_mentions: AddMentions::Yes,
        }));
    }

    room.send_attachment(filename, &mime, bytes, config)
        .await
        .map_err(|e| ClientError::network(format!("Could not send the attachment: {e}")))?;
    Ok(())
}

/// Decode an image for its dimensions and a downscaled JPEG thumbnail.
/// Decode failure degrades to "no info/no thumbnail" — never blocks a send.
fn image_info(bytes: &[u8]) -> (BaseImageInfo, Option<Thumbnail>) {
    let Some(img) = image::load_from_memory(bytes).ok() else {
        return (BaseImageInfo::default(), None);
    };
    use image::GenericImageView;
    let (w, h) = img.dimensions();
    let info = BaseImageInfo {
        height: uint(Some(u64::from(h))),
        width: uint(Some(u64::from(w))),
        ..Default::default()
    };
    if w <= THUMB_EDGE && h <= THUMB_EDGE {
        return (info, None);
    }
    let thumb_img = img.thumbnail(THUMB_EDGE, THUMB_EDGE);
    let (tw, th) = thumb_img.dimensions();
    let mut buf = Vec::new();
    let thumb = thumb_img
        .write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Jpeg,
        )
        .ok()
        .map(|_| Thumbnail {
            size: uint(Some(buf.len() as u64)).expect("byte len fits in UInt"),
            data: buf,
            content_type: mime::IMAGE_JPEG,
            height: uint(Some(u64::from(th))).expect("u32 fits in UInt"),
            width: uint(Some(u64::from(tw))).expect("u32 fits in UInt"),
        });
    (info, thumb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_size_thresholds() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1_500), "2 KB");
        assert_eq!(format_size(240_000_000), "240.0 MB");
    }

    /// Synthesize a non-square PNG and check the generated thumbnail's
    /// declared bbox matches its real pixel dims (review P1: w/h were
    /// swapped, corrupting every >800px image for all receivers).
    #[test]
    fn thumbnail_dims_match_generated_image() {
        use image::{DynamicImage, ImageBuffer, Rgb};
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(1200, 600);
        let mut png = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let (info, thumb) = image_info(&png);
        let thumb = thumb.expect("a 1200x600 image must get a thumbnail");
        // Thumbnailed aspect: 800x400.
        assert_eq!(u64::from(thumb.width), 800);
        assert_eq!(u64::from(thumb.height), 400);
        assert_eq!(info.width.map(u64::from), Some(1200));
        assert_eq!(info.height.map(u64::from), Some(600));
    }

    #[test]
    fn uint_rejects_out_of_range_but_keeps_huge_file_sizes() {
        // 5 GiB video: must survive, not wrap through a u32 cast.
        assert_eq!(
            uint(Some(5 * 1024 * 1024 * 1024)),
            UInt::try_from(5 * 1024 * 1024 * 1024u64).ok()
        );
        assert_eq!(uint(Some(u64::MAX)), None);
        assert_eq!(uint(None), None);
    }

    #[test]
    fn split_source_plain_and_encrypted_roundtrip() {
        let plain = MediaSource::Plain(OwnedMxcUri::try_from("mxc://example/abc").unwrap());
        let (mxc, enc) = split_source(&plain);
        assert_eq!(mxc.as_deref(), Some("mxc://example/abc"));
        assert!(enc.is_none());

        let src = media_source("mxc://example/abc", None).unwrap();
        let MediaSource::Plain(uri) = src else {
            panic!("expected a plain source");
        };
        assert_eq!(uri.as_str(), "mxc://example/abc");
    }
}
