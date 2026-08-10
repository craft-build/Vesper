use std::rc::Rc;

use dioxus::prelude::*;

use super::composer::Composer;
use super::message_row::MessageRow;
use crate::data::{Convo, ConvoKind, VesperClient};
use crate::design_system::{Tag, TagTone};
use crate::icons::{Icon, IconName};

#[component]
pub fn Conversation(
    convo: Convo,
    is_mobile: bool,
    #[props(default = None)] on_back: Option<EventHandler<()>>,
    on_start_call: EventHandler<bool>,
    on_open_thread: EventHandler<String>,
    on_open_profile: EventHandler<String>,
    on_open_room_info: EventHandler<()>,
    #[props(default = false)] hide_header: bool,
) -> Element {
    let client = use_context::<Rc<dyn VesperClient>>();
    let convo_id = convo.id.clone();

    let history = {
        let client = client.clone();
        let convo_id = convo_id.clone();
        use_resource(move || {
            let client = client.clone();
            let convo_id = convo_id.clone();
            async move { client.messages(&convo_id).await }
        })
    };

    let mut messages = use_signal(Vec::new);
    use_effect(move || {
        if let Some(list) = history() {
            messages.set(list);
        }
    });

    let mut replying_to = use_signal(|| None);

    let send = {
        let client = client.clone();
        let convo_id = convo_id.clone();
        move |(text, attachment): (String, Option<crate::data::Attachment>)| {
            let reply_to = replying_to()
                .as_ref()
                .map(|m: &crate::data::Message| m.id.clone());
            let optimistic = crate::data::Message {
                id: format!("pending-{}", messages().len()),
                from: "You".into(),
                time: "now".into(),
                mine: true,
                system: false,
                text: text.clone(),
                reply_to: reply_to.clone(),
                reactions: Vec::new(),
                thread_count: 0,
                attachment: attachment.clone(),
                read_by: Vec::new(),
            };
            messages.write().push(optimistic);
            replying_to.set(None);

            let client = client.clone();
            let convo_id = convo_id.clone();
            spawn(async move {
                client
                    .send_message(&convo_id, text, attachment, reply_to)
                    .await;
            });
        }
    };

    let react = {
        let client = client.clone();
        let convo_id = convo_id.clone();
        move |(message_id, emoji): (String, String)| {
            messages.write().iter_mut().for_each(|m| {
                if m.id != message_id {
                    return;
                }
                match m.reactions.iter().position(|r| r.emoji == emoji) {
                    Some(idx) => {
                        let r = &mut m.reactions[idx];
                        if r.me {
                            r.count -= 1;
                            r.me = false;
                        } else {
                            r.count += 1;
                            r.me = true;
                        }
                        if m.reactions[idx].count == 0 {
                            m.reactions.remove(idx);
                        }
                    }
                    None => m.reactions.push(crate::data::Reaction {
                        emoji: emoji.clone(),
                        count: 1,
                        me: true,
                    }),
                }
            });

            let client = client.clone();
            let convo_id = convo_id.clone();
            spawn(async move {
                client.react(&convo_id, &message_id, &emoji).await;
            });
        }
    };

    let placeholder = if convo.kind == ConvoKind::Room {
        format!("Message #{}", convo.name)
    } else {
        format!("Message {}", convo.name)
    };
    let members_label = match convo.kind {
        ConvoKind::Room => format!("{} members", convo.members.unwrap_or(0)),
        ConvoKind::Dm => convo.mxid.clone().unwrap_or_default(),
    };
    let msgs = messages();

    rsx! {
        div { style: "flex:1;display:flex;flex-direction:column;min-width:0;height:100%;",
            if !hide_header {
                div { style: "height:56px;border-bottom:1px solid var(--border-subtle);display:flex;align-items:center;justify-content:space-between;padding:0 16px;flex-shrink:0;gap:8px;",
                    div { style: "display:flex;align-items:center;gap:8px;min-width:0;",
                        if is_mobile {
                            button {
                                onclick: move |_| { if let Some(h) = &on_back { h.call(()); } },
                                style: "background:none;border:none;color:var(--text-secondary);cursor:pointer;display:flex;",
                                Icon { name: IconName::ArrowLeft, size: 19 }
                            }
                        }
                        button {
                            onclick: move |_| on_open_room_info.call(()),
                            style: "background:none;border:none;text-align:left;cursor:pointer;min-width:0;",
                            div { style: "font-weight:700;font-size:15px;display:flex;align-items:center;gap:4px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;",
                                if convo.kind == ConvoKind::Room {
                                    Icon { name: IconName::Hash, size: 13, color: "var(--text-tertiary)".to_string() }
                                }
                                "{convo.name}"
                            }
                            div { style: "font-size:12px;color:var(--text-tertiary);font-family:var(--font-mono);", "{members_label}" }
                        }
                    }
                    div { style: "display:flex;gap:4px;align-items:center;flex-shrink:0;",
                        if convo.encrypted {
                            Tag { tone: TagTone::Brand, "e2ee" }
                        }
                        button {
                            title: "Voice call",
                            onclick: move |_| on_start_call.call(false),
                            style: "width:36px;height:36px;border-radius:var(--radius-md);border:none;background:transparent;color:var(--text-secondary);cursor:pointer;display:flex;align-items:center;justify-content:center;",
                            Icon { name: IconName::Phone, size: 16 }
                        }
                        button {
                            title: "Video call",
                            onclick: move |_| on_start_call.call(true),
                            style: "width:36px;height:36px;border-radius:var(--radius-md);border:none;background:transparent;color:var(--text-secondary);cursor:pointer;display:flex;align-items:center;justify-content:center;",
                            Icon { name: IconName::Video, size: 16 }
                        }
                        button {
                            title: "Room info",
                            onclick: move |_| on_open_room_info.call(()),
                            style: "width:36px;height:36px;border-radius:var(--radius-md);border:none;background:transparent;color:var(--text-secondary);cursor:pointer;display:flex;align-items:center;justify-content:center;",
                            Icon { name: IconName::Info, size: 16 }
                        }
                        button {
                            title: "Search",
                            style: "width:36px;height:36px;border-radius:var(--radius-md);border:none;background:transparent;color:var(--text-secondary);cursor:pointer;display:flex;align-items:center;justify-content:center;",
                            Icon { name: IconName::Search, size: 16 }
                        }
                    }
                }
            }
            div { style: "flex:1;overflow-y:auto;padding:20px 24px;display:flex;flex-direction:column;gap:16px;",
                for m in msgs.iter() {
                    MessageRow {
                        key: "{m.id}",
                        m: m.clone(),
                        all_messages: msgs.clone(),
                        on_react: react.clone(),
                        on_reply: move |m| replying_to.set(Some(m)),
                        on_open_thread,
                        on_open_profile,
                    }
                }
            }
            Composer {
                on_send: send,
                replying_to: replying_to(),
                on_cancel_reply: move |_| replying_to.set(None),
                placeholder,
            }
        }
    }
}
