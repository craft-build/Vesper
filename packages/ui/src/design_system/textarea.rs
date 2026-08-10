use dioxus::prelude::*;

#[component]
pub fn Textarea(
    #[props(default = None)] label: Option<String>,
    #[props(default = String::new())] placeholder: String,
    #[props(default = String::new())] value: String,
    #[props(default = None)] on_change: Option<EventHandler<String>>,
    #[props(default = 3)] rows: i64,
) -> Element {
    let mut focus = use_signal(|| false);
    let border = if focus() {
        "var(--border-brand)"
    } else {
        "var(--border-default)"
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
            textarea {
                rows: "{rows}",
                placeholder: "{placeholder}",
                value: "{value}",
                oninput: move |evt| {
                    if let Some(handler) = &on_change {
                        handler.call(evt.value());
                    }
                },
                onfocus: move |_| focus.set(true),
                onblur: move |_| focus.set(false),
                style: "font:14px/1.5 var(--font-sans);color:var(--text-primary);background:var(--bg-surface);border:1px solid {border};border-radius:var(--radius-sm);padding:10px 12px;outline:none;resize:vertical;box-shadow:{shadow};",
            }
        }
    }
}
