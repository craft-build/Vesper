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
    time::Instant,
};

use dioxus_signals::{ReadableExt, WritableExt};
use futures::StreamExt;
use matrix_sdk::{
    ruma::{
        events::{
            relation::Thread,
            room::message::{
                Relation, RoomMessageEventContent, RoomMessageEventContentWithoutRelation,
            },
        },
        EventId, OwnedEventId, OwnedTransactionId, RoomId,
    },
    send_queue::SendHandle,
    Client, RoomState,
};
use matrix_sdk_ui::timeline::{
    EncryptedMessage, EventSendState, EventTimelineItem, MembershipChange, MsgLikeContent,
    MsgLikeKind, Timeline, TimelineBuilder, TimelineDetails, TimelineEventItemId, TimelineFocus,
    TimelineItem, TimelineItemContent, TimelineItemKind, VirtualTimelineItem,
};

use crate::{
    api::{ClientError, ClientState},
    model::{Message, Reaction, SendState, ThreadReply},
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
    /// Room handle for operations that don't go through the timeline (e.g.
    /// attachment uploads, checkpoint 07 — `Timeline` is not `Clone`, so a
    /// spawned upload task needs its own `Room` clone).
    room: matrix_sdk::Room,
    inner: Arc<Mutex<EntryState>>,
    task: tokio::task::JoinHandle<()>,
    refcount: usize,
    /// Our own MXID, captured at open — needed to map reaction aggregates.
    own_id: String,
    /// Send handles for local echoes we originated, correlated to their
    /// timeline echo via the shared `created_at` timestamp (`SendHandle`
    /// has no transaction-id accessor in 0.18). Retried (`unwedge`) from
    /// the UI; discard goes through `Timeline::redact` on the echo.
    /// Pruned in [`publish`] once an echo resolves to Sent.
    send_handles: Arc<Mutex<Vec<SendHandle>>>,
    /// Background task driving the incoming-typing signal for this room
    /// (checkpoint 06); aborted on close so the handler unregisters with it.
    typing_task: Option<tokio::task::JoinHandle<()>>,
    /// Throttled read-receipt state (checkpoint 06): the last event id we
    /// sent a receipt for, and the timestamp of that send (>=1s throttle).
    receipt: Arc<Mutex<Option<ReceiptState>>>,
}

/// Per-room read-receipt throttle state. Held under a mutex so the
/// `mark_read` command (sync) and any deferred send task never race.
struct ReceiptState {
    /// The last event id a `m.read` receipt was sent for.
    last_event: String,
    /// When that send happened, for the >=1s throttle.
    last_sent: Instant,
}

struct ThreadEntry {
    task: tokio::task::JoinHandle<()>,
    refcount: usize,
    /// State for publishing + clearing the threads-map entry on close.
    state: ClientState,
}

/// One live timeline per open room + one thread-focused timeline per open
/// thread panel (docs/04, docs/05). Owned by the runtime command loop.
#[derive(Default)]
pub struct TimelineRegistry {
    entries: HashMap<String, Entry>,
    /// Open threads, keyed by the thread's root event id — globally unique,
    /// so no room id needed.
    threads: HashMap<String, ThreadEntry>,
    /// Last state seen via `open`/`open_thread`, used by one-shot reads
    /// that want to republish (and by close/clear to prune the threads map).
    state: Option<ClientState>,
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
        self.state = Some(state);
        if room.state() != RoomState::Joined {
            tracing::info!(room_id, "skipping timeline for non-joined room");
            return;
        }

