use dioxus::prelude::*;

#[component]
pub fn Radio(
    label: String,
    #[props(default = false)] checked: bool,
    on_change: EventHandler<()>,
) -> Element {
    let border = if checked {
        "var(--bg-brand)"
    } else {
        "var(--border-strong)"
    };
    rsx! {
        label {
            style: "display:flex;align-items:center;gap:10px;font-family:var(--font-sans);font-size:14px;color:var(--text-primary);cursor:pointer;",
            onclick: move |_| on_change.call(()),
            span {
                style: "width:18px;height:18px;border-radius:var(--radius-full);flex-shrink:0;border:1px solid {border};display:flex;align-items:center;justify-content:center;",
                if checked {
                    span { style: "width:9px;height:9px;border-radius:var(--radius-full);background:var(--bg-brand);" }
                }
            }
            "{label}"
        }
    }
}
