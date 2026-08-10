//! Minimal markdown renderer ported from the design system's `icons.jsx`
//! (`window.renderMarkdown`). Escapes HTML first, then layers on bold/italic/code/link —
//! same order, same four rules, no more. Used via `dangerous_inner_html` in `MessageRow`,
//! exactly like the prototype's `dangerouslySetInnerHTML`.

use std::sync::LazyLock;

use regex::Regex;

static BOLD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*(.+?)\*\*").unwrap());
static ITALIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*(.+?)\*").unwrap());
static CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`(.+?)`").unwrap());
static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[(.+?)\]\((.+?)\)").unwrap());

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn render_markdown(text: &str) -> String {
    let mut s = escape_html(text);
    s = BOLD.replace_all(&s, "<strong>$1</strong>").into_owned();
    s = ITALIC.replace_all(&s, "<em>$1</em>").into_owned();
    s = CODE.replace_all(&s, "<code>$1</code>").into_owned();
    s = LINK
        .replace_all(&s, r#"<a href="$2" target="_blank" rel="noopener">$1</a>"#)
        .into_owned();
    s
}