        // `hide_threaded_events`: thread replies belong in the thread panel
        // (thread-focused timelines below), not as main-timeline rows — the
        // root's "N replies" badge carries them.
        let timeline = match TimelineBuilder::new(&room)
            .with_focus(TimelineFocus::Live {
                hide_threaded_events: true,
            })
            .build()
            .await
        {
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
        let send_handles: Arc<Mutex<Vec<SendHandle>>> = Default::default();
        publish(&inner, &send_handles, &state, room_id, &own_id);

        let task = {
            let inner = inner.clone();
            let send_handles = send_handles.clone();
            let room_id = room_id.to_string();
            let own_id = own_id.clone();
            tokio::spawn(async move {
                let mut stream = Box::pin(stream);
                while let Some(diffs) = stream.next().await {
                    {
                        let mut state_ref = lock(&inner);
                        apply_diffs(&mut state_ref.items, diffs);
                    }
                    publish(&inner, &send_handles, &state, &room_id, &own_id);
                }
                tracing::debug!(room_id, "timeline diff stream ended");
            })
        };

        // Incoming typing (checkpoint 06): the SDK delivers `m.typing`
        // ephemeral events as a broadcast of typing user ids (own user
        // already filtered). Resolve each to a display name and publish
        // into `state.typing[room_id]`; a 10s safety prune clears a stale
        // entry if a "stopped typing" update is missed. The guard lives for
        // the task's duration: dropping the task (on close) drops the guard,
        // unregistering the ephemeral handler.
        let typing_task = spawn_typing_task(room.clone(), room_id.to_string(), state);

        self.entries.insert(
            room_id.to_string(),
            Entry {
                timeline,
                room,
                inner,
                task,
                refcount: 1,
                own_id,
                send_handles,
                typing_task: Some(typing_task),
                receipt: Arc::new(Mutex::new(None)),
            },
        );
        tracing::info!(room_id, "timeline opened");
    }

    /// Open (refcounted) a live, thread-focused timeline for the thread
    /// rooted at `root_id` in `room_id` (docs/05: live thread panel).
    /// Requires the room's timeline to be open (the panel is only reachable
    /// from an open conversation). Failures log, never fail the UI.
    pub async fn open_thread(&mut self, room_id: &str, root_id: &str, state: ClientState) {
        if let Some(entry) = self.threads.get_mut(root_id) {
            entry.refcount += 1;
            return;
        }
        let Ok(root) = EventId::parse(root_id).map(|e| e.to_owned()) else {
            tracing::warn!(root_id, "ignoring open_thread with bad event id");
            return;
        };
        let Some(room) = self
            .entries
            .get(room_id)
            .map(|entry| entry.timeline.room().clone())
        else {
            tracing::warn!(room_id, "open_thread without an open room timeline");
            return;
        };
        let timeline = match TimelineBuilder::new(&room)
            .with_focus(TimelineFocus::Thread {
                root_event_id: root,
            })
            .build()
            .await
        {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(room_id, "thread timeline build failed: {e}");
                return;
            }
        };
        self.state = Some(state);

