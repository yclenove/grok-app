use super::*;
use crate::session_migration::catalog::open_catalog;
use crate::session_migration::jobs::{publish_item, CommitItem};
use crate::session_migration::selection::{PreviewSelection, SelectionFields, SelectionSet};
use crate::session_migration::types::{
    AdapterOutput, ContentBlock, EventBase, EventRelation, ImportOutcome, ImportedEvent, Inventory,
    MessageRole, Provider as Prov, RelationConfidence, RelationKind, SourceEvidence, TimeOrigin,
    ToolInput, ToolOutcome,
};

fn home_lock() -> crate::session_migration::test_home::IsolatedAppHome {
    crate::session_migration::test_home::IsolatedAppHome::acquire("auth")
}

fn base(id: &str, ordinal: u32) -> EventBase {
    EventBase {
        id: id.into(),
        ordinal,
        spans: vec![],
        timestamp: None,
        time_origin: TimeOrigin::Unknown,
    }
}

fn parent(from: &str, to: &str) -> EventRelation {
    EventRelation {
        kind: RelationKind::Parent,
        from: from.into(),
        to: to.into(),
        confidence: RelationConfidence::Explicit,
    }
}

fn msg(id: &str, ordinal: u32, role: MessageRole, text: &str) -> ImportedEvent {
    ImportedEvent::Message {
        base: base(id, ordinal),
        role,
        blocks: vec![ContentBlock::text(text)],
        complete: true,
    }
}

fn publish_matrix() -> (String, String, AllowedSelection) {
    let suffix = uuid::Uuid::new_v4().to_string();
    let conn = open_catalog().unwrap();
    let mut assistant = msg("a1", 1, MessageRole::Assistant, "visible-assistant");
    if let ImportedEvent::Message { blocks, .. } = &mut assistant {
        blocks.push(ContentBlock::VisibleReasoning {
            text: "secret-think".into(),
        });
        blocks.push(ContentBlock::Opaque {
            source_type: "secret-opaque".into(),
            payload: None,
        });
        blocks.push(ContentBlock::Asset {
            asset_id: "secret-asset".into(),
            alt: Some("secret-asset-alt".into()),
        });
    }
    let adapter = AdapterOutput {
        evidence: SourceEvidence {
            provider: Prov::GenericFile,
            source_version: None,
            schema_fingerprint: "t".into(),
            adapter_version: "1.0.0".into(),
            platform: "test".into(),
            artifacts: vec![],
        },
        graph: crate::session_migration::types::NormalizedGraph {
            events: vec![
                msg("u1", 0, MessageRole::User, "visible-user"),
                assistant,
                msg("sys", 2, MessageRole::System, "secret-system"),
                msg("sib", 3, MessageRole::User, "secret-sibling"),
                msg("sub", 4, MessageRole::User, "secret-subagent"),
                ImportedEvent::ToolCall {
                    base: base("t1", 5),
                    call_id: "c1".into(),
                    name: "apply_patch".into(),
                    input: ToolInput::Json {
                        value: serde_json::json!({"path": "secret-tool-input"}),
                    },
                    parent_message_id: None,
                },
                ImportedEvent::ToolResult {
                    base: base("r1", 6),
                    call_id: "c1".into(),
                    blocks: vec![ContentBlock::text("secret-tool-result")],
                    outcome: ToolOutcome::Success,
                },
                ImportedEvent::Compaction {
                    base: base("k1", 7),
                    summary: vec![ContentBlock::text("secret-compact")],
                    covered_event_ids: None,
                    coverage_known: false,
                },
            ],
            relations: vec![
                parent("a1", "u1"),
                parent("sys", "a1"),
                parent("sib", "a1"),
                parent("t1", "a1"),
                parent("r1", "t1"),
                parent("k1", "r1"),
                EventRelation {
                    kind: RelationKind::Subagent,
                    from: "sub".into(),
                    to: "u1".into(),
                    confidence: RelationConfidence::Explicit,
                },
            ],
            active_leaf: Some("k1".into()),
            native_session_id: format!("n-secret-{suffix}"),
            title: Some("n-secret".into()),
            cwd: None,
        },
        inventory: Inventory::default(),
        issues: vec![],
        assets: vec![],
        outcome: ImportOutcome::Complete,
    };
    let selection = AllowedSelection {
        version: 2,
        event_ids: vec!["u1".into()],
        pinned_event_ids: vec![],
        asset_ids: vec![],
        fields: SelectionFields {
            message_text: true,
            tool_call_name: false,
            tool_call_input: false,
            tool_result_content: false,
            compaction_summary: false,
            visible_reasoning: false,
        },
        include_subagents: false,
    };
    let published = publish_item(
        &conn,
        &format!("job-secret-{suffix}"),
        &crate::session_migration::jobs::prepare_test_commit_item(CommitItem {
            candidate_id: format!("c-secret-{suffix}"),
            installation_id: format!("inst-secret-{suffix}"),
            provider: "generic-file".into(),
            root_path: "/tmp".into(),
            adapter,
            prepared: None,
            options: serde_json::json!({}),
            mapping_project_id: None,
            explicit_reimport: false,
            create_project: false,
            import_unassigned_anyway: false,
            explicit_target: None,
            identity_association: None,
            selection_set: SelectionSet::single(PreviewSelection {
                preview_id: uuid::Uuid::new_v4().to_string(),
                leaf_id: "k1".into(),
                selection: selection.clone(),
            }),
        }),
        &|| false,
    )
    .unwrap();
    (
        published.import_id.unwrap(),
        published.revision_id.unwrap(),
        selection,
    )
}

