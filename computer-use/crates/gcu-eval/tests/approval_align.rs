//! The eval allow-list must stay the same as `gcu-cli`'s `GCU_APPROVAL` parser.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_eval::{parse_approval, GCU_ACCEPTED_APPROVAL};

#[test]
fn gcu_approval_match_arm_equals_the_eval_allow_list() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../gcu-cli/src/args.rs");
    let text = std::fs::read_to_string(&path).unwrap();
    let start = text
        .find("pub(crate) fn approval_mode()")
        .expect("approval_mode");
    let body = &text[start..];
    let end = body.find("\n}\n").expect("end of approval_mode");
    let body = &body[..end];
    assert!(
        !body.contains("auto-deny-sensitive"),
        "gcu must not accept auto-deny-sensitive:\n{body}"
    );
    let quoted = quoted_match_arms(body);
    assert_eq!(
        quoted,
        GCU_ACCEPTED_APPROVAL
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>(),
        "gcu match arm is not the eval allow-list:\n{body}"
    );
    for value in GCU_ACCEPTED_APPROVAL {
        assert!(parse_approval(Some(value), true).is_ok(), "{value}");
        assert_eq!(
            parse_approval(Some(value), true).unwrap().unwrap().as_str(),
            *value
        );
    }
    assert!(parse_approval(Some("auto-deny-sensitive"), true).is_err());
}

/// Quoted strings inside every `matches!` invocation of `body`, in source order.
///
/// Parentheses are balanced, so `matches!(value.as_str(), "a" | "b")` includes
/// the pattern. Stopping at the first `)` would end inside `as_str()` and
/// report no arms. Strings outside the invocation are ignored.
fn quoted_match_arms(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let needle = b"matches!";
    let mut cursor = 0;
    while cursor + needle.len() <= bytes.len() {
        if &bytes[cursor..cursor + needle.len()] != needle {
            cursor += 1;
            continue;
        }
        cursor += needle.len();
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] != b'(' {
            continue;
        }
        let span_start = cursor;
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        while cursor < bytes.len() {
            let byte = bytes[cursor];
            cursor += 1;
            if in_string {
                if escaped {
                    escaped = false;
                    continue;
                }
                if byte == b'\\' {
                    escaped = true;
                    continue;
                }
                if byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match byte {
                b'"' => in_string = true,
                b'(' => depth += 1,
                b')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        push_quoted(&body[span_start..cursor], &mut out);
    }
    out
}

fn push_quoted(span: &str, out: &mut Vec<String>) {
    let bytes = span.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'"' {
            cursor += 1;
            continue;
        }
        cursor += 1;
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor] != b'"' {
            if bytes[cursor] == b'\\' {
                cursor += 1;
            }
            cursor += 1;
        }
        out.push(span[start..cursor.min(span.len())].to_string());
        if cursor < bytes.len() {
            cursor += 1;
        }
    }
}
