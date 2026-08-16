//! Media resolution hook (checkpoint 07): MXC URI → renderable `data:` URI,
//! memoized app-wide in `ClientState::media`.
//!
//! MXC content is content-addressed, so cached entries never expire; the
//! on-disk half of the cache (`client::media_cache`, checkpoint 11) handles
//! restarts with a 500 MB LRU cap, this signal map handles refetch flicker
//! within a session. Resolutions are keyed per MXC *and* requested thumb
//! size (`"{mxc}|{w}x{h}"`, bare `"{mxc}"` for full content).
//!
//! The in-memory map is itself capped (checkpoint 11 §C): data URIs are
//! whole media payloads in RAM, so a long session scrolling large rooms
//! would otherwise grow without bound. A FIFO/LRU order tracker evicts the
//! oldest resolved entries past a byte budget (in-flight sentinels are
//! zero-byte and never evicted while pending).

use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Mutex;

use dioxus::prelude::*;

use crate::data::{ClientState, VesperClient};

/// Memory budget for resolved data URIs (checkpoint 11 §C). Generous —
/// thousands of avatars/thumbnails — but bounded.
const MEM_CAP_BYTES: usize = 192 * 1024 * 1024;

/// FIFO bookkeeping for the media map's byte cap. The map signal is only
/// written from the UI thread's async runtime, so a plain static Mutex is
/// race-free in practice (and correct regardless).
static MEM_LRU: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

fn cache_key(mxc: &str, thumb: Option<(u32, u32)>) -> String {
    match thumb {
        Some((w, h)) => format!("{mxc}|{w}x{h}"),
        None => mxc.to_string(),
    }
}

/// Record an insert and enforce the byte cap, evicting the oldest resolved
/// entries (never empty sentinels — they're in-flight fetch markers, and
/// never the entry that triggered the sweep).
fn enforce_mem_cap(
    media: &mut Signal<std::collections::BTreeMap<String, String>, SyncStorage>,
    inserted: &str,
) {
    let mut map = media.write();
    let total: usize = map.values().map(|v| v.len()).sum();
    if total <= MEM_CAP_BYTES {
        return;
    }
    let mut order = MEM_LRU.lock().unwrap_or_else(|e| e.into_inner());
    // Push keys we haven't seen (first insert wins; re-inserts keep the
    // original position — a re-resolved key is rare and old-position is
    // still a valid eviction order).
    if !order.iter().any(|k| k == inserted) {
        order.push_back(inserted.to_string());
    }
    let mut budget = total;
    while budget > MEM_CAP_BYTES {
        let Some(candidate) = order.front().cloned() else {
            break;
        };
        if candidate == *inserted {
            break; // never evict the entry that triggered this sweep
        }
        if let Some(uri) = map.get(&candidate) {
            if uri.is_empty() {
                break; // sentinel ordering guard; skip forward instead? keep simple
            }
            budget -= uri.len();
            map.remove(&candidate);
        }
        order.pop_front();
    }
}

/// Resolve media to a `data:` URI string for `img { src }`.
///
/// `None` means "not resolved (yet), or the backend can't do media" —
/// callers keep their designed fallback (initials / icon card) and repaint
/// when the URI lands: cache hits are read at render time, so the component
/// is subscribed to the shared media map, and the resolution task writes
/// into that map.
///
/// The fetch is kicked off once on mount; a mounted instance whose `mxc`
/// prop *changes* re-checks the cache on that re-render but does not
/// re-fetch — media-bearing rows are keyed by stable ids in every current
/// call site (nav rows, message rows, profile panel). Concurrent mounts of
/// the same MXC collapse onto one fetch via an empty-string sentinel left
/// in the cache map while in flight (removed on error so a later mount
/// retries).
pub fn use_media_src(
    mxc: Option<String>,
    encrypted: Option<String>,
    thumb: Option<(u32, u32)>,
) -> Option<String> {
    let state = use_context::<ClientState>();
    let client = use_context::<Rc<dyn VesperClient>>();

    // Kick off one resolution task per component instance. The
    // `state.media` reads below happen at render time — writing into that
    // map is what repaints this component (and peers) on success.
    use_effect({
        let mut media_signal = state.media;
        let mxc = mxc.clone();
        let encrypted = encrypted.clone();
        move || {
            let Some(mxc) = mxc.clone() else { return };
            let key = cache_key(&mxc, thumb);
            {
                let mut map = media_signal.write();
                if map.contains_key(&key) {
                    // Resolved already, or another instance's fetch is in
                    // flight (empty-string sentinel) — either way, do not
                    // spawn a duplicate fetch of the same bytes.
                    return;
                }
                map.insert(key.clone(), String::new());
            }
            let client = client.clone();
            let encrypted = encrypted.clone();
            let mut media = media_signal;
            spawn(async move {
                match client.media_uri(&mxc, encrypted, thumb).await {
                    Ok(uri) => {
                        media.write().insert(key.clone(), uri);
                        enforce_mem_cap(&mut media, &key);
                    }
                    Err(e) => {
                        tracing::warn!("media {mxc}: {e}");
                        // Drop the sentinel so a later mount retries.
                        media.write().remove(&key);
                    }
                }
            });
        }
    });

    // An empty-string entry is an in-flight placeholder, not a URI.
    mxc.map(|mxc| {
        state
            .media
            .read()
            .get(&cache_key(&mxc, thumb))
            .filter(|uri| !uri.is_empty())
            .cloned()
    })?
}