fn assert_no_secrets(body: &str) {
    for secret in [
        "secret-system",
        "secret-think",
        "secret-opaque",
        "secret-sibling",
        "secret-subagent",
        "secret-tool-input",
        "secret-tool-result",
        "secret-compact",
        "secret-asset-alt",
    ] {
        assert!(!body.contains(secret), "secret {secret} leaked in: {body}");
    }
}

fn reader_event_ids(events: &[serde_json::Value]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| event.get("id").and_then(serde_json::Value::as_str))
        .collect()
}

#[test]
fn published_local_reader_is_branch_scoped_and_reuses_full_verification() {
    let _home = home_lock();
    let (import_id, rev, _) = publish_matrix();
    crate::session_migration::jobs::take_full_published_read_verifications();

    let active = events_window(&import_id, Some(&rev), None, Some(20), Some("k1"), None).unwrap();
    assert_eq!(
        reader_event_ids(&active.events),
        ["u1", "a1", "t1", "r1", "k1"]
    );

    let context = event_context(
        &import_id,
        Some(&rev),
        Some("k1"),
        &crate::session_migration::reader::EventContextRequest {
            event_id: "t1".into(),
            before: Some(1),
            after: Some(1),
            cursor: None,
        },
    )
    .unwrap();
    assert_eq!(reader_event_ids(&context.events), ["a1", "t1", "r1"]);
    assert_eq!(context.target_index, 1);
    assert!(event_context(
        &import_id,
        Some(&rev),
        Some("k1"),
        &crate::session_migration::reader::EventContextRequest {
            event_id: "sib".into(),
            before: Some(1),
            after: Some(1),
            cursor: None,
        },
    )
    .is_err());
    assert!(event_context(
        &import_id,
        Some(&rev),
        Some("k1"),
        &crate::session_migration::reader::EventContextRequest {
            event_id: "missing".into(),
            before: None,
            after: None,
            cursor: None,
        },
    )
    .is_err());

    let sibling = event_context(
        &import_id,
        Some(&rev),
        Some("sib"),
        &crate::session_migration::reader::EventContextRequest {
            event_id: "sib".into(),
            before: Some(8),
            after: Some(8),
            cursor: None,
        },
    )
    .unwrap();
    assert_eq!(reader_event_ids(&sibling.events), ["u1", "a1", "sib"]);

    let active_search = search_imported_with_leaf_window(
        &import_id,
        Some(&rev),
        Some("k1"),
        "secret-sibling",
        None,
        Some(20),
        None,
    )
    .unwrap();
    assert!(active_search.hits.is_empty());
    let sibling_search = search_imported_with_leaf_window(
        &import_id,
        Some(&rev),
        Some("sib"),
        "secret-sibling",
        None,
        Some(20),
        None,
    )
    .unwrap();
    assert_eq!(
        sibling_search
            .hits
            .iter()
            .map(|hit| hit.event_id.as_str())
            .collect::<Vec<_>>(),
        ["sib"]
    );

    assert_eq!(
        crate::session_migration::jobs::take_full_published_read_verifications(),
        0,
        "the complete publish-time verification must be reused by reader operations"
    );
    assert!(events_window(
        &import_id,
        Some(&uuid::Uuid::new_v4().to_string()),
        None,
        Some(20),
        Some("k1"),
        None,
    )
    .is_err());

    let conn = open_catalog().unwrap();
    crate::session_migration::catalog::hide_entry(&conn, &import_id).unwrap();
    assert!(events_window(&import_id, Some(&rev), None, Some(20), Some("k1"), None,).is_err());
}

