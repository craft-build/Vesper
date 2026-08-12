use std::rc::Rc;

use dioxus::prelude::*;

use super::composer::Composer;
use crate::data::{ClientState, Message, ThreadReply, VesperClient};
use crate::design_system::Avatar;
use crate::icons::{Icon, IconName};

#[component]
pub fn ThreadPanel(
    root_message: Option<Message>,
    /// Event id of the thread root — keys the live threads map.
    root_id: String,
    thread: Vec<ThreadReply>,
    /// The room containing the thread — needed to build the `m.thread`
    /// relation when sending (checkpoint 05).
    convo_id: String,
    on_close: EventHandler<()>,
) -> Element {
    let client = use_context::<Rc<dyn VesperClient>>();
    let sync = use_context::<ClientState>();
    // Mock-only optimistic rows; the live backend's echo arrives through the
    // thread-diff stream below.
    let mut optimistic = use_signal(Vec::<ThreadReply>::new);

    // Live thread: refcounted open/close pairs the panel's mount with the
    // thread-focused timeline's lifetime. The mock ignores both calls.
    use_effect({
        let client = client.clone();
        let convo_id = convo_id.clone();
        let root_id = root_id.clone();
        move || client.open_thread(&convo_id, &root_id)
    });
    use_drop({
        let client = client.clone();
        let root_id = root_id.clone();
        move || client.close_thread(&root_id)
    });

    let live = sync.threads.read().get(&root_id).cloned();
    let replies = live.unwrap_or_else(|| {
        let mut base = thread.clone();
        base.extend(optimistic());
        base
    });

    let send = move |(text, _attachment): (String, Option<crate::data::Attachment>)| {
        if text.is_empty() {
            return;
        }
        if !sync.threads.read().contains_key(&root_id) {
            optimistic.write().push(ThreadReply {
                from: "You".into(),
                mine: true,
                time: "now".into(),
                text: text.clone(),
            });
        }
        let client = client.clone();
        let convo_id = convo_id.clone();
        let root_id = root_id.clone();
        spawn(async move {
            client.send_thread_reply(&convo_id, &root_id, text).await;
        });
    };

    rsx! {
        div { style: "width:100%;height:100%;display:flex;flex-direction:column;background:var(--bg-canvas);border-left:1px solid var(--border-subtle);",
            div { style: "height:56px;border-bottom:1px solid var(--border-subtle);display:flex;align-items:center;gap:10px;padding:0 16px;flex-shrink:0;",
                Icon { name: IconName::MessageSquare, size: 16, color: "var(--text-brand)".to_string() }
                span { style: "font-weight:700;font-size:15px;flex:1;", "Thread" }
                button {
                    onclick: move |_| on_close.call(()),
                    style: "background:none;border:none;color:var(--text-secondary);cursor:pointer;display:flex;",
                    Icon { name: IconName::X, size: 18 }
                }
            }
            div { style: "flex:1;overflow-y:auto;padding:16px 18px;display:flex;flex-direction:column;gap:14px;",
                if let Some(root) = &root_message {
                    div { style: "display:flex;gap:10px;",
                        Avatar { name: "{root.from}", size: 30 }
                        div {
                            div { style: "font-size:13px;font-weight:600;",
                                "{root.from} "
                                span { style: "font-size:11px;color:var(--text-tertiary);font-family:var(--font-mono);font-weight:400;", "{root.time}" }
                            }
                            div { style: "margin-top:4px;padding:10px 14px;border-radius:var(--radius-md);font-size:14px;background:var(--bg-surface);border:1px solid var(--border-subtle);", "{root.text}" }
                        }
                    }
                }
                div { style: "font-size:11px;color:var(--text-tertiary);text-align:center;letter-spacing:0.04em;", "{replies.len()} REPLIES" }
                for r in replies.iter() {
                    {
                        let dir = if r.mine { "row-reverse" } else { "row" };
                        let align = if r.mine { "right" } else { "left" };
                        let bg = if r.mine { "var(--bg-brand)" } else { "var(--bg-surface)" };
                        let color = if r.mine { "var(--black)" } else { "var(--text-primary)" };
                        let border = if r.mine { "none".to_string() } else { "1px solid var(--border-subtle)".to_string() };
                        rsx! {
                            div { style: "display:flex;gap:10px;flex-direction:{dir};",
                                Avatar { name: "{r.from}", size: 30 }
                                div { style: "max-width:300px;",
                                    div { style: "font-size:13px;font-weight:600;text-align:{align};",
                                        "{r.from} "
                                        span { style: "font-size:11px;color:var(--text-tertiary);font-family:var(--font-mono);font-weight:400;", "{r.time}" }
                                    }
                                    div { style: "margin-top:4px;padding:10px 14px;border-radius:var(--radius-md);font-size:14px;background:{bg};color:{color};border:{border};", "{r.text}" }
                                }
                            }
                        }
                    }
                }
            }
            Composer { on_send: send, placeholder: "Reply in thread" }
        }
    }
}
