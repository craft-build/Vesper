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
        div { class: "ds-tabs",
            for tab in tabs.iter() {
                {
                    let value = tab.value.clone();
                    let class = if active == tab.value {
                        "ds-tab ds-tab--active"
                    } else {
                        "ds-tab"
                    };
                    rsx! {
                        button {
                            key: "{tab.value}",
                            class: "{class}",
                            onclick: move |_| {
                                if let Some(handler) = &on_change {
                                    handler.call(value.clone());
                                }
                            },
                            "{tab.label}"
                        }
                    }
                }
            }
        }
    }
}
