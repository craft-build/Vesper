//! Live room-list sync: sliding-sync room list service → `Vec<Convo>`.
//!
//! [`start_room_list`] drives a [`SyncService`] (native simplified sliding
//! sync: MSC4186, no proxy URL needed) and a [`RoomListService`], subscribes
//! to the `all_rooms` list's `VectorDiff` stream, and publishes the mapped
//! [`Convo`] list into the [`ClientState`] signals plus a synchronous
//! snapshot (for `conversations()`).
//!
//! Ordering: instead of sorting by latest-event timestamps ourselves, we
//! apply the diffs faithfully — sliding sync's bump stamp already orders by
//! recency, and the UI splits DMs vs rooms while preserving that order
//! (deviation from docs/03 §Mapping, which predicted a latest-events sort
//! that doesn't fit the `VectorDiff` API in 0.18).
//!
//! Invites: they live in a separate RoomListService list which has no public
//! accessor in 0.18 and no UI surface yet — deferred (docs/03 §Design
//! decisions permits this).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

use dioxus_signals::{ReadableExt, Signal, SyncStorage, WritableExt};
use eyeball_im::VectorDiff;
use futures::StreamExt;
use matrix_sdk::{ruma::events::space::child::SpaceChildEventContent, Client, RoomState};
use matrix_sdk_ui::{
    room_list_service::{
        filters::new_filter_all, RoomListItem, RoomListLoadingState, RoomListService,
    },
    sync_service::{State as SyncState, SyncService},
};

use crate::{
    api::{ClientError, ClientState},
    model::{Convo, ConvoKind, Presence, Space},
};

/// Page size for the room list's dynamic entries controller. Pagination is a
/// render-window hint for the stream, not a hard cap — all rooms flow through.
const PAGE_SIZE: usize = 20;

/// Handles for a running room-list sync, so logout can shut it down cleanly.
pub struct SyncHandles {
    service: SyncService,
    task: tokio::task::JoinHandle<()>,
}

impl SyncHandles {
    /// Stop gracefully (the sync service winds down its sessions), then abort
    /// the diff-consumption task. `SyncService` has no `Drop` impl, so callers
    /// MUST go through this — dropping without stopping leaves sync running.
    pub async fn stop(self) {
        self.service.stop().await;
        self.task.abort();
    }
}

