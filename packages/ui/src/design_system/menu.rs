use dioxus::prelude::*;

use crate::icons::{Icon, IconName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub value: String,
    pub label: String,
    pub icon: Option<IconName>,
    pub danger: bool,
}

#[component]
pub fn Menu(
    #[props(default = Vec::new())] items: Vec<MenuItem>,
    #[props(default = None)] on_select: Option<EventHandler<String>>,
) -> Element {
    rsx! {
        div { style: "background:var(--bg-surface-raised);border:1px solid var(--border-default);border-radius:var(--radius-md);box-shadow:var(--shadow-md);padding:6px;min-width:180px;font-family:var(--font-sans);display:flex;flex-direction:column;gap:1px;",
            for item in items.iter() {
                {
                    let value = item.value.clone();
                    let color = if item.danger { "var(--status-danger)" } else { "var(--text-primary)" };
                    rsx! {
                        button {
                            key: "{item.value}",
                            onclick: move |_| {
                                if let Some(handler) = &on_select {
                                    handler.call(value.clone());
                                }
                            },
                            style: "display:flex;align-items:center;gap:8px;background:none;border:none;text-align:left;padding:8px 10px;border-radius:var(--radius-sm);cursor:pointer;font-size:14px;color:{color};",
                            if let Some(icon) = item.icon {
                                Icon { name: icon, size: 15 }
                            }
                            "{item.label}"
                        }
                    }
                }
            }
        }
    }
}
