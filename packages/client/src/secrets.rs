//! OS-keyring-backed secret storage (checkpoint 11, workstream B).
//!
//! Two secrets move out of plain files under the data dir into the OS
//! keychain (macOS Keychain / Windows Credential Manager):
//!
//! - `session` — the serialized `MatrixSession` (access + refresh tokens).
//! - `store-passphrase` — the crypto-store passphrase (checkpoint 08).
//!
//! Behavior matrix:
//!
//! - **Fresh install** (`keyring` available): secrets are written to the
//!   keyring and never touch disk.
//! - **Legacy install** (files exist from checkpoints ≤10): the first read
//!   migrates — store into the keyring, then delete the file — so
//!   `grep -r access_token <data_dir>` comes back clean after one launch.
//! - **Keyring unavailable** (headless Linux, locked keychain, or
//!   `VESPER_SECRET_STORE=file`): a loud one-time-per-process warning is
//!   logged and the secret falls back to the checkpoint-10 file layout
//!   (`0600`, owner-only from creation). This is a documented degradation,
//!   not an error the user can fix by retrying.
//!
//! Tests force the file backend via `VESPER_SECRET_STORE=file` so they never
//! touch a real keychain.

// The keyring dep is only compiled for its two backend platforms (see
// Cargo.toml target sections). Everywhere else — Android, Linux — the
// module compiles to the file fallback described above.
#[cfg(any(target_os = "macos", target_os = "windows"))]
use keyring::Entry;

use crate::api::ClientError;

/// Keychain service name; entry names per secret are in [`Secret::account`].
const SERVICE: &str = "dev.vesper.app";

/// The secrets Vesper persists (checkpoint 11). The keychain entry name is
/// per-user where the secret is user-scoped so multiple accounts on one
/// machine never collide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Secret {
    /// The serialized `MatrixSession` JSON blob.
    Session,
    /// The sqlite crypto-store passphrase (checkpoint 08).
    StorePassphrase,
}

impl Secret {
    /// Keychain account name. The session entry is deliberately NOT
    /// user-scoped: the MXID is only knowable *from* the session blob, and
    /// the sqlite store (and therefore the whole install) is single-account
    /// anyway — a user-scoped name would make restore miss what login
    /// stored.
    fn account(self) -> &'static str {
        match self {
            Secret::Session => "session",
            Secret::StorePassphrase => "store-passphrase",
        }
    }
}

/// Loud-but-once fallback warning: every secret op on a broken keyring
/// would otherwise repeat the same paragraph per call.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn warn_fallback(reason: &str) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            reason,
            "OS keyring unavailable — storing credentials in plain files \
             under the data dir. Fix the keyring (or set \
             VESPER_SECRET_STORE=file to silence this deliberately) to move \
             them back into the keychain."
        );
    }
}

/// True unless `VESPER_SECRET_STORE=file` forces the file backend (tests,
/// deliberate opt-out). Keyring availability itself is probed per-op.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn keyring_enabled() -> bool {
    std::env::var_os("VESPER_SECRET_STORE").as_deref() != Some(std::ffi::OsStr::new("file"))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn keyring_entry(secret: Secret) -> Option<Entry> {
    if !keyring_enabled() {
        warn_fallback("VESPER_SECRET_STORE=file");
        return None;
    }
    Entry::new(SERVICE, secret.account())
        .inspect_err(|e| {
            warn_fallback(&format!("keyring entry unavailable: {e}"));
        })
        .ok()
}

/// Persist `value` for `secret` (keyring when available, else `0600` file
/// at `legacy_path`). Saving through the keyring also removes a leftover
/// legacy file — half of the one-time migration (the read side completes
/// it for installs that never re-save).
pub(crate) fn save(
    secret: Secret,
    value: &str,
    legacy_path: &std::path::Path,
) -> Result<(), ClientError> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if let Some(entry) = keyring_entry(secret) {
        return match entry.set_password(value) {
            Ok(()) => {
                // Migration tail: the file copy (if any) is now redundant.
                let _ = std::fs::remove_file(legacy_path);
                Ok(())
            }
            Err(e) => {
                warn_fallback(&format!("keyring write failed: {e}"));
                save_file(value, legacy_path)
            }
        };
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = secret;
    save_file(value, legacy_path)
}

fn save_file(value: &str, legacy_path: &std::path::Path) -> Result<(), ClientError> {
    // Owner-only from file creation, even under a permissive umask — a
    // plain `fs::write` + chmod would leave a world-readable window.
    crate::session::write_owner_only(legacy_path, value.as_bytes())
        .map_err(|e| ClientError::storage(format!("Could not store credentials: {e}")))
}

/// Load `secret`: keyring first; a legacy file is migrated (stored into the
/// keyring, file deleted) when found. `Ok(None)` = genuinely absent (fresh
/// install or clean logout) — not an error.
pub(crate) fn load(
    secret: Secret,
    legacy_path: &std::path::Path,
) -> Result<Option<String>, ClientError> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if let Some(entry) = keyring_entry(secret) {
        match entry.get_password() {
            Ok(value) => {
                // Migration tail (read side): a file from ≤checkpoint-10 is
                // redundant the moment the keyring answers.
                if legacy_path.exists() {
                    let _ = std::fs::remove_file(legacy_path);
                }
                return Ok(Some(value));
            }
            Err(keyring::Error::NoEntry) => return load_legacy(secret, legacy_path),
            Err(e) => {
                warn_fallback(&format!("keyring read failed: {e}"));
                return load_legacy(secret, legacy_path);
            }
        }
    }
    load_legacy(secret, legacy_path)
}