/// Start syncing the room list for a freshly connected `client`.
///
/// Publishes every batch into `state.convos` / `state.spaces` /
/// `state.connecting` and keeps the snapshots identical to the signals.
/// Errors are returned (not panicked) — typically a homeserver without
/// simplified sliding sync.
pub async fn start_room_list(
    client: Client,
    state: ClientState,
    snapshot: &Arc<RwLock<Vec<Convo>>>,
    spaces_snapshot: &Arc<RwLock<Vec<Space>>>,
) -> Result<SyncHandles, ClientError> {
    let snapshot = snapshot.clone();
    let spaces_snapshot = spaces_snapshot.clone();
    let service = SyncService::builder(client.clone())
        // Retry internally on network loss instead of dying: state flips to
        // `SyncState::Offline` and we surface "connecting…" in the UI.
        .with_offline_mode()
        .build()
        .await
        .map_err(|e| ClientError::server(format!("Could not build the sync service: {e}")))?;

    // Our own MXID, captured once for DM-counterpart resolution (checkpoint
    // 06): a DM's `Convo.mxid` is the *other* member, and `Convo.status` is
    // that member's presence from `state.presence`. Captured before
    // `RoomListService::new` moves `client`.
    let own_id = client.user_id().map(|u| u.to_owned());

    // `RoomListService::new` subscribes the event cache itself (required for
    // unread counts under sliding sync); do not call it again here.
    let room_list_service = RoomListService::new(client)
        .await
        .map_err(|e| ClientError::server(format!("Could not build the room list service: {e}")))?;

    let all_rooms = room_list_service.all_rooms().await.map_err(|e| {
        ClientError::unsupported(format!(
            "The server does not support simplified sliding sync: {e}"
        ))
    })?;

    // Signals are `Copy` handles; bind them as locals so writes through them
    // from this runtime thread are plainly legal (they use SyncStorage).
    let mut convos_signal = state.convos;
    let mut spaces_signal = state.spaces;
    let mut connecting = state.connecting;
    let presence_signal = state.presence;
    // `connecting` should only flip to false once (the sync state's offline
    // handling takes over from there); avoid redundant signal writes.
    let mut warmed = false;

    service.start().await;
    // `Box::pin` everything handed to `select!`: the entries stream is a
    // `!Unpin` `async_stream` chain.
    let mut sync_status = Box::pin(service.state());

    let task = tokio::spawn(async move {
        let (entries, controller) = all_rooms.entries_with_dynamic_adapters(PAGE_SIZE);
        // The stream yields nothing until a filter is set; `new_filter_all`
        // keeps every room (DM vs room split happens in the UI).
        controller.set_filter(Box::new(new_filter_all(vec![])));
        let mut entries = Box::pin(entries);
        let mut loading = Box::pin(all_rooms.loading_state());
        // Checkpoint 11 §C: `rows` keeps each `RoomListItem` paired with its
        // mapped `Convo`, so a diff batch only (re)maps the values it
        // carries — O(batch) remaps, never a full-list rebuild on the
        // high-churn path (busy accounts used to re-run every per-room
        // async store lookup per batch). Placeholder convos ride along for
        // spaces and invites; publish filters them out.
        let mut rows: Vec<(RoomListItem, Convo)> = Vec::new();
        // Mapped spaces keyed by room id; a batch carrying a space room
        // refreshes its entry (spaces diff like any other room when their
        // children change), a room that stops being a space drops out.
        let mut space_cache: HashMap<String, Space> = HashMap::new();
        let mut spaces: Vec<Space>;
        let mut membership: HashMap<String, String> = HashMap::new();
        // Last presence snapshot the convos were mapped against: DM status
        // dots refresh by remapping only the rooms whose counterpart
        // changed (previously this fell out of the per-batch full remap).
        let mut presence_seen: BTreeMap<String, Presence> = BTreeMap::new();
        // Only log the invite count when it changes — it re-derives per batch.
        let mut logged_invites = 0usize;
        loop {
            tokio::select! {
                batch = entries.next() => match batch {
                    Some(batch) => {
                        tracing::info!(
                            diffs = batch.len(),
                            total = rows.len(),
                            "applying room list diff batch"
                        );
                        // Map only the carried values (see `attach`), then
                        // reuse the tested diff applier over paired rows.
                        let paired = attach(
                            batch,
                            own_id.as_ref(),
                            presence_signal,
                            &membership,
                            &mut space_cache,
                        )
                        .await;
                        apply_diffs(&mut rows, paired);
                        // Invites show up in `all_rooms` too; the UI has
                        // nowhere to render them — skip + log count (docs/03).
                        const JOINED: fn(&&(RoomListItem, Convo)) -> bool =
                            |r| r.0.state() == RoomState::Joined;
                        let invite_count = rows.len() - rows.iter().filter(JOINED).count();
                        if invite_count != logged_invites {
                            tracing::info!(
                                count = invite_count,
                                "invited rooms present but not rendered (deferred)"
                            );
                            logged_invites = invite_count;
                        }
                        // Spaces (checkpoint 09): rebuilt from the cache —
                        // only spaces carried by this batch were re-mapped.
                        spaces = rows
                            .iter()
                            .filter(|(i, _)| i.state() == RoomState::Joined && i.is_space())
                            .filter_map(|(i, _)| space_cache.get(i.room_id().as_str()).cloned())
                            .collect();
                        let new_membership = space_membership(&spaces);
                        if new_membership != membership {
                            // A space's children changed: refresh only the
                            // rooms whose grouping actually moved.
                            for row in rows.iter_mut() {
                                if row.0.state() != RoomState::Joined || row.0.is_space() {
                                    continue;
                                }
                                let want = new_membership.get(row.0.room_id().as_str()).cloned();
                                if row.1.space != want {
                                    row.1 = map_item(
                                        &row.0,
                                        own_id.as_ref(),
                                        presence_signal,
                                        &new_membership,
                                    )
                                    .await;
                                }
                            }
                            membership = new_membership;
                        }
                        publish_rows(&mut convos_signal, &snapshot, &rows);
                        publish_spaces(&mut spaces_signal, &spaces_snapshot, spaces);
                    }
                    None => break,
                },
                status = sync_status.next() => match status {
                    Some(status) => {
                        // Offline / Error / Idle mean the user is looking at
                        // stale (or no) data. During warm-up (no `Loaded`
                        // yet), stay "connecting" even while `Running` — the
                        // service reports Running long before the first page
                        // of rooms actually lands.
                        connecting.set(!matches!(status, SyncState::Running) || !warmed);
                    }
                    None => break,
                },
                state = loading.next() => match state {
                    Some(RoomListLoadingState::Loaded { .. }) => {
                        // First full page landed: even if the sync later drops
                        // offline, this is the "warm" state.
                        if !warmed {
                            connecting.set(false);
                            warmed = true;
                        }
                    }
                    Some(_) => {}
                    None => break,
                },
            }
            // Presence-driven DM refresh (every loop wake): remap only DM
            // rows whose counterpart's presence changed since the last
            // pass, then republish if anything moved.
            let current = presence_signal.peek().clone();
            let changed: Vec<&String> = current
                .iter()
                .filter(|(id, p)| presence_seen.get(*id).as_ref() != Some(p))
                .map(|(id, _)| id)
                .collect();
            if !changed.is_empty() {
                let mut changed_any = false;
                for row in rows.iter_mut() {
                    if row.0.state() != RoomState::Joined
                        || row.0.is_space()
                        || row.1.kind != ConvoKind::Dm
                    {
                        continue;
                    }
                    if let Some(mxid) = row.1.mxid.as_ref() {
                        if changed.contains(&mxid) {
                            row.1 =
                                map_item(&row.0, own_id.as_ref(), presence_signal, &membership)
                                    .await;
                            changed_any = true;
                        }
                    }
                }
                presence_seen = current;
                if changed_any {
                    publish_rows(&mut convos_signal, &snapshot, &rows);
                }
            }
        }
    });

    Ok(SyncHandles { service, task })
}

