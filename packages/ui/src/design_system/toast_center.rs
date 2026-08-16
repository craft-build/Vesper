//! The single toast surface (checkpoint 11, workstream A).
//!
//! Every non-form error in the app funnels through one [`ToastCenter`]
//! provided at the `App` scope and rendered once by [`ToastHost`] (mounted
//! next to the router). Errors arrive as structured
//! [`ClientError`]s: the kind picks the tone and title, the fixed message
//! becomes the body — no ad-hoc string plumbing at call sites.
//!
//! Form-adjacent errors (login fields, dialog password prompts) stay
//! inline next to their inputs where the user is looking; everything
//! else — background failures, downloads, settings saves, retries — is a
//! toast. One surface, one look, one dismissal pattern.

use dioxus::prelude::*;

use crate::data::{ClientError, ClientErrorKind};

use super::{Toast, ToastTone};

/// How long a toast stays up before auto-dismissing. Long enough to read
/// two sentences; short enough not to stack up during a flappy-network
/// burst (the host caps the stack at four regardless).
const AUTO_DISMISS_MS: u64 = 6000;
/// Never more than this many on screen; oldest get dropped first.
const MAX_STACK: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastEntry {
    pub id: u64,
    pub tone: ToastTone,
    pub title: String,
    pub description: Option<String>,
}

impl From<ToastTone> for ToastEntry {
    fn from(tone: ToastTone) -> Self {
        let title = match tone {
            ToastTone::Info => "Heads up",
            ToastTone::Success => "Done",
            ToastTone::Danger => "Something went wrong",
        };
        Self {
            id: next_id(),
            tone,
            title: title.into(),
            description: None,
        }
    }
}

/// Global id source for toast entries (monotonic; `AtomicU64` because
/// pushes can come from any spawned task).
fn next_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Kind → tone/title mapping, the one place error presentation is decided.
fn tone_for(kind: ClientErrorKind) -> (ToastTone, &'static str) {
    match kind {
        ClientErrorKind::Network => (ToastTone::Danger, "Connection problem"),
        ClientErrorKind::Auth => (ToastTone::Danger, "Sign-in required"),
        ClientErrorKind::RateLimited => (ToastTone::Info, "Slow down"),
        ClientErrorKind::Invalid => (ToastTone::Info, "That didn't work"),
        ClientErrorKind::Server => (ToastTone::Danger, "Server error"),
        ClientErrorKind::Storage => (ToastTone::Danger, "Storage problem"),
        ClientErrorKind::Unsupported => (ToastTone::Info, "Not available here"),
        ClientErrorKind::Unknown => (ToastTone::Danger, "Something went wrong"),
    }
}

/// App-scoped toast queue. Provided by `App`, consumed via `use_context`
/// from anywhere (event handlers, spawned tasks — the signal storage is
/// thread-safe `UnsyncStorage` by default; pushes from async tasks are
/// fine because they run on the UI thread's async runtime).
#[derive(Clone, Copy)]
pub struct ToastCenter {
    toasts: Signal<Vec<ToastEntry>>,
}

impl ToastCenter {
    /// Create + provide the center at the root scope (called once in
    /// `App`; re-runs of the component body must re-provide the same
    /// signal, so the signal itself is created here per call — Dioxus
    /// context providers overwrite idempotently).
    pub fn provide() -> Self {
        let toasts = use_signal(Vec::new);
        let center = Self { toasts };
        use_context_provider(|| center);
        center
    }

    /// Push a structured error (the standard path). The kind picks the
    /// tone + title; the message is the body.
    pub fn error(&self, err: &ClientError) {
        let (tone, title) = tone_for(err.kind);
        self.push(ToastEntry {
            id: next_id(),
            tone,
            title: title.into(),
            description: Some(err.message.clone()),
        });
    }

    /// Push a plain informational toast.
    pub fn info(&self, title: impl Into<String>, description: Option<String>) {
        self.push(ToastEntry {
            id: next_id(),
            tone: ToastTone::Info,
            title: title.into(),
            description,
        });
    }

    /// Push a success toast.
    pub fn success(&self, title: impl Into<String>, description: Option<String>) {
        self.push(ToastEntry {
            id: next_id(),
            tone: ToastTone::Success,
            title: title.into(),
            description,
        });
    }

    fn push(&self, entry: ToastEntry) {
        // `Signal` is `Copy`: take it by value so `write` works through
        // the shared `&self`.
        let mut toasts = self.toasts;
        let mut guard = toasts.write();
        guard.push(entry);
        let overflow = guard.len().saturating_sub(MAX_STACK);
        if overflow > 0 {
            guard.drain(..overflow);
        }
    }

    fn dismiss(&self, id: u64) {
        let mut toasts = self.toasts;
        toasts.write().retain(|t| t.id != id);
    }
}

/// The single render point for the toast stack: bottom-right, newest last,
/// each card auto-dismissing after [`AUTO_DISMISS_MS`]. Mount once in the
/// root layout (it reads the center from context).
#[component]
pub fn ToastHost() -> Element {
    let center = use_context::<ToastCenter>();
    let toasts = (center.toasts)();

    rsx! {
        div { style: "position:fixed;bottom:20px;right:20px;z-index:100;display:flex;flex-direction:column;gap:10px;align-items:flex-end;pointer-events:none;",
            for toast in toasts.iter() {
                ToastCard { key: "{toast.id}", entry: toast.clone() }
            }
        }
    }
}

/// One toast card: pointer-events re-enabled (the stack container lets
/// clicks through), click-to-dismiss, and a self-removing timer spawned on
/// mount. The timer races nothing: dismissal by id is idempotent.
#[component]
fn ToastCard(entry: ToastEntry) -> Element {
    let id = entry.id;
    use_effect(move || {
        let center = use_context::<ToastCenter>();
        spawn(async move {
            // `futures_timer::Delay` is the same portable sleep the
            // discovery-modal debounce uses — works on desktop and wasm.
            let _ =
                futures_timer::Delay::new(std::time::Duration::from_millis(AUTO_DISMISS_MS)).await;
            center.dismiss(id);
        });
    });

    let center = use_context::<ToastCenter>();
    rsx! {
        div { style: "pointer-events:auto;cursor:pointer;", onclick: move |_| center.dismiss(id),
            Toast {
                tone: entry.tone,
                title: entry.title.clone(),
                description: entry.description.clone(),
            }
        }
    }
}
