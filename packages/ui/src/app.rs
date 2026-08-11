use dioxus::document;
use dioxus::prelude::*;

use crate::chat::{AppShell, ChatUiState, ChatView, DiscoveryModal};
use crate::data::{self, ClientState, Convo, Me};
use crate::screens::{LoginScreen, SettingsScreen};

const STYLES: Asset = asset!("/assets/vesper/styles.css");

#[derive(Clone, Routable, PartialEq)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Shell)]
        #[route("/")]
        Home {},
        #[route("/room/:room_id")]
        RoomView { room_id: String },
        #[route("/settings")]
        SettingsPage {},
}

/// Root component. Owns the login gate (a plain signal, outside the router — there's
/// nothing to deep-link to before you're signed in) and provides the one piece every
/// screen depends on: `Rc<dyn VesperClient>`. The backend (mock vs real Matrix) is
/// chosen once inside [`data::backend`] — the ONLY place it gets decided.
#[component]
pub fn App() -> Element {
    let client = data::backend();
    use_context_provider(|| client.clone());
    use_context_provider(ChatUiState::new);
    let mut me = use_context_provider(|| Signal::new(Option::<Me>::None));

    // Live backend state, written by sync tasks that run on the Matrix
    // runtime thread. These MUST be sync-storage signals: the default
    // `Signal::<T>::new` storage is thread-local and would fail when written
    // off the UI thread. Provided as context and handed to the backend, which
    // publishes into them (idempotent — re-run bodies re-bind harmlessly).
    let convos = use_signal_sync(Vec::<Convo>::new);
    let connecting = use_signal_sync(|| false);
    // Per-room timelines (checkpoint 04): one map signal, rooms added lazily
    // as they are opened — see `ClientState::messages`.
    let messages = use_signal_sync(std::collections::BTreeMap::<String, Vec<data::Message>>::new);
    let sync_state = ClientState {
        convos,
        connecting,
        messages,
    };
    use_context_provider(|| sync_state);
    use_effect({
        let client = client.clone();
        move || client.bind_state(sync_state)
    });

    // Before the login screen paints, try to restore a persisted session.
    // `Ok(None)` = no stored session (normal first run); `Err` = a stored
    // session that is unusable — a warning is logged and we fall through to
    // the login screen either way. Never a panic.
    //
    // `me` is written *inside* the resource's future, not a post-render effect:
    // effects run a render after the resource resolves, which would flash the
    // LoginScreen for one frame on a successful relaunch restore.
    let restore = use_resource({
        let client = client.clone();
        move || {
            let client = client.clone();
            async move {
                let result = client.restore().await;
                match &result {
                    Ok(Some(restored)) => me.set(Some(restored.clone())),
                    Ok(None) => {}
                    Err(e) => tracing::warn!("session restore failed, showing login: {e}"),
                }
                result
            }
        }
    });

    let restoring = me().is_none() && restore.read().is_none();

    rsx! {
        document::Link { rel: "stylesheet", href: STYLES }
        if restoring {
            Splash {}
        } else if me().is_some() {
            Router::<Route> {}
        } else {
            LoginScreen { on_login: move |m| me.set(Some(m)) }
        }
        crate::window_chrome::ResizeBorders {}
    }
}

/// Shown while a stored session is being validated: just the logo, centered.
#[component]
fn Splash() -> Element {
    rsx! {
        div { style: "width:100%;height:100%;display:flex;align-items:center;justify-content:center;background:var(--bg-canvas);",
            img {
                src: asset!("/assets/vesper/logo.svg"),
                alt: "Vesper",
                style: "width:64px;height:64px;border-radius:999px;",
            }
        }
    }
}

#[component]
fn Shell() -> Element {
    let mut ui = use_context::<ChatUiState>();
    let sync = use_context::<ClientState>();

    use_effect(move || {
        spawn(async move {
            let mut eval = document::eval(
                r#"
                const send = (v) => dioxus.send(v);
                const mq = window.matchMedia('(max-width:860px)');
                send(mq.matches);
                mq.addEventListener('change', (e) => send(e.matches));
                "#,
            );
            while let Ok(value) = eval.recv::<bool>().await {
                ui.is_mobile.set(value);
            }
        });
    });

    use_effect(move || {
        spawn(async move {
            let mut eval = document::eval(
                r#"
                const send = (v) => dioxus.send(v);
                window.addEventListener('keydown', (e) => {
                    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') { e.preventDefault(); send('switcher'); }
                    if (e.key === 'Escape') { send('escape'); }
                });
                "#,
            );
            while let Ok(value) = eval.recv::<String>().await {
                match value.as_str() {
                    "switcher" => ui.switcher_open.set(true),
                    "escape" => {
                        ui.switcher_open.set(false);
                        ui.nav_open.set(false);
                    }
                    _ => {}
                }
            }
        });
    });

    rsx! {
        AppShell {
            Outlet::<Route> {}
        }
        if (sync.connecting)() {
            div { style: "position:absolute;top:12px;left:50%;transform:translateX(-50%);z-index:50;background:var(--bg-surface-raised);border:1px solid var(--border-subtle);color:var(--text-secondary);font-size:12px;padding:4px 12px;border-radius:999px;",
                "Connecting…"
            }
        }
        if (ui.discovery_open)() {
            DiscoveryModal { on_close: move |_| ui.discovery_open.set(false) }
        }
    }
}

#[component]
fn Home() -> Element {
    rsx! {
        ChatView {}
    }
}

#[component]
fn RoomView(room_id: String) -> Element {
    rsx! {
        ChatView { room_id }
    }
}

#[component]
fn SettingsPage() -> Element {
    let navigator = use_navigator();
    rsx! {
        SettingsScreen { on_close: move |_| { navigator.push(Route::Home {}); } }
    }
}