/// Write the mapped list (joined, non-space rows) to both the signal
/// (reactive reads in the UI) and the synchronous snapshot
/// (`conversations()`).
fn publish_rows(
    signal: &mut Signal<Vec<Convo>, SyncStorage>,
    snapshot: &Arc<RwLock<Vec<Convo>>>,
    rows: &[(RoomListItem, Convo)],
) {
    let convos: Vec<Convo> = rows
        .iter()
        .filter(|(item, _)| item.state() == RoomState::Joined && !item.is_space())
        .map(|(_, convo)| convo.clone())
        .collect();
    *snapshot.write().unwrap_or_else(|e| e.into_inner()) = convos.clone();
    signal.set(convos);
}

/// Same as [`publish`] for the spaces list (`spaces()`).
fn publish_spaces(
    signal: &mut Signal<Vec<Space>, SyncStorage>,
    snapshot: &Arc<RwLock<Vec<Space>>>,
    spaces: Vec<Space>,
) {
    *snapshot.write().unwrap_or_else(|e| e.into_inner()) = spaces.clone();
    signal.set(spaces);
}

/// Placeholder convo for rows that never publish (spaces, invites).
fn placeholder_convo(id: &str) -> Convo {
    Convo {
        id: id.to_string(),
        kind: ConvoKind::Room,
        name: String::new(),
        last: String::new(),
        unread: 0,
        encrypted: false,
        avatar: None,
        mxid: None,
        status: None,
        topic: None,
        space: None,
        members: None,
    }
}

/// Pair one carried item with its mapped convo, maintaining the space
/// cache on the way (spaces in, demoted rooms out).
async fn pair(
    item: RoomListItem,
    own_id: Option<&matrix_sdk::ruma::OwnedUserId>,
    presence_signal: dioxus_signals::Signal<BTreeMap<String, Presence>, dioxus_signals::SyncStorage>,
    membership: &HashMap<String, String>,
    space_cache: &mut HashMap<String, Space>,
) -> (RoomListItem, Convo) {
    let id = item.room_id().to_string();
    if item.is_space() {
        let space = map_space(&item).await;
        space_cache.insert(id, space);
        return (item, placeholder_convo(""));
    }
    space_cache.remove(&id);
    if item.state() == RoomState::Joined {
        let convo = map_item(&item, own_id, presence_signal, membership).await;
        (item, convo)
    } else {
        (item, placeholder_convo(&id))
    }
}