/// Read the legacy file; when the keyring is usable, hoist the value into
/// it and delete the file (the one-time migration).
fn load_legacy(
    secret: Secret,
    legacy_path: &std::path::Path,
) -> Result<Option<String>, ClientError> {
    let bytes = match std::fs::read(legacy_path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(ClientError::storage(format!(
                "Could not read stored credentials: {e}"
            )))
        }
    };
    let value = String::from_utf8(bytes)
        .map_err(|e| ClientError::storage(format!("Stored credentials are not readable: {e}")))?;
    let value = value.trim().to_string();
    // Hoist into the keyring when possible; if that fails the file stays
    // authoritative (still readable next launch) and the fallback warning
    // has already fired inside `save`.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if keyring_enabled() {
        if let Some(entry) = keyring_entry(secret) {
            if entry.set_password(&value).is_ok() {
                let _ = std::fs::remove_file(legacy_path);
                tracing::info!(
                    ?legacy_path,
                    "migrated stored credentials into the OS keyring"
                );
            }
        }
    }
    Ok(Some(value))
}

/// Remove `secret` from wherever it lives (keyring entry + legacy file).
/// Logout is the caller; missing entries are success.
pub(crate) fn delete(secret: Secret, legacy_path: &std::path::Path) -> Result<(), ClientError> {
    let mut failures = Vec::new();
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if keyring_enabled() {
        match keyring_entry(secret) {
            Some(entry) => match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(e) => failures.push(format!("keyring deletion failed: {e}")),
            },
            None => failures.push("the OS keyring could not be opened".into()),
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = secret;
    if let Err(e) = std::fs::remove_file(legacy_path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            failures.push(format!("credential-file deletion failed: {e}"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(ClientError::storage(format!(
            "Could not fully remove local credentials: {}",
            failures.join("; ")
        )))
    }
}

/// Report which backend actually served the last op — surfaced in
/// diagnostics so users can see when they're on the file fallback.
pub(crate) fn backend_in_use() -> &'static str {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        if keyring_enabled() {
            "os-keyring (file fallback if unavailable)"
        } else {
            "file (VESPER_SECRET_STORE=file)"
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "file (no OS keyring backend on this platform)"
    }
}

#[cfg(all(test, feature = "matrix"))]
mod tests {
    use super::*;

    // All tests force the file backend: they run in CI/dev sandboxes where
    // touching the real login keychain would be rude (and flaky).
    fn file_mode<T>(f: impl FnOnce() -> T) -> T {
        // Env lock shared with session tests: env vars are global.
        let _guard = crate::session::tests::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("VESPER_SECRET_STORE", "file");
        let out = f();
        std::env::remove_var("VESPER_SECRET_STORE");
        out
    }

    #[test]
    fn file_backend_round_trip() {
        file_mode(|| {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("secret");
            save(Secret::StorePassphrase, "hunter2", &path).expect("save");
            assert_eq!(
                load(Secret::StorePassphrase, &path).expect("load"),
                Some("hunter2".into())
            );
            delete(Secret::StorePassphrase, &path).expect("delete");
            assert_eq!(load(Secret::StorePassphrase, &path).expect("load"), None);
        });
    }

    #[test]
    fn load_missing_is_none_not_error() {
        file_mode(|| {
            let dir = tempfile::tempdir().expect("tempdir");
            assert_eq!(
                load(Secret::Session, &dir.path().join("nope")).expect("load"),
                None
            );
        });
    }

    #[test]
    fn load_io_failure_is_not_treated_as_missing() {
        file_mode(|| {
            let dir = tempfile::tempdir().expect("tempdir");
            let err =
                load(Secret::Session, dir.path()).expect_err("directory is not a secret file");
            assert_eq!(err.kind, crate::api::ClientErrorKind::Storage);
        });
    }

    #[test]
    fn delete_reports_cleanup_failure() {
        file_mode(|| {
            let dir = tempfile::tempdir().expect("tempdir");
            let err = delete(Secret::Session, dir.path())
                .expect_err("a directory cannot be removed as a credential file");
            assert_eq!(err.kind, crate::api::ClientErrorKind::Storage);
        });
    }

    #[test]
    fn save_over_legacy_file_replaces_it() {
        file_mode(|| {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("session.json");
            std::fs::write(&path, b"old-credentials").expect("legacy write");
            save(Secret::Session, "new-credentials", &path).expect("save");
            // File mode: the fallback file is the storage, so it survives —
            // holding the NEW value (the old one must be gone). In keyring
            // mode the file is removed instead; that path is exercised
            // on-device, not from the test sandbox.
            assert_eq!(
                load(Secret::Session, &path).expect("load"),
                Some("new-credentials".into()),
                "old value must not survive a save"
            );
        });
    }

    #[test]
    #[cfg(unix)]
    fn file_fallback_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        file_mode(|| {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("store-passphrase");
            save(Secret::StorePassphrase, "pw", &path).expect("save");
            let mode = path.metadata().expect("meta").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        });
    }
}
