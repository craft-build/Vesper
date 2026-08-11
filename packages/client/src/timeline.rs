//! Live room timelines: one `matrix_sdk_ui::timeline::Timeline` per open room,
//! mapped to `Vec<Message>` and published into `ClientState::messages`
//! (docs/04).
//!
//! Mirrors the checkpoint-03 room-list pattern: the runtime thread owns the
//! SDK `Timeline` and its diff-subscription task, a plain `Vec` is kept
//! up-to-date with [`sync::apply_diffs`], and every batch re-publishes the
//! mapped messages into the App-scope `SyncStorage` signal. A small refcount
//! (one `Conversation` open at a time today, but cheap insurance) disposes
//! timelines for rooms nobody is looking at — see `clear` for logout.
//!
//! Encrypted-but-undecryptable events (`MsgLikeKind::UnableToDecrypt`) and
//! redactions map to `system` placeholder rows until checkpoints 08/05 flesh
//! them out; attachments are text placeholders until 07.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use dioxus_signals::{ReadableExt, WritableExt};
use futures::StreamExt;
use matrix_sdk::{Client, RoomState, ruma::RoomId};
use matrix_sdk_ui::timeline::{
    EventTimelineItem, MembershipChange, MsgLikeContent, MsgLikeKind, Timeline, TimelineBuilder,
    TimelineDetails, TimelineEventItemId, TimelineItem, TimelineItemContent, TimelineItemKind,
    VirtualTimelineItem,
};

use crate::{
    api::{ClientError, ClientState},
    model::{Message, Reaction},
    sync::apply_diffs,
};

/// Events requested per back-pagination page (docs/04 §design decisions).
pub const PAGE: u16 = 30;

/// Shared snapshot of one room's timeline: raw items plus the most recently
/// published mapped messages. The mapped length doubles as the cheap "did
/// pagination add anything" probe for [`TimelineRegistry::load_older`].
#[derive(Default)]
struct EntryState {
    items: Vec<Arc<TimelineItem>>,
    mapped_len: usize,
}

struct Entry {
    timeline: Timeline,
    inner: Arc<Mutex<EntryState>>,
    task: tokio::task::JoinHandle<()>,
    refcount: usize,
}

/// One live timeline per open room (docs/04 §design decisions). Owned by the
/// runtime command loop; nothing here is `Send`-verbose beyond the SDK types.
#[derive(Default)]
pub struct TimelineRegistry {
    entries: HashMap<String, Entry>,
}

