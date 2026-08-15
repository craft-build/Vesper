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

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use dioxus_signals::{ReadableExt, Signal, SyncStorage, WritableExt};
use eyeball_im::VectorDiff;
use futures::StreamExt;
use matrix_sdk::{Client, RoomState};
use matrix_sdk_ui::{
    room_list_service::{
        filters::new_filter_all, RoomListItem, RoomListLoadingState, RoomListService,
    },
    sync_service::{State as SyncState, SyncService},
};

use crate::{
    api::{ClientError, ClientState},
    model::{Convo, ConvoKind, Presence},
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
/// Publishes every batch into `state.convos` / `state.connecting` and keeps
/// `snapshot` identical to the signal's value. Errors are returned (not
/// panicked) — typically a homeserver without simplified sliding sync.
pub async fn start_room_list(
    client: Client,
    state: ClientState,
    snapshot: Arc<RwLock<Vec<Convo>>>,
) -> Result<SyncHandles, ClientError> {
    let service = SyncService::builder(client.clone())
        // Retry internally on network loss instead of dying: state flips to
        // `SyncState::Offline` and we surface "connecting…" in the UI.
        .with_offline_mode()
        .build()
        .await
        .map_err(|e| ClientError(format!("Could not build the sync service: {e}")))?;

    // Our own MXID, captured once for DM-counterpart resolution (checkpoint
    // 06): a DM's `Convo.mxid` is the *other* member, and `Convo.status` is
    // that member's presence from `state.presence`. Captured before
    // `RoomListService::new` moves `client`.
    let own_id = client.user_id().map(|u| u.to_owned());

    // `RoomListService::new` subscribes the event cache itself (required for
    // unread counts under sliding sync); do not call it again here.
    let room_list_service = RoomListService::new(client)
        .await
        .map_err(|e| ClientError(format!("Could not build the room list service: {e}")))?;

    let all_rooms = room_list_service.all_rooms().await.map_err(|e| {
        ClientError(format!(
            "The server does not support simplified sliding sync: {e}"
        ))
    })?;

    // Signals are `Copy` handles; bind them as locals so writes through them
    // from this runtime thread are plainly legal (they use SyncStorage).
    let mut convos_signal = state.convos;
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
        let mut items: Vec<RoomListItem> = Vec::new();
        // Only log the invite count when it changes — it re-derives per batch.
        let mut logged_invites = 0usize;
        loop {
            tokio::select! {
                batch = entries.next() => match batch {
                    Some(batch) => {
                        tracing::info!(
                            diffs = batch.len(),
                            total = items.len(),
                            "applying room list diff batch"
                        );
                        apply_diffs(&mut items, batch);
                        // Invites show up in `all_rooms` too; the UI has
                        // nowhere to render them — skip + log count (docs/03).
                        const NOT_INVITED: fn(&&RoomListItem) -> bool =
                            |i| i.state() != RoomState::Invited;
                        let invite_count = items.len()
                            - items.iter().filter(NOT_INVITED).count();
                        if invite_count != logged_invites {
                            tracing::info!(
                                count = invite_count,
                                "invited rooms present but not rendered (deferred)"
                            );
                            logged_invites = invite_count;
                        }
                        let mapped: Vec<Convo> = futures::future::join_all(
                            items
                                .iter()
                                .filter(NOT_INVITED)
                                .map(|i| map_item(i, own_id.as_ref(), presence_signal)),
                        )
                        .await;
                        publish(&mut convos_signal, &snapshot, mapped);
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
        }
    });

    Ok(SyncHandles { service, task })
}

/// Write the mapped list to both the signal (reactive reads in the UI) and
/// the synchronous snapshot (`conversations()`).
fn publish(
    signal: &mut Signal<Vec<Convo>, SyncStorage>,
    snapshot: &Arc<RwLock<Vec<Convo>>>,
    convos: Vec<Convo>,
) {
    *snapshot.write().unwrap_or_else(|e| e.into_inner()) = convos.clone();
    signal.set(convos);
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
        // Spaces are checkpoint 09.
        space: None,
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
