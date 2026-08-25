//! Homeserver handshake and on-disk session persistence.
//!
//! Everything here runs inside the runtime thread's tokio runtime; the
//! [`matrix_sdk::Client`] it produces must never leave that thread.
//!
//! Storage layout (checkpoint 11): secrets live in the OS keyring via the
//! [`secrets`] module (service `dev.vesper.app`); everything else under the
//! OS data dir (macOS: `~/Library/Application Support/vesper/`):
//!
//! - `matrix-store/` — the sqlite state/crypto store the SDK maintains.
//! - keyring entries `session` + `store-passphrase` — the serialized
//!   [`MatrixSession`] (access + refresh tokens) and the crypto-store
//!   passphrase (checkpoint 08). The legacy plain files (`session.json`,
//!   `store-passphrase`) are migrated into the keyring and deleted on first
//!   touch; when the keyring is unavailable the module falls back to those
//!   files (`0600`, owner-only from creation) with a loud warning.
//! - `prefs.json` — device-local application preferences (checkpoint 10),
//!   versioned + serde-default tolerant. Wiped with the session: preferences
//!   are device-local by definition, and a fresh device starts fresh.
//!
//! Credentials never land in logs or user-facing strings: SDK errors are
//! classified by [`friendly_error`] and reduced to fixed sentences (raw errors
//! can carry server responses we don't control).

use std::path::{Path, PathBuf};

use matrix_sdk::{
    authentication::matrix::MatrixSession, ruma::api::error::ErrorKind, Client, ClientBuildError,
    HttpError, ThreadingSupport,
};

use crate::{
    api::ClientError,
    model::{Me, Prefs},
    secrets::{self, Secret},
};

pub(crate) fn data_dir() -> Result<PathBuf, ClientError> {
    // Honor an explicit override first — tests and dev tooling set this so we
    // never touch the real profile directory.
    if let Some(dir) = std::env::var_os("VESPER_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    #[cfg(target_os = "android")]
    {
        return android_data_dir();
    }
    #[cfg(not(target_os = "android"))]
    directories::ProjectDirs::from("dev", "vesper", "vesper")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .ok_or_else(|| ClientError::storage("Could not determine this platform's data directory"))
}

/// Android has no desktop-style data dir (`directories` returns `None`), and
/// the app's private files dir is only reachable through JNI. The context
/// pointers are process-global and initialized by dioxus (manganis) at
/// activity creation, so by the time session restore runs they are valid.
/// Same mechanism `dioxus-asset-resolver` uses to reach the AssetManager.
#[cfg(target_os = "android")]
fn android_data_dir() -> Result<PathBuf, ClientError> {
    use jni::{objects::JObject, JavaVM};

    let ctx = ndk_context::android_context();
    if ctx.vm().is_null() || ctx.context().is_null() {
        return Err(ClientError::storage(
            "android JNI context not initialized (restore raced app startup)",
        ));
    }
    let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }
        .map_err(|e| ClientError::storage(format!("android JVM unavailable: {e}")))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|e| ClientError::storage(format!("android JNI attach failed: {e}")))?;
    let context = unsafe { JObject::from_raw(ctx.context().cast()) };

    fn storage_err(what: &str) -> impl FnOnce(jni::errors::Error) -> ClientError + '_ {
        move |e| ClientError::storage(format!("{what}: {e}"))
    }
    let files_dir = env
        .call_method(context, "getFilesDir", "()Ljava/io/File;", &[])
        .and_then(|v| v.l())
        .map_err(storage_err("Context.getFilesDir failed"))?;
    let path = env
        .call_method(files_dir, "getAbsolutePath", "()Ljava/lang/String;", &[])
        .and_then(|v| v.l())
        .map_err(storage_err("File.getAbsolutePath failed"))?;
    let path: String = env
        .get_string(&path.into())
        .map_err(storage_err("JNI string conversion failed"))?
        .into();
    Ok(PathBuf::from(path))
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

fn prefs_path() -> Result<PathBuf, ClientError> {
    Ok(data_dir()?.join("prefs.json"))
}

/// Load device-local preferences (checkpoint 10). A missing file is the
/// normal first-run state → defaults; an unreadable/corrupt file is a warn
/// + defaults, never a failure — bad prefs must not brick the app.
pub(crate) fn load_prefs() -> Prefs {
    let bytes = match std::fs::read(prefs_path().unwrap_or_else(|_| PathBuf::from("prefs.json"))) {
        Ok(bytes) => bytes,
        Err(_) => return Prefs::default(),
    };
    match serde_json::from_slice(&bytes) {
        Ok(prefs) => prefs,
        Err(e) => {
            tracing::warn!("prefs file unreadable, using defaults: {e}");
            Prefs::default()
        }
    }
}

