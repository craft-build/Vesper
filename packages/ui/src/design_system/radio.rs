use dioxus::prelude::*;

#[component]
pub fn Radio(
    label: String,
    #[props(default = false)] checked: bool,
    on_change: EventHandler<()>,
) -> Element {
    let ring_class = if checked {
        "ds-radio ds-radio--checked"
    } else {
        "ds-radio"
    };
    rsx! {
        label {
            class: "ds-check-label",
            onclick: move |_| on_change.call(()),
            span { class: "{ring_class}",
                if checked {
                    span { class: "ds-radio-dot" }
                }
            }
            "{label}"
        }
    }
}