/// Map the values a diff batch carries onto paired rows — the O(batch)
/// heart of the checkpoint-11 remap. Diff shapes pass through untouched;
/// only carried values pay the (async) mapping cost.
async fn attach(
    diffs: Vec<VectorDiff<RoomListItem>>,
    own_id: Option<&matrix_sdk::ruma::OwnedUserId>,
    presence_signal: dioxus_signals::Signal<BTreeMap<String, Presence>, dioxus_signals::SyncStorage>,
    membership: &HashMap<String, String>,
    space_cache: &mut HashMap<String, Space>,
) -> Vec<VectorDiff<(RoomListItem, Convo)>> {
    #[allow(clippy::items_after_statements)]
    async fn pair_ref(
        item: &RoomListItem,
        own_id: Option<&matrix_sdk::ruma::OwnedUserId>,
        presence_signal: dioxus_signals::Signal<
            BTreeMap<String, Presence>,
            dioxus_signals::SyncStorage,
        >,
        membership: &HashMap<String, String>,
        space_cache: &mut HashMap<String, Space>,
    ) -> (RoomListItem, Convo) {
        pair(item.clone(), own_id, presence_signal, membership, space_cache).await
    }

    let mut out = Vec::with_capacity(diffs.len());
    for diff in diffs {
        let paired = match diff {
            VectorDiff::Append { values } => {
                let mut mapped = Vec::with_capacity(values.len());
                for v in values.iter() {
                    mapped.push(pair_ref(v, own_id, presence_signal, membership, space_cache).await);
                }
                VectorDiff::Append {
                    values: mapped.into_iter().collect(),
                }
            }
            VectorDiff::Clear => VectorDiff::Clear,
            VectorDiff::PushFront { value } => VectorDiff::PushFront {
                value: pair_ref(&value, own_id, presence_signal, membership, space_cache).await,
            },
            VectorDiff::PushBack { value } => VectorDiff::PushBack {
                value: pair_ref(&value, own_id, presence_signal, membership, space_cache).await,
            },
            VectorDiff::PopFront => VectorDiff::PopFront,
            VectorDiff::PopBack => VectorDiff::PopBack,
            VectorDiff::Insert { index, value } => VectorDiff::Insert {
                index,
                value: pair_ref(&value, own_id, presence_signal, membership, space_cache).await,
            },
            VectorDiff::Set { index, value } => VectorDiff::Set {
                index,
                value: pair_ref(&value, own_id, presence_signal, membership, space_cache).await,
            },
            VectorDiff::Remove { index } => VectorDiff::Remove { index },
            VectorDiff::Truncate { length } => VectorDiff::Truncate { length },
            VectorDiff::Reset { values } => {
                let mut mapped = Vec::with_capacity(values.len());
                for v in values.iter() {
                    mapped.push(pair_ref(v, own_id, presence_signal, membership, space_cache).await);
                }
                VectorDiff::Reset {
                    values: mapped.into_iter().collect(),
                }
            }
        };
        out.push(paired);
    }
    out
}

/// Map one space room to the UI's [`Space`] summary. Children come from the
/// room's `m.space.child` state events, spec-ordered (the `order` key
/// lexicographically first, then event time, then room id). Children are
/// kept as plain ids — membership filtering against the joined-room set
/// happens in [`space_membership`], and the drawer only ever finds joined
/// rooms to group anyway. One level deep (docs/09): a nested space's entry
/// stays in `children` but also renders as its own top-level space.
async fn map_space(item: &RoomListItem) -> Space {
    let mut children: Vec<(String, Option<String>, u64)> = item
        .get_state_events_static::<SpaceChildEventContent>()
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|raw| raw.deserialize().ok())
        .filter_map(|event| {
            let event = event.as_sync()?.as_original()?;
            // Spec: a child only counts when its content carries a non-empty
            // `via` (redacted / stripped entries don't).
            if event.content.via.is_empty() {
                return None;
            }
            Some((
                event.state_key.to_string(),
                event.content.order.as_ref().map(|o| o.to_string()),
                u64::from(event.origin_server_ts.get()),
            ))
        })
        .collect();
    children.sort_by(|a, b| {
        // `order` present beats absent; present orders compare as strings
        // (spec: lexicographic on codepoints), then by event time, then by
        // room id (the tuple's first element).
        (a.1.is_none(), &a.1, a.2, &a.0).cmp(&(b.1.is_none(), &b.1, b.2, &b.0))
    });
    Space {
        id: item.room_id().to_string(),
        name: item
            .cached_display_name()
            .map(|name| name.to_string())
            .unwrap_or_else(|| "Unknown space".into()),
        avatar: item.avatar_url().map(|u| u.to_string()),
        members: Some(item.joined_members_count().min(u32::MAX as u64) as u32),
        children: children.into_iter().map(|(id, _, _)| id).collect(),
    }
}

/// Reverse index of [`Space::children`]: room id → the space it should group
/// under. A room listed by several spaces joins the first one that lists it
/// (spaces are iterated in sync order); everything missing from the index
/// lands in the drawer's ungrouped bucket.
fn space_membership(spaces: &[Space]) -> HashMap<String, String> {
    let mut membership = HashMap::new();
    for space in spaces {
        for child in &space.children {
            membership
                .entry(child.clone())
                .or_insert_with(|| space.id.clone());
        }
    }
    membership
}

