//! Homeserver handshake and on-disk session persistence.
//!
//! Everything here runs inside the runtime thread's tokio runtime; the
//! [`matrix_sdk::Client`] it produces must never leave that thread.
//!
//! Storage layout under the OS data dir (macOS: `~/Library/Application Support/vesper/`):
//!
//! - `matrix-store/` — the sqlite state/crypto store the SDK maintains.
//! - `session.json` — the serialized [`MatrixSession`] (access + refresh
//!   tokens), created with `0600` permissions. Hardening this into the OS
//!   keyring is a checkpoint-11 task; the file is at least mode-restricted
//!   from creation, never a chmod after the fact.
//! - `store-passphrase` — the crypto-store passphrase (checkpoint 08),
//!   same `0600` treatment. Generated on first login, read back on every
//!   restore; deleting it (or reusing the store without it) makes the sqlite
//!   crypto store unopenable, so `cleanup_files` removes it together with
//!   the session and store dir. Keyring migration is checkpoint-11.
//!
//! Credentials never land in logs or user-facing strings: SDK errors are
//! classified by [`friendly_error`] and reduced to fixed sentences (raw errors
//! can carry server responses we don't control).

use std::path::PathBuf;

use matrix_sdk::{
    authentication::matrix::MatrixSession, ruma::api::error::ErrorKind, Client, ClientBuildError,
    HttpError, ThreadingSupport,
};

use crate::{api::ClientError, model::Me};

