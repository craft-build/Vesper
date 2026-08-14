use dioxus::prelude::*;

use crate::chat::use_media_src;

#[component]
pub fn Avatar(
    #[props(default = String::new())] name: String,
    #[props(default = 40)] size: i64,
    #[props(default = None)] src: Option<String>,
    /// MXC avatar URI (checkpoint 07): resolved through the SDK media cache
    /// to a thumbnail `data:` URI; initials remain the fallback while/never
    /// resolved. Explicit `src` wins over `mxc`.
    #[props(default = None)]
    mxc: Option<String>,
) -> Element {
    // Hooks can't be conditional: pass `None` when an explicit `src` makes
    // resolution unnecessary (the effect no-ops on `None`).
    let resolved = use_media_src(
        if src.is_some() { None } else { mxc },
        None,
        Some((128, 128)),
    );
    let src = src.or(resolved);
    let initials: String = name
        .trim()
        .split_whitespace()
        .take(2)
        .filter_map(|w| w.chars().next())
        .collect::<String>()
        .to_uppercase();
    let font_size = (size as f64) * 0.38;
    let style = format!(
        "width:{size}px;height:{size}px;border-radius:var(--radius-full);overflow:hidden;background:var(--blue-80);color:var(--white);display:flex;align-items:center;justify-content:center;font-family:var(--font-sans);font-weight:700;font-size:{font_size}px;flex-shrink:0;"
    );
    rsx! {
        div { style: "{style}",
            if let Some(src) = src {
                img { src: "{src}", alt: "{name}", style: "width:100%;height:100%;object-fit:cover;" }
            } else {
                "{initials}"
            }
        }
    }
}