#[test]
fn published_reader_cache_detects_same_size_tamper_and_receipt_corruption() {
    let _home = home_lock();
    let (import_id, rev, _) = publish_matrix();
    let dir = crate::session_migration::catalog::revision_dir(&import_id, &rev).unwrap();
    let events_path = dir.join("events.jsonl");

    crate::session_migration::jobs::take_full_published_read_verifications();
    events_window(&import_id, Some(&rev), None, Some(20), Some("k1"), None).unwrap();
    assert_eq!(
        crate::session_migration::jobs::take_full_published_read_verifications(),
        0,
        "the first reader must reuse the publish-time verification"
    );

    let original = std::fs::read(&events_path).unwrap();
    let original_mtime = std::fs::metadata(&events_path).unwrap().modified().unwrap();
    let mut tampered = original.clone();
    let needle = b"visible-user";
    let replacement = b"changed-user";
    let offset = tampered
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("fixture text");
    tampered[offset..offset + needle.len()].copy_from_slice(replacement);
    assert_eq!(tampered.len(), original.len());
    std::fs::write(&events_path, &tampered).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&events_path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(original_mtime))
        .unwrap();

    let tamper_error =
        events_window(&import_id, Some(&rev), None, Some(20), Some("k1"), None).unwrap_err();
    assert_eq!(tamper_error.code, ErrorCode::HistoryAccessUnavailable);
    assert_eq!(
        crate::session_migration::jobs::take_full_published_read_verifications(),
        1,
        "same-size write with restored mtime must invalidate and rehash"
    );

    std::fs::write(&events_path, &original).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&events_path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(original_mtime))
        .unwrap();
    events_window(&import_id, Some(&rev), None, Some(20), Some("k1"), None).unwrap();
    assert_eq!(
        crate::session_migration::jobs::take_full_published_read_verifications(),
        1
    );

    let conn = open_catalog().unwrap();
    let mut receipt =
        crate::session_migration::jobs::latest_receipt_for_revision(&conn, &import_id, &rev)
            .unwrap()
            .unwrap();
    let original_receipt = serde_json::to_string(&receipt).unwrap();
    receipt
        .file_hashes
        .insert("manifest.json".into(), "00".repeat(32));
    conn.execute(
        "UPDATE receipts SET json=?1 WHERE receipt_id=?2",
        rusqlite::params![serde_json::to_string(&receipt).unwrap(), receipt.receipt_id],
    )
    .unwrap();
    let receipt_error =
        events_window(&import_id, Some(&rev), None, Some(20), Some("k1"), None).unwrap_err();
    assert_eq!(receipt_error.code, ErrorCode::HistoryAccessUnavailable);
    conn.execute(
        "UPDATE receipts SET json=?1 WHERE receipt_id=?2",
        rusqlite::params![original_receipt, receipt.receipt_id],
    )
    .unwrap();
}