impl TimelineRegistry {
    /// How many timelines are open right now (acceptance check: bounded by
    /// actually-open rooms).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Open `room_id` (incrementing its refcount). First open builds the SDK
    /// timeline — warm starts paint from the event cache subscribed by the
    /// room list — and spawns the diff-consumption task. Failures (room not
    /// joined, server without back-pagination support) are logged, not
    /// returned: opening a conversation must never fail the UI.
    pub async fn open(&mut self, client: &Client, room_id: &str, state: ClientState) {
        if let Some(entry) = self.entries.get_mut(room_id) {
            entry.refcount += 1;
            return;
        }

        let Ok(rid) = RoomId::parse(room_id) else {
            tracing::warn!(room_id, "ignoring open_timeline with bad room id");
            return;
        };
        let Some(room) = client.get_room(&rid) else {
            tracing::warn!(room_id, "open_timeline for unknown room");
            return;
        };
        if room.state() != RoomState::Joined {
            tracing::info!(room_id, "skipping timeline for non-joined room");
            return;
        }

        let timeline = match TimelineBuilder::new(&room).build().await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(room_id, "failed to build timeline: {e}");
                return;
            }
        };

        let own_id = client.user_id().map(|u| u.to_string()).unwrap_or_default();
        // Backfill BEFORE publishing: `subscribe` starts with whatever the
        // event cache already holds, which can be a single message. That
        // renders one short row with no scrollbar, and since back-pagination
        // is triggered by scrolling, a room like that could never load its
        // history. Paginate until there's at least one page of mapped
        // messages (enough to overflow the viewport → scrollable → user
        // pagination works) or the timeline start is reached. Batched here,
        // pre-publish, so the first paint is the filled history, not a flash
        // of an almost-empty room.
        let (initial, _stream) = timeline.subscribe().await;
        let mut items: Vec<Arc<TimelineItem>> = initial.iter().cloned().collect();
        for _ in 0..10 {
            let mapped = items
                .iter()
                .filter(|i| map_item(i, &own_id).is_some())
                .count();
            if mapped >= PAGE as usize {
                break;
            }
            match timeline.paginate_backwards(PAGE).await {
                Ok(true) => break, // reached the start of the timeline
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(room_id, "initial backfill failed: {e}");
                    break;
                }
            }
            // Refresh the snapshot after each page.
            let (fresh, _s) = timeline.subscribe().await;
            items = fresh.iter().cloned().collect();
        }
        // Final subscription: its stream starts cleanly at the backfilled
        // state (the first stream was never polled and can lag).
        let (final_items, stream) = timeline.subscribe().await;
        let inner: Arc<Mutex<EntryState>> = Arc::new(Mutex::new(EntryState {
            items: final_items.iter().cloned().collect(),
            mapped_len: 0,
        }));
        publish(&inner, &state, room_id, &own_id);

        let task = {
            let inner = inner.clone();
            let room_id = room_id.to_string();
            let own_id = own_id.clone();
            tokio::spawn(async move {
                let mut stream = Box::pin(stream);
                while let Some(diffs) = stream.next().await {
                    {
                        let mut state_ref = lock(&inner);
                        apply_diffs(&mut state_ref.items, diffs);
                    }
                    publish(&inner, &state, &room_id, &own_id);
                }
                tracing::debug!(room_id, "timeline diff stream ended");
            })
        };

        self.entries.insert(
            room_id.to_string(),
            Entry {
                timeline,
                inner,
                task,
                refcount: 1,
            },
        );
        tracing::info!(room_id, "timeline opened");
    }

    /// Release one reference to `room_id`; disposes task + timeline at zero.
    pub fn close(&mut self, room_id: &str) {
        let Some(entry) = self.entries.get_mut(room_id) else {
            return;
        };
        entry.refcount = entry.refcount.saturating_sub(1);
        if entry.refcount == 0 {
            if let Some(entry) = self.entries.remove(room_id) {
                entry.task.abort();
                tracing::info!(room_id, "timeline closed");
            }
        }
    }

    /// Back-paginate one page; returns how many mapped messages were added
    /// (0 on timeline-start or when pagination is unsupported).
    pub async fn load_older(&self, room_id: &str) -> Result<usize, ClientError> {
        let Some(entry) = self.entries.get(room_id) else {
            return Ok(0);
        };
        let before = lock(&entry.inner).mapped_len;
        entry
            .timeline
            .paginate_backwards(PAGE)
            .await
            .map_err(|e| ClientError(format!("Back-pagination failed: {e}")))?;
        // The pagination request is done, but subscriber delivery is async;
        // wait briefly for the diff to land so the returned count is honest
        // and the UI spinner can stop at the right time.
        for _ in 0..40 {
            let now = lock(&entry.inner).mapped_len;
            if now != before {
                return Ok(now.saturating_sub(before));
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        Ok(0)
    }

    /// Abort every timeline task (logout / session teardown).
    pub fn abort_all(&mut self) {
        for (_, entry) in self.entries.drain() {
            entry.task.abort();
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Re-map the whole item list and publish it into `ClientState::messages`.
/// Remapping per batch is O(n) in the visible window but side-effect free; a
/// diff-preserving mapper is premature for chat-sized pages.
fn publish(inner: &Mutex<EntryState>, state: &ClientState, room_id: &str, own_id: &str) {
    let mapped: Vec<Message> = {
        let guard = lock(inner);
        guard
            .items
            .iter()
            .filter_map(|item| map_item(item, own_id))
            .collect()
    };
    lock(inner).mapped_len = mapped.len();
    let mut map = state.messages.peek().clone();
    map.insert(room_id.to_string(), mapped);
    let mut messages = state.messages;
    messages.set(map);
}

/// Map one timeline item to a UI message. `None` = nothing to render
/// (read markers, timeline-start sentinel, typing, unknown state events).
fn map_item(item: &TimelineItem, own_id: &str) -> Option<Message> {
    match item.kind() {
        TimelineItemKind::Event(event) => map_event(event, own_id),
        TimelineItemKind::Virtual(virt) => map_virtual(virt),
    }
}

fn map_event(event: &EventTimelineItem, own_id: &str) -> Option<Message> {
    // Remote events key by event id; local echoes only carry a transaction
    // id (pending styling is checkpoint 05) but still need a stable row key.
    let id = match event.identifier() {
        TimelineEventItemId::EventId(id) => id.to_string(),
        TimelineEventItemId::TransactionId(id) => format!("txn-{id}"),
    };
    let from = match event.sender_profile() {
        TimelineDetails::Ready(profile) => profile
            .display_name
            .clone()
            .unwrap_or_else(|| event.sender().to_string()),
        _ => event.sender().to_string(),
    };
    let time = format_hhmm(event.timestamp().get().into());

    let mut msg = |text: String, system: bool| {
        let mut m = Message::new(id.clone(), from.clone(), time.clone(), text);
        m.mine = event.is_own();
        m.system = system;
        m
    };

    match event.content() {
        TimelineItemContent::MsgLike(msglike) => Some(map_msglike(&mut msg, msglike, own_id)),
        TimelineItemContent::MembershipChange(change) => Some(msg(membership_text(change), true)),
        TimelineItemContent::ProfileChange(profile) => {
            let text = match profile.displayname_change() {
                Some(change) => {
                    if let Some(new) = &change.new {
                        format!("{from} is now known as {new}")
                    } else if let Some(old) = &change.old {
                        format!("{old} removed their display name")
                    } else {
                        format!("{from} changed their profile")
                    }
                }
                None => format!("{from} changed their profile"),
            };
            Some(msg(text, true))
        }
        TimelineItemContent::FailedToParseMessageLike { .. }
        | TimelineItemContent::FailedToParseState { .. } => {
            Some(msg("unable to display this event".into(), true))
        }
        TimelineItemContent::CallInvite => Some(msg("started a call".into(), true)),
        // Topic/room state changes, call notifications & friends: no UI row
        // this checkpoint.
        TimelineItemContent::OtherState(_) | TimelineItemContent::RtcNotification { .. } => None,
    }
}

fn map_msglike(
    msg: &mut impl FnMut(String, bool) -> Message,
    msglike: &MsgLikeContent,
    own_id: &str,
) -> Message {
    let reactions = aggregate_reactions(msglike, own_id);
    let reply_to = msglike
        .in_reply_to
        .as_ref()
        .map(|details| details.event_id.to_string());

    let (text, system) = match &msglike.kind {
        MsgLikeKind::Message(message) => {
            use matrix_sdk::ruma::events::room::message::MessageType as T;
            let body = match message.msgtype() {
                T::Text(t) => t.body.clone(),
                T::Notice(n) => n.body.clone(),
                T::Emote(e) => return emote_row(msg, &e.body, own_id, msglike),
                // Real attachment rendering is checkpoint 07; the body here is
                // the filename/caption.
                T::Image(_) | T::File(_) | T::Audio(_) | T::Video(_) => {
                    format!("[attachment: {}]", message.body())
                }
                T::Location(l) => format!("[location: {}]", l.geo_uri),
                T::ServerNotice(n) => n.body.clone(),
                _ => message.body().to_string(),
            };
            (
                if message.is_edited() {
                    format!("{body} (edited)")
                } else {
                    body
                },
                false,
            )
        }
        MsgLikeKind::Redacted => ("message removed".into(), true),
        // Encryption failure details (session id etc.) are checkpoint 08
        // debugging material, not placeholder-row text.
        MsgLikeKind::UnableToDecrypt(_) => ("🔒 unable to decrypt".into(), true),
        MsgLikeKind::Sticker(s) => (format!("sticker: {}", s.content().body), false),
        MsgLikeKind::Poll(_) => ("[poll — open in another client]".into(), true),
        _ => ("[unsupported message type]".into(), true),
    };

    let mut m = msg(text, system);
    m.reactions = reactions;
    m.reply_to = reply_to;
    m
}

/// Emotes render as a system-style row: `{sender} does the thing`.
fn emote_row(
    msg: &mut impl FnMut(String, bool) -> Message,
    body: &str,
    own_id: &str,
    msglike: &MsgLikeContent,
) -> Message {
    let mut m = msg(String::new(), true);
    m.text = format!("{} {}", m.from, body);
    m.reactions = aggregate_reactions(msglike, own_id);
    m.reply_to = msglike
        .in_reply_to
        .as_ref()
        .map(|details| details.event_id.to_string());
    m
}

fn aggregate_reactions(msglike: &MsgLikeContent, own_id: &str) -> Vec<Reaction> {
    msglike
        .reactions
        .iter()
        .map(|(emoji, senders)| Reaction {
            emoji: emoji.clone(),
            count: senders.len().min(u32::MAX as usize) as u32,
            me: senders.keys().any(|s| s.as_str() == own_id),
        })
        .collect()
}

fn membership_text(change: &matrix_sdk_ui::timeline::RoomMembershipChange) -> String {
    let who = change
        .display_name()
        .map(|n| n.to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| change.user_id().to_string());
    let verb = match change.change() {
        Some(MembershipChange::Joined) => "joined the room",
        Some(MembershipChange::Left) => "left the room",
        Some(MembershipChange::Invited) => "was invited",
        Some(MembershipChange::InvitationAccepted) => "accepted the invite",
        Some(MembershipChange::Kicked) => "was kicked",
        Some(MembershipChange::Banned) => "was banned",
        Some(MembershipChange::Unbanned) => "was unbanned",
        Some(MembershipChange::Knocked) => "knocked",
        Some(MembershipChange::KnockAccepted) => "knock accepted",
        Some(MembershipChange::KnockDenied) => "knock denied",
        Some(MembershipChange::KnockRetracted) => "knock retracted",
        Some(MembershipChange::InvitationRejected) => "declined the invite",
        Some(MembershipChange::InvitationRevoked) => "invite revoked",
        Some(MembershipChange::KickedAndBanned) => "was kicked and banned",
        _ => "changed membership",
    };
    format!("{who} {verb}")
}

fn map_virtual(virt: &VirtualTimelineItem) -> Option<Message> {
    match virt {
        VirtualTimelineItem::DateDivider(ts) => {
            let ms: u64 = ts.get().into();
            let mut m = Message::new(
                format!("divider-{ms}"),
                "",
                "",
                format!("— {} —", format_ymd(ms)),
            );
            m.system = true;
            Some(m)
        }
        // Read/typing markers are checkpoint 06; the timeline-start sentinel
        // is invisible.
        VirtualTimelineItem::ReadMarker | VirtualTimelineItem::TimelineStart => None,
    }
}

/// `HH:mm` from unix milliseconds (UTC). No chrono dependency.
pub(crate) fn format_hhmm(millis: u64) -> String {
    let secs = millis / 1000;
    let sod = secs % 86_400;
    format!("{:02}:{:02}", sod / 3600, (sod % 3600) / 60)
}

/// `YYYY-MM-DD` from unix milliseconds (UTC), civil-from-days conversion.
pub(crate) fn format_ymd(millis: u64) -> String {
    let days = (millis / 1000 / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    // Howard Hinnant's algorithm, https://howardhinnant.github.io/date_algorithms.html
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_hhmm_wraps_within_day() {
        assert_eq!(format_hhmm(0), "00:00");
        assert_eq!(format_hhmm(59_000), "00:00");
        assert_eq!(format_hhmm(60_000), "00:01");
        assert_eq!(format_hhmm(3_599_000), "00:59");
        assert_eq!(format_hhmm(3_600_000), "01:00");
        // 2026-08-10 12:34:56 UTC
        assert_eq!(format_hhmm(1_786_365_296_000), "12:34");
    }

    #[test]
    fn format_ymd_known_dates() {
        assert_eq!(format_ymd(0), "1970-01-01");
        // 2026-08-10
        assert_eq!(format_ymd(1_786_365_296_000), "2026-08-10");
        // 2000-02-29 (leap year, leap day)
        assert_eq!(format_ymd(951_782_400_000), "2000-02-29");
        // 1999-12-31
        assert_eq!(format_ymd(946_598_400_000), "1999-12-31");
    }

    #[test]
    fn civil_boundaries() {
        assert_eq!(super::civil_from_days(0), (1970, 1, 1));
        assert_eq!(super::civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn registry_refcount_lifecycle() {
        // No client available in unit tests; drive the refcount path by
        // closing rooms that were never opened (must be a no-op, not panic).
        let mut registry = TimelineRegistry::default();
        registry.close("!never:opened");
        assert_eq!(registry.len(), 0);
        registry.abort_all();
        assert_eq!(registry.len(), 0);
    }
}
