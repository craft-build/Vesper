use dioxus::prelude::*;

#[component]
pub fn Switch(
    #[props(default = false)] checked: bool,
    on_change: EventHandler<()>,
    #[props(default = None)] label: Option<String>,
) -> Element {
    let bg = if checked {
        "var(--bg-brand)"
    } else {
        "var(--gray-70)"
    };
    let justify = if checked { "flex-end" } else { "flex-start" };
    rsx! {
        label { style: "display:flex;align-items:center;gap:10px;font-family:var(--font-sans);font-size:14px;color:var(--text-primary);cursor:pointer;",
            span {
                onclick: move |_| on_change.call(()),
                style: "width:38px;height:22px;border-radius:var(--radius-full);padding:2px;background:{bg};display:flex;justify-content:{justify};transition:background var(--duration-normal) var(--ease-standard);",
                span { style: "width:18px;height:18px;border-radius:var(--radius-full);background:#fff;transition:transform var(--duration-normal) var(--ease-standard);" }
            }
            if let Some(label) = &label {
                "{label}"
            }
        }
    }
}
