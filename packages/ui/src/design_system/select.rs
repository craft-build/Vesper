use dioxus::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

#[component]
pub fn Select(
    #[props(default = None)] label: Option<String>,
    #[props(default = String::new())] value: String,
    #[props(default = None)] on_change: Option<EventHandler<String>>,
    #[props(default = Vec::new())] options: Vec<SelectOption>,
) -> Element {
    rsx! {
        label { class: "ds-label",
            if let Some(label) = &label {
                span { class: "ds-field-label", "{label}" }
            }
            select {
                class: "ds-field",
                value: "{value}",
                onchange: move |evt| {
                    if let Some(handler) = &on_change {
                        handler.call(evt.value());
                    }
                },
                for opt in options.iter() {
                    option { key: "{opt.value}", value: "{opt.value}", "{opt.label}" }
                }
            }
        }
    }
}
