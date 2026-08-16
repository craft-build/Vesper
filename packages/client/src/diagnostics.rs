//! Diagnostics support (checkpoint 11, workstream D): rolling log files,
//! panic capture, last-screen state, and the redacted "copy diagnostics"
//! payload.
//!
//! The subscriber + panic hook live in the launcher binaries (`desktop`'s
//! `main.rs`), because a global tracing subscriber must be installed before
//! any UI code runs. This module owns everything the hook and the settings
//! button need: where logs live, what the user was looking at when things
//! went wrong (a screen *name* — never message content), and how to
//! redact a log tail before it leaves the machine.
//!
//! Redaction policy (docs/11 §D): MXIDs and room ids stay — they're what
//! support actually needs to look up an account — while token-shaped
//! values (`access_token=…`, `Bearer …`) are replaced. Message bodies
//! never enter the logs in the first place (a standing rule from
//! checkpoint 02), so the redactor is a seatbelt, not the only line of
//! defense.

use std::path::PathBuf;

use crate::api::ClientError;

/// How many lines of the newest log file the copied diagnostics include.
const LOG_TAIL_LINES: usize = 250;

/// Where rolling log files live: `<data_dir>/logs/`. Errors when the
/// platform data dir can't be determined (launcher then skips file
/// logging rather than crashing).
pub fn logs_dir() -> Result<PathBuf, ClientError> {
    Ok(crate::session::data_dir()?.join("logs"))
}

// ----------------------------------------------------------------------
// Last-screen state (panic dumps). A plain screen name, no ids, no
// content: enough to reproduce "it crashed while opening a room", not
// enough to leak anything.
// ----------------------------------------------------------------------

/// Coarse screen the user had open — written by the UI on route changes,
/// read by the panic hook (checkpoint 11 §D: "last-screen state dump").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Startup,
    Login,
    Home,
    Room,
    Settings,
}

impl Screen {
    fn as_u8(self) -> u8 {
        match self {
            Screen::Startup => 0,
            Screen::Login => 1,
            Screen::Home => 2,
            Screen::Room => 3,
            Screen::Settings => 4,
        }
    }
    fn from_u8(v: u8) -> &'static str {
        match v {
            1 => "login",
            2 => "home",
            3 => "room",
            4 => "settings",
            _ => "startup",
        }
    }
}

static LAST_SCREEN: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Record the screen the user is looking at (called from the UI).
pub fn set_last_screen(screen: Screen) {
    LAST_SCREEN.store(screen.as_u8(), std::sync::atomic::Ordering::Relaxed);
}

/// The last recorded screen, for panic dumps.
#[must_use]
pub fn last_screen() -> &'static str {
    Screen::from_u8(LAST_SCREEN.load(std::sync::atomic::Ordering::Relaxed))
}

// ----------------------------------------------------------------------
// Redaction
// ----------------------------------------------------------------------

/// Replace token-shaped values with `[REDACTED]`. Handled markers:
///
/// - `access_token=…` / `refresh_token=…` (query strings, log fields)
/// - `"access_token":"…"` (JSON blobs echoed into logs by SDK debug output)
/// - `Bearer …` (Authorization headers)
///
/// Values end at whitespace, a quote, an ampersand, or a closing brace —
/// the shapes that actually occur in query strings and JSON.
#[must_use]
pub fn redact(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    redact_into(input, &mut out);
    out
}

fn redact_into(input: &str, out: &mut String) {
    let markers: &[&str] = &[
        "access_token=",
        "refresh_token=",
        "\"access_token\":\"",
        "\"refresh_token\":\"",
        "Bearer ",
    ];
    let Some((idx, marker)) = markers
        .iter()
        .filter_map(|m| input.find(m).map(|i| (i, *m)))
        .min_by_key(|(i, _)| *i)
    else {
        out.push_str(input);
        return;
    };
    out.push_str(&input[..idx]);
    out.push_str(marker);
    let value_start = idx + marker.len();
    let rest = &input[value_start..];
    let value_end = rest
        .find(|c: char| c.is_whitespace() || c == '"' || c == '&' || c == '}' || c == ',')
        .unwrap_or(rest.len());
    if !rest[..value_end].is_empty() {
        out.push_str("[REDACTED]");
    }
    redact_into(&rest[value_end..], out);
}

// ----------------------------------------------------------------------
// The copied payload
// ----------------------------------------------------------------------

