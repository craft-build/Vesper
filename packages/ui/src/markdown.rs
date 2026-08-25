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
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Relative and scheme-less URLs (e.g. `/path`, `#frag`, `page.html`) are
/// always allowed; absolute URLs must use the whitelisted schemes.
/// `url` is already HTML-escaped (from `escape_html` output), so quotes cannot
/// break out of the `href` attribute; this check only blocks dangerous
/// schemes like `javascript:`.
fn is_allowed_link_scheme(url: &str) -> bool {
    // Browsers strip leading C0 controls/whitespace and inner tabs/newlines
    // before scheme parsing, so reject control chars outright to avoid
    // obfuscated `javascript:` bypasses.
    if url.chars().any(|c| c < ' ' || c == '\u{7f}') {
        return false;
    }
    let url = url.trim_start();
    let split = |c: char| matches!(c, ':' | '/' | '?' | '#');
    let head = url.split(split).next().unwrap_or("");
    let has_scheme = url[head.len()..].starts_with(':')
        && head.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && head
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !has_scheme {
        // Relative or scheme-less URL (e.g. `/path`, `#frag`, `page.html`).
        return !url.chars().any(|c: char| c.is_whitespace());
    }
    ["https://", "http://", "mailto:", "matrix:"]
        .iter()
        .any(|scheme| url.len() >= scheme.len() && url[..scheme.len()].eq_ignore_ascii_case(scheme))
}

pub fn render_markdown(text: &str) -> String {
    let mut s = escape_html(text);
    s = BOLD.replace_all(&s, "<strong>$1</strong>").into_owned();
    s = ITALIC.replace_all(&s, "<em>$1</em>").into_owned();
    s = CODE.replace_all(&s, "<code>$1</code>").into_owned();
    s = LINK
        .replace_all(&s, |caps: &regex::Captures<'_>| {
            let text = &caps[1];
            let url = &caps[2];
            if is_allowed_link_scheme(url) {
                format!(r#"<a href="{url}" target="_blank" rel="noopener">{text}</a>"#)
            } else {
                // Disallowed scheme: render the source literally (already escaped).
                format!("[{text}]({url})")
            }
        })
        .into_owned();
    s
}

#[cfg(test)]
mod tests {
    use super::render_markdown;

    #[test]
    fn escapes_html() {
        assert_eq!(render_markdown("<b>&</b>"), "&lt;b&gt;&amp;&lt;/b&gt;");
    }

    #[test]
    fn renders_bold_italic_code() {
        assert_eq!(render_markdown("**b**"), "<strong>b</strong>");
        assert_eq!(render_markdown("*i*"), "<em>i</em>");
        assert_eq!(render_markdown("`c`"), "<code>c</code>");
    }

    #[test]
    fn renders_allowed_links() {
        assert_eq!(
            render_markdown("[site](https://example.com)"),
            r#"<a href="https://example.com" target="_blank" rel="noopener">site</a>"#
        );
        assert_eq!(
            render_markdown("[site](http://example.com)"),
            r#"<a href="http://example.com" target="_blank" rel="noopener">site</a>"#
        );
        assert_eq!(
            render_markdown("[mail](mailto:a@b.c)"),
            r#"<a href="mailto:a@b.c" target="_blank" rel="noopener">mail</a>"#
        );
        assert_eq!(
            render_markdown("[room](matrix:r/a:b)"),
            r#"<a href="matrix:r/a:b" target="_blank" rel="noopener">room</a>"#
        );
    }

    #[test]
    fn renders_relative_links() {
        assert_eq!(
            render_markdown("[room](/room/!abc:server)"),
            r#"<a href="/room/!abc:server" target="_blank" rel="noopener">room</a>"#
        );
        assert_eq!(
            render_markdown("[here](#section)"),
            r##"<a href="#section" target="_blank" rel="noopener">here</a>"##
        );
    }

    #[test]
    fn blocks_obfuscated_dangerous_schemes() {
        assert!(!render_markdown("[x](\u{1}javascript:alert(1))").contains("<a "));
        assert!(!render_markdown("[x](java\tscript:alert(1))").contains("<a "));
        assert!(!render_markdown("[x](java\nscript:alert(1))").contains("<a "));
        assert!(!render_markdown("[x]( JavaScript:alert(1))").contains("<a "));
    }

    #[test]
    fn blocks_javascript_urls() {
        assert_eq!(
            render_markdown("[x](javascript:alert(1))"),
            "[x](javascript:alert(1))"
        );
        assert!(!render_markdown("[x](JAVASCRIPT:alert(1))").contains("<a "));
    }

    #[test]
    fn neutralizes_attribute_injection() {
        let out = render_markdown(r#"[x](" onmouseover="alert(document.domain) a=")"#);
        assert!(!out.contains("<a "), "got: {out}");
        assert!(!out.contains('"'), "got: {out}");
        assert_eq!(
            out,
            r#"[x](&quot; onmouseover=&quot;alert(document.domain) a=&quot;)"#
        );
    }

    #[test]
    fn escapes_quotes_in_link_text() {
        assert_eq!(
            render_markdown(r#"[a"b'c](https://example.com)"#),
            r#"<a href="https://example.com" target="_blank" rel="noopener">a&quot;b&#39;c</a>"#
        );
    }
}
