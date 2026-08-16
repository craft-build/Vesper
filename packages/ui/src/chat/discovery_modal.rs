//! Discovery modal (checkpoint 09): the real public room directory.
//!
//! Search is debounced and server-side; results page through the trait's
//! batch tokens with a "Show more" button. Join buttons carry pending /
//! joined / inline-error states; the joined room itself arrives through the
//! room-list stream (the convos signal), so a row flipping to "Joined" only
//! reflects the request's outcome.

use std::collections::HashMap;
use std::rc::Rc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use dioxus::prelude::*;
use dioxus_signals::ReadableExt;
#[cfg(not(target_arch = "wasm32"))]
use futures_timer::Delay;

use crate::data::{ClientState, PublicRoom, PublicSpace, VesperClient};
use crate::design_system::{Button, ButtonSize, ButtonVariant, Input, TabItem, Tabs};
use crate::icons::{Icon, IconName};

/// How long the search field stays idle before a query fires (rapid
/// keystrokes coalesce into one directory hit). Native only — see the
/// debounce effect for the web story.
#[cfg(not(target_arch = "wasm32"))]
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiscoveryTab {
    Rooms,
    Spaces,
}

/// Per-row join button state.
#[derive(Debug, Clone, PartialEq, Eq)]
enum JoinRow {
    Idle,
    Joining,
    Joined,
    Failed(String),
}

/// One fetched directory page, tab-agnostic so a single guard/write path
/// serves both tabs.
enum DirPage {
    Rooms(Vec<PublicRoom>, Option<String>),
    Spaces(Vec<PublicSpace>, Option<String>),
}

