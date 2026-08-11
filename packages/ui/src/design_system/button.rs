use dioxus::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonSize {
    Sm,
    Md,
    Lg,
}

#[component]
pub fn Button(
    #[props(default = ButtonVariant::Primary)] variant: ButtonVariant,
    #[props(default = ButtonSize::Md)] size: ButtonSize,
    #[props(default = false)] disabled: bool,
    #[props(default = None)] onclick: Option<EventHandler<MouseEvent>>,
    children: Element,
) -> Element {
    // Hover/press states are CSS pseudo-classes; a signal-driven style string
    // would be corrupted by dioxus's style patching (see styles.css header).
    let variant_class = match variant {
        ButtonVariant::Primary => "ds-button--primary",
        ButtonVariant::Secondary => "ds-button--secondary",
        ButtonVariant::Ghost => "ds-button--ghost",
        ButtonVariant::Danger => "ds-button--danger",
    };
    let size_class = match size {
        ButtonSize::Sm => "ds-button--sm",
        ButtonSize::Md => "ds-button--md",
        ButtonSize::Lg => "ds-button--lg",
    };

    rsx! {
        button {
            class: "ds-button {variant_class} {size_class}",
            disabled,
            onclick: move |evt| {
                if let Some(handler) = &onclick {
                    handler.call(evt);
                }
            },
            {children}
        }
    }
}