        // Backfill, first publish, and diff consumption all run off the
        // command loop. The runtime processes commands sequentially, so
        // doing the `/relations` pagination inline would stall sends, reacts,
        // and pagination in *every* room until the thread finishes loading.
        // `TimelineBuilder::build` is local (it subscribes to an in-memory
        // thread cache; the network fetch is `paginate_backwards`), so it can
        // stay inline. The first publish still happens after the seed pages,
        // as before — so the UI's snapshot fallback covers the gap; only who
        // does the waiting changes.
        let root_id = root_id.to_string();
        let task = {
            let root_id = root_id.clone();
            tokio::spawn(async move {
                // Seed a couple of pages before the first publish so reopening
                // a thread isn't a flash of emptiness.
                for _ in 0..2 {
                    match timeline.paginate_backwards(PAGE).await {
                        Ok(true) => break, // beginning of the thread
                        Ok(false) => {}
                        Err(e) => {
                            tracing::warn!(root_id, "thread backfill failed: {e}");
                            break;
                        }
                    }
                }
                let (initial, stream) = timeline.subscribe().await;
                let inner: Arc<Mutex<Vec<Arc<TimelineItem>>>> =
                    Arc::new(Mutex::new(initial.iter().cloned().collect()));
                publish_thread(&state, &root_id, &inner);

                let mut stream = Box::pin(stream);
                while let Some(diffs) = stream.next().await {
                    {
                        let mut items = lock(&inner);
                        apply_diffs(&mut items, diffs);
                    }
                    publish_thread(&state, &root_id, &inner);
                }
                tracing::debug!(root_id, "thread diff stream ended");
            })
        };
        self.threads.insert(
            root_id.clone(),
            ThreadEntry {
                task,
                refcount: 1,
                state,
            },
        );
        tracing::info!(root_id, "thread opened");
    }

    /// Release one reference to `root_id`'s thread; disposes at zero and
    /// drops the published replies so a later open starts clean.
    pub fn close_thread(&mut self, root_id: &str) {
        let Some(entry) = self.threads.get_mut(root_id) else {
            return;
        };
        entry.refcount = entry.refcount.saturating_sub(1);
        if entry.refcount > 0 {
            return;
        }
        if let Some(entry) = self.threads.remove(root_id) {
            entry.task.abort();
            let mut map = entry.state.threads.peek().clone();
            map.remove(root_id);
            let mut threads = entry.state.threads;
            threads.set(map);
            tracing::info!(root_id, "thread closed");
        }
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
                if let Some(t) = entry.typing_task {
                    t.abort();
                }
                // Clear the published typing row so a reopened room doesn't
                // show a stale "typing…" from the previous session.
                if let Some(state) = &self.state {
                    let mut map = state.typing.peek().clone();
                    if map.remove(room_id).is_some() {
                        let mut typing = state.typing;
                        typing.set(map);
                    }
                }
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
            .map_err(|e| ClientError::server(format!("Back-pagination failed: {e}")))?;
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
            if let Some(t) = entry.typing_task {
                t.abort();
            }
        }
        for (_, entry) in self.threads.drain() {
            entry.task.abort();
        }
    }

    /// Mark `room_id` as read up to its latest *remote* event (checkpoint 06):
    /// sends a throttled `m.read` receipt. "Throttled" = >=1s between sends
    /// and only when the latest event advances past the last-sent one; the
    /// SDK additionally dedups identical event ids. Runs the actual send on a
    /// spawned task so the command loop never blocks on the network call.
    pub fn mark_read(&self, client: &Client, room_id: &str, _state: ClientState) {
        let Some(entry) = self.entries.get(room_id) else {
            return;
        };
        let latest = latest_remote_event_id(&entry.inner);
        let Some(event_id) = latest else {
            // No remote event to receipt yet (only local echoes): nothing to do.
            return;
        };
        let receipt = entry.receipt.clone();
        let now = Instant::now();
        {
            let mut guard = lock(&receipt);
            match &*guard {
                Some(r) if r.last_event == event_id => return, // no advance
                Some(r) if now.duration_since(r.last_sent) < std::time::Duration::from_secs(1) => {
                    // Too soon: schedule a deferred send for the latest event,
                    // replacing any pending one. The deferred task re-reads the
                    // *then-current* latest event from the shared entry state, so
                    // rapid advances coalesce into one receipt.
                    let inner = entry.inner.clone();
                    let room = entry.timeline.room().clone();
                    let receipt = receipt.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        send_read_receipt_deferred(&room, &inner, &receipt).await;
                    });
                    return;
                }
                _ => {}
            }
            *guard = Some(ReceiptState {
                last_event: event_id.clone(),
                last_sent: now,
            });
        }
        let room = entry.timeline.room().clone();
        let event_id = match EventId::parse(&event_id) {
            Ok(e) => e.to_owned(),
            Err(_) => return,
        };
        let _ = client; // room is already in hand; client not needed for the send
        tokio::spawn(async move {
            send_read_receipt(&room, event_id).await;
        });
    }
}

/// Strip the `txn-` prefix the mapped row id gives local echoes, returning
/// the raw transaction id, or `None` when the row is a remote/sent event.
fn txn_of(mapped_id: &str) -> Option<String> {
    mapped_id.strip_prefix("txn-").map(str::to_owned)
}

impl TimelineRegistry {
    /// A cloneable room handle for an open room, used to spawn attachment
    /// uploads off the sequential command loop (checkpoint 07).
    pub fn room_for_send(&self, room_id: &str) -> Option<matrix_sdk::Room> {
        self.entries.get(room_id).map(|e| e.room.clone())
    }

