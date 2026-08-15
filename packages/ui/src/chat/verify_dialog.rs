use dioxus::prelude::*;

use crate::data::{ClientState, VerificationState};
use crate::design_system::{Button, ButtonSize, ButtonVariant, Dialog};

/// SAS emoji verification dialog (checkpoint 08). Reads the active session
/// from the context-provided `ClientState::verification` — the backend
/// (matrix verification driver or mock) publishes state + the 7-emoji
/// short-auth string there; this dialog is pure presentation plus user
/// decisions forwarded through `on_action`/`on_close`.
#[component]
pub fn VerifyDialog(
    #[props(default = false)] open: bool,
    on_close: EventHandler<()>,
    /// Forwarded for "They match" / "They don't match"; closing also implies
    /// Cancel (the caller decides whether to send it).
    on_action: EventHandler<crate::data::VerificationAction>,
) -> Element {
    let sync = use_context::<ClientState>();
    let session = sync.verification.read().clone();

    // Empty-slot body while no session object exists yet (the backend
    // publishes one as soon as StartVerification lands).
    let Some(session) = session else {
        return rsx! {};
    };

    let subject_label = if session.subject.is_empty() {
        "the other device".to_string()
    } else {
        session.subject.clone()
    };

    use_effect({
        let open = open;
        move || {
            // Auto-close on terminal states. Reads the signal inside the
            // callback (so the effect re-runs on change) but only calls
            // `on_close` — never writes a signal this effect reads, the
            // checkpoint-06 effect-loop lesson.
            let terminal = sync.verification.read().as_ref().is_some_and(|s| {
                matches!(
                    s.state,
                    VerificationState::Done | VerificationState::Cancelled
                )
            });
            if open && terminal {
                on_close.call(());
            }
        }
    });

    match &session.state {
        VerificationState::Failed(msg) => rsx! {
            Dialog {
                title: "Verify session",
                open,
                onclose: move |_| on_close.call(()),
                actions: rsx! {
                    Button { variant: ButtonVariant::Primary, size: ButtonSize::Sm, onclick: move |_| on_close.call(()), "Close" }
                },
                div { style: "margin-bottom:14px;", "{msg}" }
            }
        },
        VerificationState::Done => rsx! {
            Dialog {
                title: "Verify session",
                open,
                onclose: move |_| on_close.call(()),
                actions: rsx! {
                    Button { variant: ButtonVariant::Primary, size: ButtonSize::Sm, onclick: move |_| on_close.call(()), "Close" }
                },
                div { style: "margin-bottom:14px;", "Verified! You can close this dialog." }
            }
        },
        VerificationState::Cancelled => rsx! {
            Dialog {
                title: "Verify session",
                open,
                onclose: move |_| on_close.call(()),
                actions: rsx! {
                    Button { variant: ButtonVariant::Primary, size: ButtonSize::Sm, onclick: move |_| on_close.call(()), "Close" }
                },
                div { style: "margin-bottom:14px;", "Verification was cancelled." }
            }
        },
        state @ (VerificationState::Requested
        | VerificationState::Ready
        | VerificationState::EmojisShown
        | VerificationState::Confirmed) => {
            let waiting = matches!(
                state,
                VerificationState::Requested | VerificationState::Ready
            );
            let confirmed = matches!(state, VerificationState::Confirmed);
            rsx! {
                Dialog {
                    title: "Verify session",
                    open,
                    onclose: move |_| on_close.call(()),
                    actions: rsx! {
                        Button { variant: ButtonVariant::Secondary, size: ButtonSize::Sm, onclick: move |_| on_action.call(crate::data::VerificationAction::Mismatch), "They don't match" }
                        Button {
                            variant: ButtonVariant::Primary,
                            size: ButtonSize::Sm,
                            disabled: waiting || confirmed,
                            onclick: move |_| on_action.call(crate::data::VerificationAction::Confirm),
                            if confirmed { "Waiting for them…" } else { "They match" }
                        }
                    },
                    if waiting {
                        div { style: "margin-bottom:14px;color:var(--text-secondary);", "Waiting for {subject_label} to accept…" }
                    } else {
                        div { style: "margin-bottom:14px;", "Confirm these emoji appear in the same order on {subject_label}." }
                        div { style: "display:flex;flex-wrap:wrap;justify-content:space-around;gap:8px;",
                            for emoji in session.emojis.iter() {
                                div { key: "{emoji.description}", style: "text-align:center;flex:0 0 60px;",
                                    div { style: "font-size:28px;", "{emoji.symbol}" }
                                    div { style: "font-size:11px;color:var(--text-tertiary);margin-top:4px;", "{emoji.description}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