#[test]
fn published_reader_cold_verification_is_single_flight_and_persists_across_restart() {
    let _home = home_lock();
    let (import_id, rev, _) = publish_matrix();
    crate::session_migration::jobs::clear_published_read_verification_proofs();
    crate::session_migration::jobs::clear_published_read_verification_cache();
    crate::session_migration::jobs::take_full_published_read_verifications();

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(6));
    let mut readers = Vec::new();
    for _ in 0..6 {
        let barrier = barrier.clone();
        let import_id = import_id.clone();
        let rev = rev.clone();
        readers.push(std::thread::spawn(move || {
            barrier.wait();
            events_window(&import_id, Some(&rev), None, Some(20), Some("k1"), None)
                .map(|page| page.events.len())
        }));
    }
    for reader in readers {
        assert_eq!(reader.join().unwrap().unwrap(), 5);
    }
    assert_eq!(
        crate::session_migration::jobs::take_full_published_read_verifications(),
        1,
        "concurrent cold readers must share one complete hash verification"
    );

    crate::session_migration::jobs::clear_published_read_verification_cache();
    crate::session_migration::cursor::reset_cursor_secret_cache();
    events_window(&import_id, Some(&rev), None, Some(20), Some("k1"), None).unwrap();
    assert_eq!(
        crate::session_migration::jobs::take_full_published_read_verifications(),
        0,
        "an empty process-local cache must accept the installation-signed proof"
    );

    let proof_dir = crate::session_migration::catalog::imports_dir().join(".verification-cache");
    let proof_path = std::fs::read_dir(&proof_dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .expect("persisted verification proof");
    std::fs::write(&proof_path, b"{corrupt-proof").unwrap();
    crate::session_migration::jobs::clear_published_read_verification_cache();
    events_window(&import_id, Some(&rev), None, Some(20), Some("k1"), None).unwrap();
    assert_eq!(
        crate::session_migration::jobs::take_full_published_read_verifications(),
        1,
        "a corrupt proof must fall back to a complete verification"
    );

    crate::session_migration::jobs::clear_published_read_verification_cache();
    events_window(&import_id, Some(&rev), None, Some(20), Some("k1"), None).unwrap();
    assert_eq!(
        crate::session_migration::jobs::take_full_published_read_verifications(),
        0,
        "a successful fallback must repair the persisted proof"
    );

    let mut signed_proof: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&proof_path).unwrap()).unwrap();
    signed_proof["mac"] = serde_json::Value::String("00".repeat(32));
    std::fs::write(&proof_path, serde_json::to_vec(&signed_proof).unwrap()).unwrap();
    crate::session_migration::jobs::clear_published_read_verification_cache();
    crate::session_migration::cursor::reset_cursor_secret_cache();
    events_window(&import_id, Some(&rev), None, Some(20), Some("k1"), None).unwrap();
    assert_eq!(
        crate::session_migration::jobs::take_full_published_read_verifications(),
        1,
        "an invalid proof MAC must fall back to a complete verification"
    );
}

fn deny_secret(body: &str) {
    assert_no_secrets(body);
    assert!(
        body.contains("visible-user"),
        "permitted text missing: {body}"
    );
}

#[test]
fn published_selection_is_the_only_model_facing_authorization() {
    let _home = home_lock();
    let (import_id, rev, selection) = publish_matrix();
    let md = export_markdown(&import_id, Some(&rev), Some("k1")).unwrap();
    deny_secret(&md);
    let model = export_for_model(&import_id, Some(&rev), Some("k1"), &selection).unwrap();
    deny_secret(&model);
    let expanded = AllowedSelection::for_events(vec!["u1".into(), "sib".into()]);
    assert!(export_for_model(&import_id, Some(&rev), Some("k1"), &expanded).is_err());
    let draft =
        continue_prepare_with_selection(&import_id, Some(&rev), Some("k1"), None, None, None)
            .unwrap();
    deny_secret(&draft.text);
    crate::session_migration::continuation::take_load_events_calls();
    let sid = continue_create(&draft.draft_id, &draft.draft_hash, "secret-cont", None).unwrap();
    assert_eq!(
        crate::session_migration::continuation::take_load_events_calls(),
        0
    );
    let grant = grant_for_session(&sid).expect("grant");
    assert_eq!(grant.selection.as_ref().unwrap().hash(), selection.hash());
    assert_eq!(grant.granted_selection().unwrap().hash(), selection.hash());
    let search = crate::session_migration::mcp_stdio::call_tool(
        &grant,
        "import_history_search",
        &serde_json::json!({"query": "secret-sibling"}),
    )
    .unwrap()
    .to_string();
    assert_no_secrets(&search);
    let visible = crate::session_migration::mcp_stdio::call_tool(
        &grant,
        "import_history_search",
        &serde_json::json!({"query": "visible-user"}),
    )
    .unwrap()
    .to_string();
    assert!(visible.contains("visible-user"));
    for citation in ["sys", "sib", "sub", "t1", "r1", "k1"] {
        let denied = crate::session_migration::mcp_stdio::call_tool(
            &grant,
            "import_history_read",
            &serde_json::json!({"citation": citation}),
        )
        .unwrap_err();
        assert!(
            denied.contains("HISTORY_ACCESS_UNAVAILABLE") || denied.contains("outside"),
            "{citation}: {denied}"
        );
    }
    let asset = crate::session_migration::mcp_stdio::call_tool(
        &grant,
        "import_history_asset_read",
        &serde_json::json!({"assetId": "secret-asset"}),
    )
    .unwrap_err();
    assert!(asset.contains("HISTORY_ACCESS_UNAVAILABLE"));
}

