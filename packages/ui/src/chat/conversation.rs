use std::rc::Rc;

use dioxus::prelude::*;

use super::composer::Composer;
use super::message_row::MessageRow;
use crate::data::{ClientState, Convo, ConvoKind, VesperClient};
use crate::design_system::{Tag, TagTone};
use crate::icons::{Icon, IconName};

#[component]
pub fn Conversation(
    convo: Convo,
    is_mobile: bool,
    #[props(default = None)] on_back: Option<EventHandler<()>>,
    on_start_call: EventHandler<bool>,
    on_open_thread: EventHandler<String>,
    on_open_profile: EventHandler<String>,
    on_open_room_info: EventHandler<()>,
    #[props(default = false)] hide_header: bool,
) -> Element {
    let client = use_context::<Rc<dyn VesperClient>>();
    let sync = use_context::<ClientState>();
    let convo_id = convo.id.clone();

    // Checkpoint 04: the real backend live-publishes this room's timeline into
    // `sync.messages[convo_id]`; open/close are refcounted so leaving the room
    // disposes the backend timeline task. The mock ignores both calls.
    use_effect({
        let client = client.clone();
        let convo_id = convo_id.clone();
        move || client.open_timeline(&convo_id)
    });
    use_drop({
        let client = client.clone();
        let convo_id = convo_id.clone();
        move || client.close_timeline(&convo_id)
    });

    // Read receipts (checkpoint 06): mark this room read on mount and again
    // whenever its live message list advances while the conversation is
    // mounted. The reactive read of `sync.messages[convo_id]` happens INSIDE
    // the effect (so it re-runs per publish); `mark_read` is fire-and-forget
    // and writes no signals, so there's no write-loop hazard.
    use_effect({
        let client = client.clone();
        let convo_id = convo_id.clone();
        move || {
            // Subscribe this effect to the live map for this room: a new
            // publish batch (our own echo or an incoming message) re-runs it.
            let _len = sync
                .messages
                .read()
                .get(&convo_id)
                .map(Vec::len)
                .unwrap_or(0);
            client.mark_read(&convo_id);
        }
    });

    // Snapshot path: used by the mock backend (which never publishes the live
    // map) and as first paint before the first timeline batch lands.
    let history = {
        let client = client.clone();
        let convo_id = convo_id.clone();
        use_resource(move || {
            let client = client.clone();
            let convo_id = convo_id.clone();
            async move { client.messages(&convo_id).await }
        })
    };

    let mut messages = use_signal(Vec::new);
    use_effect(move || {
        if let Some(list) = history() {
            messages.set(list);
        }
    });

    let mut replying_to = use_signal(|| None);
    let mut loading_older = use_signal(|| false);
    let mut anchored = use_signal(|| false);
    // Scroll management (checkpoint 05 follow-up): `at_bottom` lets new
    // content pin the viewport only when the user is already near the
    // newest message; `just_sent` forces a pin for our own echo; and the
    // prepend flag restores the reading offset after a back-pagination
    // page lands (otherwise prepended rows shove the viewport).
    let mut at_bottom = use_signal(|| true);
    let mut just_sent = use_signal(|| false);
    let mut prepend_capture = use_signal(|| false);

    // Back-pagination on scroll-to-top (checkpoint 04). Live rooms only — the
    // mock serves its whole history up front.
    let on_scroll = {
        let client = client.clone();
        let convo_id = convo_id.clone();
        move |evt: dioxus::html::ScrollEvent| {
            let room_below =
                evt.scroll_height() as f64 - evt.scroll_top() - evt.client_height() as f64;
            let pinned = room_below < 60.0;
            // Guard the write: caching same-value sets would still notify
            // subscribers (the scroll effect) on every scroll event.
            if at_bottom() != pinned {
                at_bottom.set(pinned);
            }
            if evt.scroll_top() > 200.0
                || loading_older()
                || !sync.messages.read().contains_key(&convo_id)
            {
                return;
            }
            loading_older.set(true);
            // Remember where the user is reading so the prepend restore can
            // put them back after the older rows land.
            prepend_capture.set(true);
            spawn(async {
                document::eval(
                    r#"
                    {
                        const el = document.querySelector('[data-vesper-chat]');
                        if (el) window.__vesperPrepend = { top: el.scrollTop, height: el.scrollHeight };
                    }
                    "#,
                );
            });
            let client = client.clone();
            let convo_id = convo_id.clone();
            spawn(async move {
                let added = client.load_older(&convo_id).await;
                loading_older.set(false);
                // Nothing prepended => the capture never gets consumed by a
                // publish. Drop it — otherwise the NEXT publish (e.g. our
                // own echo) restores the viewport against a stale snapshot,
                // visibly yanking the conversation around.
                if added.unwrap_or(0) == 0 {
                    prepend_capture.set(false);
                }
            });
        }
    };

    let send = {
        let client = client.clone();
        let convo_id = convo_id.clone();
        move |(text, attachment): (String, Option<crate::data::Attachment>)| {
            let reply_to = replying_to()
                .as_ref()
                .map(|m: &crate::data::Message| m.id.clone());
            // Pin the viewport to the bottom once the echo lands: without
            // this the send is invisible when the user is scrolled up.
            just_sent.set(true);
            // Live backend: the SDK local echo paints through the timeline
            // diff stream (docs/05); only the mock/snapshot path gets a
            // hand-rolled optimistic row.
            if !sync.messages.read().contains_key(&convo_id) {
                let optimistic = crate::data::Message {
                    id: format!("pending-{}", messages().len()),
                    from: "You".into(),
                    time: "now".into(),
                    mine: true,
                    system: false,
                    text: text.clone(),
                    reply_to: reply_to.clone(),
                    reactions: Vec::new(),
                    thread_count: 0,
                    attachment: attachment.clone(),
                    read_by: Vec::new(),
                    send_state: crate::data::SendState::Sent,
                    avatar: None,
                };
                messages.write().push(optimistic);
            }
            replying_to.set(None);

            let client = client.clone();
            let convo_id = convo_id.clone();
            spawn(async move {
                client
                    .send_message(&convo_id, text, attachment, reply_to)
                    .await;
            });
        }
    };

    let react = {
        let client = client.clone();
        let convo_id = convo_id.clone();
        move |(message_id, emoji): (String, String)| {
            // Live backend: the reaction local echo flows through the
            // timeline diffs (docs/05); the manual toggle below is for the
            // mock/snapshot path only.
            if !sync.messages.read().contains_key(&convo_id) {
                let mut target = messages();
                target.iter_mut().for_each(|m| {
                    if m.id != message_id {
                        return;
                    }
                    match m.reactions.iter().position(|r| r.emoji == emoji) {
                        Some(idx) => {
                            let r = &mut m.reactions[idx];
                            if r.me {
                                r.count -= 1;
                                r.me = false;
                            } else {
                                r.count += 1;
                                r.me = true;
                            }
                            if m.reactions[idx].count == 0 {
                                m.reactions.remove(idx);
                            }
                        }
                        None => m.reactions.push(crate::data::Reaction {
                            emoji: emoji.clone(),
                            count: 1,
                            me: true,
                        }),
                    }
                });
                messages.set(target);
            }

            let client = client.clone();
            let convo_id = convo_id.clone();
            spawn(async move {
                client.react(&convo_id, &message_id, &emoji).await;
            });
        }
    };

    // Retry / discard of a failed send (checkpoint 05). Mock: no-ops.
    let retry_send = {
        let client = client.clone();
        let convo_id = convo_id.clone();
        move |message_id: String| {
            let client = client.clone();
            let convo_id = convo_id.clone();
            spawn(async move {
                let _ = client.retry_send(&convo_id, &message_id).await;
            });
        }
    };
    let discard_send = {
        let client = client.clone();
        let convo_id = convo_id.clone();
        move |message_id: String| {
            let client = client.clone();
            let convo_id = convo_id.clone();
            // The mock keeps its manually-painted row; nothing to remove.
            spawn(async move {
                let _ = client.discard_send(&convo_id, &message_id).await;
            });
        }
    };

    // Attachment download (checkpoint 07): the save dialog must live on the
    // UI thread, the fetch+write goes to the backend. wasm has no rfd — the
    // button silently no-ops there for now (web is checkpoint 11).
    let download_attachment = {
        let client = client.clone();
        let convo_id = convo_id.clone();
        move |attachment: crate::data::Attachment| {
            let client = client.clone();
            let convo_id = convo_id.clone();
            let suggested = attachment.name.clone();
            // NEVER open an rfd modal inside the click handler itself: on
            // macOS the modal pumps the tao event loop, that nested pump
            // re-renders/diffs — while the handler's props-owner borrow is
            // still live below the dialog on the stack ("RefCell already
            // borrowed" → destructor panic, verified crash). Deferring past
            // the handler via spawn breaks the overlap.
            spawn(async move {
                #[cfg(not(target_arch = "wasm32"))]
                let dest = rfd::FileDialog::new()
                    .set_file_name(&suggested)
                    .save_file()
                    .map(|p| p.display().to_string());
                #[cfg(target_arch = "wasm32")]
                let dest = None::<String>;
                let _ = suggested; // suppress unused on wasm
                let Some(dest) = dest else { return };
                let _ = client.save_attachment(&convo_id, attachment, dest).await;
            });
        }
    };

    let placeholder = if convo.kind == ConvoKind::Room {
        format!("Message #{}", convo.name)
    } else {
        format!("Message {}", convo.name)
    };
    // Incoming typing label (checkpoint 06): resolved from the per-room typing
    // list the backend publishes. Empty → the row renders nothing.
    let typing_label = sync
        .typing
        .read()
        .get(&convo_id)
        .cloned()
        .filter(|t| !t.is_empty())
        .map(|typing| match typing.len() {
            1 => format!("{} is typing\u{2026}", typing[0]),
            2 => format!("{} and {} are typing\u{2026}", typing[0], typing[1]),
            _ => format!("{} people are typing\u{2026}", typing.len()),
        });
    let members_label = match convo.kind {
        ConvoKind::Room => format!("{} members", convo.members.unwrap_or(0)),
        ConvoKind::Dm => convo.mxid.clone().unwrap_or_default(),
    };
    // Anchor at the newest message on first paint: the initial backfill
    // loads a full page of history and without scrolling to the bottom the
    // room opens on the OLDEST loaded message.
    use_effect({
        let convo_id = convo_id.clone();
        move || {
            let has = sync
                .messages
                .read()
                .get(&convo_id)
                .is_some_and(|v| !v.is_empty())
                || !messages.read().is_empty();
            if !has || anchored() {
                return;
            }
            anchored.set(true);
            // Double rAF: run after Dioxus commits the DOM and the browser
            // has laid out the message list.
            document::eval(
                r#"
                requestAnimationFrame(() => requestAnimationFrame(() => {
                    const el = document.querySelector('[data-vesper-chat]');
                    if (el) el.scrollTop = el.scrollHeight;
                }));
                "#,
            );
        }
    });

    // Live map entry wins when the backend has opened this room's timeline;
    // otherwise the snapshot path (mock / first paint).
    #[allow(clippy::redundant_closure)] // Signal isn't FnOnce; closure is load-bearing
    let msgs = sync
        .messages
        .read()
        .get(&convo_id)
        .cloned()
        .unwrap_or_else(|| messages());

    // Content-driven scroll behavior. The reactive reads all happen INSIDE
    // this callback — that's what makes the effect re-run on each publish
    // batch. And every `.set` is guarded by a genuinely changing value: a
    // use_effect re-runs whenever a signal it read is written, even with an
    // equal value, so writing `just_sent` unconditionally would dirty its
    // own dependency forever and hang the app (learned the hard way).
    use_effect({
        let convo_id = convo_id.clone();
        move || {
            if prepend_capture() {
                prepend_capture.set(false);
                // Older rows prepended: keep the user's reading position by
                // shifting scrollTop by the added height. Height unchanged
                // (start of timeline / no new page) => drop the capture.
                document::eval(
                    r#"
                    requestAnimationFrame(() => requestAnimationFrame(() => {
                        const el = document.querySelector('[data-vesper-chat]');
                        if (el && window.__vesperPrepend) {
                            if (el.scrollHeight > window.__vesperPrepend.height) {
                                el.scrollTop = window.__vesperPrepend.top + (el.scrollHeight - window.__vesperPrepend.height);
                            }
                            window.__vesperPrepend = null;
                        }
                    }));
                    "#,
                );
                return;
            }
            // Recompute the newest row here (subscribes the effect to the
            // live map) so a pin fires when our own echo actually lands.
            let latest_is_pending_mine = sync
                .messages
                .read()
                .get(&convo_id)
                .and_then(|list| list.last())
                .is_some_and(|m| m.mine && m.send_state != crate::data::SendState::Sent);
            let mut should_pin = at_bottom() || latest_is_pending_mine;
            if just_sent() {
                just_sent.set(false);
                should_pin = true;
            }
            if should_pin {
                // Commit-driven pin (not fixed-time timeouts, which guess at
                // layout/reflow latency): pin now, then re-pin on every DOM
                // change via a MutationObserver, and disconnect 700ms after
                // the tree goes quiet — with a 4s hard cap. Any previous pin
                // observer is dropped so sends don't stack them.
                document::eval(
                    r#"
                    (() => {
                        const el = document.querySelector('[data-vesper-chat]');
                        if (!el) return;
                        const pin = () => { el.scrollTop = el.scrollHeight; };
                        if (window.__vesperPin) {
                            window.__vesperPin.obs.disconnect();
                            clearTimeout(window.__vesperPin.quiet);
                            clearTimeout(window.__vesperPin.hard);
                        }
                        const holder = {};
                        const renew = () => {
                            clearTimeout(holder.quiet);
                            holder.quiet = setTimeout(() => {
                                holder.obs.disconnect();
                                clearTimeout(holder.hard);
                                if (window.__vesperPin === holder) window.__vesperPin = null;
                            }, 700);
                        };
                        holder.obs = new MutationObserver(() => { pin(); renew(); });
                        holder.hard = setTimeout(() => {
                            holder.obs.disconnect();
                            clearTimeout(holder.quiet);
                            if (window.__vesperPin === holder) window.__vesperPin = null;
                        }, 4000);
                        window.__vesperPin = holder;
                        holder.obs.observe(el, { childList: true, subtree: true, characterData: true });
                        pin();
                        renew();
                    })();
                    "#,
                );
            }
        }
    });

    rsx! {
        // flex:1 + min-height:0 (NOT height:100%): this root sits BELOW the
        // focus header in a column flex, so height:100% would size it to the
        // full parent and push the composer off-screen once the message list
        // overflows. min-height:0 lets it shrink to the remaining space.
        div { style: "flex:1;display:flex;flex-direction:column;min-width:0;min-height:0;",
            if !hide_header {
                div { style: "height:56px;border-bottom:1px solid var(--border-subtle);display:flex;align-items:center;justify-content:space-between;padding:0 16px;flex-shrink:0;gap:8px;",
                    div { style: "display:flex;align-items:center;gap:8px;min-width:0;",
                        if is_mobile {
                            button {
                                onclick: move |_| { if let Some(h) = &on_back { h.call(()); } },
                                style: "background:none;border:none;color:var(--text-secondary);cursor:pointer;display:flex;",
                                Icon { name: IconName::ArrowLeft, size: 19 }
                            }
                        }
                        button {
                            onclick: move |_| on_open_room_info.call(()),
                            style: "background:none;border:none;text-align:left;cursor:pointer;min-width:0;",
                            div { style: "font-weight:700;font-size:15px;display:flex;align-items:center;gap:4px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;",
                                if convo.kind == ConvoKind::Room {
                                    Icon { name: IconName::Hash, size: 13, color: "var(--text-tertiary)".to_string() }
                                }
                                "{convo.name}"
                            }
                            div { style: "font-size:12px;color:var(--text-tertiary);font-family:var(--font-mono);", "{members_label}" }
                        }
                    }
                    div { style: "display:flex;gap:4px;align-items:center;flex-shrink:0;",
                        if convo.encrypted {
                            Tag { tone: TagTone::Brand, "e2ee" }
                        }
                        button {
                            title: "Voice call",
                            onclick: move |_| on_start_call.call(false),
                            style: "width:36px;height:36px;border-radius:var(--radius-md);border:none;background:transparent;color:var(--text-secondary);cursor:pointer;display:flex;align-items:center;justify-content:center;",
                            Icon { name: IconName::Phone, size: 16 }
                        }
                        button {
                            title: "Video call",
                            onclick: move |_| on_start_call.call(true),
                            style: "width:36px;height:36px;border-radius:var(--radius-md);border:none;background:transparent;color:var(--text-secondary);cursor:pointer;display:flex;align-items:center;justify-content:center;",
                            Icon { name: IconName::Video, size: 16 }
                        }
                        button {
                            title: "Room info",
                            onclick: move |_| on_open_room_info.call(()),
                            style: "width:36px;height:36px;border-radius:var(--radius-md);border:none;background:transparent;color:var(--text-secondary);cursor:pointer;display:flex;align-items:center;justify-content:center;",
                            Icon { name: IconName::Info, size: 16 }
                        }
                        button {
                            title: "Search",
                            style: "width:36px;height:36px;border-radius:var(--radius-md);border:none;background:transparent;color:var(--text-secondary);cursor:pointer;display:flex;align-items:center;justify-content:center;",
                            Icon { name: IconName::Search, size: 16 }
                        }
                    }
                }
            }
            // min-height:0 is load-bearing: without it a flex child defaulting
            // to min-height:auto grows past the conversation height and pushes
            // the composer off-screen once the history fills a page.
            // overflow-anchor:none: the browser's own scroll anchoring
            // otherwise "holds" the viewport to the previous content when a
            // row appends, fighting our bottom pins (the visible snap-back).
            div { style: "flex:1;min-height:0;overflow-y:auto;padding:20px 24px;display:flex;flex-direction:column;gap:16px;overflow-anchor:none;", onscroll: on_scroll,
                "data-vesper-chat": true,
                if loading_older() {
                    div { style: "text-align:center;font-size:12px;color:var(--text-tertiary);font-family:var(--font-mono);padding:4px 0;", "Loading older…" }
                }
                for m in msgs.iter() {
                    MessageRow {
                        key: "{m.id}",
                        m: m.clone(),
                        all_messages: msgs.clone(),
                        on_react: react.clone(),
                        on_reply: move |m| replying_to.set(Some(m)),
                        on_retry_send: retry_send.clone(),
                        on_discard_send: discard_send.clone(),
                        on_open_thread,
                        on_open_profile,
                        on_download: download_attachment.clone(),
                    }
                }
            }
            // Incoming typing row (checkpoint 06): hidden when nobody is
            // typing. `typing_label` is computed in the body from the live
            // `sync.typing` map.
            if let Some(label) = &typing_label {
                div { style: "padding:2px 24px 0;font-size:12px;color:var(--text-secondary);font-style:italic;min-height:18px;",
                    "{label}"
                }
            }
            Composer {
                on_send: send,
                replying_to: replying_to(),
                on_cancel_reply: move |_| replying_to.set(None),
                on_typing: move |typing| client.set_typing(&convo_id, typing),
                placeholder,
            }
        }
    }
}
