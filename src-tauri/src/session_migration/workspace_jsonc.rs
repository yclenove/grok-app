//! JSONC lexical normalization only. JSON schema validation remains in workspace.rs.

pub(super) fn normalize(text: &str) -> Result<String, String> {
    let mut bytes = text.trim_start_matches('\u{feff}').as_bytes().to_vec();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            i = string_end(&bytes, i);
        } else if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && !matches!(bytes[i], b'\r' | b'\n') {
                bytes[i] = b' ';
                i += 1;
            }
        } else if bytes[i..].starts_with(b"/*") {
            let start = i;
            i += 2;
            while i + 1 < bytes.len() && !bytes[i..].starts_with(b"*/") {
                i += 1;
            }
            if i + 1 >= bytes.len() {
                return Err("invalid workspace JSONC: unterminated comment".into());
            }
            i += 2;
            for byte in &mut bytes[start..i] {
                if !matches!(*byte, b'\r' | b'\n') {
                    *byte = b' ';
                }
            }
        } else {
            i += 1;
        }
    }
    i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            i = string_end(&bytes, i);
            continue;
        }
        if bytes[i] == b',' {
            let mut next = i + 1;
            while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                next += 1;
            }
            let previous = bytes[..i]
                .iter()
                .rev()
                .find(|byte| !byte.is_ascii_whitespace());
            // Do not turn invalid empty arrays or double commas into valid JSON.
            if matches!(bytes.get(next), Some(b']' | b'}'))
                && !matches!(previous, None | Some(b'[' | b'{' | b',' | b':'))
            {
                bytes[i] = b' ';
            }
        }
        i += 1;
    }
    String::from_utf8(bytes).map_err(|error| format!("invalid workspace JSONC: {error}"))
}

fn string_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i = (i + 2).min(bytes.len()),
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    i // serde_json reports unterminated strings.
}