#[test]
fn old_continuation_grant_stays_on_pinned_revision_when_current_moves() {
    let _home = home_lock();
    let (import_id, rev, _) = publish_matrix();
    let draft = continue_prepare(&import_id, Some(&rev), Some("k1"), None, None).unwrap();
    let sid = continue_create(&draft.draft_id, &draft.draft_hash, "pin-cont", None).unwrap();
    let grant = grant_for_session(&sid).expect("grant");
    assert_eq!(grant.revision_id, rev);
    let conn = open_catalog().unwrap();
    conn.execute(
        "UPDATE entries SET current_revision=?1 WHERE import_id=?2",
        rusqlite::params![uuid::Uuid::new_v4().to_string(), import_id],
    )
    .unwrap();
    let visible = crate::session_migration::mcp_stdio::call_tool(
        &grant,
        "import_history_search",
        &serde_json::json!({"query": "visible-user"}),
    )
    .unwrap()
    .to_string();
    assert!(
        visible.contains("visible-user"),
        "old grant must keep reading the pinned revision after current_revision moves"
    );
    assert!(
        export_markdown(&import_id, Some(&rev), Some("k1")).is_ok(),
        "pinned revision export must still work"
    );
}

#[test]
fn hide_tamper_and_new_revision_revoke_old_grant_and_draft() {
    let _home = home_lock();
    let (import_id, rev, _) = publish_matrix();
    let draft = continue_prepare(&import_id, Some(&rev), Some("k1"), None, None).unwrap();
    let sid = continue_create(&draft.draft_id, &draft.draft_hash, "revoke-cont", None).unwrap();
    let grant = grant_for_session(&sid).expect("grant");
    let conn = open_catalog().unwrap();
    crate::session_migration::catalog::hide_entry(&conn, &import_id).unwrap();
    assert!(export_markdown(&import_id, Some(&rev), Some("k1")).is_err());
    assert!(crate::session_migration::mcp_stdio::call_tool(
        &grant,
        "import_history_search",
        &serde_json::json!({"query": "visible-user"}),
    )
    .is_err());
    conn.execute(
        "UPDATE entries SET hidden=0 WHERE import_id=?1",
        rusqlite::params![import_id],
    )
    .unwrap();
    let dir = crate::session_migration::catalog::revision_dir(&import_id, &rev).unwrap();
    std::fs::write(dir.join("selection.json"), b"{not-json").unwrap();
    assert!(export_markdown(&import_id, Some(&rev), Some("k1")).is_err());
    let (import_id2, rev2, _) = publish_matrix();
    let draft2 = continue_prepare(&import_id2, Some(&rev2), Some("k1"), None, None).unwrap();
    conn.execute(
        "UPDATE entries SET current_revision=?1 WHERE import_id=?2",
        rusqlite::params![uuid::Uuid::new_v4().to_string(), import_id2],
    )
    .unwrap();
    let err = continue_create(&draft2.draft_id, &draft2.draft_hash, "stale-rev", None).unwrap_err();
    assert_eq!(err.code, ErrorCode::HistoryAccessUnavailable);
}

fn rewrite_selection(dir: &std::path::Path, mutate: impl FnOnce(&mut SelectionSet)) {
    let path = dir.join("selection.json");
    let raw = std::fs::read(&path).unwrap();
    let mut set: SelectionSet = serde_json::from_slice(&raw).unwrap();
    mutate(&mut set);
    std::fs::write(&path, serde_json::to_vec_pretty(&set).unwrap()).unwrap();
}

