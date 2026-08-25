use dioxus::prelude::*;

#[component]
pub fn Dialog(
    title: String,
    children: Element,
    #[props(default = true)] open: bool,
    #[props(default = None)] onclose: Option<EventHandler<()>>,
    #[props(default = None)] actions: Option<Element>,
) -> Element {
    if !open {
        return rsx! {};
    }
    rsx! {
        div {
            style: "position:absolute;inset:0;background:rgba(0,0,0,0.5);display:flex;align-items:center;justify-content:center;z-index:30;",
            onclick: move |_| {
                if let Some(handler) = &onclose {
                    handler.call(());
                }
            },
            div {
                style: "width:min(400px,calc(100vw - 32px));background:var(--bg-surface-raised);border-radius:var(--radius-lg);border:1px solid var(--border-default);box-shadow:var(--shadow-lg);padding:24px;font-family:var(--font-sans);",
                onclick: move |evt| evt.stop_propagation(),
                div { style: "font-size:18px;font-weight:700;color:var(--text-primary);margin-bottom:12px;", "{title}" }
                div { style: "font-size:14px;color:var(--text-secondary);line-height:1.5;", {children} }
                if let Some(actions) = actions {
                    div { style: "display:flex;justify-content:flex-end;gap:8px;margin-top:20px;", {actions} }
                }
            }
        }
    }
}
