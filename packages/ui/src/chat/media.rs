//! Media resolution hook (checkpoint 07): MXC URI → renderable `data:` URI,
//! memoized app-wide in `ClientState::media`.
//!
//! MXC content is content-addressed, so cached entries never expire; the
//! on-disk half of the cache (SqliteMediaStore) handles restarts, this
//! signal map handles refetch flicker within a session. Resolutions are
//! keyed per MXC *and* requested thumb size (`"{mxc}|{w}x{h}"`, bare
//! `"{mxc}"` for full content).

use std::rc::Rc;

use dioxus::prelude::*;

use crate::data::{ClientState, VesperClient};

fn cache_key(mxc: &str, thumb: Option<(u32, u32)>) -> String {
    match thumb {
        Some((w, h)) => format!("{mxc}|{w}x{h}"),
        None => mxc.to_string(),
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
                        media.write().insert(key, uri);
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
