use dioxus::prelude::*;

#[component]
pub fn Switch(
    #[props(default = false)] checked: bool,
    on_change: EventHandler<()>,
    #[props(default = None)] label: Option<String>,
) -> Element {
    let track_class = if checked {
        "ds-switch ds-switch--checked"
    } else {
        "ds-switch"
    };
    rsx! {
        label { class: "ds-check-label",
            span {
                class: "{track_class}",
                onclick: move |_| on_change.call(()),
                span { class: "ds-switch-knob" }
            }
            if let Some(label) = &label {
                "{label}"
            }
        }
    }
}
