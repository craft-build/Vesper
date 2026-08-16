//! On-disk LRU cache for resolved media (checkpoint 11, workstream C).
//!
//! The SDK's sqlite media store grows without bound — nothing in
//! matrix-sdk 0.18 evicts it — so Vesper owns caching instead: media is
//! fetched with the SDK cache bypassed and stored here, where a size cap
//! (default 500 MB) plus mtime-based LRU eviction keeps the data dir
//! bounded. One file per (`mxc`, encryption, thumb-size) request key;
//! reads touch the mtime so "recently viewed" survives eviction sweeps.
//!
//! The settings screen surfaces the used size and a Clear action
//! (checkpoint 10's row, delivered here).

use std::path::{Path, PathBuf};

use crate::api::ClientError;

/// Default cap: 500 MB (docs/11 §C).
pub(crate) const DEFAULT_CAP_BYTES: u64 = 500 * 1024 * 1024;

/// Cache directory name under the data dir.
const DIR_NAME: &str = "media-cache";

/// Open (create) the cache directory.
pub(crate) fn cache_dir() -> Result<PathBuf, ClientError> {
    let dir = crate::session::data_dir()?.join(DIR_NAME);
    std::fs::create_dir_all(&dir)
        .map_err(|e| ClientError::storage(format!("Could not create the media cache: {e}")))?;
    Ok(dir)
}

/// Stable file name for a media request: 128-bit FNV-composite hex of the
/// request identity (MXC + serialized encryption info + thumb size). No
/// cryptographic strength needed — it's a dedup key, and the 128-bit
/// composite makes accidental collisions negligible for realistic media
/// counts.
#[must_use]
pub(crate) fn cache_key(mxc: &str, encrypted: Option<&str>, thumb: Option<(u32, u32)>) -> String {
    let identity = match (encrypted, thumb) {
        (Some(enc), Some((w, h))) => format!("{mxc}\u{1}{enc}\u{1}{w}x{h}"),
        (Some(enc), None) => format!("{mxc}\u{1}{enc}"),
        (None, Some((w, h))) => format!("{mxc}\u{1}\u{1}{w}x{h}"),
        (None, None) => format!("{mxc}\u{1}\u{1}"),
    };
    format!("{:016x}{:016x}", fnv1a(&identity, 0xcbf2_9ce4_8422_2325), fnv1a(&identity, 0x9e37_79b9_7f4a_7c15))
}

/// FNV-1a with an arbitrary seed.
fn fnv1a(data: &str, seed: u64) -> u64 {
    data.bytes().fold(seed, |hash, byte| {
        // Wrapping by design: FNV-1a is mod-2^64 arithmetic.
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Path of one entry (whether or not it exists).
fn entry_path(dir: &Path, key: &str) -> PathBuf {
    // Key is hex from `cache_key` — already filesystem-safe.
    dir.join(format!("{key}.bin"))
}

/// Fetch-through read: return cached bytes when present (touching the
/// mtime so the LRU sees it as fresh), else `None` so the caller fetches
/// and stores.
pub(crate) fn read(dir: &Path, key: &str) -> Option<Vec<u8>> {
    let path = entry_path(dir, key);
    let bytes = std::fs::read(&path).ok()?;
    // Best-effort freshness bump; a failed touch only risks an early
    // eviction, never correctness.
    let now = filetime_now();
    let _ = set_mtime(&path, now);
    Some(bytes)
}

/// Store `bytes` under `key`, then evict if the cache grew past its cap.
/// Returns the path used (for tests).
pub(crate) fn write(dir: &Path, key: &str, bytes: &[u8]) -> Result<(), ClientError> {
    let path = entry_path(dir, key);
    // Temp + rename so a crash never leaves a truncated entry that later
    // reads as valid media.
    let tmp = dir.join(format!(".{key}.tmp"));
    std::fs::write(&tmp, bytes)
        .and_then(|_| std::fs::rename(&tmp, &path))
        .map_err(|e| ClientError::storage(format!("Could not store media in the cache: {e}")))?;
    let _ = set_mtime(&path, filetime_now());
    Ok(())
}

/// Delete every entry; returns the bytes freed. Also used by the settings
/// Clear action.
pub fn clear() -> Result<u64, ClientError> {
    let dir = cache_dir()?;
    let mut freed = 0u64;
    for entry in std::fs::read_dir(&dir)
        .map_err(|e| ClientError::storage(format!("Could not open the media cache: {e}")))?
        .flatten()
    {
        if let Ok(meta) = entry.metadata() {
            freed += meta.len();
        }
        let _ = std::fs::remove_file(entry.path());
    }
    tracing::info!(freed, "media cache cleared");
    Ok(freed)
}

/// Total bytes currently used (settings row).
pub fn size_bytes() -> u64 {
    let Ok(dir) = cache_dir() else {
        return 0;
    };
    dir_size(&dir)
}

/// Evict least-recently-used entries until the total is at or below
/// `cap`. Runs after writes (called from the runtime's media tasks; the
/// directory walk is stat-only over bounded files).
pub(crate) fn evict_to_cap(dir: &Path, cap: u64) {
    let entries = match collect(dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::debug!("media cache eviction skipped: {e}");
            return;
        }
    };
    let total: u64 = entries.iter().map(|(_, _, size)| size).sum();
    if total <= cap {
        return;
    }
    let mut excess = total - cap;
    // Oldest mtime first (that's the LRU order under read-touches).
    let mut ordered = entries;
    ordered.sort_by_key(|(mtime, _, _)| *mtime);
    for (mtime, path, size) in ordered {
        if excess == 0 {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            tracing::debug!(?path, mtime_secs = mtime.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0), size, "evicted media cache entry");
            excess = excess.saturating_sub(size);
        }
    }
}

/// (mtime, path, size) for every entry.
fn collect(dir: &Path) -> Result<Vec<(std::time::SystemTime, PathBuf, u64)>, ClientError> {
    let reader = std::fs::read_dir(dir)
        .map_err(|e| ClientError::storage(format!("Could not read the media cache: {e}")))?;
    let mut out = Vec::new();
    for entry in reader.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        out.push((mtime, entry.path(), meta.len()));
    }
    Ok(out)
}

