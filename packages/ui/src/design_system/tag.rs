use dioxus::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagTone {
    Neutral,
    Brand,
    Success,
    Danger,
}

#[component]
pub fn Tag(#[props(default = TagTone::Neutral)] tone: TagTone, children: Element) -> Element {
    let (bg, color) = match tone {
        TagTone::Neutral => ("var(--bg-surface-raised)", "var(--text-secondary)"),
        TagTone::Brand => ("rgba(14,165,233,0.16)", "var(--text-brand)"),
        TagTone::Success => ("rgba(55,214,122,0.14)", "#37d67a"),
        TagTone::Danger => ("rgba(242,84,91,0.14)", "var(--status-danger)"),
    };
    let style = format!(
        "background:{bg};color:{color};font-family:var(--font-mono);font-size:12px;font-weight:500;padding:4px 10px;border-radius:var(--radius-sm);display:inline-block;"
    );
    rsx! {
        span { style: "{style}", {children} }
    }
}