fn assert_model_paths_fail_closed(
    import_id: &str,
    rev: &str,
    grant: &crate::session_migration::mcp_stdio::ImportHistoryGrant,
    original_selection: &AllowedSelection,
) {
    assert!(
        export_markdown(import_id, Some(rev), Some("k1")).is_err(),
        "export_markdown must fail closed after privilege expansion"
    );
    assert!(
        export_for_model(import_id, Some(rev), Some("k1"), original_selection).is_err(),
        "export_for_model must fail closed after privilege expansion"
    );
    assert!(
        continue_prepare(import_id, Some(rev), Some("k1"), None, None).is_err(),
        "handoff must fail closed after privilege expansion"
    );
    assert!(crate::session_migration::mcp_stdio::call_tool(
        grant,
        "import_history_search",
        &serde_json::json!({"query": "visible-user"}),
    )
    .is_err());
    assert!(crate::session_migration::mcp_stdio::call_tool(
        grant,
        "import_history_read",
        &serde_json::json!({"citation": "u1"}),
    )
    .is_err());
    assert!(crate::session_migration::mcp_stdio::call_tool(
        grant,
        "import_history_asset_read",
        &serde_json::json!({"assetId": "secret-asset"}),
    )
    .is_err());
}

#[test]
fn legal_selection_json_privilege_expansion_is_fail_closed() {
    let _home = home_lock();
    let (import_id, rev, selection) = publish_matrix();
    let dir = crate::session_migration::catalog::revision_dir(&import_id, &rev).unwrap();
    let original = std::fs::read(dir.join("selection.json")).unwrap();
    deny_secret(&export_markdown(&import_id, Some(&rev), Some("k1")).unwrap());
    let draft =
        continue_prepare_with_selection(&import_id, Some(&rev), Some("k1"), None, None, None)
            .unwrap();
    let sid = continue_create(&draft.draft_id, &draft.draft_hash, "auth-tamper", None).unwrap();
    let grant = grant_for_session(&sid).expect("grant");

    rewrite_selection(&dir, |set| {
        set.entries[0].selection.event_ids.push("sib".into());
    });
    assert_model_paths_fail_closed(&import_id, &rev, &grant, &selection);

    std::fs::write(dir.join("selection.json"), &original).unwrap();
    rewrite_selection(&dir, |set| {
        set.entries[0].selection.fields.visible_reasoning = true;
        set.entries[0].selection.fields.tool_call_input = true;
        set.entries[0].selection.event_ids.push("t1".into());
    });
    assert_model_paths_fail_closed(&import_id, &rev, &grant, &selection);

    std::fs::write(dir.join("selection.json"), &original).unwrap();
    rewrite_selection(&dir, |set| {
        set.entries[0].leaf_id = "sib".into();
    });
    assert_model_paths_fail_closed(&import_id, &rev, &grant, &selection);

    std::fs::write(dir.join("selection.json"), &original).unwrap();
    rewrite_selection(&dir, |set| {
        set.entries[0]
            .selection
            .asset_ids
            .push("secret-asset".into());
    });
    assert_model_paths_fail_closed(&import_id, &rev, &grant, &selection);

    std::fs::write(dir.join("selection.json"), &original).unwrap();
    let (import_b, rev_b, _) = publish_matrix();
    let dir_b = crate::session_migration::catalog::revision_dir(&import_b, &rev_b).unwrap();
    let other = std::fs::read(dir_b.join("selection.json")).unwrap();
    std::fs::write(dir.join("selection.json"), other).unwrap();
    assert_model_paths_fail_closed(&import_id, &rev, &grant, &selection);

    std::fs::write(dir.join("selection.json"), &original).unwrap();
    let restored = export_markdown(&import_id, Some(&rev), Some("k1")).unwrap();
    deny_secret(&restored);
    let visible = crate::session_migration::mcp_stdio::call_tool(
        &grant,
        "import_history_search",
        &serde_json::json!({"query": "visible-user"}),
    )
    .unwrap()
    .to_string();
    assert!(visible.contains("visible-user"));
}