    /// Send a markdown text message into `room_id`'s open timeline, as a
    /// reply when `reply_to` (a mapped row id) is given (checkpoint 05).
    /// The local echo paints through the diff stream; failures land as
    /// `EventSendState::SendingFailed` on that echo.
    pub async fn send_message(
        &self,
        room_id: &str,
        text: String,
        reply_to: Option<String>,
    ) -> Result<(), ClientError> {
        let Some(entry) = self.entries.get(room_id) else {
            return Err(ClientError::invalid("That conversation is not open."));
        };
        let content = RoomMessageEventContentWithoutRelation::text_markdown(text);
        if let Some(reply_to) = reply_to {
            let event_id = EventId::parse(&reply_to)
                .map(|e| e.to_owned())
                .map_err(|_| ClientError::network("Could not send the reply."))?;
            // `send_reply` builds the spec-shaped fallback body and mentions
            // itself, and does not return the send handle — retries of a
            // failed reply fall back to "message already sent or unknown".
            entry
                .timeline
                .send_reply(content, event_id)
                .await
                .map_err(|e| {
                    tracing::warn!(room_id, "reply send failed: {e}");
                    ClientError::network("Could not send the reply.")
                })?;
            Ok(())
        } else {
            let handle = entry
                .timeline
                .send(content.with_relation(None).into())
                .await
                .map_err(|e| {
                    tracing::warn!(room_id, "send failed: {e}");
                    ClientError::network("Could not send the message.")
                })?;
            lock(&entry.send_handles).push(handle);
            Ok(())
        }
    }

    /// Send a markdown text message as a `m.thread` reply rooted at
    /// `root_id` (checkpoint 05). The reply-to fallback points at the root:
    /// we don't track thread tails, and a root fallback is spec-legal.
    pub async fn send_thread_reply(
        &self,
        room_id: &str,
        root_id: &str,
        text: String,
    ) -> Result<(), ClientError> {
        let Some(entry) = self.entries.get(room_id) else {
            return Err(ClientError::invalid("That conversation is not open."));
        };
        let root = EventId::parse(root_id)
            .map(|e| e.to_owned())
            .map_err(|_| ClientError::network("Could not send the thread reply."))?;
        let mut content = RoomMessageEventContent::text_markdown(text);
        content.relates_to = Some(Relation::Thread(Thread::plain(root.clone(), root)));
        let handle = entry.timeline.send(content.into()).await.map_err(|e| {
            tracing::warn!(room_id, "thread reply send failed: {e}");
            ClientError::network("Could not send the thread reply.")
        })?;
        lock(&entry.send_handles).push(handle);
        Ok(())
    }

    /// Toggle `emoji` on `event_id` (a mapped row id of a remote/sent event):
    /// sends an `m.annotation` or redacts the user's matching reaction.
    /// NOTE: the returned aggregate can still be pre-toggle — the SDK defers
    /// local reflection to the echo; repainting comes from the diff stream
    /// and callers so far ignore the return (mock parity only).
    pub async fn toggle_reaction(
        &self,
        room_id: &str,
        event_id: &str,
        emoji: &str,
    ) -> Result<Vec<Reaction>, ClientError> {
        let Some(entry) = self.entries.get(room_id) else {
            return Err(ClientError::invalid("That conversation is not open."));
        };
        let Ok(cid) = EventId::parse(event_id).map(|e| e.to_owned()) else {
            // Not a real event id (e.g. a `txn-` local echo) — nothing the
            // timeline can toggle a reaction on.
            return Err(ClientError::network("Could not react to that message."));
        };
        entry
            .timeline
            .toggle_reaction(&TimelineEventItemId::EventId(cid.clone()), emoji)
            .await
            .map_err(|e| {
                tracing::warn!(room_id, "reaction toggle failed: {e}");
                ClientError::network("Could not update the reaction.")
            })?;
        let guard = lock(&entry.inner);
        for item in &guard.items {
            let TimelineItemKind::Event(event) = item.kind() else {
                continue;
            };
            if event.event_id() != Some(cid.as_ref()) {
                continue;
            }
            if let TimelineItemContent::MsgLike(msglike) = event.content() {
                return Ok(aggregate_reactions(msglike, &entry.own_id));
            }
        }
        Ok(Vec::new())
    }