fn data_dir() -> Result<PathBuf, ClientError> {
    // Honor an explicit override first — tests and dev tooling set this so we
    // never touch the real profile directory.
    if let Some(dir) = std::env::var_os("VESPER_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    directories::ProjectDirs::from("dev", "vesper", "vesper")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .ok_or_else(|| ClientError("Could not determine this platform's data directory".into()))
}

fn store_dir() -> Result<PathBuf, ClientError> {
    Ok(data_dir()?.join("matrix-store"))
}

fn session_path() -> Result<PathBuf, ClientError> {
    Ok(data_dir()?.join("session.json"))
}

fn passphrase_path() -> Result<PathBuf, ClientError> {
    Ok(data_dir()?.join("store-passphrase"))
}

/// The state database file name matrix-sdk-sqlite uses inside the store dir
/// (`state_store::DATABASE_NAME`); its presence is the legacy-store signal.
const STATE_DB_NAME: &str = "matrix-sdk-state.sqlite3";

/// The crypto-store passphrase to open (or create) the sqlite store with.
///
/// Fresh installs: 48 random bytes, hex-encoded, persisted `0600` next to
/// the session file so restore finds the same passphrase.
///
/// Legacy migration (checkpoint 07 and earlier stores): if the store dir
/// already contains a state database but no passphrase file does, the store
/// was created with `None` and its rows are unencrypted — reopening it with
/// a passphrase would mint a new store cipher and make every existing row
/// unreadable. Return `None` with a warn so those installs keep working.
/// Logging out (which deletes the store) and back in upgrades them to a
/// passphrase-protected store.
fn store_passphrase() -> Result<Option<String>, ClientError> {
    let path = passphrase_path()?;
    match std::fs::read_to_string(&path) {
        Ok(pass) if !pass.trim().is_empty() => Ok(Some(pass.trim().to_string())),
        _ => {
            let store_exists = store_dir().map(|dir| dir.join(STATE_DB_NAME).exists()) == Ok(true);
            if store_exists {
                tracing::warn!(
                    "existing crypto store was created without a passphrase; \
                     continuing unencrypted (logout + login upgrades it)"
                );
                return Ok(None);
            }
            let mut bytes = [0u8; 48];
            getrandom::fill(&mut bytes)
                .map_err(|e| ClientError(format!("Could not generate a store passphrase: {e}")))?;
            let pass: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            write_owner_only(&path, pass.as_bytes())
                .map_err(|e| ClientError(format!("Could not write store passphrase: {e}")))?;
            Ok(Some(pass))
        }
    }
}

const UNREACHABLE: &str =
    "Could not reach that homeserver — check the server name and your connection.";
const SIGN_IN_AGAIN: &str = "Your session has expired — please sign in again.";
const GENERIC: &str = "Sign-in failed — check your connection and try again.";

/// True when the server says the token is dead (not merely unreachable). Only
/// these justify deleting the stored session.
fn is_auth_failure(kind: Option<&ErrorKind>) -> bool {
    matches!(
        kind,
        Some(ErrorKind::MissingToken)
            | Some(ErrorKind::UnknownToken(_))
            | Some(ErrorKind::TokenIncorrect)
            | Some(ErrorKind::Forbidden)
    )
}

/// Classify an SDK error into a friendly string. Never quotes server text.
fn friendly_error(e: &matrix_sdk::Error) -> ClientError {
    match e {
        matrix_sdk::Error::Http(http_err, ..) => friendly_http_error(http_err.as_ref()),
        _ => ClientError(GENERIC.into()),
    }
}

fn friendly_http_error(http_err: &HttpError) -> ClientError {
    match http_err.client_api_error_kind() {
        Some(ErrorKind::Forbidden) => ClientError("Incorrect username or password.".into()),
        Some(ErrorKind::InvalidParam) | Some(ErrorKind::InvalidUsername) => {
            ClientError("The server didn't recognize that username.".into())
        }
        Some(ErrorKind::UserDeactivated) => {
            ClientError("This account has been deactivated.".into())
        }
        Some(ErrorKind::UserLocked) | Some(ErrorKind::UserSuspended) => {
            ClientError("This account is suspended.".into())
        }
        Some(ErrorKind::LimitExceeded(_)) => {
            ClientError("Too many attempts — wait a moment and try again.".into())
        }
        Some(ErrorKind::MissingToken)
        | Some(ErrorKind::UnknownToken(_))
        | Some(ErrorKind::TokenIncorrect) => ClientError(SIGN_IN_AGAIN.into()),
        Some(_) => ClientError(GENERIC.into()),
        None => ClientError(UNREACHABLE.into()),
    }
}

fn map_build_error(e: &ClientBuildError) -> ClientError {
    match e {
        ClientBuildError::InvalidServerName => {
            ClientError("The homeserver name looks invalid — try e.g. `matrix.org`.".into())
        }
        ClientBuildError::AutoDiscovery(_) | ClientBuildError::Http(_) => {
            ClientError(UNREACHABLE.into())
        }
        _ => ClientError("Could not initialize the client.".into()),
    }
}

/// Build a client bound to `homeserver` (a server name like `matrix.org`;
/// well-known discovery finds the client API URL) with the persistent sqlite
/// store attached. Discovery/network failures surface here.
async fn build_client(homeserver: &str) -> Result<Client, ClientError> {
    let store_dir = store_dir()?;
    std::fs::create_dir_all(&store_dir)
        .map_err(|e| ClientError(format!("Could not create data directory: {e}")))?;

    // Checkpoint 08: the sqlite store (state + crypto) is passphrase-locked
    // when created fresh; see [`store_passphrase`] for the legacy branch.
    let passphrase = store_passphrase()?;

    Client::builder()
        .server_name_or_homeserver_url(homeserver)
        .sqlite_store(&store_dir, passphrase.as_deref())
        .handle_refresh_tokens()
        // Enable thread support so the event cache tracks thread roots and
        // populates `MsgLikeContent::thread_summary` (num_replies) on them as
        // replies land via sync — the "N replies" badge on thread roots is
        // mapped from this in `timeline::map_msglike`. `with_subscriptions`
        // is false: we want summary computation, not MSC4306/4308 sliding-sync
        // thread-subscription filtering.
        .with_threading_support(ThreadingSupport::Enabled {
            with_subscriptions: false,
        })
        .build()
        .await
        .map_err(|e| map_build_error(&e))
}

fn write_session_file(session: &MatrixSession) -> Result<(), ClientError> {
    let path = session_path()?;
    let bytes = serde_json::to_vec(session)
        .map_err(|e| ClientError(format!("Could not serialize session: {e}")))?;
    write_owner_only(&path, &bytes)
        .map_err(|e| ClientError(format!("Could not write session file: {e}")))?;
    Ok(())
}

/// Write `bytes` to `path` readable only by the owner from the moment the
/// file exists (no create-then-chmod window, even under `umask 000`).
#[cfg(unix)]
fn write_owner_only(path: &PathBuf, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    std::io::Write::write_all(&mut file, bytes)?;
    // `mode` only applies to *creation* — clamp a pre-existing file too.
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

/// On non-unix platforms the filesystem's ACLs govern access; nothing more
/// we can do portably here (keyring storage is checkpoint-11 work).
#[cfg(not(unix))]
fn write_owner_only(path: &PathBuf, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

/// The small piece of identity the UI shows. Display name falls back to the
/// MXID localpart when the profile lookup fails (e.g. offline first paint).
/// The user id comes from the session itself — no extra `whoami` round trip
/// (callers that need one, like restore, do it explicitly).
async fn me_snapshot(client: &Client) -> Me {
    let id = client
        .session_meta()
        .map(|m| m.user_id.to_string())
        .unwrap_or_default();
    let name = client
        .account()
        .get_display_name()
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| {
            id.strip_prefix('@')
                .and_then(|rest| rest.split(':').next())
                .filter(|local| !local.is_empty())
                .unwrap_or(&id)
                .to_owned()
        });
    // Own avatar (checkpoint 07): account profile MXC, consumed by the
    // "You" footer button and the self profile panel.
    let avatar = client
        .account()
        .get_avatar_url()
        .await
        .ok()
        .flatten()
        .map(|u| u.to_string());
    Me { name, id, avatar }
}

/// Full password login against `homeserver`. On success the fresh session is
/// persisted so the next launch restores without prompting.
pub async fn connect_login(
    homeserver: String,
    user_id: String,
    password: String,
) -> Result<(Client, Me), ClientError> {
    let client = build_client(&homeserver).await?;

    client
        .matrix_auth()
        .login_username(&user_id, &password)
        .initial_device_display_name("Vesper (macOS)")
        .send()
        .await
        .map_err(|e| friendly_error(&e))?;

    if let Some(session) = client.matrix_auth().session() {
        write_session_file(&session)?;
    }

    let me = me_snapshot(&client).await;
    Ok((client, me))
}

/// Restore a persisted session. `Ok(None)` means no session file exists — the
/// normal first-run state. Validation does a `whoami` round trip.
///
/// Deletion policy: the stored session (and the sqlite store holding this
/// device's crypto state) is only purged when the server says the token is
/// *dead* — corrupt files, invalid sessions, explicit auth failures. A
/// transient failure (offline, 5xx, captive portal) instead returns `Err`
/// with the files untouched, so the next launch retries the same device
/// rather than orphaning it. `App` turns either `Err` into the login screen
/// (with a warn log), never a panic.
pub async fn connect_restore() -> Result<Option<(Client, Me)>, ClientError> {
    let bytes = match std::fs::read(session_path()?) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ClientError(format!("Could not read session file: {e}"))),
    };
    let session: MatrixSession = serde_json::from_slice(&bytes).map_err(|_| {
        cleanup_files();
        ClientError("Stored session could not be read — please sign in again.".into())
    })?;

    let homeserver = session.meta.user_id.server_name().to_string();
    let client = build_client(&homeserver).await?;
    client.restore_session(session).await.map_err(|e| {
        cleanup_files();
        friendly_error(&e)
    })?;

    if let Err(e) = client.whoami().await {
        if is_auth_failure(e.client_api_error_kind()) {
            cleanup_files();
        }
        return Err(friendly_http_error(&e));
    }

    // Tokens may have rotated during restore; rewrite the file so it stays fresh.
    if let Some(fresh) = client.matrix_auth().session() {
        let _ = write_session_file(&fresh);
    }

    let me = me_snapshot(&client).await;
    Ok(Some((client, me)))
}

