//! Nav drawer (checkpoint 09): DMs on top, then one collapsible section per
//! joined space with its child rooms grouped underneath, then the flat
//! "ROOMS" bucket for everything no space claims — so grouping never hides
//! a room (docs/09: default grouping, one level deep; nested spaces render
//! as their own top-level sections with a "nested" hint).
//!
//! Leaving is a right-click away: a context menu on any row offers "Leave
//! room". The ⌘K switcher stays flat over all rooms.

use std::collections::{BTreeMap, HashMap, HashSet};

use dioxus::prelude::*;

use crate::data::{ClientState, Convo, Presence, Space};
use crate::design_system::{Avatar, Badge, StatusDot};
use crate::icons::{Icon, IconName};
use crate::window_chrome::DragStrip;

const LOGO: Asset = asset!("/assets/vesper/logo.svg");

/// A pending context menu: the room id it acts on plus the fixed-viewport
/// position where the right-click happened.
type RoomMenu = (String, i64, i64);

#[derive(Clone, Copy)]
struct RowEvents {
    on_select: EventHandler<String>,
    on_open_menu: EventHandler<RoomMenu>,
}

/// Resolve a DM's status dot: the live presence map (freshest — written
/// by the backend's presence poll) beats the snapshot `Convo.status`
/// (only refreshed when the room-list sync loop happens to wake) beats
/// Offline (unknown / mock without presence).
fn resolve_status(item: &Convo, presence: &BTreeMap<String, Presence>) -> Presence {
    item.mxid
        .as_deref()
        .and_then(|mxid| presence.get(mxid).copied())
        .or(item.status)
        .unwrap_or(Presence::Offline)
}

fn row(
    item: &Convo,
    is_dm: bool,
    active_id: &str,
    events: RowEvents,
    presence: &BTreeMap<String, Presence>,
) -> Element {
    let id = item.id.clone();
    let menu_id = item.id.clone();
    let is_active = active_id == item.id;
    let bg = if is_active {
        "var(--bg-selected)"
    } else {
        "transparent"
    };
    let status = if is_dm {
        resolve_status(item, presence)
    } else {
        Presence::Offline
    };
    let RowEvents {
        on_select,
        on_open_menu,
    } = events;
    rsx! {
        button {
            key: "{item.id}",
            onclick: move |_| on_select.call(id.clone()),
            oncontextmenu: move |evt: Event<MouseData>| {
                evt.prevent_default();
                let pos = evt.client_coordinates();
                on_open_menu.call((menu_id.clone(), pos.x as i64, pos.y as i64));
            },
            style: "width:100%;display:flex;align-items:center;gap:10px;padding:8px 10px;background:{bg};border:none;border-radius:var(--radius-md);cursor:pointer;text-align:left;margin-bottom:2px;",
            title: "Right-click for options",
            span { style: "position:relative;flex-shrink:0;",
                Avatar { name: "{item.name}", size: 32, mxc: item.avatar.clone() }
                if is_dm {
                    span { style: "position:absolute;right:-2px;bottom:-2px;", StatusDot { status, size: 9 } }
                }
            }
            span { style: "flex:1;min-width:0;font-size:14px;font-weight:600;display:flex;align-items:center;gap:4px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;",
                if !is_dm {
                    Icon { name: IconName::Hash, size: 12, color: "var(--text-tertiary)".to_string() }
                }
                "{item.name}"
            }
            Badge { count: item.unread as i64 }
        }
    }
}

