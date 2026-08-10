use dioxus::prelude::*;

use crate::data::Convo;
use crate::design_system::{Avatar, Badge};
use crate::icons::{Icon, IconName};

fn result_row(item: &Convo, on_select: EventHandler<String>) -> Element {
    let id = item.id.clone();
    let is_room = item.is_room();
    rsx! {
        button {
            key: "{item.id}",
            onclick: move |_| on_select.call(id.clone()),
            style: "width:100%;display:flex;align-items:center;gap:10px;padding:9px 10px;background:transparent;border:none;border-radius:var(--radius-md);cursor:pointer;text-align:left;",
            Avatar { name: "{item.name}", size: 30 }
            span { style: "flex:1;min-width:0;",
                div { style: "font-size:14px;font-weight:600;display:flex;align-items:center;gap:4px;",
                    if is_room {
                        Icon { name: IconName::Hash, size: 12, color: "var(--text-tertiary)".to_string() }
                    }
                    "{item.name}"
                }
                div { style: "font-size:12px;color:var(--text-tertiary);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", "{item.last}" }
            }
            Badge { count: item.unread as i64 }
        }
    }
}

#[component]
pub fn Switcher(
    dms: Vec<Convo>,
    rooms: Vec<Convo>,
    on_close: EventHandler<()>,
    on_select: EventHandler<String>,
) -> Element {
    let mut query = use_signal(String::new);
    let q = query().to_lowercase();
    let filtered: Vec<Convo> = dms
        .into_iter()
        .chain(rooms.into_iter())
        .filter(|c| c.name.to_lowercase().contains(&q))
        .collect();

    rsx! {
        div {
            onclick: move |_| on_close.call(()),
            style: "position:absolute;inset:0;background:rgba(0,0,0,0.5);z-index:60;display:flex;align-items:flex-start;justify-content:center;padding-top:10vh;",
            div {
                onclick: move |evt| evt.stop_propagation(),
                style: "width:480px;max-height:65vh;display:flex;flex-direction:column;background:var(--bg-surface-raised);border:1px solid var(--border-default);border-radius:var(--radius-lg);box-shadow:var(--shadow-lg);overflow:hidden;",
                div { style: "display:flex;align-items:center;gap:10px;padding:14px 16px;border-bottom:1px solid var(--border-subtle);",
                    Icon { name: IconName::Search, size: 16, color: "var(--text-tertiary)".to_string() }
                    input {
                        value: "{query}",
                        oninput: move |evt| query.set(evt.value()),
                        placeholder: "Jump to a room or person",
                        autofocus: true,
                        style: "flex:1;border:none;outline:none;background:transparent;font:15px var(--font-sans);color:var(--text-primary);",
                    }
                    span { style: "font-size:11px;color:var(--text-tertiary);font-family:var(--font-mono);border:1px solid var(--border-subtle);border-radius:4px;padding:1px 5px;", "esc" }
                }
                div { style: "overflow-y:auto;padding:8px;",
                    if filtered.is_empty() {
                        div { style: "padding:20px;text-align:center;color:var(--text-tertiary);font-size:13px;", "No matches." }
                    }
                    for item in filtered.iter() {
                        {result_row(item, on_select)}
                    }
                }
            }
        }
    }
}
