//! Source-shaped legacy rollout media, not installed vendor histories.
use super::*;
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
        Path::new("synthetic-codex-media.jsonl"),
        &JsonlPrefix::default(),
    )
    .unwrap()
}

fn response(id: &str, content: Value) -> Value {
    json!({"type":"response_item","payload":{"type":"message","id":id,
        "role":"user","content":content}})
}

fn blocks(event: &ImportedEvent) -> &[ContentBlock] {
    match event {
        ImportedEvent::Message { blocks, .. } => blocks,
        _ => panic!("expected retained message, got {event:?}"),
    }
}

#[test]
fn codex_media_response_retains_order_and_image_only_turns_with_distinct_references() {
    let out = normalize(vec![
        response(
            "mixed",
            json!([
                {"type":"input_text","text":"before"},
                {"type":"input_image","image_url":PNG},
                {"type":"input_text","text":"after"}
            ]),
        ),
        response("pure", json!([{"type":"input_image","image_url":PNG}])),
    ]);
    assert_eq!(out.graph.events.len(), 2, "pure images are real user turns");
    assert_eq!(blocks(&out.graph.events[0]).len(), 3);
    assert_eq!(blocks(&out.graph.events[0])[0].plain_text(), Some("before"));
    assert!(matches!(
        blocks(&out.graph.events[0])[1],
        ContentBlock::Asset { .. }
    ));
    assert_eq!(blocks(&out.graph.events[0])[2].plain_text(), Some("after"));
    assert!(matches!(
        blocks(&out.graph.events[1]),
        [ContentBlock::Asset { .. }]
    ));
    assert_eq!(out.assets.len(), 2);
    assert_ne!(out.assets[0].asset_id, out.assets[1].asset_id);
    assert_eq!(out.assets[0].source_span.record, "record:0");
    assert_eq!(out.assets[0].source_span.block, Some(1));
    assert_eq!(out.assets[1].source_span.record, "record:1");
    assert_eq!(out.assets[1].source_span.block, Some(0));
    assert_eq!(out.graph.active_leaf.as_deref(), Some("codex:pure"));
}

#[test]
fn codex_media_opposite_projections_merge_media_in_both_orders_without_losing_wire_order() {
    let event = json!({"type":"event_msg","payload":{"type":"user_message",
        "message":"before\nafter","images":[PNG]}});
    let item = response(
        "mixed",
        json!([
            {"type":"input_text","text":"before"},
            {"type":"input_image","image_url":PNG},
            {"type":"input_text","text":"after"}
        ]),
    );
    for rows in [vec![event.clone(), item.clone()], vec![item, event]] {
        let out = normalize(rows);
        assert_eq!(
            out.graph.events.len(),
            1,
            "matching media projections are one turn"
        );
        let message = &out.graph.events[0];
        assert_eq!(message.base().spans.len(), 2);
        assert_eq!(blocks(message).len(), 3);
        assert!(matches!(blocks(message)[1], ContentBlock::Asset { .. }));
        assert_eq!(out.assets.len(), 1);
        assert_eq!(out.assets[0].source_span.block, Some(1));
    }
}

#[test]
fn codex_media_unknown_and_opaque_image_references_are_explicit_not_silent_complete_text() {
    let out = normalize(vec![response(
        "unknown",
        json!([
            {"type":"input_text","text":"readable"},
            {"type":"future_private_block","text":"MUST_NOT_BECOME_TEXT","payload":"PRIVATE"},
            {"type":"input_image","file_id":"../not-a-local-image.png"},
            {"type":"input_image","image_url":"https://example.invalid/remote.png"},
            {"type":"input_image"},
            {"type":"input_audio","audio_url":"PRIVATE_AUDIO"}
        ]),
    )]);
    assert_eq!(out.outcome, ImportOutcome::Partial);
    assert_eq!(blocks(&out.graph.events[0]).len(), 6);
    assert!(matches!(
        blocks(&out.graph.events[0])[1],
        ContentBlock::Opaque { payload: None, .. }
    ));
    assert_eq!(out.assets.len(), 3);
    assert!(!out.issues.is_empty());
    let graph = serde_json::to_string(&out.graph).unwrap();
    assert!(!graph.contains("MUST_NOT_BECOME_TEXT"));
    assert!(!graph.contains("PRIVATE"));
    assert_eq!(out.graph.events[0].searchable_text(), "readable");
}

