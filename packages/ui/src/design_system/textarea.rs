use dioxus::prelude::*;

#[component]
pub fn Textarea(
    #[props(default = None)] label: Option<String>,
    #[props(default = String::new())] placeholder: String,
    #[props(default = String::new())] value: String,
    #[props(default = None)] on_change: Option<EventHandler<String>>,
    #[props(default = 3)] rows: i64,
) -> Element {
    rsx! {
        label { class: "ds-label",
            if let Some(label) = &label {
                span { class: "ds-field-label", "{label}" }
            }
            textarea {
                class: "ds-field",
                rows: "{rows}",
                placeholder: "{placeholder}",
                value: "{value}",
                oninput: move |evt| {
                    if let Some(handler) = &on_change {
                        handler.call(evt.value());
                    }
                },
            }
        }
    }
}