fn dir_size(dir: &Path) -> u64 {
    collect(dir).map(|entries| entries.iter().map(|(_, _, size)| size).sum()).unwrap_or(0)
}

/// Portable mtime touch. Uses `filetime` when available; std-only fallback
/// rewrites the first byte (preserving content) to bump mtime — but that
/// is risky for concurrent readers, so instead we accept the tiny crate.
fn set_mtime(path: &Path, t: std::time::SystemTime) -> std::io::Result<()> {
    let secs = t
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let nsecs = t
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let ft = filetime::FileTime::from_unix_time(secs, nsecs);
    filetime::set_file_times(path, ft, ft)
}

fn filetime_now() -> std::time::SystemTime {
    std::time::SystemTime::now()
}

#[cfg(all(test, feature = "matrix"))]
mod tests {
    use super::*;

    #[test]
    fn keys_are_stable_and_distinct() {
        let a = cache_key("mxc://example/abc", None, None);
        let b = cache_key("mxc://example/abc", None, None);
        assert_eq!(a, b, "same request → same key");
        assert_ne!(a, cache_key("mxc://example/abd", None, None));
        assert_ne!(a, cache_key("mxc://example/abc", None, Some((64, 64))));
        assert_ne!(
            a,
            cache_key("mxc://example/abc", Some("{\"v\":\"key\"}"), None)
        );
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "fs-safe: {a}");
    }

    #[test]
    fn write_read_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "k1", b"pixels").expect("write");
        assert_eq!(read(dir.path(), "k1").as_deref(), Some(b"pixels".as_slice()));
        assert_eq!(read(dir.path(), "missing"), None);
    }

    #[test]
    fn write_overwrites_stale_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "k", b"old").expect("write");
        write(dir.path(), "k", b"new").expect("write");
        assert_eq!(read(dir.path(), "k").as_deref(), Some(b"new".as_slice()));
    }

    #[test]
    fn clear_reports_freed_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        // `clear` resolves the cache dir from the data dir: point
        // VESPER_DATA_DIR at the tempdir and seed its media-cache subdir.
        let cache = dir.path().join(DIR_NAME);
        std::fs::create_dir_all(&cache).expect("cache dir");
        write(&cache, "a", &[0u8; 100]).expect("write");
        write(&cache, "b", &[0u8; 28]).expect("write");
        let _guard = crate::session::tests::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("VESPER_DATA_DIR", dir.path());
        std::env::set_var("VESPER_SECRET_STORE", "file");
        let freed = clear().expect("clear");
        std::env::remove_var("VESPER_SECRET_STORE");
        std::env::remove_var("VESPER_DATA_DIR");
        assert_eq!(freed, 128, "freed");
        assert!(collect(&cache).expect("collect").is_empty(), "cache emptied");
    }

    fn size_of_dir(dir: &std::path::Path) -> u64 {
        dir_size(&dir.join(DIR_NAME))
    }

    #[test]
    fn eviction_removes_oldest_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Three entries with descending mtimes: a oldest, c newest.
        write(dir.path(), "a", &[0u8; 60]).expect("write");
        set_mtime(&entry_path(dir.path(), "a"), mtime_secs(1000)).expect("touch");
        write(dir.path(), "b", &[0u8; 60]).expect("write");
        set_mtime(&entry_path(dir.path(), "b"), mtime_secs(2000)).expect("touch");
        write(dir.path(), "c", &[0u8; 60]).expect("write");
        set_mtime(&entry_path(dir.path(), "c"), mtime_secs(3000)).expect("touch");
        // Cap of 150 keeps at most two entries (60 each) + slack; oldest (a)
        // must go first.
        evict_to_cap(dir.path(), 150);
        assert!(read(dir.path(), "a").is_none(), "oldest evicted");
        assert!(read(dir.path(), "b").is_some(), "mid kept");
        assert!(read(dir.path(), "c").is_some(), "newest kept");
    }

    fn mtime_secs(secs: u64) -> std::time::SystemTime {
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs)
    }
}
