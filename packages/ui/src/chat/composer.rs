use dioxus::prelude::*;

use crate::data::{Attachment, AttachmentKind, Message};
use crate::design_system::{Button, ButtonSize, IconButton};
use crate::icons::{Icon, IconName};

/// Same quick-set as the message reaction picker (message_row.rs); local
/// copy since the composer inserts into the draft rather than reacting.
const EMOJI_CHOICES: [&str; 6] = ["👍", "❤️", "😂", "🎉", "✅", "👀"];

/// Extensions that map to inline-renderable image messages; anything else
/// sends as `m.file` (mime itself is sniffed from the bytes at send time).
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn kind_of(path: &str) -> AttachmentKind {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg"
    ) {
        AttachmentKind::Image
    } else {
        AttachmentKind::File
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{} KB", bytes.div_ceil(1_000))
    } else {
        format!("{bytes} B")
    }
}

/// Native file pick (checkpoint 07). The dialog must run on the UI thread;
/// the picked path travels into the model's `local_path` — the backend
/// reads the bytes at send time. Desktop only: rfd has no Android backend
/// and wasm has no rfd yet (web: checkpoint 11).
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub fn pick_attachment() -> Option<Attachment> {
    let file = rfd::FileDialog::new().pick_file()?;
    let path = file.display().to_string();
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let size = std::fs::metadata(&path)
        .map(|m| format_size(m.len()))
        .unwrap_or_default();
    let mut attachment = Attachment::new(kind_of(&path), name, size);
    attachment.local_path = Some(path);
    Some(attachment)
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn pick_attachment() -> Option<Attachment> {
    None
}

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
    let mut show_emoji = use_signal(|| false);

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
                    onclick: move |_| {
                        // Same modal rule as the download save dialog (see
                        // conversation.rs): opening the picker inside the
                        // click handler lets its nested event pump re-render
                        // while this handler's borrow is live → crash.
                        spawn(async move {
                            if let Some(picked) = pick_attachment() {
                                attachment.set(Some(picked));
                            }
                        });
                    },
                    Icon { name: IconName::Paperclip, size: 17 }
                }
                div {
                    onmouseleave: move |_| show_emoji.set(false),
                    style: "display:flex;gap:2px;position:relative;",
                    IconButton {
                        label: "Emoji",
                        onclick: move |_| show_emoji.set(!show_emoji()),
                        Icon { name: IconName::Smile, size: 15 }
                    }
                    if show_emoji() {
                        div { style: "position:absolute;bottom:36px;left:0;display:flex;gap:4px;background:var(--bg-surface-raised);border:1px solid var(--border-subtle);border-radius:var(--radius-md);box-shadow:var(--shadow-md);padding:6px;z-index:4;",
                            for e in EMOJI_CHOICES.iter() {
                                button {
                                    key: "{e}",
                                    onclick: move |_| {
                                        val.write().push_str(e);
                                        show_emoji.set(false);
                                    },
                                    style: "background:none;border:none;cursor:pointer;font-size:16px;padding:2px;",
                                    "{e}"
                                }
                            }
                        }
                    }
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
