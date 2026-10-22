//! Public-shape compact summaries must retain content without becoming prompts.

use super::*;
use crate::session_migration::graph::lineage_ids;
use crate::session_migration::selection::AllowedSelection;
use serde_json::json;

fn normalized(records: &[Value]) -> AdapterOutput {
    let path =
        std::env::temp_dir().join(format!("claude-compaction-{}.jsonl", uuid::Uuid::new_v4()));
    let source = records
        .iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>();
    std::fs::write(&path, &source).unwrap();
    let output = normalize_file(&path);
    let after = std::fs::read_to_string(&path);
    std::fs::remove_file(path).unwrap();
    assert_eq!(after.unwrap(), source);
    output.unwrap()
}

#[test]
fn compact_summary_is_typed_and_keeps_the_live_branch_cut() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/session-migration/claude/compaction.jsonl");
    let original = std::fs::read(&path).unwrap();
    let output = normalize_file(&path).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), original);
    let compactions = output
        .graph
        .events
        .iter()
        .filter(|event| matches!(event, ImportedEvent::Compaction { .. }))
        .collect::<Vec<_>>();
    assert_eq!(compactions.len(), 1, "{:?}", output.graph.events);
    let ImportedEvent::Compaction {
        base,
        summary,
        covered_event_ids,
        coverage_known,
    } = compactions[0]
    else {
        unreachable!()
    };
    assert_eq!(base.id, "claude:compact-summary");
    assert_eq!(base.timestamp.as_deref(), Some("2026-01-01T00:00:03Z"));
    assert_eq!(base.spans[0].record, "record:3");
    assert_eq!(
        summary
            .iter()
            .filter_map(ContentBlock::plain_text)
            .collect::<Vec<_>>(),
        ["保留兼容约束（压缩摘要）"]
    );
    assert!(!coverage_known);
    assert!(
        covered_event_ids.is_none(),
        "A logical parent does not prove coverage"
    );
    assert_eq!(
        output
            .graph
            .events
            .iter()
            .filter(|event| matches!(event, ImportedEvent::Message { .. }))
            .count(),
        4
    );
    assert!(output.graph.events.iter().any(|event| matches!(event, ImportedEvent::Metadata { source_type, .. } if source_type == "compact_boundary")));
    assert!(output
        .graph
        .relations
        .iter()
        .any(|relation| relation.kind == RelationKind::Compacts
            && relation.from == "claude:boundary"
            && relation.to == "claude:old-answer"
            && relation.confidence == RelationConfidence::Explicit));
    let active = output.graph.active_leaf.as_deref().unwrap();
    assert_eq!(active, "claude:new-answer");
    let lineage = lineage_ids(&output.graph.relations, active);
    assert!(lineage.contains(&"claude:compact-summary".into()));
    assert!(!lineage.contains(&"claude:old-question".into()));
    assert!(!lineage.contains(&"claude:old-answer".into()));
    let selection = AllowedSelection::for_events(vec![
        "claude:compact-summary".into(),
        "claude:new-question".into(),
        "claude:new-answer".into(),
    ]);
    selection
        .validate_for_branch(&output.graph.events, &output.graph.relations, active)
        .unwrap();
    let projected = output
        .graph
        .events
        .iter()
        .filter_map(|event| selection.project_event(event, &output.graph.relations))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(projected.contains("保留兼容约束"));
    assert!(!projected.contains("synthetic-old-secret"));
    assert_eq!(
        output.outcome,
        ImportOutcome::Complete,
        "{:?}",
        output.issues
    );
}

#[test]
fn compact_summary_cannot_supply_a_prompt_title() {
    for (content, real_prompt) in [
        (json!("摘要不能充当标题"), true),
        (json!([{"type":"text","text":"摘要不能充当标题"}]), false),
    ] {
        let mut records = vec![
            json!({"type":"user","uuid":"summary","sessionId":"compact-title-id","isCompactSummary":true,"message":{"content":content}}),
            json!({"type":"assistant","uuid":"answer","parentUuid":"summary","message":{"content":"done"}}),
        ];
        if real_prompt {
            records.push(json!({"type":"user","uuid":"real","parentUuid":"answer","message":{"content":"新的真实问题"}}));
        }
        let output = normalized(&records);
        assert_eq!(
            output.graph.title.as_deref(),
            Some(if real_prompt {
                "新的真实问题"
            } else {
                "compact-title-id"
            })
        );
    }
}

