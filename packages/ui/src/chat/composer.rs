use dioxus::prelude::*;

use crate::data::{Attachment, AttachmentKind, Message};
use crate::design_system::{Button, ButtonSize, IconButton};
use crate::icons::{Icon, IconName};

#[component]
pub fn Composer(
    on_send: EventHandler<(String, Option<Attachment>)>,
    #[props(default = None)] replying_to: Option<Message>,
    #[props(default = None)] on_cancel_reply: Option<EventHandler<()>>,
    #[props(default = String::new())] placeholder: String,
    /// Typing-notice hook (checkpoint 06): fired `true` on input when the
    /// composer is non-empty, `false` when it empties or a message is sent.
    /// The backend debounces a 4s idle reset; the composer just reports
    /// transitions.
    #[props(default)]
    on_typing: EventHandler<bool>,
) -> Element {
    let mut val = use_signal(String::new);
    let mut attachment = use_signal(|| Option::<Attachment>::None);

    let mut send = move || {
        let text = val().trim().to_string();
        let att = attachment();
        if text.is_empty() && att.is_none() {
            return;
        }
        on_send.call((text, att));
        on_typing.call(false);
        val.set(String::new());
        attachment.set(None);
        if let Some(handler) = &on_cancel_reply {
            handler.call(());
        }
    };

    rsx! {
        div { style: "padding:10px 20px 16px;border-top:1px solid var(--border-subtle);flex-shrink:0;",
            if let Some(reply) = &replying_to {
                div { style: "display:flex;align-items:center;gap:8px;font-size:12px;color:var(--text-secondary);background:var(--bg-surface-raised);border-radius:var(--radius-md);padding:6px 10px;margin-bottom:6px;",
                    Icon { name: IconName::Reply, size: 13 }
                    span { style: "flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", "Replying to {reply.from}: {reply.text}" }
                    button {
                        onclick: move |_| {
                            if let Some(handler) = &on_cancel_reply {
                                handler.call(());
                            }
                        },
                        style: "background:none;border:none;cursor:pointer;color:var(--text-tertiary);display:flex;",
                        Icon { name: IconName::X, size: 13 }
                    }
                }
            }
            if let Some(att) = attachment() {
                div { style: "display:flex;align-items:center;gap:8px;font-size:12px;color:var(--text-secondary);background:var(--bg-surface-raised);border-radius:var(--radius-md);padding:6px 10px;margin-bottom:6px;",
                    Icon { name: IconName::File, size: 13 }
                    span { style: "flex:1;", "{att.name}" }
                    button {
                        onclick: move |_| attachment.set(None),
                        style: "background:none;border:none;cursor:pointer;color:var(--text-tertiary);display:flex;",
                        Icon { name: IconName::X, size: 13 }
                    }
                }
            }
            div { style: "display:flex;align-items:flex-end;gap:4px;background:var(--bg-surface);border:1px solid var(--border-default);border-radius:var(--radius-lg);padding:6px 8px;",
                IconButton {
                    label: "Attach file",
                    onclick: move |_| attachment.set(Some(Attachment { kind: AttachmentKind::File, name: "screenshot.png".into(), size: "340 KB".into() })),
                    Icon { name: IconName::Paperclip, size: 17 }
                }
                div { style: "display:flex;gap:2px;",
                    IconButton { label: "Emoji", Icon { name: IconName::Smile, size: 15 } }
                }
                textarea {
                    value: "{val}",
                    oninput: move |evt| {
                        let next = evt.value();
                        on_typing.call(!next.is_empty());
                        val.set(next);
                    },
                    onkeydown: move |evt| {
                        if evt.key() == Key::Enter && !evt.modifiers().shift() {
                            evt.prevent_default();
                            send();
                        }
                    },
                    placeholder: "{placeholder}",
                    rows: "1",
                    style: "flex:1;background:transparent;border:none;outline:none;resize:none;font:14px/1.4 var(--font-sans);color:var(--text-primary);padding:8px 4px;max-height:120px;",
                }
                div { style: "align-self:center;display:flex;",
                    Button { variant: crate::design_system::ButtonVariant::Primary, size: ButtonSize::Sm, onclick: move |_| send(),
                        Icon { name: IconName::Send, size: 14, color: "var(--black)".to_string() }
                    }
                }
            }
        }
    }
}