    /// One-shot read of the thread rooted at `root_id` (checkpoint 05,
    /// thread panel): builds a thread-focused timeline against the same
    /// room — cached thread events plus `/relations` back-pagination —
    /// maps message rows to `ThreadReply`s, then drops it. Does not touch
    /// the room's live timeline entry.
    pub async fn thread_replies(
        &self,
        room_id: &str,
        root_id: &str,
    ) -> Result<Vec<ThreadReply>, ClientError> {
        let Some(entry) = self.entries.get(room_id) else {
            return Err(ClientError::invalid("That conversation is not open."));
        };
        let root = EventId::parse(root_id)
            .map(|e| e.to_owned())
            .map_err(|_| ClientError::network("Could not open that thread."))?;
        let timeline = TimelineBuilder::new(entry.timeline.room())
            .with_focus(TimelineFocus::Thread {
                root_event_id: root,
            })
            .build()
            .await
            .map_err(|e| {
                tracing::warn!(room_id, "thread timeline build failed: {e}");
                ClientError::network("Could not open that thread.")
            })?;
        // Backfill up to two pages of thread replies from the server so
        // pre-existing threads are actually populated, not just live ones.
        for _ in 0..2 {
            match timeline.paginate_backwards(PAGE).await {
                Ok(true) => break, // beginning of the thread
                Ok(false) => continue,
                Err(e) => {
                    tracing::warn!(room_id, "thread pagination failed: {e}");
                    break;
                }
            }
        }
        let (items, _stream) = timeline.subscribe().await;
        Ok(items
            .iter()
            .filter_map(|item| thread_reply_row(item))
            .collect())
    }

    /// Retry a wedged local echo. Correlates the row (mapped `txn-` id) to
    /// its `SendHandle` via the echo's `local_created_at` timestamp, then
    /// `unwedge`s it out of the send queue's stuck state.
    pub async fn retry_send(&self, room_id: &str, mapped_id: &str) -> Result<(), ClientError> {
        let Some(entry) = self.entries.get(room_id) else {
            return Err(ClientError::invalid("That conversation is not open."));
        };
        let Some(txn) = txn_of(mapped_id) else {
            return Err(ClientError::invalid("Only pending messages can be retried."));
        };
        let created_at = {
            let guard = lock(&entry.inner);
            guard.items.iter().find_map(|item| match item.kind() {
                TimelineItemKind::Event(event) => {
                    let matches_txn = matches!(
                        event.identifier(),
                        TimelineEventItemId::TransactionId(id) if id.as_str() == txn
                    );
                    matches_txn.then(|| event.local_created_at()).flatten()
                }
                TimelineItemKind::Virtual(_) => None,
            })
        };
        let Some(created_at) = created_at else {
            return Err(ClientError::invalid("Message already sent or unknown."));
        };
        let handle = {
            let handles = lock(&entry.send_handles);
            let mut matches = handles.iter().filter(|h| h.created_at == created_at);
            match (matches.next(), matches.next()) {
                // Two sends in the same millisecond share `created_at`;
                // rather than retrying the wrong message, report unknown.
                (Some(h), None) => Some(h.clone()),
                _ => None,
            }
        };
        let Some(handle) = handle else {
            return Err(ClientError::invalid("Message already sent or unknown."));
        };
        handle.unwedge().await.map_err(|e| {
            tracing::warn!(room_id, "retry failed: {e}");
            ClientError::network("Could not retry the message.")
        })
    }

