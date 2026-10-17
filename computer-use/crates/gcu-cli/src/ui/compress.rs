//! Turn the onboarding sources into one offline HTML document.
//!
//! The build script (`xtask ui embed`) writes the result. The binary
//! `include_str!`s that file and does not read HTML or CSS at runtime.

/// Inline `css` into the single `{{slot:css}}` marker and minify.
///
/// Fails when the marker is missing or repeated. Blocking: none. Timeout:
/// none. Cancellation: none. Thread safety: pure.
pub fn embed_document(html: &str, css: &str) -> Result<String, &'static str> {
    if html.matches("{{slot:css}}").count() != 1 {
        return Err("css slot");
    }
    let css = minify_css(css);
    let html = html.replacen("{{slot:css}}", &css, 1);
    Ok(minify_html(&html))
}

fn minify_css(css: &str) -> String {
    let stripped = strip_css_comments(css);
    let mut collapsed = String::new();
    let mut space = false;
    for ch in stripped.chars() {
        if ch.is_whitespace() {
            space = true;
            continue;
        }
        if space && !collapsed.is_empty() {
            collapsed.push(' ');
        }
        space = false;
        collapsed.push(ch);
    }
    let mut out = String::new();
    let mut chars = collapsed.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == ' ' {
            let prev = out.chars().last();
            let next = chars.peek().copied();
            if prev.is_some_and(|item| matches!(item, '{' | '}' | ';' | ':' | ','))
                || next.is_some_and(|item| matches!(item, '{' | '}' | ';' | ':' | ','))
            {
                continue;
            }
        }
        out.push(ch);
    }
    out
}

fn strip_css_comments(css: &str) -> String {
    let mut out = String::new();
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        if let Some(end) = rest[start + 2..].find("*/") {
            rest = &rest[start + 2 + end + 2..];
        } else {
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out
}

fn minify_html(html: &str) -> String {
    let Some(start) = html.find("<script>") else {
        return collapse_markup(html);
    };
    let after = &html[start..];
    let Some(end_rel) = after.find("</script>") else {
        return collapse_markup(html);
    };
    let end = start + end_rel + "</script>".len();
    let mut out = collapse_markup(&html[..start]);
    out.push_str(&html[start..end]);
    out.push_str(&collapse_markup(&html[end..]));
    out
}

fn collapse_markup(input: &str) -> String {
    let bare = strip_html_comments(input);
    let mut out = String::new();
    let mut space = false;
    for ch in bare.chars() {
        if ch.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.ends_with('>') && ch != '<' && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(ch);
    }
    out.trim().to_string()
}

fn strip_html_comments(input: &str) -> String {
    let mut out = String::new();
    let mut rest = input;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        if let Some(end) = rest[start + 4..].find("-->") {
            rest = &rest[start + 4 + end + 3..];
        } else {
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out
}
