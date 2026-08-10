use dioxus::prelude::*;

#[component]
pub fn Input(
    #[props(default = None)] label: Option<String>,
    #[props(default = String::new())] placeholder: String,
    #[props(default = "text".to_string())] input_type: String,
    #[props(default = String::new())] value: String,
    #[props(default = None)] on_change: Option<EventHandler<String>>,
    #[props(default = None)] error: Option<String>,
) -> Element {
    let mut focus = use_signal(|| false);
    let border = if error.is_some() {
        "var(--status-danger)".to_string()
    } else if focus() {
        "var(--border-brand)".to_string()
    } else {
        "var(--border-default)".to_string()
    };
    let shadow = if focus() {
        "var(--shadow-focus)"
    } else {
        "none"
    };
    rsx! {
        label { style: "display:flex;flex-direction:column;gap:6px;font-family:var(--font-sans);",
            if let Some(label) = &label {
                span { style: "font-size:13px;font-weight:500;color:var(--text-secondary);", "{label}" }
            }
            input {
                r#type: "{input_type}",
                placeholder: "{placeholder}",
                value: "{value}",
                oninput: move |evt| {
                    if let Some(handler) = &on_change {
                        handler.call(evt.value());
                    }
                },
                onfocus: move |_| focus.set(true),
                onblur: move |_| focus.set(false),
                style: "font:14px var(--font-sans);color:var(--text-primary);background:var(--bg-surface);border:1px solid {border};border-radius:var(--radius-sm);padding:10px 12px;outline:none;box-shadow:{shadow};transition:box-shadow var(--duration-fast) var(--ease-standard), border-color var(--duration-fast) var(--ease-standard);",
            }
            if let Some(error) = &error {
                span { style: "font-size:12px;color:var(--status-danger);", "{error}" }
            }
        }
    }
}
