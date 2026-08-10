use std::rc::Rc;

use dioxus::prelude::*;

use super::verify_dialog::VerifyDialog;
use crate::data::{Convo, Presence, VesperClient};
use crate::design_system::{Avatar, Button, ButtonVariant, StatusDot, Tag, TagTone};
use crate::icons::{Icon, IconName};

/// Either a room/DM (`Convo`) or a synthesized "someone mentioned in this room" person —
/// the same shape the prototype's `openProfileByName` fabricates on the fly.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileTarget {
    pub id: Option<String>,
    pub name: String,
    pub mxid: Option<String>,
    pub status: Option<Presence>,
    pub is_room: bool,
    pub topic: Option<String>,
    pub members: Option<u32>,
    pub encrypted: bool,
}

impl From<Convo> for ProfileTarget {
    fn from(c: Convo) -> Self {
        let is_room = c.is_room();
        Self {
            id: Some(c.id),
            name: c.name,
            mxid: c.mxid,
            status: c.status,
            is_room,
            topic: c.topic,
            members: c.members,
            encrypted: c.encrypted,
        }
    }
}

impl ProfileTarget {
    pub fn person(name: impl Into<String>, mxid: impl Into<String>) -> Self {
        Self {
            id: None,
            name: name.into(),
            mxid: Some(mxid.into()),
            status: Some(Presence::Online),
            is_room: false,
            topic: None,
            members: None,
            encrypted: false,
        }
    }
}

#[component]
pub fn ProfilePanel(
    target: ProfileTarget,
    on_close: EventHandler<()>,
    on_start_call: EventHandler<bool>,
    on_message: EventHandler<ProfileTarget>,
) -> Element {
    let client = use_context::<Rc<dyn VesperClient>>();
    let mut verify_open = use_signal(|| false);
    let mut verified = use_signal(|| false);

    let mxid = target.mxid.clone().unwrap_or_default();

    rsx! {
        div { style: "width:100%;height:100%;display:flex;flex-direction:column;background:var(--bg-canvas);border-left:1px solid var(--border-subtle);",
            div { style: "height:56px;border-bottom:1px solid var(--border-subtle);display:flex;align-items:center;padding:0 16px;flex-shrink:0;",
                span { style: "font-weight:700;font-size:15px;flex:1;", if target.is_room { "Room info" } else { "Profile" } }
                button {
                    onclick: move |_| on_close.call(()),
                    style: "background:none;border:none;color:var(--text-secondary);cursor:pointer;display:flex;",
                    Icon { name: IconName::X, size: 18 }
                }
            }
            div { style: "flex:1;overflow-y:auto;padding:24px;display:flex;flex-direction:column;gap:18px;align-items:center;text-align:center;",
                span { style: "position:relative;",
                    Avatar { name: "{target.name}", size: 72 }
                    if !target.is_room {
                        span { style: "position:absolute;right:-2px;bottom:-2px;", StatusDot { status: target.status.unwrap_or(Presence::Offline), size: 14 } }
                    }
                }
                div {
                    div { style: "font-size:18px;font-weight:700;", if target.is_room { "#{target.name}" } else { "{target.name}" } }
                    div { style: "font-size:13px;color:var(--text-tertiary);font-family:var(--font-mono);margin-top:2px;",
                        if target.is_room { "{target.topic.clone().unwrap_or_default()}" } else { "{mxid}" }
                    }
                }
                if target.encrypted {
                    Tag { tone: TagTone::Brand, "Encrypted" }
                }
                if target.is_room {
                    div { style: "font-size:13px;color:var(--text-secondary);display:flex;align-items:center;gap:6px;",
                        Icon { name: IconName::Users, size: 14 }
                        "{target.members.unwrap_or(0)} members"
                    }
                } else {
                    div { style: "width:100%;display:flex;flex-direction:column;gap:8px;",
                        div { style: "display:flex;gap:8px;justify-content:center;",
                            Button { variant: ButtonVariant::Secondary, size: crate::design_system::ButtonSize::Sm, onclick: { let t = target.clone(); move |_| on_message.call(t.clone()) },
                                Icon { name: IconName::MessageSquare, size: 14 }
                                " Message"
                            }
                            Button { variant: ButtonVariant::Secondary, size: crate::design_system::ButtonSize::Sm, onclick: move |_| on_start_call.call(true),
                                Icon { name: IconName::Video, size: 14 }
                                " Call"
                            }
                        }
                        div { style: "margin-top:10px;text-align:left;background:var(--bg-surface);border:1px solid var(--border-subtle);border-radius:var(--radius-md);padding:14px;display:flex;align-items:center;gap:10px;",
                            Icon {
                                name: if verified() { IconName::ShieldCheck } else { IconName::ShieldAlert },
                                size: 20,
                                color: if verified() { "var(--status-online)".to_string() } else { "var(--status-away)".to_string() },
                            }
                            div { style: "flex:1;",
                                div { style: "font-size:13px;font-weight:600;", if verified() { "Identity verified" } else { "Not verified" } }
                                div { style: "font-size:12px;color:var(--text-tertiary);", if verified() { "You have verified this user." } else { "Verify to confirm their identity." } }
                            }
                            if !verified() {
                                Button { variant: ButtonVariant::Secondary, size: crate::design_system::ButtonSize::Sm, onclick: move |_| verify_open.set(true), "Verify" }
                            }
                        }
                    }
                }
            }
            VerifyDialog {
                open: verify_open(),
                subject: target.name.clone(),
                on_close: move |_| verify_open.set(false),
                on_confirm: move |_| {
                    verified.set(true);
                    verify_open.set(false);
                    let client = client.clone();
                    let mxid = mxid.clone();
                    spawn(async move {
                        let _ = client.verify_user(&mxid).await;
                    });
                },
            }
        }
    }
}
