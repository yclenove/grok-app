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
    let arm = GCU_ACCEPTED_APPROVAL
        .iter()
        .map(|value| format!("\"{value}\""))
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        body.contains(&arm),
        "eval allow-list {arm} is not the gcu match arm:\n{body}"
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
