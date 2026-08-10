use dioxus::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabItem {
    pub value: String,
    pub label: String,
}

#[component]
pub fn Tabs(
    #[props(default = Vec::new())] tabs: Vec<TabItem>,
    #[props(default = String::new())] active: String,
    #[props(default = None)] on_change: Option<EventHandler<String>>,
) -> Element {
    rsx! {
        div { style: "display:flex;gap:4px;border-bottom:1px solid var(--border-subtle);font-family:var(--font-sans);",
            for tab in tabs.iter() {
                {
                    let value = tab.value.clone();
                    let is_active = active == tab.value;
                    let color = if is_active { "var(--text-primary)" } else { "var(--text-tertiary)" };
                    let border = if is_active { "var(--bg-brand)" } else { "transparent" };
                    rsx! {
                        button {
                            key: "{tab.value}",
                            onclick: move |_| {
                                if let Some(handler) = &on_change {
                                    handler.call(value.clone());
                                }
                            },
                            style: "background:none;border:none;cursor:pointer;padding:10px 14px;font-size:14px;font-weight:600;color:{color};border-bottom:2px solid {border};margin-bottom:-1px;transition:color var(--duration-fast) var(--ease-standard);",
                            "{tab.label}"
                        }
                    }
                }
            }
        }
    }
}
