use dioxus::prelude::*;

#[component]
pub fn IconButton(
    label: String,
    #[props(default = 36)] size: i64,
    #[props(default = false)] active: bool,
    #[props(default = None)] onclick: Option<EventHandler<MouseEvent>>,
    children: Element,
) -> Element {
    let class = if active {
        "ds-icon-button ds-icon-button--active"
    } else {
        "ds-icon-button"
    };
    // Size stays a custom property instead of a class: it is per-instance
    // data, and the style string never changes for interactive states.
    let size_style = format!("--ds-icon-size:{size}px");
    rsx! {
        button {
            class: "{class}",
            style: "{size_style}",
            "aria-label": "{label}",
            title: "{label}",
            onclick: move |evt| {
                if let Some(handler) = &onclick {
                    handler.call(evt);
                }
            },
            {children}
        }
    }
}
