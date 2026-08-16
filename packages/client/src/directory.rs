//! Public room directory: browse, search, join, leave (checkpoint 09).
//!
//! Everything here runs on the runtime thread (called from the spawned
//! `PublicRooms`/`JoinRoom`/`LeaveRoom` command handlers) and maps the SDK's
//! directory responses into the plain [`crate::model`] shapes.

use matrix_sdk::{
    ruma::{
        api::client::directory::get_public_rooms_filtered,
        api::error::{ErrorKind, RetryAfter},
        directory::{Filter, PublicRoomsChunk, RoomTypeFilter},
        room::RoomType,
        OwnedRoomOrAliasId, RoomOrAliasId, UInt,
    },
    Client,
};

use crate::{
    api::ClientError,
    model::{PublicRoom, PublicRoomPage, PublicSpace, PublicSpacePage},
};

/// Rooms per directory page. Small enough that search feels instant, large
/// enough that matrix.org's busy directory isn't a thousand-tap scroll.
pub const DIRECTORY_PAGE_SIZE: u32 = 20;

/// Fetch one page of the homeserver's public room directory matching
/// `query` (server-side `generic_search_term`; empty browses everything),
/// continuing after `batch_token`.
async fn directory_chunk(
    client: &Client,
    query: &str,
    batch_token: Option<String>,
    room_types: Vec<RoomTypeFilter>,
) -> Result<(Vec<PublicRoomsChunk>, Option<String>), ClientError> {
    let mut filter = Filter::new();
    if !query.trim().is_empty() {
        filter.generic_search_term = Some(query.trim().to_owned());
    }
    filter.room_types = room_types;
    let mut request = get_public_rooms_filtered::v3::Request::new();
    request.limit = Some(UInt::from(DIRECTORY_PAGE_SIZE));
    request.since = batch_token;
    request.filter = filter;
    let response = client
        .public_rooms_filtered(request)
        .await
        .map_err(|e| ClientError(format!("Directory search failed: {e}")))?;
    Ok((response.chunk, response.next_batch))
}

/// Public rooms page. See [`directory_chunk`] for the query semantics.
pub async fn public_rooms(
    client: &Client,
    query: &str,
    batch_token: Option<String>,
) -> Result<PublicRoomPage, ClientError> {
    let (chunk, next) = directory_chunk(client, query, batch_token, Vec::new()).await?;
    Ok(PublicRoomPage {
        rooms: chunk.iter().map(chunk_to_room).collect(),
        next,
    })
}

/// Public spaces page: asks the server to filter `room_types: [m.space]`
/// (spec v1.7+; Synapse has it since 1.57). Servers that reject the filter
/// fall back to client-side filtering of unfiltered pages.
pub async fn public_spaces(
    client: &Client,
    query: &str,
    batch_token: Option<String>,
) -> Result<PublicSpacePage, ClientError> {
    let attempt = directory_chunk(
        client,
        query,
        batch_token.clone(),
        vec![RoomTypeFilter::Space],
    )
    .await;
    let (chunk, next) = match attempt {
        Ok(page) => page,
        Err(e) => {
            tracing::warn!("server-side space filter rejected, client-filtering: {e}");
            let (chunk, next) = directory_chunk(client, query, batch_token, Vec::new()).await?;
            (
                chunk
                    .into_iter()
                    .filter(|c| c.room_type == Some(RoomType::Space))
                    .collect(),
                next,
            )
        }
    };
    Ok(PublicSpacePage {
        spaces: chunk.iter().map(chunk_to_space).collect(),
        next,
    })
}

/// Display name for a directory chunk: the human name if the room has one,
/// else its canonical alias, else the bare room id (the spec allows all
/// three to be missing-but-present).
fn chunk_display_name(chunk: &PublicRoomsChunk) -> String {
    chunk
        .name
        .clone()
        .or_else(|| chunk.canonical_alias.as_ref().map(|a| a.to_string()))
        .unwrap_or_else(|| chunk.room_id.to_string())
}

fn member_count(chunk: &PublicRoomsChunk) -> u32 {
    u32::try_from(u64::from(chunk.num_joined_members)).unwrap_or(u32::MAX)
}

fn chunk_to_room(chunk: &PublicRoomsChunk) -> PublicRoom {
    PublicRoom {
        id: chunk.room_id.to_string(),
        name: chunk_display_name(chunk),
        members: member_count(chunk),
        topic: chunk.topic.clone().unwrap_or_default(),
    }
}

fn chunk_to_space(chunk: &PublicRoomsChunk) -> PublicSpace {
    PublicSpace {
        id: chunk.room_id.to_string(),
        name: chunk_display_name(chunk),
        members: member_count(chunk),
        topic: chunk.topic.clone().unwrap_or_default(),
    }
}

/// Join a public room by id or alias. Re-joining an already-joined room is
/// a success, not an error; rate limits surface with a retry-after hint so
/// the modal can show an inline, retryable message.
pub async fn join_room(client: &Client, id_or_alias: &str) -> Result<(), ClientError> {
    let target: OwnedRoomOrAliasId = RoomOrAliasId::parse(id_or_alias)
        .map_err(|_| ClientError(format!("“{id_or_alias}” is not a room id or alias.")))?;
    match client.join_room_by_id_or_alias(&target, &[]).await {
        Ok(_) => Ok(()),
        Err(e) => {
            // Some servers answer a redundant join with an error instead of
            // a no-op; if we're verifiably in the room already, that's the
            // success the caller asked for.
            if is_already_joined(client, &target) {
                return Ok(());
            }
            Err(join_error(&e))
        }
    }
}

/// Leave a joined room. Unknown/already-left ids return an error string the
/// UI can surface (the room-list stream is the source of truth for removal).
pub async fn leave_room(client: &Client, room_id: &str) -> Result<(), ClientError> {
    let room_id = matrix_sdk::ruma::RoomId::parse(room_id)
        .map_err(|_| ClientError(format!("“{room_id}” is not a room id.")))?;
    let Some(room) = client.get_room(&room_id) else {
        return Err(ClientError("That room isn't in your list anymore.".into()));
    };
    room.leave()
        .await
        .map_err(|e| ClientError(format!("Could not leave the room: {e}")))
}

/// Whether the target (a room id or alias) resolves to a room we're joined
/// to right now. Best-effort: aliases that fail to resolve just report
/// "not joined", leaving the original error in charge.
fn is_already_joined(client: &Client, target: &OwnedRoomOrAliasId) -> bool {
    let inner: &RoomOrAliasId = target;
    if !inner.is_room_id() {
        return false;
    }
    let Ok(room_id) = matrix_sdk::ruma::RoomId::parse(inner.as_str()) else {
        return false;
    };
    client
        .get_room(&room_id)
        .is_some_and(|room| room.state() == matrix_sdk::RoomState::Joined)
}

/// Map join failures to short, actionable messages. Rate limits keep their
/// retry-after hint (`M_LIMIT_EXCEEDED` is common on big public rooms).
fn join_error(err: &matrix_sdk::Error) -> ClientError {
    match err.client_api_error_kind() {
        Some(ErrorKind::LimitExceeded(data)) => {
            let hint = match data.retry_after.as_ref() {
                Some(RetryAfter::Delay(delay)) => {
                    format!(" Try again in {} seconds.", delay.as_secs().max(1))
                }
                _ => " Try again in a moment.".into(),
            };
            ClientError(format!(
                "Rate limited — you're joining rooms too fast.{hint}"
            ))
        }
        Some(ErrorKind::Forbidden) => {
            ClientError("You can't join that room (invite-only?).".into())
        }
        _ => ClientError(format!("Could not join: {err}")),
    }
}
