use dioxus::prelude::*;

#[component]
pub fn Checkbox(
    label: String,
    #[props(default = false)] checked: bool,
    on_change: EventHandler<bool>,
) -> Element {
    let box_class = if checked {
        "ds-checkbox ds-checkbox--checked"
    } else {
        "ds-checkbox"
    };
    rsx! {
        label {
            class: "ds-check-label",
            onclick: move |_| on_change.call(!checked),
            span { class: "{box_class}",
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
