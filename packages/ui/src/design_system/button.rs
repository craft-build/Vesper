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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InteractionState {
    Rest,
    Hover,
    Press,
}

#[component]
pub fn Button(
    #[props(default = ButtonVariant::Primary)] variant: ButtonVariant,
    #[props(default = ButtonSize::Md)] size: ButtonSize,
    #[props(default = false)] disabled: bool,
    #[props(default = None)] onclick: Option<EventHandler<MouseEvent>>,
    children: Element,
) -> Element {
    let mut state = use_signal(|| InteractionState::Rest);

    let (padding, font_size) = match size {
        ButtonSize::Sm => ("6px 12px", 13),
        ButtonSize::Md => ("10px 16px", 14),
        ButtonSize::Lg => ("13px 22px", 16),
    };

    let (rest_bg, color, border, hover_bg, press_bg) = match variant {
        ButtonVariant::Primary => (
            "var(--bg-brand)",
            "#05070a",
            "none",
            "var(--bg-brand-hover)",
            "var(--bg-brand-pressed)",
        ),
        ButtonVariant::Secondary => (
            "var(--bg-surface-raised)",
            "var(--text-primary)",
            "1px solid var(--border-default)",
            "var(--bg-hover)",
            "var(--bg-pressed)",
        ),
        ButtonVariant::Ghost => (
            "transparent",
            "var(--text-primary)",
            "none",
            "var(--bg-hover)",
            "var(--bg-pressed)",
        ),
        ButtonVariant::Danger => (
            "var(--status-danger)",
            "#ffffff",
            "none",
            "#d9464d",
            "#c03d43",
        ),
    };

    let bg = if disabled {
        rest_bg
    } else {
        match state() {
            InteractionState::Hover => hover_bg,
            InteractionState::Press => press_bg,
            InteractionState::Rest => rest_bg,
        }
    };

    let opacity = if disabled { 0.45 } else { 1.0 };
    let cursor = if disabled { "not-allowed" } else { "pointer" };

    let style = format!(
        "font-family:var(--font-sans);font-weight:600;font-size:{font_size}px;border:{border};border-radius:var(--radius-md);cursor:{cursor};display:inline-flex;align-items:center;justify-content:center;gap:8px;transition:background var(--duration-fast) var(--ease-standard), color var(--duration-fast) var(--ease-standard);padding:{padding};background:{bg};color:{color};opacity:{opacity};"
    );

    rsx! {
        button {
            style: "{style}",
            disabled,
            onclick: move |evt| {
                if let Some(handler) = &onclick {
                    handler.call(evt);
                }
            },
            onmouseenter: move |_| state.set(InteractionState::Hover),
            onmouseleave: move |_| state.set(InteractionState::Rest),
            onmousedown: move |_| state.set(InteractionState::Press),
            onmouseup: move |_| state.set(InteractionState::Hover),
            {children}
        }
    }
}
