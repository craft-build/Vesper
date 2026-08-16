//! Live state: incoming typing, presence, and desktop notifications
//! (checkpoint 06).
//!
//! Three concerns, all running on the matrix runtime thread:
//! - [`TypingManager`]: the outgoing typing debounce (4s idle reset, stop on
//!   send), spawned off the command loop so it never blocks other commands.
//! - [`start_live`]: a `PresenceEvent` handler plus a presence poll task
//!   maintaining the `presence` map, plus (native only) a
//!   `SyncMessageLikeEvent` handler firing desktop notifications for
//!   messages in background rooms.
//!
//! The notification handler reads the UI-written `focused` / `active_room`
//! signals to suppress notifications the user doesn't need.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dioxus_signals::{ReadableExt, WritableExt};
use matrix_sdk::event_handler::EventHandlerDropGuard;
use matrix_sdk::ruma::events::presence::PresenceEvent;
use matrix_sdk::ruma::presence::PresenceState;
use matrix_sdk::{
    Client, Room,
    ruma::{
        self, OwnedRoomId, RoomId, UserId,
        api::client::presence::get_presence,
        events::SyncMessageLikeEvent,
        events::room::message::{MessageType, RoomMessageEventContent},
    },
};

use crate::{
    api::ClientState,
    model::{ConvoKind, Presence},
};

/// How long after the last input we keep telling the homeserver we're typing
/// before sending a `typing = false` (docs/06 §Design decisions).
const TYPING_IDLE_RESET: Duration = Duration::from_secs(4);

/// How often the presence poll refreshes DM counterparts' presence over
/// `GET /presence/{userId}/status` (see [`poll_dm_presence`]).
const PRESENCE_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Manages outgoing typing notices per room with a 4s idle reset.
///
/// `set` is synchronous and non-blocking: every `Room::typing_notice` call is
/// `tokio::spawn`'d, so driving this from the runtime command loop never
/// stalls other commands (docs/05 "command-loop blocking" applies here too).
///
/// We do NOT dedup `typing_notice(true)` ourselves: the SDK resends `true`
/// only when called >3s after the last send (`TYPING_NOTICE_RESEND_TIMEOUT`),
/// and the homeserver expires the status 4s after the last `true`
/// (`TYPING_NOTICE_TIMEOUT`). Suppressing resends would let the status expire
/// ~4s into continuous typing. So we forward every call to the SDK (it
/// dedups per-keystroke and resends every 3s while typing — no per-keystroke
/// network, no spam) and own only the 4s idle reset → `false`. The SDK also
/// dedups `false` (a no-op when not currently typing), so a stray reset is
/// harmless.
pub struct TypingManager {
    rooms: Arc<Mutex<BTreeMap<String, RoomTyping>>>,
}

struct RoomTyping {
    /// The in-flight 4s idle-reset timer; `None` when not typing.
    idle: Option<tokio::task::JoinHandle<()>>,
}

impl TypingManager {
    pub fn new() -> Self {
        Self {
            rooms: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// Set the typing state for `room_id`. On `true` send `typing_notice(true)`
    /// and arm a 4s idle reset that sends `false`. On `false` send
    /// `typing_notice(false)`, cancel the reset, and drop the entry. Repeated
    /// `true` re-arms the reset and lets the SDK resend on its 3s cadence.
    pub fn set(&self, client: &Client, room_id: &str, typing: bool) {
        let mut rooms = self.rooms.lock().unwrap_or_else(|e| e.into_inner());
        if !typing {
            // Stop typing: cancel any pending reset, send `false` (the SDK
            // no-ops it if we weren't typing), and drop the entry so it
            // doesn't accumulate over a long session.
            if let Some(entry) = rooms.remove(room_id) {
                if let Some(h) = entry.idle {
                    h.abort();
                }
            }
            drop(rooms);
            spawn_typing_send(client, room_id, false);
            return;
        }

        let entry = rooms
            .entry(room_id.to_string())
            .or_insert_with(|| RoomTyping { idle: None });
        // Cancel any in-flight idle reset; we re-arm below.
        if let Some(h) = entry.idle.take() {
            h.abort();
        }

        // Forward every `true` to the SDK — it dedups per-keystroke and
        // resends every 3s while typing (keeps the homeserver status alive
        // during long messages). See the type-level doc for why we don't
        // dedup here.
        drop(rooms);
        spawn_typing_send(client, room_id, true);

        // Arm the 4s idle reset: if the user stops typing, send `false`.
        let rooms = self.rooms.clone();
        let client = client.clone();
        let rid = room_id.to_string();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(TYPING_IDLE_RESET).await;
            // Reset: send `false` (the SDK no-ops it if a later `set(true)`
            // already re-armed and we lost the race, or if we genuinely
            // stopped) and clear the timer slot.
            let mut rooms = rooms.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = rooms.get_mut(&rid) {
                entry.idle.take();
            }
            drop(rooms);
            spawn_typing_send(&client, &rid, false);
        });
        self.rooms
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(room_id)
            .expect("just inserted")
            .idle = Some(handle);
    }

