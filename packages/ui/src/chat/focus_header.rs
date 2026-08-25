use dioxus::prelude::*;

use crate::data::{Convo, ConvoKind};
use crate::design_system::{Tag, TagTone};
use crate::icons::{Icon, IconName};
use crate::window_chrome::{DragStrip, WindowControls};

fn icon_button(title: &str, onclick: impl FnMut(MouseEvent) + 'static, name: IconName) -> Element {
    rsx! {
        button {
            title: "{title}",
            onclick,
            style: "width:36px;height:36px;border-radius:var(--radius-md);border:none;background:transparent;color:var(--text-secondary);cursor:pointer;display:flex;align-items:center;justify-content:center;",
            Icon { name, size: 16 }
        }
    }
}

#[component]
pub fn FocusHeader(
    convo: Convo,
    on_open_nav: EventHandler<()>,
    on_open_switcher: EventHandler<()>,
    on_open_room_info: EventHandler<()>,
) -> Element {
    rsx! {
        div {
            style: "height:56px;border-bottom:1px solid var(--border-subtle);display:flex;align-items:center;padding:0 14px;gap:6px;flex-shrink:0;",
            button {
                title: "Rooms",
                onclick: move |_| on_open_nav.call(()),
                style: "background:none;border:none;color:var(--text-secondary);cursor:pointer;display:flex;padding:8px;border-radius:var(--radius-md);",
                Icon { name: IconName::Layers, size: 18 }
            }
            button {
                onclick: move |_| on_open_switcher.call(()),
                style: "display:flex;align-items:center;gap:8px;background:var(--bg-surface-raised);border:1px solid var(--border-subtle);border-radius:var(--radius-md);padding:7px 12px;cursor:pointer;color:var(--text-primary);min-width:0;",
                if convo.kind == ConvoKind::Room {
                    Icon { name: IconName::Hash, size: 13, color: "var(--text-tertiary)".to_string() }
                }
                span { style: "font-weight:700;font-size:14px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", "{convo.name}" }
                Icon { name: IconName::Search, size: 13, color: "var(--text-tertiary)".to_string() }
            }
            if convo.encrypted {
                Tag { tone: TagTone::Brand, "e2ee" }
            }
            DragStrip {}
            {icon_button("Room info", move |_| on_open_room_info.call(()), IconName::Info)}
            WindowControls {}
        }
    }
}
