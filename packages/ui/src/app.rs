use std::rc::Rc;

use dioxus::document;
use dioxus::prelude::*;

use crate::chat::{AppShell, ChatUiState, ChatView, DiscoveryModal};
use crate::data::{Me, MockClient, VesperClient};
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
/// screen depends on: `Rc<dyn VesperClient>`. Swapping mock data for real Matrix
/// integration later means changing the line below, nothing else.
#[component]
pub fn App() -> Element {
    use_context_provider(|| Rc::new(MockClient::default()) as Rc<dyn VesperClient>);
    use_context_provider(ChatUiState::new);
    let mut me = use_context_provider(|| Signal::new(Option::<Me>::None));

    rsx! {
        document::Link { rel: "stylesheet", href: STYLES }
        match me() {
            Some(_) => rsx! {
                Router::<Route> {}
            },
            None => rsx! {
                LoginScreen { on_login: move |m| me.set(Some(m)) }
            },
        }
        crate::window_chrome::ResizeBorders {}
    }
}

#[component]
fn Shell() -> Element {
    let mut ui = use_context::<ChatUiState>();

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