    /// Discard a pending local echo: `Timeline::redact` on a local echo
    /// aborts it in the send queue (verified against matrix-sdk-ui 0.18:
    /// the `TimelineItemHandle::Local` branch calls `SendHandle::abort`).
    pub async fn discard_send(&self, room_id: &str, mapped_id: &str) -> Result<(), ClientError> {
        let Some(entry) = self.entries.get(room_id) else {
            return Err(ClientError::invalid("That conversation is not open."));
        };
        let Some(txn) = txn_of(mapped_id) else {
            return Err(ClientError::invalid(
                "Only pending messages can be discarded.",
            ));
        };
        entry
            .timeline
            .redact(
                &TimelineEventItemId::TransactionId(OwnedTransactionId::from(txn)),
                None,
            )
            .await
            .map_err(|e| {
                tracing::warn!(room_id, "discard failed: {e}");
                ClientError::unknown("Could not discard the message.")
            })
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Spawn the incoming-typing task for one room (checkpoint 06). Owns the
/// `subscribe_to_typing_notifications` drop guard: when this task is aborted
/// (on close/logout) the guard drops and the ephemeral handler unregisters.
fn spawn_typing_task(
    room: matrix_sdk::Room,
    room_id: String,
    state: ClientState,
) -> tokio::task::JoinHandle<()> {
    let (_guard, mut rx) = room.subscribe_to_typing_notifications();
    tokio::spawn(async move {
        use tokio::time::{timeout, Duration};
        // Keep the guard alive for the task's lifetime; dropping it
        // unregisters the typing handler.
        let _guard = _guard;
        let prune = Duration::from_secs(10);
        loop {
            // Either a fresh typing list arrives, or 10s pass with nothing —
            // a safety prune in case a "stopped typing" update was missed.
            match timeout(prune, rx.recv()).await {
                Ok(Ok(user_ids)) => publish_typing(&room, &room_id, user_ids, &state).await,
                Ok(Err(_)) => break, // channel closed (client dropped)
                Err(_) => {
                    // Prune timeout: clear the row if anyone was shown typing.
                    let mut map = state.typing.peek().clone();
                    if map.remove(&room_id).is_some() {
                        let mut typing = state.typing;
                        typing.set(map);
                    }
                }
            }
        }
    })
}

/// Resolve `user_ids` to display names and publish `state.typing[room_id]`.
async fn publish_typing(
    room: &matrix_sdk::Room,
    room_id: &str,
    user_ids: Vec<matrix_sdk::ruma::OwnedUserId>,
    state: &ClientState,
) {
    let mut names: Vec<String> = Vec::with_capacity(user_ids.len());
    for id in user_ids {
        let name = room
            .get_member(&id)
            .await
            .ok()
            .flatten()
            .and_then(|m| m.display_name().map(str::to_string))
            .unwrap_or_else(|| id.as_str().to_string());
        names.push(name);
    }
    let mut map = state.typing.peek().clone();
    let changed = map.get(room_id).cloned() != Some(names.clone());
    if changed {
        if names.is_empty() {
            map.remove(room_id);
        } else {
            map.insert(room_id.to_string(), names);
        }
        let mut typing = state.typing;
        typing.set(map);
    }
}

/// The latest *remote* (sent) event id in a room's timeline, or `None` when
/// only local echoes exist. Read receipts must target a real event id.
fn latest_remote_event_id(inner: &Mutex<EntryState>) -> Option<String> {
    let guard = lock(inner);
    guard.items.iter().rev().find_map(|item| match item.kind() {
        TimelineItemKind::Event(event) => event.event_id().map(ToString::to_string),
        TimelineItemKind::Virtual(_) => None,
    })
}

/// Send an `m.read` receipt for `event_id` (unthreaded) and update throttle
/// state on success. Failures are logged, never propagated — a missed
/// receipt is corrected on the next `mark_read`.
async fn send_read_receipt(room: &matrix_sdk::Room, event_id: OwnedEventId) {
    use matrix_sdk::ruma::api::client::receipt::create_receipt::v3::ReceiptType;
    use matrix_sdk::ruma::events::receipt::ReceiptThread;
    if let Err(e) = room
        .send_single_receipt(ReceiptType::Read, ReceiptThread::Unthreaded, event_id)
        .await
    {
        tracing::warn!("read receipt failed: {e}");
    }
}

/// Deferred read-receipt send used by the throttle: re-reads the current
/// latest event from the shared entry state (which may have advanced past the
/// one that tripped the throttle) and sends for it, updating throttle state.
async fn send_read_receipt_deferred(
    room: &matrix_sdk::Room,
    inner: &Arc<Mutex<EntryState>>,
    receipt: &Mutex<Option<ReceiptState>>,
) {
    let latest = latest_remote_event_id(inner);
    let Some(event_id) = latest else {
        return;
    };
    {
        let mut guard = lock(receipt);
        // If another send already covered this event, skip.
        if let Some(r) = &*guard {
            if r.last_event == event_id {
                return;
            }
        }
        *guard = Some(ReceiptState {
            last_event: event_id.clone(),
            last_sent: Instant::now(),
        });
    }
    let event_id = match EventId::parse(&event_id) {
        Ok(e) => e.to_owned(),
        Err(_) => return,
    };
    send_read_receipt(room, event_id).await;
}

/// Re-map the whole item list and publish it into `ClientState::messages`.
/// Remapping per batch is O(n) in the visible window but side-effect free; a
/// diff-preserving mapper is premature for chat-sized pages.
///
/// Also prunes `send_handles` whose echo is no longer pending (Sent or
/// cancelled): a retry/discard against the mapped id then correctly reports
/// "already sent".
fn publish(
    inner: &Mutex<EntryState>,
    send_handles: &Mutex<Vec<SendHandle>>,
    state: &ClientState,
    room_id: &str,
    own_id: &str,
) {
    let mapped: Vec<Message> = {
        let guard = lock(inner);
        guard
            .items
            .iter()
            .filter_map(|item| map_item(item, own_id))
            .collect()
    };
    lock(inner).mapped_len = mapped.len();
    {
        let guard = lock(inner);
        // Keep handles only for echos still present as Local-kind items:
        // `local_created_at` is Some exactly for those, so an echo that
        // resolved to a remote item drops its handle.
        let pending: std::collections::HashSet<_> = guard
            .items
            .iter()
            .filter_map(|item| match item.kind() {
                TimelineItemKind::Event(event) => event.local_created_at(),
                TimelineItemKind::Virtual(_) => None,
            })
            .collect();
        lock(send_handles).retain(|h| pending.contains(&h.created_at));
    }
    let mut map = state.messages.peek().clone();
    map.insert(room_id.to_string(), mapped);
    let mut messages = state.messages;
    messages.set(map);
}

/// Re-map a thread-focused timeline and publish it into
/// `ClientState::threads[root_id]`. Same map-replace discipline as
/// [`publish`].
fn publish_thread(state: &ClientState, root_id: &str, items: &Mutex<Vec<Arc<TimelineItem>>>) {
    let replies: Vec<ThreadReply> = {
        let guard = lock(items);
        guard
            .iter()
            .filter_map(|item| thread_reply_row(item))
            .collect()
    };
    let mut map = state.threads.peek().clone();
    map.insert(root_id.to_string(), replies);
    let mut threads = state.threads;
    threads.set(map);
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
    // Remote events key by event id; local echoes key by transaction id.
    // NOTE: the SDK replaces a pending echo with the Sent/remote echo in
    // place through the diff stream — do NOT hand-merge echoes by content
    // here (docs/05). The `txn-` prefix lets retry/discard find the send
    // handle again (and lets the UI ignore reaction toggles on echoes).
    let id = match event.identifier() {
        TimelineEventItemId::EventId(id) => id.to_string(),
        TimelineEventItemId::TransactionId(id) => format!("txn-{id}"),
    };
    // 0.18's EventSendState has no separate "sending" variant: every
    // in-flight echo is `NotSentYet`.
    let send_state = match event.send_state() {
        Some(EventSendState::NotSentYet { .. }) => SendState::Sending,
        Some(EventSendState::SendingFailed { .. }) => SendState::Failed,
        Some(EventSendState::Sent { .. }) | None => SendState::Sent,
    };
    let from = match event.sender_profile() {
        TimelineDetails::Ready(profile) => profile
            .display_name
            .clone()
            .unwrap_or_else(|| event.sender().to_string()),
        _ => event.sender().to_string(),
    };
    // Sender avatar MXCs (checkpoint 07) are only known once the profile
    // resolves; until then rows fall back to initials.
    let avatar = match event.sender_profile() {
        TimelineDetails::Ready(profile) => profile.avatar_url.as_ref().map(|u| u.to_string()),
        _ => None,
    };
    let time = format_hhmm(event.timestamp().get().into());

    let mut msg = |text: String, system: bool| {
        let mut m = Message::new(id.clone(), from.clone(), time.clone(), text);
        m.mine = event.is_own();
        m.system = system;
        m.send_state = send_state;
        m
    };

    match event.content() {
        TimelineItemContent::MsgLike(msglike) => {
            let mut m = map_msglike(&mut msg, msglike, own_id);
            m.avatar = avatar;
            Some(m)
        }
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

    let mut attachment = None;
    let (text, system) = match &msglike.kind {
        MsgLikeKind::Message(message) => {
            use matrix_sdk::ruma::events::room::message::MessageType as T;
            let body = match message.msgtype() {
                T::Text(t) => t.body.clone(),
                T::Notice(n) => n.body.clone(),
                T::Emote(e) => return emote_row(msg, &e.body, own_id, msglike),
                // Checkpoint 07: real attachment mapping — the event body is
                // kept as the row text (element conventions: caption when it
                // differs from the filename); MessageRow hides a bubble that
                // only repeats the filename.
                T::Image(i) => {
                    let media = crate::media::MappedMedia::from(i);
                    attachment = Some(media.attachment);
                    media.body
                }
                T::File(f) => {
                    let media = crate::media::MappedMedia::from(f);
                    attachment = Some(media.attachment);
                    media.body
                }
                T::Video(v) => {
                    let media = crate::media::MappedMedia::from(v);
                    attachment = Some(media.attachment);
                    media.body
                }
                T::Audio(a) => {
                    let media = crate::media::MappedMedia::from(a);
                    attachment = Some(media.attachment);
                    media.body
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
        // Placeholder row unchanged; the reason class lands in debug logs for
        // support (checkpoint 08): megolm UTDs name the missing session,
        // olm/unknown name the algorithm. No crypto jargon in the UI.
        MsgLikeKind::UnableToDecrypt(encrypted) => {
            log_utd_reason(encrypted);
            ("🔒 unable to decrypt".into(), true)
        }
        MsgLikeKind::Sticker(s) => (format!("sticker: {}", s.content().body), false),
        MsgLikeKind::Poll(_) => ("[poll — open in another client]".into(), true),
        _ => ("[unsupported message type]".into(), true),
    };

    let mut m = msg(text, system);
    m.reactions = reactions;
    m.reply_to = reply_to;
    m.attachment = attachment;
    // Thread root reply count ("N replies" badge), from the SDK's thread
    // summary; updates through the same diff batch as the reply lands.
    m.thread_count = msglike
        .thread_summary
        .as_ref()
        .map(|summary| summary.num_replies)
        .unwrap_or(0);
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

/// Map one thread-timeline item to a panel row. Skips virtual items and
/// non-message events; redactions/decrypt failures use the same placeholder
/// texts as the main timeline.
fn thread_reply_row(item: &TimelineItem) -> Option<ThreadReply> {
    let TimelineItemKind::Event(event) = item.kind() else {
        return None;
    };
    let TimelineItemContent::MsgLike(msglike) = event.content() else {
        return None;
    };
    let text = match &msglike.kind {
        MsgLikeKind::Message(message) => message.body().to_string(),
        MsgLikeKind::Redacted => "message removed".into(),
        MsgLikeKind::UnableToDecrypt(_) => "\u{1f512} unable to decrypt".into(),
        MsgLikeKind::Sticker(s) => format!("sticker: {}", s.content().body),
        _ => return None,
    };
    let from = match event.sender_profile() {
        TimelineDetails::Ready(profile) => profile
            .display_name
            .clone()
            .unwrap_or_else(|| event.sender().to_string()),
        _ => event.sender().to_string(),
    };
    Some(ThreadReply {
        from,
        time: format_hhmm(event.timestamp().get().into()),
        mine: event.is_own(),
        text,
    })
}

/// Debug-log why an encrypted event couldn't be decrypted: megolm UTDs
/// name the missing session id (the key to look for in gossip/backup logs);
/// olm events name the algorithm; `Unknown` means an unrecognized scheme.
fn log_utd_reason(encrypted: &EncryptedMessage) {
    match encrypted {
        EncryptedMessage::MegolmV1AesSha2 { session_id, .. } => {
            tracing::debug!(
                session_id,
                "UTD: megolm session missing or not yet received"
            );
        }
        EncryptedMessage::OlmV1Curve25519AesSha2 { .. } => {
            tracing::debug!("UTD: olm event (not a room message)");
        }
        EncryptedMessage::Unknown => {
            tracing::debug!("UTD: unknown encryption algorithm");
        }
    }
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