/// End the session remotely, then clear all local artifacts (session file and
/// the sqlite store, which holds crypto state for this device). Takes the
/// client by value so it is *dropped* — closing its sqlite handles — before
/// the store dir is deleted (open handles would lock the files on Windows).
///
/// The remote logout is bounded (30s) so a dead homeserver can't stall the
/// caller — the runtime's sequential command loop awaits this (review P2:
/// every queued command would otherwise wait out the HTTP timeout).
pub async fn logout(client: Option<Client>) -> Result<(), ClientError> {
    if let Some(client) = client {
        // Server-side logout is best-effort: a dead homeserver must not strand
        // the local account. Local cleanup proceeds either way.
        let logout_fut = async { client.matrix_auth().logout().await };
        match tokio::time::timeout(std::time::Duration::from_secs(30), logout_fut).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tracing::warn!("remote logout failed (continuing local cleanup): {e}"),
            Err(_) => tracing::warn!("remote logout timed out (continuing local cleanup)"),
        }
        drop(client);
    }
    cleanup_files();
    Ok(())
}

fn cleanup_files() {
    let mut paths = vec![session_path(), passphrase_path(), store_dir()];
    for result in paths.drain(..) {
        let Ok(path) = result else { continue };
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(all(test, feature = "matrix"))]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn paths_live_under_data_dir() {
        if std::env::var_os("HOME").is_none() && cfg!(unix) {
            return; // directories needs HOME on unix; holds on anything real.
        }
        // Take the env lock: sibling tests set/remove `VESPER_DATA_DIR` and
        // this test reads the data dir resolution live.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("VESPER_DATA_DIR");
        let base = data_dir().expect("data dir");
        assert!(session_path().unwrap().starts_with(&base));
        assert!(store_dir().unwrap().starts_with(&base));
    }

    #[cfg(unix)]
    #[test]
    fn session_file_is_owner_only_from_creation() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("session.json");
        write_owner_only(&path, b"{}").expect("write");
        let mode = path.metadata().expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "expected 0600, got {mode:o}");
    }

    #[test]
    fn auth_failures_purge_but_transport_errors_dont() {
        assert!(is_auth_failure(Some(&ErrorKind::MissingToken)));
        assert!(is_auth_failure(Some(&ErrorKind::TokenIncorrect)));
        assert!(is_auth_failure(Some(&ErrorKind::Forbidden)));
        assert!(!is_auth_failure(Some(&ErrorKind::LimitExceeded(
            Default::default()
        ))));
        assert!(!is_auth_failure(Some(&ErrorKind::Unknown)));
        assert!(!is_auth_failure(None));
    }

    // `store_passphrase` reads the data dir from the environment at call
    // time; the lock is shared with the runtime tests so any two tests
    // touching `VESPER_DATA_DIR` never race (env vars are global).
    pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_data_dir<T>(f: impl FnOnce(&std::path::Path) -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_var("VESPER_DATA_DIR", dir.path());
        let out = f(dir.path());
        std::env::remove_var("VESPER_DATA_DIR");
        out
    }

    #[test]
    fn passphrase_generated_once_then_stable() {
        with_data_dir(|_| {
            let first = store_passphrase().expect("generate");
            assert!(first.is_some(), "fresh install gets a passphrase");
            let second = store_passphrase().expect("read back");
            assert_eq!(first, second, "restore must reuse the stored passphrase");
        });
    }

    #[cfg(unix)]
    #[test]
    fn passphrase_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        with_data_dir(|_| {
            store_passphrase().expect("generate");
            let mode = passphrase_path()
                .expect("path")
                .metadata()
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "expected 0600, got {mode:o}");
        });
    }

    #[test]
    fn legacy_store_without_passphrase_stays_unlocked() {
        with_data_dir(|dir| {
            // A pre-checkpoint-08 store exists but no passphrase file: the
            // store must be reopened the way it was created (no passphrase),
            // and no passphrase file may be written next to it.
            std::fs::create_dir_all(dir.join("matrix-store")).expect("store dir");
            std::fs::write(dir.join("matrix-store/matrix-sdk-state.sqlite3"), b"").expect("db");
            assert_eq!(store_passphrase().expect("legacy branch"), None);
            assert!(!dir.join("store-passphrase").exists());
        });
    }
}
