use dioxus::prelude::*;

#[component]
pub fn Checkbox(
    label: String,
    #[props(default = false)] checked: bool,
    on_change: EventHandler<bool>,
) -> Element {
    let border = if checked {
        "var(--bg-brand)".to_string()
    } else {
        "var(--border-strong)".to_string()
    };
    let background = if checked {
        "var(--bg-brand)"
    } else {
        "transparent"
    };
    rsx! {
        label {
            style: "display:flex;align-items:center;gap:10px;font-family:var(--font-sans);font-size:14px;color:var(--text-primary);cursor:pointer;",
            onclick: move |_| on_change.call(!checked),
            span {
                style: "width:18px;height:18px;border-radius:var(--radius-sm);flex-shrink:0;border:1px solid {border};background:{background};display:flex;align-items:center;justify-content:center;transition:background var(--duration-fast) var(--ease-standard);",
                if checked {
                    svg { width: "11", height: "11", view_box: "0 0 16 16", fill: "none",
                        path { d: "M3 8l3.5 3.5L13 5", stroke: "#05070a", stroke_width: "2.2", stroke_linecap: "round", stroke_linejoin: "round" }
                    }
                }
            }
            "{label}"
        }
    }
}
