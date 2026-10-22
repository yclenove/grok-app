//! Tool-output shapes observed in codex-cli 0.160.0 rollouts (2026-10-21 owner
//! export): `output` is an ordered content-item array, not a string. Content
//! here is synthetic; only the record structure follows the vendor files.
use super::*;
use crate::session_migration::types::ToolOutcome;
use serde_json::json;

const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aCM0AAAAASUVORK5CYII=";

fn normalize(rows: Vec<Value>) -> AdapterOutput {
    normalize_records(
        rows.into_iter().enumerate().map(|(index, value)| {
            Ok(JsonlRecord {
                index: index as u32,
                byte_start: 0,
                byte_end: 0,
                value,
            })
        }),
        Path::new("synthetic-codex-tool-output.jsonl"),
        &JsonlPrefix::default(),
    )
    .unwrap()
}

fn call(call_id: &str) -> Value {
    json!({"type":"response_item","payload":{"type":"custom_tool_call","id":"ctc_1",
        "status":"completed","call_id":call_id,"name":"exec","input":"run"}})
}

fn output(call_id: &str, output: Value) -> Value {
    json!({"type":"response_item","payload":{"type":"custom_tool_call_output","id":"out_1",
        "call_id":call_id,"output":output}})
}

fn result(out: &AdapterOutput) -> (&[ContentBlock], &ToolOutcome) {
    let found = out.graph.events.iter().find_map(|event| match event {
        ImportedEvent::ToolResult {
            blocks, outcome, ..
        } => Some((blocks.as_slice(), outcome)),
        _ => None,
    });
    found.expect("tool result is retained")
}

fn has_issue(out: &AdapterOutput, code: &str) -> bool {
    out.issues.iter().any(|issue| issue.code == code)
}

#[test]
fn codex_tool_output_text_items_keep_every_part_in_order() {
    let out = normalize(vec![
        call("c1"),
        output(
            "c1",
            json!([
                {"type":"input_text","text":"Script completed\nOutput:\n"},
                {"type":"input_text","text":"{\"exit_code\":1,\"output\":\"ValueError: x\"}"}
            ]),
        ),
    ]);
    let (blocks, outcome) = result(&out);
    let text: Vec<_> = blocks.iter().filter_map(ContentBlock::plain_text).collect();
    assert_eq!(
        text.join("\n"),
        "Script completed\nOutput:\n\n{\"exit_code\":1,\"output\":\"ValueError: x\"}",
        "array output must not be imported as an empty string"
    );
    assert_eq!(
        outcome,
        &ToolOutcome::Success,
        "no success flag: the call returned"
    );
    assert!(!has_issue(&out, "codex-content-unsupported"));
}

#[test]
fn codex_tool_output_image_items_become_ordered_assets() {
    let out = normalize(vec![
        call("c2"),
        output(
            "c2",
            json!([
                {"type":"input_text","text":"before"},
                {"type":"input_image","image_url":PNG,"detail":"original"},
                {"type":"input_text","text":"after"}
            ]),
        ),
    ]);
    let (blocks, _) = result(&out);
    assert_eq!(blocks.len(), 3);
    assert_eq!(blocks[0].plain_text(), Some("before"));
    let ContentBlock::Asset { asset_id, .. } = &blocks[1] else {
        panic!("image item must be an asset block, got {:?}", blocks[1]);
    };
    assert_eq!(blocks[2].plain_text(), Some("after"));
    let asset = out
        .assets
        .iter()
        .find(|a| &a.asset_id == asset_id)
        .expect("asset record for the tool image");
    assert_eq!(asset.source_ref.as_deref(), Some(PNG));
    assert!(!has_issue(&out, "codex-image-unresolved"));
}

#[test]
fn codex_tool_output_legacy_string_is_unchanged() {
    let out = normalize(vec![call("c3"), output("c3", json!("plain output"))]);
    let (blocks, outcome) = result(&out);
    assert_eq!(blocks, &[ContentBlock::text("plain output")]);
    assert_eq!(outcome, &ToolOutcome::Success);
}

#[test]
fn codex_tool_output_explicit_failure_flag_is_an_error() {
    let out = normalize(vec![
        call("c4"),
        output("c4", json!({"content":"permission denied","success":false})),
    ]);
    let (blocks, outcome) = result(&out);
    assert_eq!(blocks, &[ContentBlock::text("permission denied")]);
    assert_eq!(outcome, &ToolOutcome::Error);
}

#[test]
fn codex_tool_output_unknown_shapes_are_disclosed_not_empty_success() {
    for (call_id, value) in [
        ("n1", json!(42)),
        ("n2", Value::Null),
        ("n3", json!({"unexpected":true})),
        ("n4", json!([{"type":"input_audio","data":"x"}])),
    ] {
        let out = normalize(vec![call(call_id), output(call_id, value.clone())]);
        let (blocks, outcome) = result(&out);
        assert!(
            blocks
                .iter()
                .any(|b| matches!(b, ContentBlock::Opaque { .. })),
            "{value} must leave an opaque placeholder, got {blocks:?}"
        );
        assert!(
            has_issue(&out, "codex-content-unsupported"),
            "{value} must be reported as partial"
        );
        if !value.is_array() {
            assert_eq!(outcome, &ToolOutcome::Unknown, "{value}");
        }
    }
}

#[test]
fn codex_tool_output_missing_field_is_disclosed() {
    let out = normalize(vec![
        call("m1"),
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"m1"}}),
    ]);
    let (blocks, outcome) = result(&out);
    assert!(matches!(blocks, [ContentBlock::Opaque { .. }]));
    assert_eq!(outcome, &ToolOutcome::Unknown);
    assert!(has_issue(&out, "codex-content-unsupported"));
}