/// Map one room list item to the UI's `Convo` shape. Mostly cache reads
/// (they were pre-computed for the filters); `is_direct` is the only async
/// store lookup, batched via `join_all` by the caller. For DMs (checkpoint 06)
/// we additionally resolve the *other* member's MXID and look up their
/// presence in `presence_signal` — the nav drawer's status dot and the
/// profile panel read these.
async fn map_item(
    item: &RoomListItem,
    own_id: Option<&matrix_sdk::ruma::OwnedUserId>,
    presence_signal: dioxus_signals::Signal<
        BTreeMap<String, Presence>,
        dioxus_signals::SyncStorage,
    >,
    membership: &HashMap<String, String>,
) -> Convo {
    use matrix_sdk::RoomMemberships;
    let is_dm = item.is_direct().await.unwrap_or(false);
    let counts = item.unread_notification_counts();
    // DM counterpart: the other joined member. Falls back to `None` when
    // member state hasn't synced yet — the dot stays Offline until it does.
    // The DM avatar overrides the room avatar with the counterpart's
    // (checkpoint 07).
    let (mxid, status, avatar) = if is_dm {
        match item.members(RoomMemberships::JOIN).await {
            Ok(members) => {
                let other = members
                    .iter()
                    .find(|m| own_id.is_none_or(|o| o != m.user_id()));
                // `item.avatar_url()` is DM-aware (counterpart avatar) —
                // use it whenever the counterpart's member profile is
                // missing or has no avatar of its own.
                let avatar = other
                    .and_then(|m| m.avatar_url().map(|u| u.to_string()))
                    .or_else(|| item.avatar_url().map(|u| u.to_string()));
                let other = other.map(|m| m.user_id().to_owned());
                let other_ref = other.as_ref();
                let status =
                    other_ref.and_then(|id| presence_signal.peek().get(id.as_str()).copied());
                (other.map(|u| u.to_string()), status, avatar)
            }
            Err(_) => (None, None, None),
        }
    } else {
        (None, None, item.avatar_url().map(|u| u.to_string()))
    };
    Convo {
        id: item.room_id().to_string(),
        kind: if is_dm {
            ConvoKind::Dm
        } else {
            ConvoKind::Room
        },
        name: item
            .cached_display_name()
            .map(|name| name.to_string())
            .unwrap_or_else(|| "Unknown room".into()),
        // Message previews land in checkpoint 04.
        last: String::new(),
        unread: counts.notification_count.min(u32::MAX as u64) as u32,
        encrypted: item.encryption_state().is_encrypted(),
        // Room/counterpart avatar (checkpoint 07).
        avatar,
        // DM counterpart MXID + presence (checkpoint 06).
        mxid,
        status,
        topic: item.topic(),
        // Space grouping (checkpoint 09): the space whose `m.space.child`
        // lists this room; `None` → the drawer's ungrouped bucket.
        space: membership.get(item.room_id().as_str()).cloned(),
        members: Some(item.joined_members_count().min(u32::MAX as u64) as u32),
    }
}

