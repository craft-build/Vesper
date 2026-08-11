use dioxus::prelude::*;

#[component]
pub fn Input(
    #[props(default = None)] label: Option<String>,
    #[props(default = String::new())] placeholder: String,
    #[props(default = "text".to_string())] input_type: String,
    #[props(default = String::new())] value: String,
    #[props(default = None)] on_change: Option<EventHandler<String>>,
    #[props(default = None)] error: Option<String>,
) -> Element {
    // Focus/error states come from CSS classes (see styles.css): reactive
    // style-string updates get mangled by dioxus's style patching, which
    // drops shorthand declarations containing var() on re-send.
    let class = if error.is_some() {
        "ds-field ds-field--error"
    } else {
        "ds-field"
    };
    rsx! {
        label { class: "ds-label",
            if let Some(label) = &label {
                span { class: "ds-field-label", "{label}" }
            }
            input {
                class: "{class}",
                r#type: "{input_type}",
                placeholder: "{placeholder}",
                value: "{value}",
                oninput: move |evt| {
                    if let Some(handler) = &on_change {
                        handler.call(evt.value());
                    }
                },
            }
            if let Some(error) = &error {
                span { class: "ds-error-msg", "{error}" }
            }
        }
    }
}
