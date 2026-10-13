/// Enforce a field limit in Unicode scalars and escape line/control characters.
/// Counts refer to the original field, before escaping, not UTF-8 bytes.
pub(crate) fn field(text: &str, limit: Option<usize>) -> String {
    let count = text.chars().count();
    let keep = limit.unwrap_or(count).min(count);
    let mut output = String::new();
    for ch in text.chars().take(keep) {
        match ch {
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}') => {
                output.extend(ch.escape_unicode());
            }
            ch => output.push(ch),
        }
    }
    if keep < count {
        output.push_str(&format!("…(+{} chars)", count - keep));
    }
    output
}
