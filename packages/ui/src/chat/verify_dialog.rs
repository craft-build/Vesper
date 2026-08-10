use dioxus::prelude::*;

use crate::design_system::{Button, ButtonSize, ButtonVariant, Dialog};

const SAS: [(&str, &str); 4] = [
    ("🐱", "Cat"),
    ("🚀", "Rocket"),
    ("🎩", "Top hat"),
    ("🔑", "Key"),
];

#[component]
pub fn VerifyDialog(
    #[props(default = false)] open: bool,
    on_close: EventHandler<()>,
    on_confirm: EventHandler<()>,
    #[props(default = String::new())] subject: String,
) -> Element {
    let subject_label = if subject.is_empty() {
        "the other device".to_string()
    } else {
        subject.clone()
    };
    rsx! {
        Dialog {
            title: "Verify session",
            open,
            onclose: move |_| on_close.call(()),
            actions: rsx! {
                Button { variant: ButtonVariant::Secondary, size: ButtonSize::Sm, onclick: move |_| on_close.call(()), "They don't match" }
                Button { variant: ButtonVariant::Primary, size: ButtonSize::Sm, onclick: move |_| on_confirm.call(()), "They match" }
            },
            div { style: "margin-bottom:14px;", "Confirm these emoji appear in the same order on {subject_label}." }
            div { style: "display:flex;justify-content:space-around;gap:8px;",
                for (emoji, name) in SAS.iter() {
                    div { key: "{name}", style: "text-align:center;",
                        div { style: "font-size:28px;", "{emoji}" }
                        div { style: "font-size:11px;color:var(--text-tertiary);margin-top:4px;", "{name}" }
                    }
                }
            }
        }
    }
}
