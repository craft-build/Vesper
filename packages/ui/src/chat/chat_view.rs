use std::rc::Rc;

use dioxus::prelude::*;

use super::conversation::Conversation;
use super::focus_header::FocusHeader;
use super::nav_drawer::NavDrawer;
use super::profile_panel::ProfilePanel;
use super::switcher::Switcher;
use super::thread_panel::ThreadPanel;
use super::{CallScreen, CallState, ChatUiState, ProfileTarget, SidePanel};
use crate::app::Route;
use crate::data::{ConvoKind, VesperClient};

#[component]
pub fn ChatView(#[props(default = None)] room_id: Option<String>) -> Element {
    let client = use_context::<Rc<dyn VesperClient>>();
    let mut ui = use_context::<ChatUiState>();
    let navigator = use_navigator();

    let convos = {
        let client = client.clone();
        use_resource(move || {
            let client = client.clone();
            async move { client.conversations().await }
        })
    };
    let all_convos = convos().unwrap_or_default();

    let dms: Vec<_> = all_convos
        .iter()
        .filter(|c| c.kind == ConvoKind::Dm)
        .cloned()
        .collect();
    let rooms: Vec<_> = all_convos
        .iter()
        .filter(|c| c.kind == ConvoKind::Room)
        .cloned()
        .collect();

    let convo = room_id
        .as_ref()
        .and_then(|id| all_convos.iter().find(|c| &c.id == id))
        .or_else(|| all_convos.iter().find(|c| c.kind == ConvoKind::Room))
        .cloned();

    let select_convo = move |id: String| {
        if (ui.is_mobile)() {
            ui.nav_open.set(false);
        }
        ui.switcher_open.set(false);
        ui.side_panel.set(None);
        navigator.push(Route::RoomView { room_id: id });
    };

    let open_thread = {
        let client = client.clone();
        let room_id = convo.as_ref().map(|c| c.id.clone());
        move |message_id: String| {
            let client = client.clone();
            let room_id = room_id.clone();
            let mut ui = ui;
            spawn(async move {
                let Some(room_id) = room_id else { return };
                let msgs = client.messages(&room_id).await;
                let root = msgs.into_iter().find(|m| m.id == message_id);
                let thread = client.thread(&message_id).await;
                ui.side_panel.set(Some(SidePanel::Thread { root, thread }));
            });
        }
    };

    let open_profile = {
        let dms = dms.clone();
        move |name: String| {
            let target = dms
                .iter()
                .find(|d| d.name == name)
                .cloned()
                .map(ProfileTarget::from)
                .unwrap_or_else(|| {
                    let mxid = format!("@{}:matrix.org", name.to_lowercase().replace(' ', ""));
                    ProfileTarget::person(name.clone(), mxid)
                });
            ui.side_panel.set(Some(SidePanel::Profile { target }));
        }
    };

    let open_room_info = {
        let convo = convo.clone();
        move |_: ()| {
            if let Some(c) = convo.clone() {
                ui.side_panel.set(Some(SidePanel::Profile {
                    target: ProfileTarget::from(c),
                }));
            }
        }
    };

    let start_call = {
        let convo = convo.clone();
        move |video: bool| {
            if let Some(c) = &convo {
                ui.call.set(Some(CallState {
                    name: c.name.clone(),
                    video,
                    is_dm: c.kind == ConvoKind::Dm,
                }));
            }
        }
    };

    let message_target = {
        let mut select_convo = select_convo.clone();
        move |target: ProfileTarget| {
            ui.side_panel.set(None);
            if let Some(id) = target.id.clone() {
                select_convo(id);
            }
        }
    };

    let active_id = convo.as_ref().map(|c| c.id.clone()).unwrap_or_default();
    let is_mobile = (ui.is_mobile)();
    let panel_width = if is_mobile { "100%" } else { "340px" };
    let panel_position = if is_mobile { "absolute" } else { "static" };
    let panel_inset = if is_mobile { "0" } else { "auto" };
    let panel_z_index = if is_mobile { "10" } else { "1" };

    rsx! {
        div { style: "flex:1;display:flex;min-width:0;position:relative;height:100%;",
            if (ui.nav_open)() && !is_mobile {
                NavDrawer {
                    dms: dms.clone(),
                    rooms: rooms.clone(),
                    active_id: active_id.clone(),
                    on_select: select_convo.clone(),
                    on_close: move |_| ui.nav_open.set(false),
                    on_open_discovery: move |_| ui.discovery_open.set(true),
                    on_open_settings: move |_| { ui.nav_open.set(false); navigator.push(Route::SettingsPage {}); },
                    on_open_self: move |_| { ui.nav_open.set(false); ui.side_panel.set(Some(SidePanel::Profile { target: ProfileTarget::person("You", "@you:vesper.chat") })); },
                    inline: true,
                }
            }
            div { key: "{active_id}", style: "flex:1;display:flex;flex-direction:column;min-width:0;",
                if let Some(c) = &convo {
                    FocusHeader {
                        convo: c.clone(),
                        on_open_nav: move |_| ui.nav_open.set(!(ui.nav_open)()),
                        on_open_switcher: move |_| ui.switcher_open.set(true),
                        on_start_call: start_call.clone(),
                        on_open_room_info: open_room_info.clone(),
                    }
                    Conversation {
                        convo: c.clone(),
                        is_mobile: false,
                        hide_header: true,
                        on_start_call: start_call.clone(),
                        on_open_thread: open_thread,
                        on_open_profile: open_profile,
                        on_open_room_info: open_room_info.clone(),
                    }
                } else {
                    div { style: "flex:1;display:flex;align-items:center;justify-content:center;color:var(--text-tertiary);font-size:14px;text-align:center;padding:24px;",
                        "Select a room or direct message to start chatting."
                    }
                }
            }
            if let Some(panel) = ui.side_panel.read().clone() {
                div {
                    style: "width:{panel_width};flex-shrink:0;height:100%;position:{panel_position};inset:{panel_inset};z-index:{panel_z_index};",
                    match panel {
                        SidePanel::Thread { root, thread } => rsx! {
                            ThreadPanel { root_message: root, thread, on_close: move |_| ui.side_panel.set(None) }
                        },
                        SidePanel::Profile { target } => rsx! {
                            ProfilePanel {
                                target,
                                on_close: move |_| ui.side_panel.set(None),
                                on_start_call: start_call.clone(),
                                on_message: message_target,
                            }
                        },
                    }
                }
            }
            if (ui.nav_open)() && is_mobile {
                NavDrawer {
                    dms: dms.clone(),
                    rooms: rooms.clone(),
                    active_id: active_id.clone(),
                    on_select: select_convo.clone(),
                    on_close: move |_| ui.nav_open.set(false),
                    on_open_discovery: move |_| ui.discovery_open.set(true),
                    on_open_settings: move |_| { ui.nav_open.set(false); navigator.push(Route::SettingsPage {}); },
                    on_open_self: move |_| { ui.nav_open.set(false); ui.side_panel.set(Some(SidePanel::Profile { target: ProfileTarget::person("You", "@you:vesper.chat") })); },
                    inline: false,
                }
            }
            if (ui.switcher_open)() {
                Switcher { dms, rooms, on_close: move |_| ui.switcher_open.set(false), on_select: select_convo }
            }
            if let Some(call) = ui.call.read().clone() {
                CallScreen { call, on_end: move |_| ui.call.set(None) }
            }
        }
    }
}
