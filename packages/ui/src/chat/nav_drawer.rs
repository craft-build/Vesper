use dioxus::prelude::*;

use crate::data::{Convo, Presence};
use crate::design_system::{Avatar, Badge, StatusDot};
use crate::icons::{Icon, IconName};
use crate::window_chrome::DragStrip;

const LOGO: Asset = asset!("/assets/vesper/logo.svg");

fn row(item: &Convo, is_dm: bool, active_id: &str, on_select: EventHandler<String>) -> Element {
    let id = item.id.clone();
    let is_active = active_id == item.id;
    let bg = if is_active {
        "var(--bg-selected)"
    } else {
        "transparent"
    };
    let status = item.status.unwrap_or(Presence::Offline);
    rsx! {
        button {
            key: "{item.id}",
            onclick: move |_| on_select.call(id.clone()),
            style: "width:100%;display:flex;align-items:center;gap:10px;padding:8px 10px;background:{bg};border:none;border-radius:var(--radius-md);cursor:pointer;text-align:left;margin-bottom:2px;",
            span { style: "position:relative;flex-shrink:0;",
                Avatar { name: "{item.name}", size: 32 }
                if is_dm {
                    span { style: "position:absolute;right:-2px;bottom:-2px;", StatusDot { status, size: 9 } }
                }
            }
            span { style: "flex:1;min-width:0;font-size:14px;font-weight:600;display:flex;align-items:center;gap:4px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;",
                if !is_dm {
                    Icon { name: IconName::Hash, size: 12, color: "var(--text-tertiary)".to_string() }
                }
                "{item.name}"
            }
            Badge { count: item.unread as i64 }
        }
    }
}

fn drawer_body(
    dms: &[Convo],
    rooms: &[Convo],
    active_id: &str,
    on_select: EventHandler<String>,
    on_close: EventHandler<()>,
    on_open_discovery: EventHandler<()>,
    on_open_settings: EventHandler<()>,
    on_open_self: EventHandler<()>,
) -> Element {
    rsx! {
        div { style: "width:280px;height:100%;background:var(--bg-surface);display:flex;flex-direction:column;",
            div {
                style: "padding:16px 16px 8px;display:flex;align-items:center;gap:8px;height:56px;",
                img { src: LOGO, alt: "", style: "width:26px;height:26px;border-radius:999px;" }
                span { style: "font-weight:800;font-size:15px;", "Vesper" }
                DragStrip {}
                button {
                    onclick: move |_| on_close.call(()),
                    style: "background:none;border:none;color:var(--text-secondary);cursor:pointer;display:flex;",
                    Icon { name: IconName::X, size: 17 }
                }
            }
            div { style: "flex:1;overflow-y:auto;padding:4px 10px;",
                if !dms.is_empty() {
                    div { style: "font-size:11px;font-weight:700;letter-spacing:0.06em;color:var(--text-tertiary);padding:10px 8px 4px;", "DIRECT MESSAGES" }
                    for d in dms.iter() {
                        {row(d, true, active_id, on_select)}
                    }
                }
                if !rooms.is_empty() {
                    div { style: "font-size:11px;font-weight:700;letter-spacing:0.06em;color:var(--text-tertiary);padding:14px 8px 4px;", "ROOMS" }
                    for r in rooms.iter() {
                        {row(r, false, active_id, on_select)}
                    }
                }
            }
            div { style: "padding:10px;border-top:1px solid var(--border-subtle);display:flex;gap:4px;",
                button {
                    onclick: move |_| on_open_discovery.call(()),
                    style: "flex:1;display:flex;align-items:center;gap:6px;background:none;border:none;color:var(--text-secondary);cursor:pointer;padding:8px;border-radius:var(--radius-md);font-size:13px;",
                    Icon { name: IconName::Plus, size: 15 }
                    "Browse"
                }
                button {
                    onclick: move |_| on_open_settings.call(()),
                    style: "background:none;border:none;color:var(--text-secondary);cursor:pointer;padding:8px;border-radius:var(--radius-md);display:flex;",
                    Icon { name: IconName::Settings, size: 16 }
                }
                button {
                    onclick: move |_| on_open_self.call(()),
                    style: "background:none;border:none;color:var(--text-secondary);cursor:pointer;padding:8px;border-radius:var(--radius-md);display:flex;",
                    Avatar { name: "You", size: 20 }
                }
            }
        }
    }
}

#[component]
pub fn NavDrawer(
    dms: Vec<Convo>,
    rooms: Vec<Convo>,
    active_id: String,
    on_select: EventHandler<String>,
    on_close: EventHandler<()>,
    on_open_discovery: EventHandler<()>,
    on_open_settings: EventHandler<()>,
    on_open_self: EventHandler<()>,
    #[props(default = false)] inline: bool,
) -> Element {
    if inline {
        rsx! {
            div { style: "width:280px;flex-shrink:0;height:100%;border-right:1px solid var(--border-subtle);",
                {drawer_body(&dms, &rooms, &active_id, on_select, on_close, on_open_discovery, on_open_settings, on_open_self)}
            }
        }
    } else {
        rsx! {
            div { onclick: move |_| on_close.call(()), style: "position:absolute;inset:0;background:rgba(0,0,0,0.4);z-index:30;" }
            div { style: "position:absolute;left:0;top:0;bottom:0;z-index:31;box-shadow:var(--shadow-lg);",
                {drawer_body(&dms, &rooms, &active_id, on_select, on_close, on_open_discovery, on_open_settings, on_open_self)}
            }
        }
    }
}
