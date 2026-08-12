mod app_shell;
mod call_screen;
mod chat_view;
mod composer;
mod conversation;
mod discovery_modal;
mod focus_header;
mod message_row;
mod nav_drawer;
mod profile_panel;
mod switcher;
mod thread_panel;
mod verify_dialog;

pub use app_shell::AppShell;
pub use call_screen::{CallScreen, CallState};
pub use chat_view::ChatView;
pub use discovery_modal::DiscoveryModal;
pub use profile_panel::ProfileTarget;
pub use verify_dialog::VerifyDialog;

use dioxus::prelude::*;

use crate::data::{Message, ThreadReply};

#[derive(Clone, PartialEq)]
pub enum SidePanel {
    Thread {
        root: Option<Message>,
        /// Event id of the thread's root message — the key for the live
        /// threads map even when `root` failed to resolve.
        root_id: String,
        thread: Vec<ThreadReply>,
    },
    Profile {
        target: ProfileTarget,
    },
}

/// Overlay state shared across the whole authenticated shell: which side panel or call
/// is showing, and whether the discovery modal is open. Provided once by `Shell` in
/// `crate::app`, read/written from `SpacesRail`, `RoomList`, `Conversation`, and
/// `ChatLayout` alike — mirrors the prototype's single top-level `App()` owning
/// `sidePanel`/`call`/`discoveryOpen` for every child that can trigger them.
#[derive(Clone, Copy)]
pub struct ChatUiState {
    pub discovery_open: Signal<bool>,
    pub side_panel: Signal<Option<SidePanel>>,
    pub call: Signal<Option<CallState>>,
    pub nav_open: Signal<bool>,
    pub switcher_open: Signal<bool>,
    pub is_mobile: Signal<bool>,
}

impl ChatUiState {
    pub fn new() -> Self {
        Self {
            discovery_open: Signal::new(false),
            side_panel: Signal::new(None),
            call: Signal::new(None),
            nav_open: Signal::new(false),
            switcher_open: Signal::new(false),
            is_mobile: Signal::new(false),
        }
    }
}
