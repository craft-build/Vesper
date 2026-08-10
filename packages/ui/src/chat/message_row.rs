use dioxus::prelude::*;

use crate::data::{AttachmentKind, Message};
use crate::design_system::Avatar;
use crate::icons::{Icon, IconName};
use crate::markdown::render_markdown;

const EMOJI_CHOICES: [&str; 6] = ["👍", "❤️", "😂", "🎉", "✅", "👀"];

#[component]
pub fn MessageRow(
    m: Message,
    all_messages: Vec<Message>,
    on_react: EventHandler<(String, String)>,
    on_reply: EventHandler<Message>,
    on_open_thread: EventHandler<String>,
    on_open_profile: EventHandler<String>,
) -> Element {
    let mut hover = use_signal(|| false);
    let mut show_emoji = use_signal(|| false);

    if m.system {
        return rsx! {
            div { style: "text-align:center;font-size:12px;color:var(--text-tertiary);font-family:var(--font-mono);padding:4px 0;", "{m.text}" }
        };
    }

    let reply_src = m
        .reply_to
        .as_ref()
        .and_then(|id| all_messages.iter().find(|x| &x.id == id));
    let side = if m.mine { "left" } else { "right" };
    let bubble_bg = if m.mine {
        "var(--bg-brand)"
    } else {
        "var(--bg-surface)"
    };
    let bubble_color = if m.mine {
        "var(--black)"
    } else {
        "var(--text-primary)"
    };
    let bubble_border = if m.mine {
        "none".to_string()
    } else {
        "1px solid var(--border-subtle)".to_string()
    };
    let row_dir = if m.mine { "row-reverse" } else { "row" };
    let html = render_markdown(&m.text);

    rsx! {
        div {
            onmouseenter: move |_| hover.set(true),
            onmouseleave: move |_| { hover.set(false); show_emoji.set(false); },
            style: "display:flex;gap:10px;flex-direction:{row_dir};position:relative;",
            button {
                onclick: { let from = m.from.clone(); move |_| on_open_profile.call(from.clone()) },
                style: "background:none;border:none;padding:0;cursor:pointer;",
                Avatar { name: "{m.from}", size: 32 }
            }
            div { style: "max-width:440px;position:relative;",
                div { style: "display:flex;gap:8px;align-items:baseline;flex-direction:{row_dir};",
                    span { style: "font-size:13px;font-weight:600;", "{m.from}" }
                    span { style: "font-size:11px;color:var(--text-tertiary);font-family:var(--font-mono);", "{m.time}" }
                }
                if let Some(src) = reply_src {
                    div { style: "margin-top:4px;font-size:12px;color:var(--text-tertiary);border-left:2px solid var(--border-default);padding-left:8px;",
                        "{src.from}: {src.text}"
                    }
                }
                div {
                    style: "margin-top:4px;padding:10px 14px;border-radius:var(--radius-md);font-size:14px;line-height:1.45;background:{bubble_bg};color:{bubble_color};border:{bubble_border};",
                    dangerous_inner_html: "{html}",
                }
                if let Some(attachment) = &m.attachment {
                    div { style: "margin-top:6px;display:flex;align-items:center;gap:8px;padding:8px 12px;background:var(--bg-surface-raised);border:1px solid var(--border-subtle);border-radius:var(--radius-md);",
                        Icon {
                            name: if attachment.kind == AttachmentKind::Image { IconName::Image } else { IconName::File },
                            size: 16,
                            color: "var(--text-brand)".to_string(),
                        }
                        span { style: "font-size:13px;flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", "{attachment.name}" }
                        span { style: "font-size:11px;color:var(--text-tertiary);font-family:var(--font-mono);", "{attachment.size}" }
                    }
                }
                if !m.reactions.is_empty() {
                    div { style: "display:flex;gap:6px;margin-top:6px;flex-direction:{row_dir};",
                        for r in m.reactions.iter() {
                            {
                                let emoji = r.emoji.clone();
                                let msg_id = m.id.clone();
                                let border = if r.me { "var(--border-brand)" } else { "var(--border-subtle)" };
                                let bg = if r.me { "var(--bg-selected)" } else { "var(--bg-surface-raised)" };
                                rsx! {
                                    button {
                                        key: "{r.emoji}",
                                        onclick: move |_| on_react.call((msg_id.clone(), emoji.clone())),
                                        style: "display:flex;align-items:center;gap:4px;font-size:12px;padding:2px 8px;border-radius:999px;border:1px solid {border};background:{bg};cursor:pointer;color:var(--text-primary);",
                                        "{r.emoji} {r.count}"
                                    }
                                }
                            }
                        }
                    }
                }
                if m.thread_count > 0 {
                    button {
                        onclick: { let id = m.id.clone(); move |_| on_open_thread.call(id.clone()) },
                        style: "display:flex;align-items:center;gap:6px;margin-top:6px;background:var(--bg-surface-raised);border:1px solid var(--border-subtle);border-radius:var(--radius-md);padding:5px 10px;cursor:pointer;color:var(--text-brand);font-size:12px;font-weight:600;",
                        Icon { name: IconName::MessageSquare, size: 13 }
                        "{m.thread_count} replies"
                    }
                }
                if !m.read_by.is_empty() {
                    {
                        let justify = if m.mine { "flex-end" } else { "flex-start" };
                        let read_by = m.read_by.join(", ");
                        rsx! {
                    div { style: "display:flex;gap:4px;margin-top:5px;justify-content:{justify};align-items:center;font-size:11px;color:var(--text-tertiary);",
                        Icon { name: IconName::CheckCheck, size: 12, color: "var(--text-brand)".to_string() }
                        " Seen by {read_by}"
                    }
                        }
                    }
                }
                if hover() {
                    div { style: "position:absolute;top:-14px;{side}:0;display:flex;gap:2px;background:var(--bg-surface-raised);border:1px solid var(--border-subtle);border-radius:var(--radius-md);box-shadow:var(--shadow-sm);padding:3px;",
                        button {
                            title: "React",
                            onclick: move |_| show_emoji.set(!show_emoji()),
                            style: "background:none;border:none;cursor:pointer;padding:5px;color:var(--text-secondary);display:flex;",
                            Icon { name: IconName::Smile, size: 15 }
                        }
                        button {
                            title: "Reply",
                            onclick: { let msg = m.clone(); move |_| on_reply.call(msg.clone()) },
                            style: "background:none;border:none;cursor:pointer;padding:5px;color:var(--text-secondary);display:flex;",
                            Icon { name: IconName::Reply, size: 15 }
                        }
                        button {
                            title: "Reply in thread",
                            onclick: { let id = m.id.clone(); move |_| on_open_thread.call(id.clone()) },
                            style: "background:none;border:none;cursor:pointer;padding:5px;color:var(--text-secondary);display:flex;",
                            Icon { name: IconName::MessageSquare, size: 15 }
                        }
                    }
                }
                if show_emoji() {
                    div { style: "position:absolute;top:12px;{side}:0;display:flex;gap:4px;background:var(--bg-surface-raised);border:1px solid var(--border-subtle);border-radius:var(--radius-md);box-shadow:var(--shadow-md);padding:6px;z-index:4;",
                        for e in EMOJI_CHOICES.iter() {
                            {
                                let emoji = e.to_string();
                                let msg_id = m.id.clone();
                                rsx! {
                                    button {
                                        key: "{e}",
                                        onclick: move |_| {
                                            on_react.call((msg_id.clone(), emoji.clone()));
                                            show_emoji.set(false);
                                        },
                                        style: "background:none;border:none;cursor:pointer;font-size:16px;padding:2px;",
                                        "{e}"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