#[component]
pub fn DiscoveryModal(on_close: EventHandler<()>) -> Element {
    let client = use_context::<Rc<dyn VesperClient>>();
    let sync = use_context::<ClientState>();
    let mut tab = use_signal(|| DiscoveryTab::Rooms);
    let mut query = use_signal(String::new);
    let mut debounced = use_signal(String::new);
    // Manual refetch trigger (the error banner's Retry button).
    let mut reload = use_signal(|| 0u32);

    // Directory results for the active tab: accumulated pages + the token
    // for the next one. The inactive tab keeps its last results so
    // switching back doesn't refetch.
    let mut rooms = use_signal(Vec::<PublicRoom>::new);
    let mut rooms_next = use_signal(|| None::<String>);
    let mut spaces = use_signal(Vec::<PublicSpace>::new);
    let mut spaces_next = use_signal(|| None::<String>);
    // Starts `true`: the mount fetch fills the first page, so the empty
    // state must not flash before it lands.
    let mut loading = use_signal(|| true);
    let mut loading_more = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut join_rows = use_signal(HashMap::<String, JoinRow>::new);
    // Generation guard: only the newest fetch may write results (debounce,
    // tab switches, and retries can start several). Peeking (not reading)
    // keeps the fetch effect from subscribing to its own writes.
    let mut fetch_gen = use_signal(|| 0u64);

    // Debounce: coalesce rapid edits of `query` into one `debounced` write.
    // Stale timers no-op by re-checking the query they captured.
    // The web (wasm) build is a visual-debug target only — `Delay` panics
    // there (no `std::time::Instant` on this target), so the debounce
    // collapses to an immediate fire. Native (desktop/Android) debounces.
    use_effect(move || {
        let q = query();
        #[cfg(target_arch = "wasm32")]
        {
            if debounced() != q {
                debounced.set(q);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            spawn(async move {
                Delay::new(SEARCH_DEBOUNCE).await;
                if query() == q && debounced() != q {
                    debounced.set(q);
                }
            });
        }
    });

    // First page of the active tab whenever the debounced query, the tab,
    // or the reload counter changes.
    {
        let client = client.clone();
        use_effect(move || {
            let tab = tab();
            let q = debounced();
            let _ = reload();
            let gen = *fetch_gen.peek() + 1;
            fetch_gen.set(gen);
            error.set(None);
            loading.set(true);
            let client = client.clone();
            spawn(async move {
                let result = match tab {
                    DiscoveryTab::Rooms => client
                        .public_rooms(q, None)
                        .await
                        .map(|page| DirPage::Rooms(page.rooms, page.next)),
                    DiscoveryTab::Spaces => client
                        .public_spaces(q, None)
                        .await
                        .map(|page| DirPage::Spaces(page.spaces, page.next)),
                };
                if *fetch_gen.peek() != gen {
                    return; // a newer fetch owns the results now
                }
                loading.set(false);
                match result {
                    Ok(DirPage::Rooms(items, next)) => {
                        rooms.set(items);
                        rooms_next.set(next);
                    }
                    Ok(DirPage::Spaces(items, next)) => {
                        spaces.set(items);
                        spaces_next.set(next);
                    }
                    Err(e) => error.set(Some(e.0)),
                }
            });
        });
    }

    // Rooms this account already belongs to (convos + spaces): rows for
    // those start life as "Joined" unless a join attempt says otherwise.
    let joined_ids: HashMap<String, ()> = {
        let convos = sync.convos.read();
        let spaces = sync.spaces.read();
        convos
            .iter()
            .map(|c| c.id.clone())
            .chain(spaces.iter().map(|s| s.id.clone()))
            .map(|id| (id, ()))
            .collect()
    };

    let load_more = {
        let client = client.clone();
        move |_| {
            let tab = tab();
            let q = debounced();
            let token = match tab {
                DiscoveryTab::Rooms => rooms_next(),
                DiscoveryTab::Spaces => spaces_next(),
            };
            let Some(token) = token else { return };
            let gen = *fetch_gen.peek();
            loading_more.set(true);
            let client = client.clone();
            spawn(async move {
                let result = match tab {
                    DiscoveryTab::Rooms => client
                        .public_rooms(q, Some(token))
                        .await
                        .map(|page| DirPage::Rooms(page.rooms, page.next)),
                    DiscoveryTab::Spaces => client
                        .public_spaces(q, Some(token))
                        .await
                        .map(|page| DirPage::Spaces(page.spaces, page.next)),
                };
                loading_more.set(false);
                if *fetch_gen.peek() != gen {
                    return; // a fresh first-page fetch superseded this page
                }
                match result {
                    Ok(DirPage::Rooms(items, next)) => {
                        rooms.write().extend(items);
                        rooms_next.set(next);
                    }
                    Ok(DirPage::Spaces(items, next)) => {
                        spaces.write().extend(items);
                        spaces_next.set(next);
                    }
                    Err(e) => error.set(Some(e.0)),
                }
            });
        }
    };

    let join: EventHandler<String> = {
        let client = client.clone();
        EventHandler::new(move |id: String| {
            let client = client.clone();
            join_rows.write().insert(id.clone(), JoinRow::Joining);
            spawn(async move {
                let state = match client.join_room(&id).await {
                    Ok(()) => JoinRow::Joined,
                    Err(e) => JoinRow::Failed(e.0),
                };
                join_rows.write().insert(id, state);
            });
        })
    };

    let row_state = |id: &str| -> JoinRow {
        if let Some(state) = join_rows().get(id) {
            return state.clone();
        }
        if joined_ids.contains_key(id) {
            JoinRow::Joined
        } else {
            JoinRow::Idle
        }
    };

    let (is_rooms_tab, items_len, next) = match tab() {
        DiscoveryTab::Rooms => (true, rooms.read().len(), rooms_next.read().clone()),
        DiscoveryTab::Spaces => (false, spaces.read().len(), spaces_next.read().clone()),
    };

    rsx! {
        div {
            style: "position:absolute;inset:0;background:rgba(0,0,0,0.5);display:flex;align-items:center;justify-content:center;z-index:40;",
            onclick: move |_| on_close.call(()),
            div {
                onclick: move |evt| evt.stop_propagation(),
                style: "width:460px;max-height:70vh;display:flex;flex-direction:column;background:var(--bg-surface-raised);border-radius:var(--radius-lg);border:1px solid var(--border-default);box-shadow:var(--shadow-lg);font-family:var(--font-sans);",
                div { style: "padding:18px 20px 0;display:flex;align-items:center;justify-content:space-between;",
                    div { style: "font-size:17px;font-weight:700;", "Browse the network" }
                    button {
                        onclick: move |_| on_close.call(()),
                        style: "background:none;border:none;color:var(--text-secondary);cursor:pointer;display:flex;",
                        Icon { name: IconName::X, size: 18 }
                    }
                }
                div { style: "padding:10px 20px 0;",
                    Tabs {
                        tabs: vec![
                            TabItem { value: "rooms".into(), label: "Rooms".into() },
                            TabItem { value: "spaces".into(), label: "Spaces".into() },
                        ],
                        active: if tab() == DiscoveryTab::Rooms { "rooms".to_string() } else { "spaces".to_string() },
                        on_change: move |v: String| tab.set(if v == "rooms" { DiscoveryTab::Rooms } else { DiscoveryTab::Spaces }),
                    }
                }
                div { style: "padding:16px;",
                    Input {
                        placeholder: if is_rooms_tab { "Search rooms" } else { "Search spaces" },
                        value: query(),
                        on_change: move |v: String| query.set(v),
                    }
                }
                div { style: "overflow-y:auto;padding:0 16px 16px;display:flex;flex-direction:column;gap:8px;",
                    if let Some(error) = error() {
                        div { style: "display:flex;align-items:center;gap:8px;padding:10px 12px;border:1px solid var(--border-default);border-radius:var(--radius-md);color:var(--text-secondary);font-size:13px;",
                            Icon { name: IconName::ShieldAlert, size: 15, color: "var(--text-tertiary)".to_string() }
                            span { style: "flex:1;", "{error}" }
                            Button {
                                variant: ButtonVariant::Ghost,
                                size: ButtonSize::Sm,
                                onclick: move |_| reload += 1,
                                "Retry"
                            }
                        }
                    }
                    if loading() {
                        div { style: "padding:20px;text-align:center;color:var(--text-tertiary);font-size:13px;",
                            "Searching…"
                        }
                    } else if items_len == 0 && error().is_none() {
                        div { style: "padding:20px;text-align:center;color:var(--text-tertiary);font-size:13px;",
                            if is_rooms_tab { "No rooms found." } else { "No spaces found." }
                        }
                    } else if is_rooms_tab {
                        for item in rooms.read().iter() {
                            {directory_row(
                                IconName::Hash,
                                &item.id,
                                &item.name,
                                &format!("{} · {} members", topic_or_none(&item.topic), item.members),
                                row_state(&item.id),
                                join,
                            )}
                        }
                    } else {
                        for item in spaces.read().iter() {
                            {directory_row(
                                IconName::Layers,
                                &item.id,
                                &item.name,
                                &format!("{} · {} members", topic_or_none(&item.topic), item.members),
                                row_state(&item.id),
                                join,
                            )}
                        }
                    }
                    if !loading() && next.is_some() {
                        Button {
                            variant: ButtonVariant::Secondary,
                            size: ButtonSize::Sm,
                            disabled: loading_more(),
                            onclick: load_more,
                            if loading_more() { "Loading…" } else { "Show more" }
                        }
                    }
                }
            }
        }
    }
}

fn topic_or_none(topic: &str) -> &str {
    if topic.is_empty() {
        "No topic"
    } else {
        topic
    }
}

/// One directory row: icon, name, summary line, join button in its current
/// state (pending / joined / failed with inline error + retry).
fn directory_row(
    icon: IconName,
    id: &str,
    name: &str,
    summary: &str,
    state: JoinRow,
    on_join: EventHandler<String>,
) -> Element {
    let id = id.to_string();
    let error_message = match &state {
        JoinRow::Failed(message) => Some(message.clone()),
        _ => None,
    };
    rsx! {
        div {
            key: "{id}",
            style: "display:flex;flex-direction:column;gap:4px;padding:10px 12px;border:1px solid var(--border-subtle);border-radius:var(--radius-md);",
            div { style: "display:flex;align-items:center;gap:10px;",
                Icon { name: icon, size: 16, color: "var(--text-tertiary)".to_string() }
                div { style: "flex:1;min-width:0;",
                    div { style: "font-size:14px;font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", "{name}" }
                    div { style: "font-size:12px;color:var(--text-tertiary);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", "{summary}" }
                }
                match state {
                    JoinRow::Idle => rsx! {
                        Button {
                            variant: ButtonVariant::Primary,
                            size: ButtonSize::Sm,
                            onclick: move |_| on_join.call(id.clone()),
                            "Join"
                        }
                    },
                    JoinRow::Joining => rsx! {
                        Button { variant: ButtonVariant::Secondary, size: ButtonSize::Sm, disabled: true, "Joining…" }
                    },
                    JoinRow::Joined => rsx! {
                        Button { variant: ButtonVariant::Secondary, size: ButtonSize::Sm, disabled: true, "Joined" }
                    },
                    JoinRow::Failed(_) => rsx! {
                        Button {
                            variant: ButtonVariant::Primary,
                            size: ButtonSize::Sm,
                            onclick: move |_| on_join.call(id.clone()),
                            "Retry"
                        }
                    },
                }
            }
            if let Some(message) = error_message {
                div { style: "font-size:12px;color:var(--text-tertiary);padding-left:26px;", "{message}" }
            }
        }
    }
}