/// Assemble the "copy diagnostics" payload (checkpoint 11 §D): app + OS
/// facts, secret-storage backend in use, and the tail of the newest log
/// file, redacted. MXIDs/room ids are kept on purpose (docs/11) — support
/// needs them to look anything up.
#[must_use]
pub fn collect() -> String {
    let mut out = String::new();
    out.push_str("Vesper diagnostics\n");
    out.push_str(&format!("version: {}\n", env!("CARGO_PKG_VERSION")));
    out.push_str(&format!("os: {}\n", std::env::consts::OS));
    out.push_str(&format!("screen: {}\n", last_screen()));
    out.push_str(&format!("secrets: {}\n", crate::secrets::backend_in_use()));
    match logs_dir().and_then(|dir| newest_log_file(&dir)) {
        Ok(Some((path, size))) => {
            out.push_str(&format!(
                "log: {} ({})\n",
                path.display(),
                crate::media::format_size(size)
            ));
            match tail(&path, LOG_TAIL_LINES) {
                Ok(tail) => {
                    out.push_str("--- log tail (redacted) ---\n");
                    out.push_str(&redact(&tail));
                }
                Err(e) => out.push_str(&format!("(log unreadable: {e})\n")),
            }
        }
        Ok(None) => out.push_str("log: none yet\n"),
        Err(e) => out.push_str(&format!("log dir unavailable: {e}\n")),
    }
    out
}

/// The newest file in `dir` (rolling appender files sort by name/date).
fn newest_log_file(dir: &std::path::Path) -> Result<Option<(PathBuf, u64)>, ClientError> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| ClientError::storage(format!("Could not read the log directory: {e}")))?;
    let mut newest: Option<(PathBuf, u64)> = None;
    for entry in entries.flatten() {
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() {
            continue;
        }
        let size = meta.len();
        let newer = newest.as_ref().is_none_or(|(old, _)| entry.path() > *old);
        if newer {
            newest = Some((entry.path(), size));
        }
    }
    Ok(newest)
}

/// Last `n` lines of `path`, robustly: read the whole file (logs are
/// bounded by the rolling appender) and keep the tail.
fn tail(path: &std::path::Path, n: usize) -> std::io::Result<String> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(n);
    Ok(lines[start..].join("\n"))
}

#[cfg(all(test, feature = "matrix"))]
mod tests {
    use super::*;

    #[test]
    fn redact_query_string_tokens() {
        assert_eq!(
            redact("GET /_matrix/client/v3/sync?access_token=syt_Abc123&since=xyz"),
            "GET /_matrix/client/v3/sync?access_token=[REDACTED]&since=xyz"
        );
    }

    #[test]
    fn redact_json_tokens() {
        assert_eq!(
            redact(r#"{"access_token":"syt_secret","expires_in":3607}"#),
            r#"{"access_token":"[REDACTED]","expires_in":3607}"#
        );
    }

    #[test]
    fn redact_bearer_headers() {
        assert_eq!(
            redact("Authorization: Bearer syt_aaaaaaaaaaaa"),
            "Authorization: Bearer [REDACTED]"
        );
    }

    #[test]
    fn redact_keeps_mxids_and_room_ids() {
        let line = "@user:matrix.org in !roomid:matrix.org said nothing logged";
        assert_eq!(redact(line), line);
    }

    #[test]
    fn redact_multiple_markers_in_one_line() {
        assert_eq!(
            redact("access_token=abc refresh_token=def"),
            "access_token=[REDACTED] refresh_token=[REDACTED]"
        );
    }

    #[test]
    fn tail_takes_last_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("log");
        std::fs::write(&path, "1\n2\n3\n4\n5").expect("write");
        assert_eq!(tail(&path, 2).expect("tail"), "4\n5");
        assert_eq!(tail(&path, 10).expect("tail"), "1\n2\n3\n4\n5");
    }

    #[test]
    fn newest_log_prefers_lexically_largest() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("vesper.log.2026-08-14"), b"a").expect("write");
        std::fs::write(dir.path().join("vesper.log.2026-08-15"), b"bb").expect("write");
        let (path, size) =
            newest_log_file(dir.path()).expect("dir").expect("some file");
        assert!(path.to_string_lossy().ends_with("2026-08-15"));
        assert_eq!(size, 2);
    }

    #[test]
    fn screen_round_trip() {
        set_last_screen(Screen::Room);
        assert_eq!(last_screen(), "room");
        set_last_screen(Screen::Startup);
        assert_eq!(last_screen(), "startup");
    }
}
