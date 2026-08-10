use dioxus::prelude::*;

use crate::design_system::Avatar;
use crate::icons::{Icon, IconName};

#[derive(Debug, Clone, PartialEq)]
pub struct CallState {
    pub name: String,
    pub video: bool,
    pub is_dm: bool,
}

struct Participant {
    name: String,
    me: bool,
    muted: bool,
}

#[component]
pub fn CallScreen(call: CallState, on_end: EventHandler<()>) -> Element {
    let mut mic_on = use_signal(|| true);
    let mut cam_on = use_signal(|| call.video);
    let mut sharing = use_signal(|| false);
    let mut show_participants = use_signal(|| false);

    let participants: Vec<Participant> = if call.is_dm {
        vec![
            Participant {
                name: "You".into(),
                me: true,
                muted: !mic_on(),
            },
            Participant {
                name: call.name.clone(),
                me: false,
                muted: false,
            },
        ]
    } else {
        vec![
            Participant {
                name: "You".into(),
                me: true,
                muted: !mic_on(),
            },
            Participant {
                name: "Akari Fur".into(),
                me: false,
                muted: false,
            },
            Participant {
                name: "Mira Solheim".into(),
                me: false,
                muted: true,
            },
            Participant {
                name: "Jonas Reyk".into(),
                me: false,
                muted: false,
            },
        ]
    };
    let participant_count = participants.len();

    let ctrl_style = |active: bool, danger: bool| -> String {
        let bg = if danger {
            "var(--status-danger)"
        } else if active {
            "var(--bg-surface-raised)"
        } else {
            "var(--white)"
        };
        let color = if danger {
            "var(--white)"
        } else if active {
            "var(--text-primary)"
        } else {
            "var(--black)"
        };
        format!(
            "width:46px;height:46px;border-radius:var(--radius-full);border:none;cursor:pointer;background:{bg};color:{color};display:flex;align-items:center;justify-content:center;"
        )
    };

    rsx! {
        div { style: "position:absolute;inset:0;background:var(--black);z-index:50;display:flex;flex-direction:column;font-family:var(--font-sans);color:var(--white);",
            div { style: "height:52px;display:flex;align-items:center;justify-content:space-between;padding:var(--space-3) var(--space-5);flex-shrink:0;",
                div { style: "font-size:14px;font-weight:600;display:flex;align-items:center;gap:var(--space-2);",
                    span { style: "width:8px;height:8px;border-radius:999px;background:var(--status-online);" }
                    "{call.name} · " if call.video { "Video call" } else { "Voice call" }
                }
                button {
                    onclick: move |_| show_participants.set(!show_participants()),
                    style: "background:none;border:none;color:var(--white);cursor:pointer;display:flex;align-items:center;gap:var(--space-2);font-size:13px;",
                    Icon { name: IconName::Users, size: 16 }
                    "{participant_count}"
                }
            }
            div { style: "flex:1;display:flex;min-width:0;",
                div { style: "flex:1;display:flex;flex-direction:column;padding:var(--space-4);gap:var(--space-3);min-width:0;",
                    if sharing() {
                        div { style: "flex:1;display:flex;flex-direction:column;gap:var(--space-3);",
                            div { style: "flex:1;background:var(--bg-surface-raised);border-radius:var(--radius-lg);border:1px solid var(--border-default);display:flex;align-items:center;justify-content:center;flex-direction:column;gap:8px;color:var(--text-tertiary);",
                                Icon { name: IconName::Monitor, size: 40 }
                                div { style: "font-size:13px;", "You are sharing your screen" }
                            }
                            div { style: "display:flex;gap:var(--space-3);height:84px;flex-shrink:0;",
                                for p in participants.iter() {
                                    div { key: "{p.name}", style: "width:130px;background:var(--bg-surface-raised);border-radius:var(--radius-md);display:flex;align-items:center;justify-content:center;flex-direction:column;gap:4px;",
                                        Avatar { name: "{p.name}", size: 30 }
                                        span { style: "font-size:11px;", if p.me { "You" } else { "{p.name}" } }
                                    }
                                }
                            }
                        }
                    } else {
                        {
                            let grid_columns = if participant_count <= 2 { "1fr" } else { "repeat(2, 1fr)" };
                            let grid_rows = if participant_count <= 2 { "1fr" } else { "minmax(0,1fr)" };
                            rsx! {
                                div {
                                    style: "flex:1;display:grid;gap:var(--space-3);grid-template-columns:{grid_columns};grid-auto-rows:{grid_rows};",
                                    for p in participants.iter() {
                                        div { key: "{p.name}", style: "position:relative;background:var(--bg-surface-raised);border-radius:var(--radius-lg);border:1px solid var(--border-default);display:flex;align-items:center;justify-content:center;",
                                            if (p.me && cam_on()) || (!p.me && call.video) {
                                                div { style: "color:var(--text-tertiary);display:flex;flex-direction:column;align-items:center;gap:8px;",
                                                    Icon { name: IconName::Video, size: 28 }
                                                    span { style: "font-size:12px;", "Camera preview" }
                                                }
                                            } else {
                                                Avatar { name: "{p.name}", size: 64 }
                                            }
                                            div { style: "position:absolute;bottom:var(--space-2);left:var(--space-2);display:flex;align-items:center;gap:var(--space-1);background:rgba(0,0,0,0.5);border-radius:999px;padding:var(--space-1) var(--space-2);font-size:12px;",
                                                if p.muted {
                                                    Icon { name: IconName::MicOff, size: 12 }
                                                }
                                                if p.me { "You" } else { "{p.name}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if show_participants() {
                    div { style: "width:240px;border-left:1px solid var(--border-subtle);padding:var(--space-4);flex-shrink:0;",
                        div { style: "font-size:13px;font-weight:700;margin-bottom:var(--space-3);", "In this call" }
                        div { style: "display:flex;flex-direction:column;gap:var(--space-3);",
                            for p in participants.iter() {
                                div { key: "{p.name}", style: "display:flex;align-items:center;gap:var(--space-2);font-size:13px;",
                                    Avatar { name: "{p.name}", size: 26 }
                                    span { style: "flex:1;", if p.me { "You" } else { "{p.name}" } }
                                    if p.muted {
                                        Icon { name: IconName::MicOff, size: 13, color: "var(--status-danger)".to_string() }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { style: "display:flex;justify-content:center;gap:var(--space-4);padding:var(--space-4) 0 var(--space-5);flex-shrink:0;",
                button { title: "Toggle mic", onclick: move |_| mic_on.set(!mic_on()), style: "{ctrl_style(mic_on(), false)}",
                    Icon { name: if mic_on() { IconName::Mic } else { IconName::MicOff }, size: 19 }
                }
                button { title: "Toggle camera", onclick: move |_| cam_on.set(!cam_on()), style: "{ctrl_style(cam_on(), false)}",
                    Icon { name: if cam_on() { IconName::Video } else { IconName::VideoOff }, size: 19 }
                }
                button { title: "Share screen", onclick: move |_| sharing.set(!sharing()), style: "{ctrl_style(sharing(), false)}",
                    Icon { name: IconName::Monitor, size: 19 }
                }
                button {
                    title: "Leave call",
                    onclick: move |_| on_end.call(()),
                    style: "{ctrl_style(false, true)}transform:rotate(135deg);",
                    Icon { name: IconName::Phone, size: 19 }
                }
            }
        }
    }
}
