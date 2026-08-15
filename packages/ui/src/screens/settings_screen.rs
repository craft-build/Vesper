use std::rc::Rc;

use dioxus::prelude::*;

use crate::chat::VerifyDialog;
use crate::data::{ClientState, Device, Me, VerificationAction, VerificationTarget, VesperClient};
use crate::design_system::{
    Avatar, Button, ButtonSize, ButtonVariant, SelectOption, SidebarNav, SidebarNavItem, Switch,
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

#[component]
pub fn SettingsScreen(
    on_close: EventHandler<()>,
    #[props(default = false)] is_mobile: bool,
) -> Element {
    let client = use_context::<Rc<dyn VesperClient>>();
    let mut notif = use_signal(|| true);
    let mut receipts = use_signal(|| true);
    let mut typing_indicators = use_signal(|| true);
    let mut tab = use_signal(|| SettingsTab::General);
    let mut verify_id = use_signal(|| Option::<String>::None);
    let sync = use_context::<ClientState>();

    let me = {
        let client = client.clone();
        use_resource(move || {
            let client = client.clone();
            async move { client.me().await }
        })
    };
    let devices_resource = {
        let client = client.clone();
        // Reading the verification signal inside the future re-subscribes the
        // resource: when a session completes (Done/Cancelled) the device list
        // refetches and the just-verified badge appears without a manual
        // refresh.
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
    // the backend publishes into `sync.verification` (mock: instant emojis;
    // matrix: Requested → EmojisShown as the other side accepts).
    let start_verify = {
        let client = client.clone();
        move |id: String| {
            let client = client.clone();
            spawn(async move {
                client.start_verification(VerificationTarget::Device(id));
            });
        }
    };

    // Clone for the logout handler without moving `client` (still used below).
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
                            div { style: "display:flex;gap:8px;",
                                for t in TABS.iter() {
                                    {
                                        let is_active = tab() == *t;
                                        let bg = if is_active { "var(--bg-selected)" } else { "var(--bg-surface-raised)" };
                                        let color = if is_active { "var(--text-brand)" } else { "var(--text-secondary)" };
                                        let tv = *t;
                                        rsx! {
                                            button {
                                                key: "{t.value()}",
                                                onclick: move |_| tab.set(tv),
                                                style: "background:{bg};color:{color};border:none;border-radius:var(--radius-md);padding:6px 12px;font-size:13px;font-weight:600;cursor:pointer;",
                                                "{t.label()}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if tab() == SettingsTab::General {
                            div { style: "display:flex;align-items:center;gap:12px;",
                                Avatar { name: "You", size: 56 }
                                div {
                                    div { style: "font-weight:700;font-size:16px;", "You" }
                                    div { style: "font-size:12px;color:var(--text-tertiary);font-family:var(--font-mono);",
                                        "{me().flatten().map(|m| m.id).unwrap_or_default()}"
                                    }
                                }
                            }
                            crate::design_system::Select {
                                label: "Theme",
                                value: "dark",
                                options: vec![
                                    SelectOption { value: "dark".into(), label: "Dark".into() },
                                    SelectOption { value: "light".into(), label: "Light".into() },
                                ],
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
                                            }
                                            // Clearing the identity signal drops the router
                                            // back to LoginScreen regardless of backend.
                                            use_context::<Signal<Option<Me>>>().set(None);
                                        });
                                    },
                                    Icon { name: IconName::LogOut, size: 14 }
                                    " Sign out"
                                }
                            }
                        }
                        if tab() == SettingsTab::Notifications {
                            Switch { label: "Notifications".to_string(), checked: notif(), on_change: move |_| notif.set(!notif()) }
                            Switch { label: "Read receipts".to_string(), checked: receipts(), on_change: move |_| receipts.set(!receipts()) }
                            Switch { label: "Typing indicators".to_string(), checked: typing_indicators(), on_change: move |_| typing_indicators.set(!typing_indicators()) }
                        }
                        if tab() == SettingsTab::Security {
                            div { style: "font-size:13px;font-weight:700;letter-spacing:0.04em;color:var(--text-tertiary);", "SESSIONS" }
                            for d in devices().iter() {
                                {
                                    let id = d.id.clone();
                                    let start_verify = start_verify.clone();
                                    rsx! {
                                        div { key: "{d.id}", style: "display:flex;align-items:center;gap:10px;padding:10px 12px;background:var(--bg-surface);border:1px solid var(--border-subtle);border-radius:var(--radius-md);",
                                            Icon {
                                                name: if d.verified { IconName::ShieldCheck } else { IconName::ShieldAlert },
                                                size: 18,
                                                color: if d.verified { "var(--status-online)".to_string() } else { "var(--status-away)".to_string() },
                                            }
                                            div { style: "flex:1;",
                                                div { style: "font-size:14px;font-weight:600;", "{d.name}" }
                                                div { style: "font-size:12px;color:var(--text-tertiary);", "{d.last_seen}" }
                                            }
                                            if !d.verified {
                                                Button { variant: ButtonVariant::Secondary, size: ButtonSize::Sm, onclick: move |_| {
                                                    verify_id.set(Some(id.clone()));
                                                    start_verify(id.clone());
                                                }, "Verify" }
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
                        // Manual close mid-session cancels it server-side; on
                        // terminal states (Done/Cancelled) this is a no-op the
                        // backend ignores.
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
        }
    }
}
