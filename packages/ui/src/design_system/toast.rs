use dioxus::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastTone {
    Info,
    Success,
    Danger,
}

#[component]
pub fn Toast(
    #[props(default = ToastTone::Info)] tone: ToastTone,
    title: String,
    #[props(default = None)] description: Option<String>,
) -> Element {
    let accent = match tone {
        ToastTone::Info => "var(--text-brand)",
        ToastTone::Success => "var(--status-online)",
        ToastTone::Danger => "var(--status-danger)",
    };
    let style = format!(
        "display:flex;gap:12px;align-items:flex-start;background:var(--bg-surface-raised);border:1px solid var(--border-subtle);border-left:3px solid {accent};border-radius:var(--radius-md);padding:12px 14px;box-shadow:var(--shadow-md);max-width:320px;font-family:var(--font-sans);"
    );
    let dot_style = format!(
        "width:8px;height:8px;border-radius:var(--radius-full);background:{accent};margin-top:5px;flex-shrink:0;"
    );
    rsx! {
        div { style: "{style}",
            div { style: "{dot_style}" }
            div {
                div { style: "font-size:14px;font-weight:600;color:var(--text-primary);", "{title}" }
                if let Some(description) = description {
                    div { style: "font-size:13px;color:var(--text-secondary);margin-top:2px;", "{description}" }
                }
            }
        }
    }
}
