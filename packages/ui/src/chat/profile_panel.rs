use std::rc::Rc;

use dioxus::prelude::*;

use super::verify_dialog::VerifyDialog;
use crate::data::{
    ClientState, Convo, Me, Presence, VerificationAction, VerificationState, VerificationTarget,
    VesperClient,
};
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
    /// Room/counterpart avatar MXC (checkpoint 07); initials fallback.
    pub avatar: Option<String>,
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
            avatar: c.avatar,
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
            avatar: None,
        }
    }

    /// The signed-in user's own profile (nav drawer "You" button,
    /// checkpoint 07): carries the account avatar MXC when known.
    pub fn own(me: &Me) -> Self {
        Self {
            id: None,
            name: me.name.clone(),
            mxid: Some(me.id.clone()),
            status: Some(Presence::Online),
            is_room: false,
            topic: None,
            members: None,
            encrypted: false,
            avatar: me.avatar.clone(),
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
    let sync = use_context::<ClientState>();
    let mut verify_open = use_signal(|| false);
    let mut verified = use_signal(|| false);

    let mxid = target.mxid.clone().unwrap_or_default();
    // Clone for the effect closure; `mxid` itself stays owned by the
    // component body for the panel markup below.
    let effect_mxid = mxid.clone();

    // Verification state follows the backend session: only a *user-targeted*
    // session for this contact's MXID marks them verified here — a device
    // verification from Settings must not (review P2). Guarded same-value
    // write (checkpoint-06 effect-loop lesson).
    use_effect(move || {
        let done = sync.verification.read().as_ref().is_some_and(|s| {
            matches!(s.state, VerificationState::Done)
                && s.target == VerificationTarget::User(effect_mxid.clone())
        });
        if done && !verified() {
            verified.set(true);
        }
    });

    // Live presence (checkpoint 06): prefer the backend's presence map for
    // the contact's MXID, falling back to the snapshot `target.status`, then
    // Offline. Reading `sync.presence` here subscribes the panel to updates.
    let presence = sync
        .presence
        .read()
        .get(&mxid)
        .copied()
        .or(target.status)
        .unwrap_or(Presence::Offline);

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
                    Avatar { name: "{target.name}", size: 72, mxc: target.avatar.clone() }
                    if !target.is_room {
                        span { style: "position:absolute;right:-2px;bottom:-2px;", StatusDot { status: presence, size: 14 } }
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
                                Button { variant: ButtonVariant::Secondary, size: crate::design_system::ButtonSize::Sm, onclick: {
                                    let client = client.clone();
                                    let mxid = mxid.clone();
                                    move |_| {
                                        verify_open.set(true);
                                        let client = client.clone();
                                        let mxid = mxid.clone();
                                        spawn(async move {
                                            client.start_verification(VerificationTarget::User(mxid));
                                        });
                                    }
                                }, "Verify" }
                            }
                        }
                    }
                }
            }
            VerifyDialog {
                open: verify_open(),
                on_close: {
                    let client = client.clone();
                    move |_| {
                        verify_open.set(false);
                        // Manual close mid-session cancels it server-side.
                        let client = client.clone();
                        spawn(async move {
                            client.verification_action(VerificationAction::Cancel);
                        });
                    }
                },
                on_action: {
                    let client = client.clone();
                    move |action: VerificationAction| {
                        let client = client.clone();
                        spawn(async move {
                            client.verification_action(action);
                        });
                    }
                },
            }
        }
    }
}