    /// Cancel all timers (logout): the homeserver learns we stopped when the
    /// session ends; we don't need to send `false` for each room.
    pub fn abort_all(&self) {
        let mut rooms = self.rooms.lock().unwrap_or_else(|e| e.into_inner());
        for entry in rooms.values_mut() {
            if let Some(h) = entry.idle.take() {
                h.abort();
            }
        }
        rooms.clear();
    }
}

impl Default for TypingManager {
    fn default() -> Self {
        Self::new()
    }
}

fn spawn_typing_send(client: &Client, room_id: &str, typing: bool) {
    let Some(rid) = parse_room_id(room_id) else {
        return;
    };
    let client = client.clone();
    tokio::spawn(async move {
        if let Some(room) = client.get_room(&rid) {
            if let Err(e) = room.typing_notice(typing).await {
                tracing::warn!(room_id = %rid, "typing_notice failed: {e}");
            }
        }
    });
}

fn parse_room_id(s: &str) -> Option<OwnedRoomId> {
    RoomId::parse(s).ok()
}

/// Aborts the wrapped task when dropped, so dropping [`LiveHandles`]
/// (logout / re-login) stops the presence poll — a bare `JoinHandle`
/// detaches instead.
struct AbortOnDrop(Option<tokio::task::AbortHandle>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

/// Handles for the live-state event handlers and the presence poll task,
/// so logout can unregister/stop them. Held only for their `Drop` side
/// effects; the fields are never read directly.
#[allow(dead_code)]
pub struct LiveHandles {
    presence: EventHandlerDropGuard,
    presence_poll: AbortOnDrop,
    #[cfg(not(target_arch = "wasm32"))]
    notify: EventHandlerDropGuard,
}

/// Register the presence + (native) notification handlers against `client`.
/// Dropping the returned [`LiveHandles`] unregisters them.
pub fn start_live(client: &Client, state: ClientState) -> LiveHandles {
    let presence = start_presence(client, state);
    let presence_poll = spawn_presence_poll(client, state);
    #[cfg(not(target_arch = "wasm32"))]
    let notify = start_notifications(client, state);
    LiveHandles {
        presence,
        presence_poll,
        #[cfg(not(target_arch = "wasm32"))]
        notify,
    }
}

/// Maintain `state.presence` from `m.presence` events.
///
/// NOTE: under the simplified sliding sync (`SyncService`, MSC4186) the
/// sync response carries no presence at all — the protocol has no presence
/// extension and `SyncResponse.presence` is synthesized empty — so this
/// handler stays dormant on matrix-sdk 0.18. [`poll_dm_presence`] is what
/// actually feeds the map; this handler remains registered (and de-dups
/// against the poll via the changed-value check) for the day the SDK/server
/// delivers presence over sync again.
fn start_presence(client: &Client, state: ClientState) -> EventHandlerDropGuard {
    let handle = client.add_event_handler(move |ev: PresenceEvent| async move {
        let mxid = ev.sender.to_string();
        let mapped = map_presence_state(&ev.content.presence);
        let mut map = state.presence.peek().clone();
        if map.get(&mxid).copied() != Some(mapped) {
            map.insert(mxid.clone(), mapped);
            let mut presence = state.presence;
            presence.set(map);
            tracing::debug!(%mxid, "presence updated");
        }
    });
    client.event_handler_drop_guard(handle)
}

fn map_presence_state(p: &PresenceState) -> Presence {
    match p {
        PresenceState::Online => Presence::Online,
        PresenceState::Unavailable => Presence::Away,
        PresenceState::Offline => Presence::Offline,
        // `PresenceState` is non-exhaustive; anything unknown reads as offline.
        _ => Presence::Offline,
    }
}

/// Spawn the presence poll loop ([`poll_dm_presence`] on a
/// [`PRESENCE_POLL_INTERVAL`] cadence, first pass immediately so dots light
/// up right after login). Until the room list lands there is nothing to
/// poll, so the loop retries fast instead of waiting out a full interval;
/// once a pass finds DM counterparts it settles to the regular cadence.
/// Aborted when the returned guard drops.
fn spawn_presence_poll(client: &Client, state: ClientState) -> AbortOnDrop {
    const FAST_RETRY: Duration = Duration::from_secs(2);
    let client = client.clone();
    let task = tokio::spawn(async move {
        let mut interval = FAST_RETRY;
        loop {
            if poll_dm_presence(&client, state).await {
                interval = PRESENCE_POLL_INTERVAL;
            }
            tokio::time::sleep(interval).await;
        }
    });
    AbortOnDrop(Some(task.abort_handle()))
}

/// One presence pass: fetch each DM counterpart's presence with
/// `GET /presence/{userId}/status` and upsert changed entries into
/// `state.presence`. Returns whether any counterpart was polled (i.e. the
/// room list had DM rows).
///
/// Why polling: MSC4186 simplified sliding sync has no presence extension,
/// so `m.presence` events never arrive over sync and the
/// [`start_presence`] handler never fires — without this, every DM status
/// dot renders permanently Offline. The REST endpoint works against any
/// homeserver, is not rate-limited per spec, and only serves users we share
/// a room with — exactly the DM counterparts polled here. Per-user failures
/// (e.g. server-side 403) log at `debug` and leave the last value standing.
async fn poll_dm_presence(client: &Client, state: ClientState) -> bool {
    let mxids: BTreeSet<String> = state
        .convos
        .peek()
        .iter()
        .filter(|c| c.kind == ConvoKind::Dm)
        .filter_map(|c| c.mxid.clone())
        .collect();
    if mxids.is_empty() {
        return false;
    }
    let mut changed = false;
    let mut map: BTreeMap<String, Presence> = state.presence.peek().clone();
    for mxid in &mxids {
        let Ok(user) = UserId::parse(mxid.as_str()) else {
            continue;
        };
        match client.send(get_presence::v3::Request::new(user)).await {
            Ok(resp) => {
                let mapped = map_presence_state(&resp.presence);
                if map.get(mxid).copied() != Some(mapped) {
                    map.insert(mxid.clone(), mapped);
                    changed = true;
                    tracing::debug!(%mxid, presence = ?mapped, "presence updated (poll)");
                }
            }
            Err(e) => tracing::debug!(%mxid, "presence fetch failed: {e}"),
        }
    }
    // One write per pass, and only if something actually changed, so the
    // drawer doesn't rerender on every poll tick.
    if changed {
        let mut presence = state.presence;
        presence.set(map);
    }
    true
}

/// Desktop notifications for incoming messages in background rooms
/// (native only: `notify-rust` has no web backend).
///
/// NOTE: this fires for `m.room.message` events including edits (`m.replace`),
/// so an edit in a background room notifies a second time. Filtering edits and
/// respecting `room.notification_mode` (muted rooms) is deferred to a later
/// checkpoint (docs/06 §Notifications); redactions are correctly excluded
/// (different event type).
#[cfg(not(target_arch = "wasm32"))]
fn start_notifications(client: &Client, state: ClientState) -> EventHandlerDropGuard {
    let own_id = client.user_id().map(|u| u.to_owned());
    let handle = client.add_event_handler(
        move |ev: SyncMessageLikeEvent<RoomMessageEventContent>, room: Room| async move {
            notify_if_background(&ev, &room, own_id.as_ref(), state).await;
        },
    );
    client.event_handler_drop_guard(handle)
}

#[cfg(not(target_arch = "wasm32"))]
async fn notify_if_background(
    ev: &SyncMessageLikeEvent<RoomMessageEventContent>,
    room: &Room,
    own_id: Option<&ruma::OwnedUserId>,
    state: ClientState,
) {
    let Some(orig) = ev.as_original() else {
        return;
    };
    let sender = orig.sender.clone();
    // Never notify on our own messages.
    if own_id.is_some_and(|o| o == &sender) {
        return;
    }
    // Suppress while the window is focused.
    if *state.focused.read() {
        return;
    }
    let room_id_str = room.room_id().to_string();
    // Suppress for the room the user is already looking at.
    if state.active_room.read().as_deref() == Some(room_id_str.as_str()) {
        return;
    }

    let body = preview_body(&orig.content);
    let preview = truncate(body, 80);
    let sender_name = room
        .get_member(&sender)
        .await
        .ok()
        .flatten()
        .and_then(|m| m.display_name().map(str::to_string))
        .unwrap_or_else(|| sender.as_str().to_string());

    // Never log the message body (docs/06).
    tracing::debug!(room = %room_id_str, "desktop notification fired");
    if let Err(e) = notify_rust::Notification::new()
        .summary(&sender_name)
        .body(&preview)
        .show()
    {
        tracing::warn!("desktop notification failed: {e}");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn preview_body(content: &RoomMessageEventContent) -> String {
    match &content.msgtype {
        MessageType::Text(t) => t.body.clone(),
        MessageType::Notice(n) => n.body.clone(),
        MessageType::Emote(e) => e.body.clone(),
        MessageType::Image(_) => "[image]".to_string(),
        MessageType::File(_) => "[file]".to_string(),
        MessageType::Audio(_) => "[audio]".to_string(),
        MessageType::Video(_) => "[video]".to_string(),
        _ => content.body().to_string(),
    }
}

/// Truncate to `max` chars (by char count) with an ellipsis.
fn truncate(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        return s;
    }
    let mut t: String = s.chars().take(max).collect();
    t.push('\u{2026}');
    t
}
