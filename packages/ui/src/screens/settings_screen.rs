use std::rc::Rc;

use dioxus::prelude::*;

use crate::chat::VerifyDialog;
use crate::data::{
    ClientState, Device, Me, NotifToggle, Prefs, VerificationAction, VerificationTarget,
    VesperClient,
};
use crate::design_system::{
    Avatar, Button, ButtonSize, ButtonVariant, Dialog, Input, SelectOption, SidebarNav,
    SidebarNavItem, Switch,
};
use crate::icons::{Icon, IconName};
use crate::window_chrome::{DragStrip, WindowControls};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Notifications,
    Security,
}

impl SettingsTab {
    fn value(self) -> &'static str {
        match self {
            SettingsTab::General => "general",
            SettingsTab::Notifications => "notifications",
            SettingsTab::Security => "security",
        }
    }
    fn label(self) -> &'static str {
        match self {
            SettingsTab::General => "General",
            SettingsTab::Notifications => "Notifications",
            SettingsTab::Security => "Security",
        }
    }
    fn from_value(v: &str) -> Self {
        match v {
            "notifications" => SettingsTab::Notifications,
            "security" => SettingsTab::Security,
            _ => SettingsTab::General,
        }
    }
}

const TABS: [SettingsTab; 3] = [
    SettingsTab::General,
    SettingsTab::Notifications,
    SettingsTab::Security,
];

/// What the Security tab's dialog is asking for, if anything. One modal at
/// a time (same policy as the verify dialog).
#[derive(Clone, PartialEq)]
enum SecurityDialog {
    /// Rename `device_id` (pre-filled with `current` name).
    Rename { device_id: String, name: String },
    /// Delete `device_id`; needs the account password (UIAA).
    Delete { device_id: String },
}

