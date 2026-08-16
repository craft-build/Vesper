use dioxus::document;
use dioxus::prelude::*;

use crate::chat::{AppShell, ChatUiState, ChatView, DiscoveryModal};
use crate::data::{self, ClientState, Convo, Me};
use crate::design_system::ToastCenter;
use crate::screens::{LoginScreen, SettingsScreen};

#[cfg(not(target_arch = "wasm32"))]
use client::diagnostics::Screen;

// wasm has no diagnostics module (client builds feature-free there); a
// local mirror keeps the call sites identical across targets.
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy)]
enum Screen {
    #[allow(dead_code)]
    Home,
    #[allow(dead_code)]
    Room,
    #[allow(dead_code)]
    Settings,
    #[allow(dead_code)]
    Login,
}

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
    // The single toast surface (checkpoint 11): errors from anywhere push
    // here; `ToastHost` below the router renders them.
    ToastCenter::provide();

    // Live backend state, written by sync tasks that run on the Matrix
    // runtime thread. These MUST be sync-storage signals: the default
    // `Signal::<T>::new` storage is thread-local and would fail when written
    // off the UI thread. Provided as context and handed to the backend, which
    // publishes into them (idempotent — re-run bodies re-bind harmlessly).
    let convos = use_signal_sync(Vec::<Convo>::new);
    let connecting = use_signal_sync(|| false);
    // Per-room timelines (checkpoint 04) and live thread panels
    // (checkpoint 05): one map signal each, entries added lazily on open.
    let messages = use_signal_sync(std::collections::BTreeMap::<String, Vec<data::Message>>::new);
    let threads =
        use_signal_sync(std::collections::BTreeMap::<String, Vec<data::ThreadReply>>::new);
    // Live state (checkpoint 06): incoming typing + presence are written by
    // the backend's tokio tasks; `focused` / `active_room` are written by the
    // UI and read by the desktop-notification task to gate notifications. All
    // four are sync-storage signals created here in the App scope and never
    // created off it (the one-map lifecycle; see `ClientState` docs).
    let typing = use_signal_sync(std::collections::BTreeMap::<String, Vec<String>>::new);
    let presence = use_signal_sync(std::collections::BTreeMap::<String, data::Presence>::new);
    let focused = use_signal_sync(|| true);
    let active_room = use_signal_sync(|| None::<String>);
    let media = use_signal_sync(std::collections::BTreeMap::<String, String>::new);
    // Joined spaces (checkpoint 09): written by the room-list sync task each
    // time the room list or a space's children change; the nav drawer reads
    // it to group rooms. Same App-scope lifecycle as the maps above.
    let spaces = use_signal_sync(Vec::<data::Space>::new);
    // Active verification session (checkpoint 08): written by the backend's
    // verification driver task, read by the verify dialog. One slot, App-scope
    // lifecycle like the maps above.
    let verification = use_signal_sync(|| None::<data::VerificationSession>);
    let sync_state = ClientState {
        convos,
        connecting,
        messages,
        threads,
        typing,
        presence,
        focused,
        active_room,
        media,
        spaces,
        verification,
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
                // The elapsed log is the store-open half of the cold-start
                // budget (checkpoint 11 §C: <3s to interactive on a warm
                // store; the launcher logs the pre-launch half).
                let started = std::time::Instant::now();
                let result = client.restore().await;
                tracing::info!(
                    target: "vesper_startup",
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    has_session = matches!(&result, Ok(Some(_))),
                    "session restore finished"
                );
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
            { track_screen(Screen::Login); }
            LoginScreen { on_login: move |m| me.set(Some(m)) }
        }
        crate::window_chrome::ResizeBorders {}
        crate::design_system::ToastHost {}
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

    // Window focus (checkpoint 06): write `sync.focused` from the browser's
    // focus/blur + visibilitychange events so the desktop-notification task
    // can suppress notifications while the user is looking at the app. The
    // signal is guarded against same-value writes to avoid needlessly
    // dirtying readers.
    use_effect(move || {
        spawn(async move {
            let mut eval = document::eval(
                r#"
                const send = (v) => dioxus.send(v);
                send(document.hasFocus());
                window.addEventListener('focus', () => send(true));
                window.addEventListener('blur', () => send(false));
                document.addEventListener('visibilitychange', () => send(document.visibilityState === 'visible' && document.hasFocus()));
                "#,
            );
            let mut focused = sync.focused;
            while let Ok(is_focused) = eval.recv::<bool>().await {
                if focused() != is_focused {
                    focused.set(is_focused);
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
    track_screen(Screen::Home);
    rsx! {
        ChatView {}
    }
}

#[component]
fn RoomView(room_id: String) -> Element {
    // Only the screen *name* is recorded — never the room id or any
    // content (checkpoint 11 §D).
    track_screen(Screen::Room);
    rsx! {
        ChatView { room_id }
    }
}

#[component]
fn SettingsPage() -> Element {
    let navigator = use_navigator();
    track_screen(Screen::Settings);
    rsx! {
        SettingsScreen { on_close: move |_| { navigator.push(Route::Home {}); } }
    }
}

/// Record the coarse screen for panic dumps (native only: the diagnostics
/// module lives behind the client crate's `matrix` feature; wasm no-ops).
#[cfg(not(target_arch = "wasm32"))]
fn track_screen(screen: client::diagnostics::Screen) {
    client::diagnostics::set_last_screen(screen);
}

#[cfg(target_arch = "wasm32")]
fn track_screen(_screen: Screen) {}