/// Apply a batch of `VectorDiff`s to `vec`, mirroring eyeball-im's semantics.
/// Indices are expected in-bounds per the diff contract; out-of-bounds input
/// is tolerated (clamped/ignored) rather than panicking the sync task.
pub(crate) fn apply_diffs<T: Clone>(vec: &mut Vec<T>, diffs: Vec<VectorDiff<T>>) {
    for diff in diffs {
        match diff {
            VectorDiff::Append { values } => vec.extend(values.iter().cloned()),
            VectorDiff::Clear => vec.clear(),
            VectorDiff::PushFront { value } => vec.insert(0, value),
            VectorDiff::PushBack { value } => vec.push(value),
            VectorDiff::PopFront => {
                if !vec.is_empty() {
                    vec.remove(0);
                }
            }
            VectorDiff::PopBack => {
                vec.pop();
            }
            VectorDiff::Insert { index, value } => vec.insert(index.min(vec.len()), value),
            VectorDiff::Set { index, value } => {
                if index < vec.len() {
                    vec[index] = value;
                }
            }
            VectorDiff::Remove { index } => {
                if index < vec.len() {
                    vec.remove(index);
                }
            }
            VectorDiff::Truncate { length } => vec.truncate(length),
            VectorDiff::Reset { values } => *vec = values.iter().cloned().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space(id: &str, children: &[&str]) -> Space {
        Space {
            id: id.into(),
            name: id.into(),
            avatar: None,
            members: None,
            children: children.iter().map(|c| (*c).into()).collect(),
        }
    }

    #[test]
    fn space_membership_first_space_wins() {
        let spaces = vec![space("one", &["a", "b"]), space("two", &["b", "c"])];
        let membership = space_membership(&spaces);
        // "b" is listed by both; the first space that lists it claims it.
        assert_eq!(membership["a"], "one");
        assert_eq!(membership["b"], "one");
        assert_eq!(membership["c"], "two");
    }

    #[test]
    fn space_membership_empty_for_ungrouped() {
        let membership = space_membership(&[space("one", &["a"])]);
        assert!(!membership.contains_key("z"));
    }

    #[test]
    fn apply_diffs_handles_every_variant() {
        let mut v: Vec<i32> = Vec::new();
        apply_diffs(
            &mut v,
            vec![VectorDiff::Append {
                values: [1, 2, 3].into_iter().collect(),
            }],
        );
        assert_eq!(v, [1, 2, 3]);

        apply_diffs(
            &mut v,
            vec![
                VectorDiff::PushFront { value: 0 },
                VectorDiff::PushBack { value: 4 },
            ],
        );
        assert_eq!(v, [0, 1, 2, 3, 4]);

        apply_diffs(&mut v, vec![VectorDiff::PopFront, VectorDiff::PopBack]);
        assert_eq!(v, [1, 2, 3]);

        apply_diffs(
            &mut v,
            vec![
                VectorDiff::Insert { index: 1, value: 9 },
                VectorDiff::Set { index: 0, value: 8 },
            ],
        );
        assert_eq!(v, [8, 9, 2, 3]);

        apply_diffs(&mut v, vec![VectorDiff::Remove { index: 1 }]);
        assert_eq!(v, [8, 2, 3]);

        apply_diffs(&mut v, vec![VectorDiff::Truncate { length: 2 }]);
        assert_eq!(v, [8, 2]);

        apply_diffs(
            &mut v,
            vec![VectorDiff::Reset {
                values: [7].into_iter().collect(),
            }],
        );
        assert_eq!(v, [7]);

        apply_diffs(&mut v, vec![VectorDiff::Clear]);
        assert!(v.is_empty());
    }

    // Checkpoint 11 §C: the paired-rows applier must keep items and their
    // mapped convos index-aligned through every diff shape — the O(batch)
    // remap depends on it.
    #[test]
    fn apply_diffs_keeps_pairs_aligned() {
        let mk = |n: i32| (n, n * 10);
        let mut rows: Vec<(i32, i32)> = Vec::new();
        apply_diffs(
            &mut rows,
            vec![VectorDiff::Append {
                values: vec![mk(1), mk(2), mk(3)].into_iter().collect(),
            }],
        );
        assert_eq!(rows, vec![mk(1), mk(2), mk(3)]);

        apply_diffs(
            &mut rows,
            vec![
                VectorDiff::Set {
                    index: 1,
                    value: mk(9),
                },
                VectorDiff::PushFront {
                    value: mk(0),
                },
                VectorDiff::Remove { index: 2 },
            ],
        );
        // After Set: [1,9,3]; PushFront: [0,1,9,3]; Remove(2): [0,1,3].
        assert_eq!(rows, vec![mk(0), mk(1), mk(3)]);
        for (item, convo) in &rows {
            assert_eq!(*convo, item * 10, "pairs stay aligned");
        }

        apply_diffs(&mut rows, vec![VectorDiff::Truncate { length: 1 }]);
        assert_eq!(rows, vec![mk(0)]);
        apply_diffs(&mut rows, vec![VectorDiff::Clear]);
        assert!(rows.is_empty());
    }

    #[test]
    fn apply_diffs_tolerates_stale_indices() {
        // Defensive: a lagging subscriber can get odd sequences; never panic.
        let mut v: Vec<i32> = Vec::new();
        apply_diffs(
            &mut v,
            vec![
                VectorDiff::PopFront,
                VectorDiff::PopBack,
                VectorDiff::Remove { index: 5 },
                VectorDiff::Set { index: 5, value: 1 },
                VectorDiff::Insert {
                    index: 5,
                    value: 42,
                },
            ],
        );
        assert_eq!(v, [42]);
    }
}