#[test]
fn codex_media_replacement_history_preserves_old_and_live_images_separately() {
    let out = normalize(vec![
        response("old", json!([{"type":"input_image","image_url":PNG}])),
        json!({"type":"compacted","payload":{"message":"summary",
            "replacement_history":[{"type":"message","id":"replacement","role":"user",
                "content":[{"type":"input_text","text":"after"},
                           {"type":"input_image","image_url":PNG}]}]}}),
    ]);
    assert_eq!(out.assets.len(), 2);
    assert_ne!(out.assets[0].asset_id, out.assets[1].asset_id);
    let live = crate::session_migration::graph::lineage(
        &out.graph.events,
        &out.graph.relations,
        out.graph.active_leaf.as_deref().unwrap(),
    );
    assert_eq!(
        live.iter().map(|event| event.id()).collect::<Vec<_>>(),
        ["codex:compact:1", "codex:replacement"]
    );
    assert!(matches!(blocks(live[1])[1], ContentBlock::Asset { .. }));
    let old = crate::session_migration::graph::lineage(
        &out.graph.events,
        &out.graph.relations,
        "codex:old",
    );
    assert_eq!(old.len(), 1);
    assert!(matches!(blocks(old[0])[0], ContentBlock::Asset { .. }));
}

#[test]
fn codex_media_event_image_order_and_legacy_order_preserve_every_reference() {
    for (order, expected) in [
        (
            json!(["file", "inline", "file"]),
            vec!["opaque:codex-file:first", PNG, "opaque:codex-file:last"],
        ),
        (
            json!([]),
            vec![PNG, "opaque:codex-file:first", "opaque:codex-file:last"],
        ),
        (
            json!(["inline"]),
            vec![PNG, "opaque:codex-file:first", "opaque:codex-file:last"],
        ),
    ] {
        let malformed = order == json!(["inline"]);
        let out = normalize(vec![
            json!({"type":"event_msg","payload":{"type":"user_message",
            "message":"", "images":[PNG], "file_ids":["first", "last"], "image_order":order,
            "local_images":["inside.png"], "audio":["PRIVATE_AUDIO"]}}),
        ]);
        let refs: Vec<_> = out
            .assets
            .iter()
            .map(|asset| asset.source_ref.as_deref().unwrap())
            .collect();
        assert_eq!(&refs[..3], expected);
        assert_eq!(refs[3], "path:inside.png");
        assert_eq!(
            out.issues
                .iter()
                .any(|issue| issue.code == "codex-image-order"),
            malformed
        );
        assert_eq!(out.outcome, ImportOutcome::Partial);
        assert!(matches!(
            blocks(&out.graph.events[0]).last(),
            Some(ContentBlock::Opaque { payload: None, .. })
        ));
        assert!(!serde_json::to_string(&out.graph)
            .unwrap()
            .contains("PRIVATE_AUDIO"));
    }
}

#[test]
fn codex_media_equal_text_does_not_swallow_different_or_unproven_images() {
    let event = json!({"type":"event_msg","payload":{"type":"user_message","message":"same","images":[PNG]}});
    for reference in [
        json!({"type":"input_image","image_url":"https://example.invalid/different.png"}),
        json!({"type":"input_image"}),
    ] {
        let item = response(
            "different",
            json!([{"type":"input_text","text":"same"}, reference]),
        );
        for rows in [
            vec![event.clone(), item.clone()],
            vec![item.clone(), event.clone()],
        ] {
            let out = normalize(rows);
            assert_eq!(out.graph.events.len(), 2);
            assert_eq!(out.assets.len(), 2);
            assert_eq!(out.outcome, ImportOutcome::Partial);
            assert!(out
                .issues
                .iter()
                .any(|issue| issue.code == "codex-projection-unmatched"));
        }
    }
}

#[test]
fn codex_media_text_only_retains_frozen_single_block_and_spans() {
    let out = normalize(vec![
        response(
            "plain",
            json!([{"type":"input_text","text":"one"}, {"type":"input_text","text":"two"}]),
        ),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"one\ntwo"}}),
    ]);
    assert_eq!(out.graph.events.len(), 1);
    assert_eq!(
        serde_json::to_value(&out.graph.events[0]).unwrap(),
        json!({
            "kind":"message","id":"codex:plain","ordinal":0,"timeOrigin":"unknown",
            "spans":[{"artifactId":"synthetic-codex-media.jsonl","record":"record:0"},
                     {"artifactId":"synthetic-codex-media.jsonl","record":"record:1"}],
            "role":"user","blocks":[{"kind":"text","text":"one\ntwo"}],"complete":true
        })
    );
    assert_eq!(out.outcome, ImportOutcome::Complete);
    assert!(out.assets.is_empty());
}