/// One space's collapsible section: header (avatar/name/counts + nested
/// hint) and the member rooms in the space's declared order. Children the
/// account hasn't joined are simply absent — grouping is over joined rooms.
#[allow(clippy::too_many_arguments)]
fn space_section(
    space: &Space,
    rooms: &[Convo],
    active_id: &str,
    events: RowEvents,
    presence: &BTreeMap<String, Presence>,
    collapsed: bool,
    on_toggle: EventHandler<String>,
    nested: bool,
) -> Element {
    let id = space.id.clone();
    let space_id = space.id.clone();
    let order: HashMap<&str, usize> = space
        .children
        .iter()
        .enumerate()
        .map(|(i, child)| (child.as_str(), i))
        .collect();
    let mut grouped: Vec<&Convo> = rooms
        .iter()
        .filter(|r| r.space.as_deref() == Some(space_id.as_str()))
        .collect();
    grouped.sort_by_key(|r| order.get(r.id.as_str()).copied().unwrap_or(usize::MAX));
    rsx! {
        div { key: "{space.id}",
            button {
                onclick: move |_| on_toggle.call(id.clone()),
                style: "width:100%;display:flex;align-items:center;gap:6px;padding:10px 8px 4px;background:none;border:none;cursor:pointer;text-align:left;",
                Icon {
                    name: if collapsed { IconName::ChevronRight } else { IconName::ChevronDown },
                    size: 12,
                    color: "var(--text-tertiary)".to_string()
                }
                span { style: "flex-shrink:0;",
                    Avatar { name: "{space.name}", size: 16, mxc: space.avatar.clone() }
                }
                span { style: "flex:1;min-width:0;font-size:11px;font-weight:700;letter-spacing:0.06em;color:var(--text-tertiary);text-transform:uppercase;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;",
                    "{space.name}"
                }
                if nested {
                    span { style: "font-size:10px;color:var(--text-tertiary);", "nested" }
                }
                span { style: "font-size:10px;color:var(--text-tertiary);", "{grouped.len()}" }
            }
            if !collapsed {
                for r in grouped.iter() {
                    {row(r, false, active_id, events, presence)}
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn drawer_body(
    dms: &[Convo],
    rooms: &[Convo],
    spaces: &[Space],
    active_id: &str,
    events: RowEvents,
    presence: &BTreeMap<String, Presence>,
    on_close: EventHandler<()>,
    on_open_discovery: EventHandler<()>,
    on_open_settings: EventHandler<()>,
    on_open_self: EventHandler<()>,
    me_avatar: Option<String>,
    collapsed: Signal<HashMap<String, bool>>,
    menu: Signal<Option<RoomMenu>>,
    on_leave: EventHandler<String>,
) -> Element {
    // Nested-space hint (v1 limitation): a space listed in *another*
    // space's children renders at top level with a "nested" note instead of
    // recursing. (Self-references don't count — a room may share a space's
    // id in mock fixtures, and Matrix ids are globally unique anyway.)
    let is_nested = |space: &Space| {
        spaces
            .iter()
            .any(|o| o.id != space.id && o.children.contains(&space.id))
    };
    // Rooms claimed by a joined space (the space itself, not its children —
    // a room whose space is joined always groups, even when the space's
    // child list hasn't seen it; it sorts after the ordered children).
    let space_ids: HashSet<&str> = spaces.iter().map(|s| s.id.as_str()).collect();
    let ungrouped: Vec<&Convo> = rooms
        .iter()
        .filter(|r| !r.space.as_deref().is_some_and(|s| space_ids.contains(s)))
        .collect();
    let leave_id = menu.read().clone().map(|(id, _, _)| id);
    let mut menu = menu;
    rsx! {
        div { style: "width:280px;height:100%;background:var(--bg-surface);display:flex;flex-direction:column;position:relative;",
            div {
                style: "padding:16px 16px 8px;display:flex;align-items:center;gap:8px;height:56px;",
                img { src: LOGO, alt: "", style: "width:26px;height:26px;border-radius:999px;" }
                span { style: "font-weight:800;font-size:15px;", "Vesper" }
                DragStrip {}
                button {
                    onclick: move |_| on_close.call(()),
                    style: "background:none;border:none;color:var(--text-secondary);cursor:pointer;display:flex;",
                    Icon { name: IconName::X, size: 17 }
                }
            }
            div { style: "flex:1;overflow-y:auto;padding:4px 10px;",
                if !dms.is_empty() {
                    div { style: "font-size:11px;font-weight:700;letter-spacing:0.06em;color:var(--text-tertiary);padding:10px 8px 4px;", "DIRECT MESSAGES" }
                    for d in dms.iter() {
                        {row(d, true, active_id, events, presence)}
                    }
                }
                for space in spaces.iter() {
                    {space_section(
                        space,
                        rooms,
                        active_id,
                        events,
                        presence,
                        collapsed.read().get(&space.id).copied().unwrap_or(false),
                        {
                            let mut collapsed = collapsed;
                            EventHandler::new(move |id: String| {
                                collapsed
                                    .write()
                                    .entry(id)
                                    .and_modify(|v| *v = !*v)
                                    .or_insert(true);
                            })
                        },
                        is_nested(space),
                    )}
                }
                if !ungrouped.is_empty() {
                    div { style: "font-size:11px;font-weight:700;letter-spacing:0.06em;color:var(--text-tertiary);padding:14px 8px 4px;", "ROOMS" }
                    for r in ungrouped.iter() {
                        {row(r, false, active_id, events, presence)}
                    }
                }
            }
            // Match the composer's resting 77px footprint so both top borders
            // form one continuous line across the app shell.
            div { style: "height:77px;flex-shrink:0;padding:12px 10px;border-top:1px solid var(--border-subtle);display:flex;align-items:center;gap:6px;",
                button {
                    onclick: move |_| on_open_discovery.call(()),
                    style: "height:40px;flex:1;display:flex;align-items:center;gap:8px;background:none;border:none;color:var(--text-secondary);cursor:pointer;padding:0 10px;border-radius:var(--radius-md);font-size:13px;font-weight:600;",
                    Icon { name: IconName::Plus, size: 16 }
                    "Browse"
                }
                button {
                    onclick: move |_| on_open_settings.call(()),
                    title: "Settings",
                    "aria-label": "Settings",
                    style: "width:40px;height:40px;background:none;border:none;color:var(--text-secondary);cursor:pointer;padding:0;border-radius:var(--radius-md);display:flex;align-items:center;justify-content:center;",
                    Icon { name: IconName::Settings, size: 17 }
                }
                button {
                    onclick: move |_| on_open_self.call(()),
                    title: "Your profile",
                    "aria-label": "Your profile",
                    style: "width:40px;height:40px;background:none;border:none;color:var(--text-secondary);cursor:pointer;padding:0;border-radius:var(--radius-md);display:flex;align-items:center;justify-content:center;",
                    Avatar { name: "You", size: 24, mxc: me_avatar.clone() }
                }
            }
            // Right-click menu (checkpoint 09 leave flow). A transparent
            // full-screen layer beneath it swallows the next click to close.
            if let Some((_, x, y)) = *menu.read() {
                div {
                    style: "position:fixed;inset:0;z-index:69;",
                    onclick: move |_| menu.set(None),
                }
                div { style: "position:fixed;left:{x}px;top:{y}px;z-index:70;background:var(--bg-surface-raised);border:1px solid var(--border-default);border-radius:var(--radius-md);box-shadow:var(--shadow-lg);padding:4px;display:flex;flex-direction:column;min-width:140px;",
                    button {
                        onclick: move |_| {
                            if let Some(id) = leave_id.clone() {
                                menu.set(None);
                                on_leave.call(id);
                            }
                        },
                        style: "display:flex;align-items:center;gap:8px;padding:7px 10px;background:none;border:none;border-radius:var(--radius-sm);color:var(--text-primary);cursor:pointer;font-size:13px;text-align:left;",
                        Icon { name: IconName::LogOut, size: 14, color: "var(--text-secondary)".to_string() }
                        "Leave room"
                    }
                }
            }
        }
    }
}

#[component]
pub fn NavDrawer(
    dms: Vec<Convo>,
    rooms: Vec<Convo>,
    spaces: Vec<Space>,
    active_id: String,
    on_select: EventHandler<String>,
    on_close: EventHandler<()>,
    on_open_discovery: EventHandler<()>,
    on_open_settings: EventHandler<()>,
    on_open_self: EventHandler<()>,
    /// Leave a room (checkpoint 09): fired from the right-click menu.
    on_leave: EventHandler<String>,
    /// Signed-in account's avatar MXC (checkpoint 07) for the "You" button.
    #[props(default = None)]
    me_avatar: Option<String>,
    #[props(default = false)] inline: bool,
) -> Element {
    // Which spaces are collapsed (absent = expanded by default — grouping
    // is the primary view, per docs/09's "pick one").
    let collapsed = use_signal(HashMap::<String, bool>::new);
    // The open right-click menu, if any.
    let mut menu = use_signal(|| None::<RoomMenu>);
    // Live presence (checkpoint 06): the DM rows resolve their status dots
    // against the backend's presence map — same precedence as the profile
    // panel (live map → snapshot `status` → Offline). Reading the signal
    // here subscribes the whole drawer, so dots update as presence lands
    // instead of waiting for the next room-list sync wake.
    let sync = use_context::<ClientState>();
    let presence: BTreeMap<String, Presence> = (sync.presence)();
    let events = RowEvents {
        on_select,
        on_open_menu: EventHandler::new(move |(id, x, y): RoomMenu| menu.set(Some((id, x, y)))),
    };

    if inline {
        rsx! {
            div { style: "width:280px;flex-shrink:0;height:100%;border-right:1px solid var(--border-subtle);",
                {drawer_body(&dms, &rooms, &spaces, &active_id, events, &presence, on_close, on_open_discovery, on_open_settings, on_open_self, me_avatar.clone(), collapsed, menu, on_leave)}
            }
        }
    } else {
        rsx! {
            div { onclick: move |_| on_close.call(()), style: "position:absolute;inset:0;background:rgba(0,0,0,0.4);z-index:30;" }
            div { style: "position:absolute;left:0;top:0;bottom:0;z-index:31;box-shadow:var(--shadow-lg);",
                {drawer_body(&dms, &rooms, &spaces, &active_id, events, &presence, on_close, on_open_discovery, on_open_settings, on_open_self, me_avatar.clone(), collapsed, menu, on_leave)}
            }
        }
    }
}
