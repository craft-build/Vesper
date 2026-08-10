use dioxus::prelude::*;

use crate::data::Presence;

#[component]
pub fn StatusDot(
    #[props(default = Presence::Offline)] status: Presence,
    #[props(default = 10)] size: i64,
) -> Element {
    let color = match status {
        Presence::Online => "var(--status-online)",
        Presence::Away => "var(--status-away)",
        Presence::Offline => "var(--status-offline)",
    };
    let style = format!(
        "width:{size}px;height:{size}px;border-radius:var(--radius-full);background:{color};display:inline-block;border:2px solid var(--bg-canvas);box-sizing:content-box;"
    );
    rsx! {
        span { style: "{style}" }
    }
}
