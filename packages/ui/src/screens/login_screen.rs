use std::rc::Rc;

use dioxus::prelude::*;

use crate::data::{Me, VesperClient};
use crate::design_system::{Button, ButtonVariant, Input};
use crate::window_chrome::{use_titlebar, WindowControls};

const LOGO: Asset = asset!("/assets/vesper/logo.svg");

#[component]
pub fn LoginScreen(on_login: EventHandler<Me>) -> Element {
    let client = use_context::<Rc<dyn VesperClient>>();
    let mut homeserver = use_signal(|| "matrix.org".to_string());
    let mut id = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);

    // `use_callback` returns a `Copy` Callback, so the button's onclick and the
    // Enter-key handler can share one submission path.
    let do_submit = use_callback(move |_: ()| {
        if pending() {
            return;
        }
        pending.set(true);
        error.set(None);
        let client = client.clone();
        let homeserver = homeserver();
        let user_id = id();
        let pw = password();
        spawn(async move {
            match client.login(homeserver, user_id, pw).await {
                Ok(me) => on_login.call(me),
                Err(e) => {
                    error.set(Some(e.message));
                    // Drop the password so a typo can't linger, but keep what
                    // the user typed in the other fields.
                    password.set(String::new());
                }
            }
            pending.set(false);
        });
    });

    let titlebar = use_titlebar();

    rsx! {
        div {
            style: "width:100%;height:100%;display:flex;align-items:center;justify-content:center;background:var(--bg-canvas);font-family:var(--font-sans);padding:16px;position:relative;",
            onmousedown: move |evt| titlebar.on_drag.call(evt),
            ondoubleclick: move |evt| titlebar.on_toggle_maximize.call(evt),
            div {
                style: "position:absolute;top:4px;right:4px;",
                // Keep the window controls clickable while the rest of the
                // background doubles as a drag region.
                onmousedown: move |evt| evt.stop_propagation(),
                WindowControls {}
            }
            div {
                style: "width:340px;display:flex;flex-direction:column;gap:20px;align-items:center;",
                // Single boundary that isolates the form's pointer events from
                // the background drag region — no per-input stop_propagation.
                onmousedown: move |evt| evt.stop_propagation(),
                // Enter from any input submits (events bubble up from the fields).
                onkeydown: move |evt| {
                    if evt.key() == Key::Enter {
                        do_submit(());
                    }
                },
                img { src: LOGO, alt: "Vesper", style: "width:64px;height:64px;border-radius:999px;" }
                div { style: "text-align:center;",
                    div { style: "font-size:22px;font-weight:800;", "Sign in to Vesper" }
                    div { style: "font-size:13px;color:var(--text-secondary);margin-top:4px;", "Encrypted, federated chat on Matrix" }
                }
                div { style: "width:100%;display:flex;flex-direction:column;gap:12px;",
                    Input {
                        label: "Homeserver",
                        placeholder: "matrix.org",
                        value: "{homeserver}",
                        on_change: move |v: String| if !pending() { homeserver.set(v) },
                    }
                    Input {
                        label: "Matrix ID or username",
                        placeholder: "@you:matrix.org",
                        value: "{id}",
                        on_change: move |v: String| if !pending() { id.set(v) },
                    }
                    Input {
                        label: "Password",
                        input_type: "password",
                        value: "{password}",
                        on_change: move |v: String| if !pending() { password.set(v) },
                    }
                    if let Some(err) = error() {
                        div {
                            style: "font-size:13px;color:var(--status-danger);text-align:center;",
                            role: "alert",
                            "{err}"
                        }
                    }
                    Button {
                        variant: ButtonVariant::Primary,
                        onclick: move |_| do_submit(()),
                        if pending() { "Signing in…" } else { "Sign in" }
                    }
                    div { style: "font-size:12px;color:var(--text-tertiary);text-align:center;", "You can also connect with an existing session QR code." }
                }
            }
        }
    }
}
