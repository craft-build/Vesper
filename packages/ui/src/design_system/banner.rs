use dioxus::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerTone {
    Info,
    Warning,
    Danger,
}

#[component]
pub fn Banner(
    #[props(default = BannerTone::Info)] tone: BannerTone,
    children: Element,
    #[props(default = None)] action: Option<Element>,
) -> Element {
    let accent = match tone {
        BannerTone::Info => "var(--blue-60)",
        BannerTone::Warning => "var(--status-away)",
        BannerTone::Danger => "var(--status-danger)",
    };
    let style = format!(
        "display:flex;align-items:center;justify-content:space-between;gap:16px;background:var(--bg-surface);border:1px solid var(--border-subtle);border-left:3px solid {accent};border-radius:var(--radius-md);padding:12px 16px;font-family:var(--font-sans);font-size:14px;color:var(--text-primary);"
    );
    rsx! {
        div { style: "{style}",
            span { {children} }
            {action}
        }
    }
}
