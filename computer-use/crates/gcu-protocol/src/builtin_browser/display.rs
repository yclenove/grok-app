//! Address-bar display. The committed URL is shown; the page title is untrusted text.

use super::url::{classify, UrlClass, UrlRole};

/// Scheme, host, and non-default port of a public committed URL.
///
/// Userinfo, non-ASCII hosts, bidi, and controls are rejected. Punycode is
/// shown as ASCII. None means the caller must keep the previous origin.
pub fn display_origin(committed: &str) -> Option<String> {
    let UrlClass::Public(canonical) = classify(committed, UrlRole::Navigation) else {
        return None;
    };
    Some(origin_of(&canonical))
}

/// Full canonical URL, abbreviating only the path and query when it exceeds `max_chars`.
///
/// The scheme and host are never shortened. A rejected URL returns None.
pub fn abbreviate_address(committed: &str, max_chars: usize) -> Option<String> {
    let UrlClass::Public(canonical) = classify(committed, UrlRole::Navigation) else {
        return None;
    };
    let origin = origin_of(&canonical);
    if canonical.chars().count() <= max_chars {
        return Some(canonical);
    }
    if origin.chars().count() >= max_chars {
        return Some(origin);
    }
    let path = canonical
        .strip_prefix(&origin)
        .unwrap_or("")
        .chars()
        .take(
            max_chars
                .saturating_sub(origin.chars().count())
                .saturating_sub(1),
        )
        .collect::<String>();
    Some(format!("{origin}{path}…"))
}

/// Escape bidi and control characters in untrusted page text.
#[must_use]
pub fn escape_untrusted(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_control()
            || ch == '\u{feff}'
            || ('\u{200b}'..='\u{200f}').contains(&ch)
            || ('\u{202a}'..='\u{202e}').contains(&ch)
            || ('\u{2066}'..='\u{2069}').contains(&ch)
        {
            out.push_str(&format!("\\u{{{:x}}}", u32::from(ch)));
        } else {
            out.push(ch);
        }
    }
    out
}

fn origin_of(canonical: &str) -> String {
    let Some(start) = canonical.find("://") else {
        return canonical.to_owned();
    };
    let after = start + 3;
    match canonical[after..].find(['/', '?', '#']) {
        Some(index) => canonical[..after + index].to_owned(),
        None => canonical.to_owned(),
    }
}
