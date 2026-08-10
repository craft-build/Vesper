use std::rc::Rc;

use dioxus::prelude::*;

use crate::data::{Me, VesperClient};
use crate::design_system::{Button, ButtonVariant, Input};
use crate::window_chrome::{use_titlebar, WindowControls};

const LOGO: Asset = asset!("/assets/vesper/logo.svg");

#[component]
pub fn LoginScreen(on_login: EventHandler<Me>) -> Element {
    let client = use_context::<Rc<dyn VesperClient>>();
    let mut id = use_signal(String::new);
    let mut password = use_signal(String::new);

    let submit = move |_| {
        let client = client.clone();
        let user_id = id();
        let pw = password();
        spawn(async move {
            if let Ok(me) = client.login(user_id, pw).await {
                on_login.call(me);
            }
        });
    };

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
                img { src: LOGO, alt: "Vesper", style: "width:64px;height:64px;border-radius:999px;" }
                div { style: "text-align:center;",
                    div { style: "font-size:22px;font-weight:800;", "Sign in to Vesper" }
                    div { style: "font-size:13px;color:var(--text-secondary);margin-top:4px;", "Encrypted, federated chat on Matrix" }
                }
                div { style: "width:100%;display:flex;flex-direction:column;gap:12px;",
                    Input { label: "Matrix ID", placeholder: "@you:matrix.org", value: "{id}", on_change: move |v| id.set(v) }
                    Input { label: "Password", input_type: "password", value: "{password}", on_change: move |v| password.set(v) }
                    Button { variant: ButtonVariant::Primary, onclick: submit, "Sign in" }
                    div { style: "font-size:12px;color:var(--text-tertiary);text-align:center;", "You can also connect with an existing session QR code." }
                }
            }
        }
    }
}
