//! Durable, synthetic import fixture for the ACP wire capture tests.
use crate::session_migration::{
    catalog,
    continuation::AllowedSelection,
    engine, jobs,
    mcp_stdio::ImportHistoryGrant,
    selection::{PreviewSelection, SelectionSet},
    types::*,
};

pub(super) const PINNED: &str = "最初约束：保持中文输出，不提交代码，保留用户修改。";
pub(super) const EXCLUDED: &str = "SYNTHETIC-EXCLUDED-SECRET-NOT-A-CREDENTIAL";

pub(super) fn published_grant() -> ImportHistoryGrant {
    let suffix = uuid::Uuid::new_v4().to_string();
    let events: Vec<_> = (0..=220)
        .map(|ordinal| {
            let text = match ordinal {
                0 => PINNED.to_string(),
                1 => EXCLUDED.to_string(),
                _ => format!(
                    "第 {ordinal} 条合成消息：{}",
                    "这是用于预算裁剪验证的合成内容。".repeat(40)
                ),
            };
            ImportedEvent::Message {
                base: EventBase {
                    id: format!("event-{ordinal}"),
                    ordinal,
                    spans: vec![],
                    timestamp: None,
                    time_origin: TimeOrigin::Unknown,
                },
                role: if ordinal % 2 == 0 {
                    MessageRole::User
                } else {
                    MessageRole::Assistant
                },
                blocks: vec![ContentBlock::text(&text)],
                complete: true,
            }
        })
        .collect();
    let selection = AllowedSelection {
        event_ids: (0..=220)
            .filter(|n| *n != 1)
            .map(|n| format!("event-{n}"))
            .collect(),
        pinned_event_ids: vec!["event-0".into()],
        ..Default::default()
    };
    let item = jobs::prepare_test_commit_item(jobs::CommitItem {
        candidate_id: format!("candidate-{suffix}"),
        installation_id: format!("installation-{suffix}"),
        provider: "generic-file".into(),
        root_path: crate::paths::app_data_root().to_string_lossy().into_owned(),
        adapter: AdapterOutput {
            evidence: SourceEvidence {
                provider: Provider::GenericFile,
                source_version: None,
                schema_fingerprint: "synthetic-acp-capture".into(),
                adapter_version: "1.0.0".into(),
                platform: "synthetic".into(),
                artifacts: vec![],
            },
            graph: NormalizedGraph {
                events,
                relations: (1..=220)
                    .map(|n| EventRelation {
                        kind: RelationKind::Parent,
                        from: format!("event-{n}"),
                        to: format!("event-{}", n - 1),
                        confidence: RelationConfidence::Explicit,
                    })
                    .collect(),
                active_leaf: Some("event-220".into()),
                native_session_id: format!("synthetic-{suffix}"),
                title: Some("Synthetic history".into()),
                cwd: None,
            },
            inventory: Inventory::default(),
            issues: vec![],
            assets: vec![],
            outcome: ImportOutcome::Complete,
        },
        prepared: None,
        options: serde_json::json!({}),
        mapping_project_id: None,
        explicit_reimport: false,
        create_project: false,
        import_unassigned_anyway: false,
        explicit_target: None,
        identity_association: None,
        selection_set: SelectionSet::single(PreviewSelection {
            preview_id: suffix.clone(),
            leaf_id: "event-220".into(),
            selection,
        }),
    });
    let conn = catalog::open_catalog().unwrap();
    let published = jobs::publish_item(&conn, &format!("job-{suffix}"), &item, &|| false).unwrap();
    let draft = engine::continue_prepare(
        &published.import_id.unwrap(),
        published.revision_id.as_deref(),
        Some("event-220"),
        None,
        None,
    )
    .unwrap();
    assert!(draft.text.contains(PINNED));
    assert!(!draft.text.contains(EXCLUDED));
    assert_eq!(draft.coverage, "truncated-to-budget");
    assert!(draft.fragment_event_ids.len() < draft.readable_event_ids.len());
    let sid = engine::continue_create(
        &draft.draft_id,
        &draft.draft_hash,
        &format!("request-{suffix}"),
        None,
    )
    .unwrap();
    engine::grant_with_selection(&sid).unwrap()
}
