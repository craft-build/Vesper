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
        label { style: "display:flex;flex-direction:column;gap:6px;font-family:var(--font-sans);",
            if let Some(label) = &label {
                span { style: "font-size:13px;font-weight:500;color:var(--text-secondary);", "{label}" }
            }
            select {
                value: "{value}",
                onchange: move |evt| {
                    if let Some(handler) = &on_change {
                        handler.call(evt.value());
                    }
                },
                style: "font:14px var(--font-sans);color:var(--text-primary);background:var(--bg-surface);border:1px solid var(--border-default);border-radius:var(--radius-sm);padding:10px 12px;outline:none;",
                for opt in options.iter() {
                    option { key: "{opt.value}", value: "{opt.value}", "{opt.label}" }
                }
            }
        }
    }
}
