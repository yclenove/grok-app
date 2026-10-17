//! Plain-text display for model reasons and unverified labels.
//!
//! Bidirectional controls and other C0/C1 controls become U+FFFD. Newlines and
//! tabs become spaces. The result is at most 280 scalar values. Identity
//! fields used for the binding are not passed through this function.

/// Maximum displayed scalar values. Matches the route A approval window.
const LIMIT: usize = 280;

/// Sanitize one displayed string. Pure and nonblocking.
pub fn sanitize_plain(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        if out.chars().count() >= LIMIT {
            break;
        }
        if matches!(ch, '\n' | '\r' | '\t') {
            out.push(' ');
            continue;
        }
        if is_hidden_control(ch) {
            out.push('\u{FFFD}');
            continue;
        }
        out.push(ch);
    }
    out
}

fn is_hidden_control(ch: char) -> bool {
    let code = u32::from(ch);
    code < 0x20
        || (0x7F..=0x9F).contains(&code)
        || matches!(
            code,
            0x61C | 0x200E | 0x200F | 0x202A..=0x202E | 0x2066..=0x2069
        )
}

/// Empty optional strings are absent, so a blank publisher is not an identity.
pub fn blank_to_none(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.is_empty())
}
