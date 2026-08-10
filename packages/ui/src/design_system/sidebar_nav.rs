use dioxus::prelude::*;

use crate::icons::{Icon, IconName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarNavItem {
    pub value: String,
    pub label: String,
    pub icon: Option<IconName>,
}

#[component]
pub fn SidebarNav(
    #[props(default = Vec::new())] items: Vec<SidebarNavItem>,
    #[props(default = String::new())] active: String,
    #[props(default = None)] on_change: Option<EventHandler<String>>,
) -> Element {
    rsx! {
        nav { style: "display:flex;flex-direction:column;gap:2px;font-family:var(--font-sans);",
            for item in items.iter() {
                {
                    let value = item.value.clone();
                    let is_active = active == item.value;
                    let bg = if is_active { "var(--bg-selected)" } else { "transparent" };
                    let color = if is_active { "var(--text-brand)" } else { "var(--text-secondary)" };
                    rsx! {
                        button {
                            key: "{item.value}",
                            onclick: move |_| {
                                if let Some(handler) = &on_change {
                                    handler.call(value.clone());
                                }
                            },
                            style: "display:flex;align-items:center;gap:10px;background:{bg};border:none;text-align:left;border-radius:var(--radius-sm);padding:9px 10px;cursor:pointer;color:{color};font-size:14px;font-weight:500;",
                            if let Some(icon) = item.icon {
                                Icon { name: icon, size: 16 }
                            }
                            "{item.label}"
                        }
                    }
                }
            }
        }
    }
}
