use dioxus::prelude::*;

#[component]
pub fn IconButton(
    label: String,
    #[props(default = 36)] size: i64,
    #[props(default = false)] active: bool,
    #[props(default = None)] onclick: Option<EventHandler<MouseEvent>>,
    children: Element,
) -> Element {
    let mut hover = use_signal(|| false);
    let bg = if active {
        "var(--bg-selected)"
    } else if hover() {
        "var(--bg-hover)"
    } else {
        "transparent"
    };
    let color = if active {
        "var(--text-brand)"
    } else {
        "var(--text-secondary)"
    };
    let style = format!(
        "width:{size}px;height:{size}px;border-radius:var(--radius-md);border:none;display:flex;align-items:center;justify-content:center;background:{bg};color:{color};cursor:pointer;transition:background var(--duration-fast) var(--ease-standard);"
    );
    rsx! {
        button {
            "aria-label": "{label}",
            title: "{label}",
            style: "{style}",
            onclick: move |evt| {
                if let Some(handler) = &onclick {
                    handler.call(evt);
                }
            },
            onmouseenter: move |_| hover.set(true),
            onmouseleave: move |_| hover.set(false),
            {children}
        }
    }
}