#[component]
pub fn SettingsScreen(
    on_close: EventHandler<()>,
    #[props(default = false)] is_mobile: bool,
) -> Element {
    #[allow(unused_mut)]
    let mut is_mobile = is_mobile;
    // `cfg!` in app.rs only covers mobile binary targets; a web build opened
    // on a phone reports falsely. Probe the user agent at runtime (eval is a
    // no-op capable API on all webview targets); UA plumbing avoids
    // misclassifying small desktop windows.
    if !is_mobile {
        let mut probed = use_signal(|| false);
        use_effect(move || {
            spawn(async move {
                let js = "return /Android|iPhone|iPad|iPod/i.test(navigator.userAgent) \
                     || navigator.userAgentData?.mobile === true;";
                if let Ok(true) = document::eval(js).recv::<bool>().await {
                    probed.set(true);
                }
            });
        });
        if probed() {
            is_mobile = true;
        }
    }

    let client = use_context::<Rc<dyn VesperClient>>();
    let mut tab = use_signal(|| SettingsTab::General);
    let mut verify_id = use_signal(|| Option::<String>::None);
    let sync = use_context::<ClientState>();

    // Account identity. Read from the root signal (source of truth) with the
    // one-shot `me()` as the bootstrap fallback.
    let me_resource = {
        let client = client.clone();
        use_resource(move || {
            let client = client.clone();
            async move { client.me().await }
        })
    };
    let identity = use_context::<Signal<Option<Me>>>();
    let me = identity().or_else(|| me_resource().flatten());
    let (me_name, me_id, me_avatar) = match &me {
        Some(m) => (m.name.clone(), m.id.clone(), m.avatar.clone()),
        None => (String::new(), String::new(), None),
    };

    // Display-name editor state (General tab). Seeded once from the current
    // name at first render — never rewritten after that, so clearing the
    // field to retype works (an empty draft stays empty until typed).
    let mut name_draft = use_signal(|| me_name.clone());
    let mut name_saving = use_signal(|| false);
    let mut name_error = use_signal(|| None::<String>);
    let name_dirty = name_draft() != me_name;

    // Local prefs (theme + the receipt/typing opt-outs). One resource, one
    // editable copy; saves rewrite the resource's backing file.
    let mut prefs = use_signal(Prefs::default);
    let prefs_resource = {
        let client = client.clone();
        use_resource(move || {
            let client = client.clone();
            async move { client.prefs().await }
        })
    };
    use_effect(move || {
        // Copy resource→local whenever the fetch lands. The effect must NOT
        // read `prefs` itself: local edits (switches, theme select) write it,
        // and an effect that reads the signal it writes re-runs on every edit
        // and stomps the edit with the stale fetched value (checkpoint-06
        // effect-loop lesson; bit the notif toggles in review). Unconditional
        // copy on resource change is safe: the effect only re-runs when the
        // resource state changes, never on local writes.
        if let Some(loaded) = prefs_resource() {
            prefs.set(loaded);
        }
    });

    // Media cache size + clear + copy diagnostics (checkpoint 11 §C/§D).
    let mut cache_bytes = use_signal(|| Option::<u64>::None);
    let mut cache_clearing = use_signal(|| false);
    let cache_resource = {
        let client = client.clone();
        use_resource(move || {
            let client = client.clone();
            async move { client.media_cache_bytes().await }
        })
    };
    use_effect(move || {
        if let Some(bytes) = cache_resource() {
            cache_bytes.set(Some(bytes));
        }
    });

    // Copy diagnostics: collects the redacted payload (client crate) and
    // puts it on the clipboard via the webview's async clipboard API —
    // dioxus-desktop 0.7 has no native clipboard writer, and the button
    // click is the user gesture WKWebView requires to allow it.
    let copy_diagnostics = move |_| {
        spawn(async move {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let payload = client::diagnostics::collect();
                let js = format!(
                    "navigator.clipboard.writeText({}).then(() => dioxus.send(true), () => dioxus.send(false))",
                    js_string_literal(&payload)
                );
                match document::eval(&js).recv::<bool>().await {
                    Ok(true) => use_context::<crate::design_system::ToastCenter>().success(
                        "Diagnostics copied",
                        Some("Paste it into your issue report.".into()),
                    ),
                    _ => use_context::<crate::design_system::ToastCenter>().info(
                        "Could not copy",
                        Some("Your clipboard denied access.".into()),
                    ),
                }
            }
        });
    };

    // Clear the on-disk media cache; shows the freed amount, refreshes the
    // size row. In-memory data URIs stay (they're the live rows' pixels).
    let clear_cache = {
        let client = client.clone();
        move |_| {
            let client = client.clone();
            if cache_clearing() {
                return;
            }
            cache_clearing.set(true);
            spawn(async move {
                match client.clear_media_cache().await {
                    Ok(freed) => {
                        use_context::<crate::design_system::ToastCenter>().success(
                            "Media cache cleared",
                            Some(format!("Freed {}.", client_bytes_label(freed))),
                        );
                    }
                    Err(e) => {
                        use_context::<crate::design_system::ToastCenter>().error(&e);
                    }
                }
                cache_bytes.set(Some(client.media_cache_bytes().await));
                cache_clearing.set(false);
            });
        }
    };

    // Server push-rule toggles (Notifications tab). Failures are toasts
    // (checkpoint 11); the toggle rows re-render from refetched truth.
    let mut notif = use_signal(Vec::<NotifToggle>::new);
    let notif_resource = {
        let client = client.clone();
        use_resource(move || {
            let client = client.clone();
            async move { client.notification_rules().await }
        })
    };
    use_effect(move || {
        // Same copy-on-resource-change pattern as prefs above: never read
        // `notif` here, or optimistic toggle writes get reverted by the
        // stale resource value one effect-run later (the "flicks back on"
        // bug). Re-runs only when the resource changes.
        if let Some(Ok(list)) = notif_resource() {
            notif.set(list);
        }
    });

    // Sessions (Security tab). Reading the verification signal inside the
    // future re-subscribes the resource: when a session completes
    // (Done/Cancelled) the device list refetches and the just-verified badge
    // appears without a manual refresh.
    let devices_resource = {
        let client = client.clone();
        use_resource(move || {
            let client = client.clone();
            let session_state = sync.verification.read().as_ref().map(|s| s.state.clone());
            async move {
                let _ = &session_state;
                client.devices().await
            }
        })
    };
    let mut devices = use_signal(Vec::<Device>::new);
    use_effect(move || {
        if let Some(list) = devices_resource() {
            devices.set(list);
        }
    });

    // Verify button starts a backend session; the dialog renders whatever
    // the backend publishes into `sync.verification`.
    let start_verify = {
        let client = client.clone();
        move |id: String| {
            let client = client.clone();
            spawn(async move {
                client.start_verification(VerificationTarget::Device(id));
            });
        }
    };

    // Security-tab modal state (rename / delete-with-password).
    let mut dialog = use_signal(|| Option::<SecurityDialog>::None);
    let mut dialog_field = use_signal(String::new);
    let mut dialog_error = use_signal(|| Option::<String>::None);
    let mut dialog_busy = use_signal(|| false);

    // Save the display name: optimistic busy state, identity signal rewrite
    // on success so the whole shell repaints.
    let save_name = {
        let client = client.clone();
        move |name: String| {
            let client = client.clone();
            spawn(async move {
                name_saving.set(true);
                name_error.set(None);
                match client.set_display_name(name).await {
                    Ok(fresh) => {
                        use_context::<Signal<Option<Me>>>().set(Some(fresh));
                    }
                    Err(e) => name_error.set(Some(e.message)),
                }
                name_saving.set(false);
            });
        }
    };

    // Avatar upload: the file dialog runs inside a spawned task (never in
    // the event callback — macOS nested-pump crash, see docs/07 notes).
    // Failures surface as toasts (checkpoint 11): there is no inline field
    // next to the avatar button to hold them. rfd is desktop-only (no
    // Android backend; wasm is web checkpoint 11) — no-op elsewhere.
    let change_avatar = {
        let client = client.clone();
        move |_| {
            let client = client.clone();
            spawn(async move {
                #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
                {
                    let Some(path) = rfd::FileDialog::new()
                        .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif"])
                        .set_title("Choose an avatar")
                        .pick_file()
                    else {
                        return;
                    };
                    match client.set_avatar(path.display().to_string()).await {
                        Ok(fresh) => {
                            use_context::<Signal<Option<Me>>>().set(Some(fresh));
                        }
                        Err(e) => use_context::<crate::design_system::ToastCenter>().error(&e),
                    }
                }
                #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
                let _ = client; // no dialog backend on this platform
            });
        }
    };

    // Clone for the logout handler without moving `client`.
    let logout_client = client.clone();

    rsx! {
        div { style: "flex:1;display:flex;min-width:0;flex-direction:column;height:100%;",
            div {
                style: "height:56px;border-bottom:1px solid var(--border-subtle);display:flex;align-items:center;padding:0 20px;gap:10px;flex-shrink:0;",
                button {
                    onclick: move |_| on_close.call(()),
                    style: "background:none;border:none;color:var(--text-secondary);cursor:pointer;display:flex;",
                    Icon { name: IconName::ArrowLeft, size: 19 }
                }
                span { style: "font-weight:700;", "Settings" }
                DragStrip {}
                WindowControls {}
            }
            div { style: "flex:1;display:flex;min-width:0;overflow:hidden;",
                if !is_mobile {
                    div { style: "width:200px;border-right:1px solid var(--border-subtle);padding:16px;flex-shrink:0;",
                        SidebarNav {
                            active: tab().value().to_string(),
                            on_change: move |v: String| tab.set(SettingsTab::from_value(&v)),
                            items: TABS.iter().map(|t| SidebarNavItem { value: t.value().to_string(), label: t.label().to_string(), icon: None }).collect::<Vec<_>>(),
                        }
                    }
                }
                div { style: "flex:1;padding:28px;overflow-y:auto;",
                    div { style: "display:flex;flex-direction:column;gap:20px;max-width:480px;",
                        if is_mobile {
                            // Class-based tabs, not dynamic inline styles:
                            // dioxus 0.7 style re-patching mangles var()
                            // shorthands on re-render (white boxes).
                            crate::design_system::Tabs {
                                active: tab().value().to_string(),
                                on_change: move |v: String| tab.set(SettingsTab::from_value(&v)),
                                tabs: TABS.iter().map(|t| crate::design_system::TabItem { value: t.value().to_string(), label: t.label().to_string() }).collect::<Vec<_>>(),
                            }
                        }
                        if tab() == SettingsTab::General {
                            div { style: "display:flex;align-items:center;gap:12px;",
                                Avatar { name: me_name.clone(), size: 56, mxc: me_avatar.clone() }
                                div {
                                    div { style: "font-weight:700;font-size:16px;", "{me_name}" }
                                    div { style: "font-size:12px;color:var(--text-tertiary);font-family:var(--font-mono);", "{me_id}" }
                                }
                                Button { variant: ButtonVariant::Secondary, size: ButtonSize::Sm, onclick: change_avatar, "Change avatar" }
                            }
                            Input {
                                label: "Display name".to_string(),
                                value: name_draft(),
                                on_change: move |v: String| name_draft.set(v),
                                error: name_error(),
                            }
                            div { style: "display:flex;justify-content:flex-end;",
                                Button {
                                    variant: ButtonVariant::Primary,
                                    size: ButtonSize::Sm,
                                    disabled: name_saving()
                                        || !name_dirty
                                        || name_draft().trim().is_empty(),
                                    onclick: move |_| {
                                        let name = name_draft();
                                        save_name(name);
                                    },
                                    if name_saving() { "Saving…" } else { "Save name" }
                                }
                            }
                            crate::design_system::Select {
                                label: "Theme",
                                value: prefs().theme.clone(),
                                on_change: {
                                    let client = client.clone();
                                    move |theme: String| {
                                        let client = client.clone();
                                        let mut next = prefs();
                                        next.theme = theme;
                                        prefs.set(next.clone());
                                        spawn(async move {
                                            if let Err(e) = client.set_prefs(next).await {
                                                tracing::warn!("prefs save failed: {e}");
                                            }
                                        });
                                    }
                                },
                                options: vec![
                                    SelectOption { value: "dark".into(), label: "Dark".into() },
                                    SelectOption { value: "light".into(), label: "Light".into() },
                                ],
                            }
                            // Storage + support (checkpoint 11): media cache
                            // usage/clear and the copy-diagnostics button.
                            div { style: "font-size:13px;font-weight:700;letter-spacing:0.04em;color:var(--text-tertiary);", "STORAGE" }
                            div { style: "display:flex;align-items:center;justify-content:space-between;gap:10px;",
                                div {
                                    div { style: "font-size:14px;", "Cached media" }
                                    div { style: "font-size:12px;color:var(--text-tertiary);", "{cache_size_label(cache_bytes())}" }
                                }
                                Button {
                                    variant: ButtonVariant::Secondary,
                                    size: ButtonSize::Sm,
                                    disabled: cache_clearing(),
                                    onclick: clear_cache,
                                    if cache_clearing() { "Clearing…" } else { "Clear" }
                                }
                            }
                            div { style: "font-size:13px;font-weight:700;letter-spacing:0.04em;color:var(--text-tertiary);margin-top:8px;", "SUPPORT" }
                            div { style: "display:flex;align-items:center;justify-content:space-between;gap:10px;",
                                div {
                                    div { style: "font-size:14px;", "Diagnostics" }
                                    div { style: "font-size:12px;color:var(--text-tertiary);", "App info + a redacted tail of the log file" }
                                }
                                Button {
                                    variant: ButtonVariant::Secondary,
                                    size: ButtonSize::Sm,
                                    onclick: copy_diagnostics,
                                    "Copy"
                                }
                            }
                                                        div {
                                Button {
                                    variant: ButtonVariant::Danger,
                                    size: ButtonSize::Sm,
                                    onclick: move |_| {
                                        let client = logout_client.clone();
                                        spawn(async move {
                                            if let Err(e) = client.logout().await {
                                                tracing::warn!("logout failed: {e}");
                                                use_context::<crate::design_system::ToastCenter>().error(&e);
                                            }
                                            use_context::<Signal<Option<Me>>>().set(None);
                                        });
                                    },
                                    Icon { name: IconName::LogOut, size: 14 }
                                    " Sign out"
                                }
                            }
                        }
                        if tab() == SettingsTab::Notifications {
                            for toggle in notif().iter() {
                                {
                                    let id = toggle.id.clone();
                                    let enabled = toggle.enabled;
                                    let client = client.clone();
                                    rsx! {
                                        div { key: "{id}", style: "display:flex;align-items:center;justify-content:space-between;gap:10px;",
                                            Switch {
                                                label: toggle.label.clone(),
                                                checked: enabled,
                                                on_change: move |_| {
                                                    let client = client.clone();
                                                    // Optimistic flip; the result
                                                    // list replaces state wholesale.
                                                    let mut optimistic = notif();
                                                    if let Some(t) = optimistic.iter_mut().find(|t| t.id == id) {
                                                        t.enabled = !t.enabled;
                                                    }
                                                    notif.set(optimistic);
                                                    let tid = id.clone();
                                                    let next = !enabled;
                                                    spawn(async move {
                                                        match client.set_notification_rule(tid, next).await {
                                                            Ok(list) => notif.set(list),
                                                            Err(e) => {
                                                                // Toast (checkpoint 11):
                                                                // the toggle itself
                                                                // re-renders from truth.
                                                                use_context::<crate::design_system::ToastCenter>().error(&e);
                                                                // Refetch to restore truth.
                                                                if let Ok(list) = client.notification_rules().await {
                                                                    notif.set(list);
                                                                }
                                                            }
                                                        }
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            div { style: "font-size:13px;font-weight:700;letter-spacing:0.04em;color:var(--text-tertiary);margin-top:8px;", "PRIVACY" }
                            Switch {
                                label: "Read receipts".to_string(),
                                checked: prefs().read_receipts,
                                on_change: {
                                    let client = client.clone();
                                    move |_| {
                                        let client = client.clone();
                                        let mut next = prefs();
                                        next.read_receipts = !next.read_receipts;
                                        prefs.set(next.clone());
                                        spawn(async move {
                                            if let Err(e) = client.set_prefs(next).await {
                                                tracing::warn!("prefs save failed: {e}");
                                            }
                                        });
                                    }
                                }
                            }
                            Switch {
                                label: "Typing indicators".to_string(),
                                checked: prefs().typing_indicators,
                                on_change: {
                                    let client = client.clone();
                                    move |_| {
                                        let client = client.clone();
                                        let mut next = prefs();
                                        next.typing_indicators = !next.typing_indicators;
                                        prefs.set(next.clone());
                                        spawn(async move {
                                            if let Err(e) = client.set_prefs(next).await {
                                                tracing::warn!("prefs save failed: {e}");
                                            }
                                        });
                                    }
                                }
                            }
                        }
                        if tab() == SettingsTab::Security {
                            div { style: "font-size:13px;font-weight:700;letter-spacing:0.04em;color:var(--text-tertiary);", "SESSIONS" }
                            for d in devices().iter() {
                                {
                                    let id = d.id.clone();
                                    let start_verify = start_verify.clone();
                                    let open_rename = {
                                        let name = d.name.clone();
                                        let id = id.clone();
                                        move |_| {
                                            dialog.set(Some(SecurityDialog::Rename { device_id: id.clone(), name: name.clone() }));
                                            dialog_field.set(name.clone());
                                            dialog_error.set(None);
                                        }
                                    };
                                    let open_delete = {
                                        let id = id.clone();
                                        move |_| {
                                            dialog.set(Some(SecurityDialog::Delete { device_id: id.clone() }));
                                            dialog_field.set(String::new());
                                            dialog_error.set(None);
                                        }
                                    };
                                    rsx! {
                                        div { key: "{d.id}", style: "display:flex;align-items:center;gap:10px;padding:10px 12px;background:var(--bg-surface);border:1px solid var(--border-subtle);border-radius:var(--radius-md);",
                                            Icon {
                                                name: if d.verified { IconName::ShieldCheck } else { IconName::ShieldAlert },
                                                size: 18,
                                                color: if d.verified { "var(--status-online)".to_string() } else { "var(--status-away)".to_string() },
                                            }
                                            div { style: "flex:1;",
                                                div { style: "font-size:14px;font-weight:600;display:flex;gap:6px;align-items:center;",
                                                    "{d.name}"
                                                    if d.current {
                                                        span { style: "font-size:11px;font-weight:700;color:var(--text-brand);", "THIS DEVICE" }
                                                    }
                                                }
                                                div { style: "font-size:12px;color:var(--text-tertiary);", "{d.last_seen}" }
                                            }
                                            if !d.verified {
                                                Button { variant: ButtonVariant::Secondary, size: ButtonSize::Sm, onclick: {
                                                    let id = id.clone();
                                                    move |_| {
                                                        verify_id.set(Some(id.clone()));
                                                        start_verify(id.clone());
                                                    }
                                                }, "Verify" }
                                            }
                                            Button { variant: ButtonVariant::Secondary, size: ButtonSize::Sm, onclick: open_rename, "Rename" }
                                            if d.current {
                                                span { style: "font-size:11px;color:var(--text-tertiary);", "Use Sign out" }
                                            } else {
                                                Button { variant: ButtonVariant::Danger, size: ButtonSize::Sm, onclick: open_delete, "Delete" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            VerifyDialog {
                open: verify_id().is_some(),
                on_close: {
                    let client = client.clone();
                    move |_| {
                        verify_id.set(None);
                        let client = client.clone();
                        spawn(async move {
                            client.verification_action(VerificationAction::Cancel);
                        });
                    }
                },
                on_action: {
                    let client = client.clone();
                    move |action: VerificationAction| {
                        let client = client.clone();
                        spawn(async move {
                            client.verification_action(action);
                        });
                    }
                },
            }

            // Rename / delete-session modal (checkpoint 10). Reuses the
            // dialog shell; the field is a name or the account password.
            {dialog().map(|current| {
                let (title, field_label, field_kind, confirm_label, danger) = match &current {
                    SecurityDialog::Rename { .. } => {
                        ("Rename session".to_string(), "Name".to_string(), "text".to_string(), "Rename".to_string(), false)
                    }
                    SecurityDialog::Delete { .. } => {
                        ("Delete session".to_string(), "Account password".to_string(), "password".to_string(), "Delete session".to_string(), true)
                    }
                };
                let device_id = match &current {
                    SecurityDialog::Rename { device_id, .. } | SecurityDialog::Delete { device_id } => device_id.clone(),
                };
                let confirm_variant = if danger { ButtonVariant::Danger } else { ButtonVariant::Primary };
                rsx! {
                    Dialog {
                        title,
                        open: true,
                        onclose: move |_| dialog.set(None),
                        actions: rsx! {
                            Button { variant: ButtonVariant::Secondary, size: ButtonSize::Sm, onclick: move |_| dialog.set(None), "Cancel" }
                            Button {
                                variant: confirm_variant,
                                size: ButtonSize::Sm,
                                disabled: dialog_busy() || dialog_field().trim().is_empty(),
                                onclick: {
                                    let client = client.clone();
                                    let current = current.clone();
                                    let device_id = device_id.clone();
                                    move |_| {
                                        let client = client.clone();
                                        let value = dialog_field();
                                        let device_id = device_id.clone();
                                        let action = current.clone();
                                        dialog_busy.set(true);
                                        dialog_error.set(None);
                                        spawn(async move {
                                            let result = match action {
                                                SecurityDialog::Rename { .. } => {
                                                    client.rename_device(device_id, value).await
                                                }
                                                SecurityDialog::Delete { .. } => {
                                                    client.delete_device(device_id, value).await
                                                }
                                            };
                                            match result {
                                                Ok(()) => {
                                                    dialog.set(None);
                                                    // Refetch so the row reflects the change.
                                                    let list = client.devices().await;
                                                    devices.set(list);
                                                }
                                                Err(e) => dialog_error.set(Some(e.message)),
                                            }
                                            dialog_busy.set(false);
                                        });
                                    }
                                },
                                if dialog_busy() { "Working…" } else { "{confirm_label}" }
                            }
                        },
                        if matches!(current, SecurityDialog::Delete { .. }) {
                            div { style: "margin-bottom:14px;", "Deleting a session signs it out everywhere. This cannot be undone." }
                        }
                        Input {
                            label: field_label,
                            input_type: field_kind,
                            value: dialog_field(),
                            on_change: move |v: String| dialog_field.set(v),
                            error: dialog_error(),
                        }
                    }
                }
            })}
        }
    }
}

/// Human label for the media cache size row.
fn cache_size_label(bytes: Option<u64>) -> String {
    match bytes {
        Some(bytes) => format!(
            "{} on disk (auto-limited to 500 MB)",
            client_bytes_label(bytes)
        ),
        None => "Unknown".into(),
    }
}

/// Compact byte formatter shared by the cache rows/toasts.
fn client_bytes_label(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1_024 {
        format!("{} KB", bytes.div_ceil(1_024))
    } else {
        format!("{bytes} B")
    }
}

/// Escape a Rust string into a JS string literal (no serde_json dep in
/// this crate; the payload is plain text with newlines and quotes).
fn js_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