/// Persist preferences atomically-enough for v1 (truncate-write, `0600`).
/// The file is tiny and written only from the settings screen.
pub(crate) fn save_prefs(prefs: &Prefs) -> Result<(), ClientError> {
    let bytes = serde_json::to_vec(prefs)
        .map_err(|e| ClientError::storage(format!("Could not serialize preferences: {e}")))?;
    write_owner_only(&prefs_path()?, &bytes)
        .map_err(|e| ClientError::storage(format!("Could not write preferences: {e}")))
}

/// The state database file name matrix-sdk-sqlite uses inside the store dir
/// (`state_store::DATABASE_NAME`); its presence is the legacy-store signal.
const STATE_DB_NAME: &str = "matrix-sdk-state.sqlite3";

/// The crypto-store passphrase to open (or create) the sqlite store with.
///
/// Fresh installs: 48 random bytes, hex-encoded, stored in the OS keyring
/// (file fallback `0600` when the keyring is unavailable — see `secrets`).
///
/// Legacy migration (checkpoint 07 and earlier stores): if the store dir
/// already contains a state database but no stored passphrase exists, the
/// store was created with `None` and its rows are unencrypted — reopening
/// it with a passphrase would mint a new store cipher and make every
/// existing row unreadable. Return `None` with a warn so those installs
/// keep working. Logging out (which deletes the store) and back in upgrades
/// them to a passphrase-protected store.
fn store_passphrase() -> Result<Option<String>, ClientError> {
    let path = passphrase_path()?;
    if let Some(pass) = secrets::load(Secret::StorePassphrase, &path)? {
        if !pass.is_empty() {
            return Ok(Some(pass));
        }
    }
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
        .map_err(|e| ClientError::storage(format!("Could not generate a store passphrase: {e}")))?;
    let pass: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    secrets::save(Secret::StorePassphrase, &pass, &path)?;
    Ok(Some(pass))
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

/// Classify an SDK error into a friendly, kind-tagged error. Never quotes
/// server text — raw errors can carry server responses we don't control.
fn friendly_error(e: &matrix_sdk::Error) -> ClientError {
    match e {
        matrix_sdk::Error::Http(http_err, ..) => friendly_http_error(http_err.as_ref()),
        _ => ClientError::unknown(GENERIC),
    }
}

fn friendly_http_error(http_err: &HttpError) -> ClientError {
    match http_err.client_api_error_kind() {
        Some(ErrorKind::Forbidden) => ClientError::auth("Incorrect username or password."),
        Some(ErrorKind::InvalidParam) | Some(ErrorKind::InvalidUsername) => {
            ClientError::invalid("The server didn't recognize that username.")
        }
        Some(ErrorKind::UserDeactivated) => ClientError::auth("This account has been deactivated."),
        Some(ErrorKind::UserLocked) | Some(ErrorKind::UserSuspended) => {
            ClientError::auth("This account is suspended.")
        }
        Some(ErrorKind::LimitExceeded(_)) => {
            ClientError::rate_limited("Too many attempts — wait a moment and try again.")
        }
        Some(ErrorKind::MissingToken)
        | Some(ErrorKind::UnknownToken(_))
        | Some(ErrorKind::TokenIncorrect) => ClientError::auth(SIGN_IN_AGAIN),
        Some(_) => ClientError::server(GENERIC),
        None => ClientError::network(UNREACHABLE),
    }
}

fn map_build_error(e: &ClientBuildError) -> ClientError {
    match e {
        ClientBuildError::InvalidServerName => {
            ClientError::invalid("The homeserver name looks invalid — try e.g. `matrix.org`.")
        }
        ClientBuildError::AutoDiscovery(_) | ClientBuildError::Http(_) => {
            ClientError::network(UNREACHABLE)
        }
        _ => ClientError::unknown("Could not initialize the client."),
    }
}

/// Build a client bound to `homeserver` (a server name like `matrix.org`;
/// well-known discovery finds the client API URL) with the persistent sqlite
/// store attached. Discovery/network failures surface here.
async fn build_client(homeserver: &str) -> Result<Client, ClientError> {
    let store_dir = store_dir()?;
    std::fs::create_dir_all(&store_dir)
        .map_err(|e| ClientError::storage(format!("Could not create data directory: {e}")))?;

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

/// Persist the session blob into the OS keyring (file fallback with a loud
/// warning; any legacy `session.json` is removed — checkpoint 11).
fn save_session(session: &MatrixSession) -> Result<(), ClientError> {
    let path = session_path()?;
    let json = serde_json::to_string(session)
        .map_err(|e| ClientError::storage(format!("Could not serialize session: {e}")))?;
    secrets::save(Secret::Session, &json, &path)
}

/// Write `bytes` to `path` readable only by the owner from the moment the
/// file exists (no create-then-chmod window, even under `umask 000`).
#[cfg(unix)]
pub(crate) fn write_owner_only(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
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
pub(crate) fn write_owner_only(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

/// The small piece of identity the UI shows. Display name falls back to the
/// MXID localpart when the profile lookup fails (e.g. offline first paint).
/// The user id comes from the session itself — no extra `whoami` round trip
/// (callers that need one, like restore, do it explicitly).
pub(crate) async fn me_snapshot(client: &Client) -> Me {
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
        save_session(&session)?;
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
    // Keyring first; a legacy `session.json` is migrated + deleted inside
    // `secrets::load` (checkpoint 11). `None` = fresh install / clean
    // logout — the normal no-session state, not an error.
    let json = secrets::load(Secret::Session, &session_path()?)?;
    let Some(json) = json else {
        return Ok(None);
    };
    let session: MatrixSession = serde_json::from_str(&json).map_err(|_| {
        cleanup_files();
        ClientError::storage("Stored session could not be read — please sign in again.")
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
        let _ = save_session(&fresh);
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
    // Keyring entries die with the session too (checkpoint 11): a stale
    // entry must never resurrect a logged-out device's tokens.
    if let (Ok(session), Ok(pass)) = (session_path(), passphrase_path()) {
        secrets::delete(Secret::Session, &session);
        secrets::delete(Secret::StorePassphrase, &pass);
    }
    let mut paths = vec![session_path(), passphrase_path(), prefs_path(), store_dir()];
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
        // Tests never touch the real OS keychain: force the file fallback
        // (checkpoint 11) so keyring-backed paths are exercised on-device.
        std::env::set_var("VESPER_SECRET_STORE", "file");
        let out = f(dir.path());
        std::env::remove_var("VESPER_SECRET_STORE");
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

    // Checkpoint 11: a checkpoint-08/10 install kept the passphrase in a
    // plain file — the first open after upgrade must keep using that exact
    // value (a fresh one would brick the crypto store).
    #[test]
    fn legacy_passphrase_file_is_reused_verbatim() {
        with_data_dir(|_| {
            write_owner_only(&passphrase_path().expect("path"), b"legacy-secret-value\n")
                .expect("legacy write");
            let pass = store_passphrase().expect("read");
            assert_eq!(
                pass.as_deref(),
                Some("legacy-secret-value"),
                "the stored passphrase must survive the secrets migration"
            );
        });
    }

    #[test]
    fn cleanup_removes_stored_passphrase() {
        with_data_dir(|_| {
            store_passphrase().expect("generate");
            assert!(passphrase_path().expect("path").exists());
            cleanup_files();
            assert!(!passphrase_path().expect("path").exists());
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

    // Checkpoint 10: prefs round-trip, tolerance, and logout wipe.
    #[test]
    fn prefs_round_trip() {
        with_data_dir(|_| {
            let prefs = Prefs {
                theme: "light".into(),
                read_receipts: false,
                ..Prefs::default()
            };
            save_prefs(&prefs).expect("save");
            assert_eq!(load_prefs(), prefs);
        });
    }

    #[test]
    fn missing_prefs_file_is_default() {
        with_data_dir(|_| {
            assert_eq!(load_prefs(), Prefs::default());
        });
    }

    #[test]
    fn prefs_tolerate_unknown_and_missing_fields() {
        with_data_dir(|_| {
            // Unknown future fields are ignored; missing ones take defaults.
            std::fs::write(
                prefs_path().expect("path"),
                br#"{"version":1,"theme":"dark","future_field":{"a":[1,2]}}"#,
            )
            .expect("write");
            assert_eq!(load_prefs(), Prefs::default());
            std::fs::write(prefs_path().expect("path"), br#"{"theme":"light"}"#).expect("write");
            let loaded = load_prefs();
            assert_eq!(loaded.theme, "light");
            assert!(loaded.read_receipts, "missing field falls back to default");
            assert_eq!(loaded.version, 1, "version default applied");
        });
    }

    #[test]
    fn corrupt_prefs_file_is_default_not_error() {
        with_data_dir(|_| {
            std::fs::write(prefs_path().expect("path"), b"{not json").expect("write");
            assert_eq!(load_prefs(), Prefs::default());
        });
    }

    #[cfg(unix)]
    #[test]
    fn prefs_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        with_data_dir(|_| {
            save_prefs(&Prefs::default()).expect("save");
            let mode = prefs_path()
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
    fn cleanup_removes_prefs() {
        with_data_dir(|_| {
            save_prefs(&Prefs::default()).expect("save");
            assert!(prefs_path().expect("path").exists());
            cleanup_files();
            assert!(!prefs_path().expect("path").exists());
        });
    }
}
