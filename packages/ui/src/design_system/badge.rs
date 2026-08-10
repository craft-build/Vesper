use dioxus::prelude::*;

#[component]
pub fn Badge(
    #[props(default = 0)] count: i64,
    #[props(default = 99)] max: i64,
    #[props(default = false)] dot: bool,
) -> Element {
    if !dot && count <= 0 {
        return rsx! {};
    }
    let label = if count > max {
        format!("{max}+")
    } else {
        count.to_string()
    };
    let (dim, padding) = if dot {
        (10, "0".to_string())
    } else {
        (20, "0 6px".to_string())
    };
    let style = format!(
        "min-width:{dim}px;height:{dim}px;padding:{padding};border-radius:var(--radius-full);background:var(--status-danger);color:#fff;font-family:var(--font-mono);font-size:11px;font-weight:600;display:inline-flex;align-items:center;justify-content:center;"
    );
    rsx! {
        span { style: "{style}",
            if !dot {
                "{label}"
            }
        }
    }
}