#[test]
fn compact_summary_meta_sidechain_and_field_permissions_are_preserved() {
    let output = normalized(&[
        json!({"type":"user","uuid":"main","sessionId":"scope-id","message":{"content":"main prompt"}}),
        json!({"type":"user","uuid":"meta","isMeta":true,"isCompactSummary":true,"message":{"content":"synthetic-system-secret"}}),
        json!({"type":"user","uuid":"side","parentUuid":"main","isSidechain":true,"isCompactSummary":true,"message":{"content":"synthetic-side-secret"}}),
        json!({"type":"user","uuid":"summary","parentUuid":"main","isCompactSummary":true,"message":{"content":[{"type":"text","text":"allowed summary"},{"type":"thinking","thinking":"synthetic-reasoning-secret"}]}}),
        json!({"type":"assistant","uuid":"answer","parentUuid":"summary","message":{"content":"done"}}),
    ]);
    assert!(output.graph.events.iter().any(|event| matches!(event, ImportedEvent::Message { base, role: MessageRole::System, .. } if base.id == "claude:meta")));
    let summary = output
        .graph
        .events
        .iter()
        .find(|event| event.id() == "claude:summary")
        .unwrap();
    assert!(matches!(summary, ImportedEvent::Compaction { .. }));
    let mut selection = AllowedSelection::for_events(
        output
            .graph
            .events
            .iter()
            .map(|event| event.id().to_string())
            .collect(),
    );
    let projected = output
        .graph
        .events
        .iter()
        .filter_map(|event| selection.project_event(event, &output.graph.relations))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(projected.contains("allowed summary"));
    for excluded in [
        "synthetic-system-secret",
        "synthetic-side-secret",
        "synthetic-reasoning-secret",
    ] {
        assert!(!projected.contains(excluded), "{projected}");
    }
    selection.fields.compaction_summary = false;
    assert!(selection
        .project_event(summary, &output.graph.relations)
        .is_none());
    assert!(
        summary.searchable_text().contains("allowed summary"),
        "Local reading must retain excluded content"
    );
}

#[test]
fn unknown_compact_summary_blocks_report_loss_and_never_become_tool_calls() {
    let output = normalized(&[
        json!({"type":"user","uuid":"summary","sessionId":"unknown-id","isCompactSummary":true,"message":{"content":[{"type":"text","text":"readable summary"},{"type":"tool_use","id":"t1","name":"Shell","input":{"command":"synthetic command"}},{"type":"redacted_thinking","data":"opaque"}]}}),
        json!({"type":"assistant","uuid":"answer","parentUuid":"summary","message":{"content":"done"}}),
    ]);
    assert_eq!(output.outcome, ImportOutcome::Partial);
    assert!(!output
        .graph
        .events
        .iter()
        .any(|event| matches!(event, ImportedEvent::ToolCall { .. })));
    let summary = output
        .graph
        .events
        .iter()
        .find(|event| event.id() == "claude:summary")
        .unwrap();
    let ImportedEvent::Compaction {
        summary: blocks, ..
    } = summary
    else {
        panic!("{summary:?}")
    };
    assert!(blocks.iter().any(|block| matches!(block, ContentBlock::Opaque { source_type, .. } if source_type == "tool_use")));
    assert!(output
        .issues
        .iter()
        .any(|issue| issue.code == "unknown-compaction-block"));
    let selection = AllowedSelection::for_events(vec![summary.id().into()]);
    assert_eq!(
        selection
            .project_event(summary, &output.graph.relations)
            .as_deref(),
        Some("readable summary")
    );
}
