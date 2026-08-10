//! Custom-titlebar building blocks. Desktop builds run fully borderless (see
//! `packages/desktop/src/main.rs`), so the app's own header rows double as the
//! titlebar and draw their own window controls. On web/mobile builds every
//! piece here is inert, so call sites stay free of `cfg` attributes.

use dioxus::prelude::*;

#[cfg(feature = "desktop")]
use crate::icons::{Icon, IconName};

/// Imperative window handlers. Both are no-ops on non-desktop builds. Prefer
/// [`DragStrip`] over attaching these to a container with children: a dedicated
/// strip avoids `stop_propagation` on every interactive descendant (which
/// silently breaks context menus and nested handlers).
#[derive(Clone, Copy)]
pub struct TitleBar {
    pub on_drag: EventHandler<MouseEvent>,
    pub on_toggle_maximize: EventHandler<MouseEvent>,
}

pub fn use_titlebar() -> TitleBar {
    #[cfg(feature = "desktop")]
    {
        let window = dioxus::desktop::use_window();
        let toggle = window.clone();
        TitleBar {
            on_drag: EventHandler::new(move |_| window.drag()),
            on_toggle_maximize: EventHandler::new(move |_| toggle.toggle_maximized()),
        }
    }
    #[cfg(not(feature = "desktop"))]
    {
        TitleBar {
            on_drag: EventHandler::new(|_| {}),
            on_toggle_maximize: EventHandler::new(|_| {}),
        }
    }
}

/// An empty drag strip that owns the titlebar behavior for a header. It
/// stretches to fill its flex parent, so on a row with `flex:1;` spacing it
/// occupies only the dead space between the left and right controls — there
/// are no interactive children inside it, so no propagation management is
/// needed and downstream context menus keep working.
///
/// Place it where the flexible gap would otherwise be, e.g.
/// `DragStrip {}` between the title controls and the trailing buttons.
#[component]
pub fn DragStrip() -> Element {
    let titlebar = use_titlebar();
    rsx! {
        div {
            style: "flex:1;align-self:stretch;",
            onmousedown: move |evt| titlebar.on_drag.call(evt),
            ondoubleclick: move |evt| titlebar.on_toggle_maximize.call(evt),
        }
    }
}

/// Unified window controls — minimize `−`, maximize `▢`, close `×` — rendered
/// the same on every desktop platform and placed at the top-right of a header.
/// The hover style dims the button background (close gets a danger tint).
/// Renders nothing on non-desktop targets.
#[component]
pub fn WindowControls() -> Element {
    #[cfg(feature = "desktop")]
    {
        let mut close_hover = use_signal(|| false);
        let mut max_hover = use_signal(|| false);
        let mut min_hover = use_signal(|| false);

        const BTN: &str = "width:38px;height:30px;border:none;cursor:default;display:flex;align-items:center;justify-content:center;border-radius:var(--radius-md);background:transparent;color:var(--text-secondary);";
        const BTN_HOVER: &str = "background:var(--bg-hover);color:var(--text-primary);";

        let window = dioxus::desktop::use_window();
        let minimize = window.clone();
        let maximize = window.clone();

        let min_style = if min_hover() {
            format!("{BTN}{BTN_HOVER}")
        } else {
            BTN.to_string()
        };
        let max_style = if max_hover() {
            format!("{BTN}{BTN_HOVER}")
        } else {
            BTN.to_string()
        };
        let close_style = if close_hover() {
            format!("{BTN}background:var(--status-danger);color:#fff;")
        } else {
            BTN.to_string()
        };

        rsx! {
            div {
                style: "display:flex;align-items:center;gap:2px;margin-left:4px;",
                button {
                    title: "Minimize",
                    "aria-label": "Minimize",
                    style: "{min_style}",
                    onmousedown: move |evt| evt.stop_propagation(),
                    onmouseenter: move |_| min_hover.set(true),
                    onmouseleave: move |_| min_hover.set(false),
                    onclick: move |_| minimize.window.set_minimized(true),
                    Icon { name: IconName::Minus, size: 15 }
                }
                button {
                    title: "Maximize",
                    "aria-label": "Maximize",
                    style: "{max_style}",
                    onmousedown: move |evt| evt.stop_propagation(),
                    onmouseenter: move |_| max_hover.set(true),
                    onmouseleave: move |_| max_hover.set(false),
                    onclick: move |_| maximize.toggle_maximized(),
                    Icon { name: IconName::Square, size: 13 }
                }
                button {
                    title: "Close",
                    "aria-label": "Close",
                    style: "{close_style}",
                    onmousedown: move |evt| evt.stop_propagation(),
                    onmouseenter: move |_| close_hover.set(true),
                    onmouseleave: move |_| close_hover.set(false),
                    onclick: move |_| window.close(),
                    Icon { name: IconName::X, size: 16 }
                }
            }
        }
    }
    #[cfg(not(feature = "desktop"))]
    {
        rsx! {}
    }
}

/// Invisible hit zones around the window edges that restore edge/corner
/// resizing for the borderless window via `drag_resize_window`. Active on
/// every desktop platform (we have no native resize handles anywhere).
/// Web/mobile: nothing.
#[component]
pub fn ResizeBorders() -> Element {
    #[cfg(feature = "desktop")]
    {
        const G: i64 = 6; // hit-zone thickness in px
        let directions = [
            (
                "n",
                format!("top:0;left:{G}px;right:{G}px;height:{G}px;cursor:ns-resize;"),
                dioxus::desktop::tao::window::ResizeDirection::North,
            ),
            (
                "s",
                format!("bottom:0;left:{G}px;right:{G}px;height:{G}px;cursor:ns-resize;"),
                dioxus::desktop::tao::window::ResizeDirection::South,
            ),
            (
                "w",
                format!("top:{G}px;bottom:{G}px;left:0;width:{G}px;cursor:ew-resize;"),
                dioxus::desktop::tao::window::ResizeDirection::West,
            ),
            (
                "e",
                format!("top:{G}px;bottom:{G}px;right:0;width:{G}px;cursor:ew-resize;"),
                dioxus::desktop::tao::window::ResizeDirection::East,
            ),
            (
                "nw",
                format!("top:0;left:0;width:{G}px;height:{G}px;cursor:nwse-resize;"),
                dioxus::desktop::tao::window::ResizeDirection::NorthWest,
            ),
            (
                "ne",
                format!("top:0;right:0;width:{G}px;height:{G}px;cursor:nesw-resize;"),
                dioxus::desktop::tao::window::ResizeDirection::NorthEast,
            ),
            (
                "sw",
                format!("bottom:0;left:0;width:{G}px;height:{G}px;cursor:nesw-resize;"),
                dioxus::desktop::tao::window::ResizeDirection::SouthWest,
            ),
            (
                "se",
                format!("bottom:0;right:0;width:{G}px;height:{G}px;cursor:nwse-resize;"),
                dioxus::desktop::tao::window::ResizeDirection::SouthEast,
            ),
        ];

        let window = dioxus::desktop::use_window();
        rsx! {
            for (key, pos, dir) in directions {
                div {
                    key: "{key}",
                    style: "position:fixed;{pos}z-index:2147483647;",
                    onmousedown: {
                        let win = window.clone();
                        move |_| {
                            let _ = win.drag_resize_window(dir);
                        }
                    },
                }
            }
        }
    }
    #[cfg(not(feature = "desktop"))]
    {
        rsx! {}
    }
}
