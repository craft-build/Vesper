use dioxus::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TooltipSide {
    Top,
    Bottom,
}

#[component]
pub fn Tooltip(
    label: String,
    children: Element,
    #[props(default = TooltipSide::Top)] side: TooltipSide,
) -> Element {
    let mut show = use_signal(|| false);
    let pos = match side {
        TooltipSide::Top => "bottom:100%;left:50%;transform:translate(-50%,-6px);",
        TooltipSide::Bottom => "top:100%;left:50%;transform:translate(-50%,6px);",
    };
    rsx! {
        span {
            style: "position:relative;display:inline-flex;",
            onmouseenter: move |_| show.set(true),
            onmouseleave: move |_| show.set(false),
            {children}
            if show() {
                span {
                    style: "position:absolute;{pos}background:var(--gray-100);color:var(--white);font:500 12px var(--font-sans);padding:5px 9px;border-radius:var(--radius-sm);white-space:nowrap;box-shadow:var(--shadow-md);pointer-events:none;z-index:20;",
                    "{label}"
                }
            }
        }
    }
}
