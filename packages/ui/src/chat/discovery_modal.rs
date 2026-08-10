use std::collections::HashMap;
use std::rc::Rc;

use dioxus::prelude::*;

use crate::data::VesperClient;
use crate::design_system::{Button, ButtonSize, ButtonVariant, Input, TabItem, Tabs};
use crate::icons::{Icon, IconName};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiscoveryTab {
    Rooms,
    Spaces,
}

#[component]
pub fn DiscoveryModal(on_close: EventHandler<()>) -> Element {
    let client = use_context::<Rc<dyn VesperClient>>();
    let mut tab = use_signal(|| DiscoveryTab::Rooms);
    let mut joined = use_signal(HashMap::<String, bool>::new);

    let public_rooms = {
        let client = client.clone();
        use_resource(move || {
            let client = client.clone();
            async move { client.public_rooms().await }
        })
    };
    let public_spaces = {
        let client = client.clone();
        use_resource(move || {
            let client = client.clone();
            async move { client.public_spaces().await }
        })
    };

    rsx! {
        div {
            style: "position:absolute;inset:0;background:rgba(0,0,0,0.5);display:flex;align-items:center;justify-content:center;z-index:40;",
            onclick: move |_| on_close.call(()),
            div {
                onclick: move |evt| evt.stop_propagation(),
                style: "width:460px;max-height:70vh;display:flex;flex-direction:column;background:var(--bg-surface-raised);border-radius:var(--radius-lg);border:1px solid var(--border-default);box-shadow:var(--shadow-lg);font-family:var(--font-sans);",
                div { style: "padding:18px 20px 0;display:flex;align-items:center;justify-content:space-between;",
                    div { style: "font-size:17px;font-weight:700;", "Browse the network" }
                    button {
                        onclick: move |_| on_close.call(()),
                        style: "background:none;border:none;color:var(--text-secondary);cursor:pointer;display:flex;",
                        Icon { name: IconName::X, size: 18 }
                    }
                }
                div { style: "padding:10px 20px 0;",
                    Tabs {
                        tabs: vec![
                            TabItem { value: "rooms".into(), label: "Rooms".into() },
                            TabItem { value: "spaces".into(), label: "Spaces".into() },
                        ],
                        active: if tab() == DiscoveryTab::Rooms { "rooms".to_string() } else { "spaces".to_string() },
                        on_change: move |v: String| tab.set(if v == "rooms" { DiscoveryTab::Rooms } else { DiscoveryTab::Spaces }),
                    }
                }
                div { style: "padding:16px;",
                    Input { placeholder: if tab() == DiscoveryTab::Rooms { "Search rooms" } else { "Search spaces" } }
                }
                div { style: "overflow-y:auto;padding:0 16px 16px;display:flex;flex-direction:column;gap:8px;",
                    if tab() == DiscoveryTab::Rooms {
                        for item in public_rooms().unwrap_or_default().iter() {
                            {
                                let id = item.id.clone();
                                let is_joined = joined().get(&id).copied().unwrap_or(false);
                                rsx! {
                                    div { key: "{item.id}", style: "display:flex;align-items:center;gap:10px;padding:10px 12px;border:1px solid var(--border-subtle);border-radius:var(--radius-md);",
                                        Icon { name: IconName::Hash, size: 16, color: "var(--text-tertiary)".to_string() }
                                        div { style: "flex:1;min-width:0;",
                                            div { style: "font-size:14px;font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", "{item.name}" }
                                            div { style: "font-size:12px;color:var(--text-tertiary);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", "{item.topic} · {item.members} members" }
                                        }
                                        Button {
                                            variant: if is_joined { ButtonVariant::Secondary } else { ButtonVariant::Primary },
                                            size: ButtonSize::Sm,
                                            onclick: move |_| {
                                                let entry = joined().get(&id).copied().unwrap_or(false);
                                                joined.write().insert(id.clone(), !entry);
                                            },
                                            if is_joined { "Joined" } else { "Join" }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        for item in public_spaces().unwrap_or_default().iter() {
                            {
                                let id = item.id.clone();
                                let is_joined = joined().get(&id).copied().unwrap_or(false);
                                rsx! {
                                    div { key: "{item.id}", style: "display:flex;align-items:center;gap:10px;padding:10px 12px;border:1px solid var(--border-subtle);border-radius:var(--radius-md);",
                                        Icon { name: IconName::Layers, size: 16, color: "var(--text-tertiary)".to_string() }
                                        div { style: "flex:1;min-width:0;",
                                            div { style: "font-size:14px;font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", "{item.name}" }
                                            div { style: "font-size:12px;color:var(--text-tertiary);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", "{item.rooms} rooms" }
                                        }
                                        Button {
                                            variant: if is_joined { ButtonVariant::Secondary } else { ButtonVariant::Primary },
                                            size: ButtonSize::Sm,
                                            onclick: move |_| {
                                                let entry = joined().get(&id).copied().unwrap_or(false);
                                                joined.write().insert(id.clone(), !entry);
                                            },
                                            if is_joined { "Joined" } else { "Join" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
