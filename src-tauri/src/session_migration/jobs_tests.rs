use crate::session_migration::catalog::open_catalog;
use crate::session_migration::types::{
    AssetRecord, AssetState, ContentBlock, EventBase, ImportedEvent, Inventory, MessageRole,
    NormalizedGraph, Provider, SourceEvidence, SourceSpan, TimeOrigin,
};

fn sample_adapter(native: &str, text: &str) -> AdapterOutput {
    AdapterOutput {
        evidence: SourceEvidence {
            provider: Provider::GenericFile,
            source_version: None,
            schema_fingerprint: "t".into(),
            adapter_version: "1.0.0".into(),
            platform: "test".into(),
            artifacts: vec![],
        },
        graph: NormalizedGraph {
            events: vec![ImportedEvent::Message {
                base: EventBase {
                    id: "e1".into(),
                    ordinal: 0,
                    spans: vec![],
                    timestamp: None,
                    time_origin: TimeOrigin::Unknown,
                },
                role: MessageRole::User,
                blocks: vec![ContentBlock::text(text)],
                complete: true,
            }],
            relations: vec![],
            active_leaf: Some("e1".into()),
            native_session_id: native.into(),
            title: Some(native.into()),
            cwd: None,
        },
        inventory: Inventory::default(),
        issues: vec![],
        assets: vec![],
        outcome: ImportOutcome::Complete,
    }
}

fn sample_item(native: &str, text: &str) -> CommitItem {
    prepare_test_commit_item(CommitItem {
        candidate_id: format!("candidate-{native}"),
        installation_id: "test-installation".into(),
        provider: "generic-file".into(),
        root_path: "test-root".into(),
        adapter: sample_adapter(native, text),
        prepared: None,
        options: serde_json::json!({"rawRetention":"normalized-only"}),
        mapping_project_id: None,
        explicit_reimport: false,
        create_project: false,
        import_unassigned_anyway: false,
        explicit_target: None,
        identity_association: None,
        selection_set: SelectionSet::default(),
    })
}

#[test]
fn prepared_selection_validation_uses_index_metadata_without_loading_event_bodies() {
    let _home = crate::session_migration::test_home::IsolatedAppHome::acquire("selection-metadata");
    let options = serde_json::json!({"rawRetention":"normalized-only"});
    let mut adapter = sample_adapter("selection-metadata", "body must stay cold");
    let prepared = prepare_revision(&mut adapter, &options, None, None).unwrap();
    let _ = crate::session_migration::search::take_event_body_reads();
    let entry = crate::session_migration::selection::PreviewSelection {
        preview_id: "preview-selection-metadata".into(),
        leaf_id: "e1".into(),
        selection: crate::session_migration::selection::AllowedSelection::for_events(vec![
            "e1".into()
        ]),
    };
    crate::session_migration::selection::validate_published_selection(
        &entry,
        &prepared.path,
        crate::session_migration::selection::PreparedSelectionIdentity {
            snapshot_digest: Some(&prepared.digest),
            provider: Some("generic-file"),
            native_session_id: Some("selection-metadata"),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        crate::session_migration::search::take_event_body_reads(),
        0,
        "plan/publish selection validation must not deserialize selected bodies"
    );
}

fn latest_receipt_json(conn: &Connection, job_id: &str) -> serde_json::Value {
    let raw: String = conn
        .query_row(
            "SELECT json FROM receipts WHERE job_id=?1 ORDER BY rowid DESC LIMIT 1",
            params![job_id],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn replace_latest_receipt_json(conn: &Connection, job_id: &str, value: &serde_json::Value) {
    conn.execute(
        "UPDATE receipts SET json=?2 WHERE rowid=(SELECT MAX(rowid) FROM receipts WHERE job_id=?1)",
        params![job_id, serde_json::to_string(value).unwrap()],
    )
    .unwrap();
}

#[test]
fn same_snapshot_and_options_with_different_selection_do_not_deduplicate() {
    let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
    let tmp = std::env::temp_dir().join(format!("selection-dedup-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::env::set_var("GROK_APP_HOME", &tmp);
    let conn = open_catalog().unwrap();
    let item = sample_item("same-selection-native", "same-body");
    let first = publish_item(&conn, "selection-job-a", &item, &|| false).unwrap();

    let mut changed = item.clone();
    changed.selection_set.entries[0]
        .selection
        .fields
        .message_text = false;
    let second = publish_item(&conn, "selection-job-b", &changed, &|| false).unwrap();
    assert_eq!(first.import_id, second.import_id);
    assert_ne!(first.revision_id, second.revision_id);
    assert_ne!(second.outcome.as_deref(), Some("deduplicated"));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn receipt_selection_options_identity_and_file_hash_tamper_fail_closed() {
    let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
    let tmp = std::env::temp_dir().join(format!("receipt-tamper-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::env::set_var("GROK_APP_HOME", &tmp);
    let conn = open_catalog().unwrap();
    let item = sample_item("receipt-native", "receipt-body");
    publish_item(&conn, "receipt-job", &item, &|| false).unwrap();
    let original = latest_receipt_json(&conn, "receipt-job");

    for (field, value) in [
        ("selectionHash", serde_json::json!("tampered")),
        ("optionsHash", serde_json::json!("tampered")),
        ("provider", serde_json::json!("cursor")),
        ("candidateId", serde_json::json!("other-candidate")),
        ("installationId", serde_json::json!("other-installation")),
        ("nativeSessionId", serde_json::json!("other-native")),
        ("sourceId", serde_json::json!("other-source")),
        ("rootPath", serde_json::json!("other-root")),
        ("legacyId", serde_json::json!("other-legacy")),
        ("snapshotHash", serde_json::json!("other-snapshot")),
    ] {
        let mut tampered = original.clone();
        tampered[field] = value;
        replace_latest_receipt_json(&conn, "receipt-job", &tampered);
        assert!(publish_item(&conn, "receipt-job", &item, &|| false).is_err());
    }

    let mut selection = original.clone();
    selection["selectionSet"]["entries"][0]["leafId"] = serde_json::json!("other-leaf");
    replace_latest_receipt_json(&conn, "receipt-job", &selection);
    assert!(publish_item(&conn, "receipt-job", &item, &|| false).is_err());

    let mut hashes = original.clone();
    hashes["fileHashes"]["selection.json"] = serde_json::json!("tampered");
    replace_latest_receipt_json(&conn, "receipt-job", &hashes);
    assert!(publish_item(&conn, "receipt-job", &item, &|| false).is_err());

    replace_latest_receipt_json(&conn, "receipt-job", &original);
    assert!(publish_item(&conn, "receipt-job", &item, &|| false).is_ok());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn published_selection_and_assets_tamper_fail_closed() {
    let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
    let tmp = std::env::temp_dir().join(format!("published-tamper-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::env::set_var("GROK_APP_HOME", &tmp);
    let conn = open_catalog().unwrap();
    let item = sample_item("published-native", "published-body");
    let published = publish_item(&conn, "published-job", &item, &|| false).unwrap();
    let dir = revision_dir(
        published.import_id.as_deref().unwrap(),
        published.revision_id.as_deref().unwrap(),
    )
    .unwrap();

    for name in ["selection.json", "assets.json"] {
        let path = dir.join(name);
        let original = fs::read(&path).unwrap();
        let mut tampered = original.clone();
        tampered.push(b' ');
        fs::write(&path, tampered).unwrap();
        assert!(publish_item(&conn, "published-job", &item, &|| false).is_err());
        fs::write(&path, original).unwrap();
    }
    assert!(publish_item(&conn, "published-job", &item, &|| false).is_ok());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn selected_asset_blob_bytes_must_match_the_recorded_digest() {
    let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
    let tmp = std::env::temp_dir().join(format!("selected-blob-tamper-{}", Uuid::new_v4()));
    let source = tmp.join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::env::set_var("GROK_APP_HOME", &tmp);
    fs::write(source.join("asset.bin"), b"original-asset").unwrap();

    let mut adapter = sample_adapter("asset-native", "asset-body");
    if let ImportedEvent::Message { blocks, .. } = &mut adapter.graph.events[0] {
        blocks.push(ContentBlock::Asset {
            asset_id: "asset-a".into(),
            alt: Some("asset".into()),
        });
    }
    adapter.assets.push(AssetRecord {
        asset_id: "asset-a".into(),
        digest: None,
        mime: None,
        size: 0,
        state: AssetState::Unresolved,
        source_span: SourceSpan {
            artifact_id: "fixture".into(),
            record: "record:0".into(),
            block: Some(1),
        },
        source_ref: Some("path:asset.bin".into()),
    });
    let options = serde_json::json!({"rawRetention":"normalized-only"});
    let preview_id = Uuid::new_v4().to_string();
    let prepared =
        prepare_revision(&mut adapter, &options, Some(&source), Some(&preview_id)).unwrap();
    let digest = adapter.assets[0].digest.clone().unwrap();
    write_preview_lease(
        &prepared.path,
        &PreviewLease {
            source_member: None,
            preview_id: preview_id.clone(),
            candidate_id: "asset-candidate".into(),
            source_revision: "test-source-revision".into(),
            prepare_batch_id: None,
            source_id: "asset-source".into(),
            root_path: source.to_string_lossy().into_owned(),
            legacy_id: "asset-legacy".into(),
            snapshot_digest: prepared.digest.clone(),
            expires_unix: chrono::Utc::now().timestamp() + 3600,
            retained_by_job: false,
            file_hashes: prepared.file_hashes.clone(),
        },
    )
    .unwrap();
    let entry = crate::session_migration::selection::PreviewSelection {
        preview_id,
        leaf_id: "e1".into(),
        selection: crate::session_migration::selection::AllowedSelection {
            event_ids: vec!["e1".into()],
            asset_ids: vec!["asset-a".into()],
            ..Default::default()
        },
    };
    validate_prepared_selection(
        &entry,
        &prepared.path,
        PreparedSelectionIdentity {
            candidate_id: Some("asset-candidate"),
            ..PreparedSelectionIdentity::default()
        },
    )
    .unwrap();

    fs::write(prepared.path.join("blobs").join(digest), b"tampered-asset").unwrap();
    let err = validate_prepared_selection(
        &entry,
        &prepared.path,
        PreparedSelectionIdentity {
            candidate_id: Some("asset-candidate"),
            ..PreparedSelectionIdentity::default()
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::SnapshotUnavailable);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn same_request_and_content_do_not_duplicate() {
    let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
    let tmp = std::env::temp_dir().join(format!("job-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::env::set_var("GROK_APP_HOME", &tmp);
    let conn = open_catalog().unwrap();
    let item = prepare_test_commit_item(CommitItem {
        candidate_id: "c1".into(),
        installation_id: "inst".into(),
        provider: "generic-file".into(),
        root_path: "/tmp".into(),
        adapter: sample_adapter("n1", "hello"),
        prepared: None,
        options: serde_json::json!({}),
        mapping_project_id: None,
        explicit_reimport: false,
        create_project: false,
        import_unassigned_anyway: false,
        explicit_target: None,
        identity_association: None,
        selection_set: SelectionSet::default(),
    });
    let a = publish_item(&conn, "job-a", &item, &|| false).unwrap();
    let b = publish_item(&conn, "job-b", &item, &|| false).unwrap();
    assert_eq!(a.import_id, b.import_id);
    assert_eq!(a.revision_id, b.revision_id);
    assert_eq!(b.outcome.as_deref(), Some("deduplicated"));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn divergent_snapshots_are_selectable_revisions_not_concatenated() {
    use crate::session_migration::catalog::list_revisions;
    use crate::session_migration::continuation::load_events;
    let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
    let tmp = std::env::temp_dir().join(format!("d03-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::env::set_var("GROK_APP_HOME", &tmp);
    let _ = crate::paths::ensure_app_dirs();
    let conn = open_catalog().unwrap();
    let native = "same-native-id";
    let item_a = prepare_test_commit_item(CommitItem {
        candidate_id: "c-d03-a".into(),
        installation_id: "inst-d03".into(),
        provider: "generic-file".into(),
        root_path: "/tmp".into(),
        adapter: sample_adapter(native, "alpha-rev-body"),
        prepared: None,
        options: serde_json::json!({}),
        mapping_project_id: None,
        explicit_reimport: false,
        create_project: false,
        import_unassigned_anyway: false,
        explicit_target: None,
        identity_association: None,
        selection_set: SelectionSet::default(),
    });
    let a = publish_item(&conn, "job-d03-a", &item_a, &|| false).unwrap();
    let mut item_b = item_a.clone();
    item_b.candidate_id = "c-d03-b".into();
    item_b.adapter = sample_adapter(native, "beta-rev-body");
    item_b = prepare_test_commit_item(item_b);
    let b = publish_item(&conn, "job-d03-b", &item_b, &|| false).unwrap();
    assert_eq!(a.import_id, b.import_id);
    assert_ne!(
        a.revision_id, b.revision_id,
        "divergent bodies must not collapse to one revision"
    );
    let import_id = a.import_id.clone().unwrap();
    let revs = list_revisions(&conn, &import_id).unwrap();
    assert_eq!(revs.len(), 2, "expected two selectable revisions: {revs:?}");
    assert_ne!(revs[0].source_snapshot_id, revs[1].source_snapshot_id);
    let rev_a = a.revision_id.clone().unwrap();
    let rev_b = b.revision_id.clone().unwrap();
    let dump_a = serde_json::to_string(&load_events(&import_id, &rev_a).unwrap()).unwrap();
    let dump_b = serde_json::to_string(&load_events(&import_id, &rev_b).unwrap()).unwrap();
    assert!(dump_a.contains("alpha-rev-body"));
    assert!(
        !dump_a.contains("beta-rev-body"),
        "revision A must not concatenate B: {dump_a}"
    );
    assert!(dump_b.contains("beta-rev-body"));
    assert!(
        !dump_b.contains("alpha-rev-body"),
        "revision B must not concatenate A: {dump_b}"
    );
    assert_eq!(
        crate::session_migration::engine::list_import_revisions(&import_id)
            .unwrap()
            .len(),
        2
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn tombstone_blocks_auto_resurrect_until_explicit_reimport() {
    use crate::session_migration::catalog::{get_entry, is_tombstoned};
    let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
    let tmp = std::env::temp_dir().join(format!("r05-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::env::set_var("GROK_APP_HOME", &tmp);
    let _ = crate::paths::ensure_app_dirs();
    let conn = open_catalog().unwrap();
    let native = "n-r05";
    let item = prepare_test_commit_item(CommitItem {
        candidate_id: "c-r05".into(),
        installation_id: "inst-r05".into(),
        provider: "generic-file".into(),
        root_path: "/tmp".into(),
        adapter: sample_adapter(native, "keep-body"),
        prepared: None,
        options: serde_json::json!({}),
        mapping_project_id: None,
        explicit_reimport: false,
        create_project: false,
        import_unassigned_anyway: false,
        explicit_target: None,
        identity_association: None,
        selection_set: SelectionSet::default(),
    });
    let published = publish_item(&conn, "job-r05-a", &item, &|| false).unwrap();
    let import_id = published.import_id.clone().unwrap();
    crate::session_migration::engine::delete_imported(&import_id).unwrap();
    assert!(is_tombstoned(&conn, "inst-r05", "generic-file", native).unwrap());
    let hidden = get_entry(&conn, &import_id).unwrap().unwrap();
    assert!(hidden.hidden);
    let auto = publish_item(&conn, "job-r05-scan", &item, &|| false).unwrap();
    assert_eq!(auto.phase, "tombstoned");
    assert!(auto.import_id.is_none());
    let still = get_entry(&conn, &import_id).unwrap().unwrap();
    assert!(
        still.hidden,
        "scan/commit without explicit reimport must not unhide: {still:?}"
    );
    let mut reimport = item.clone();
    reimport.candidate_id = "c-r05-re".into();
    reimport.explicit_reimport = true;
    reimport = prepare_test_commit_item(reimport);
    let restored = publish_item(&conn, "job-r05-re", &reimport, &|| false).unwrap();
    assert_eq!(restored.phase, "committed");
    assert_eq!(restored.import_id.as_deref(), Some(import_id.as_str()));
    let live = get_entry(&conn, &import_id).unwrap().unwrap();
    assert!(!live.hidden);
    assert!(!is_tombstoned(&conn, "inst-r05", "generic-file", native).unwrap());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn undo_then_recover_does_not_resurrect_or_duplicate() {
    use crate::session_migration::catalog::{get_entry, is_tombstoned};
    let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
    let tmp = std::env::temp_dir().join(format!("undo-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::env::set_var("GROK_APP_HOME", &tmp);
    let conn = open_catalog().unwrap();
    let item = prepare_test_commit_item(CommitItem {
        candidate_id: "c-undo".into(),
        installation_id: "inst-undo".into(),
        provider: "generic-file".into(),
        root_path: "/tmp".into(),
        adapter: sample_adapter("n-undo", "hello"),
        prepared: None,
        options: serde_json::json!({}),
        mapping_project_id: None,
        explicit_reimport: false,
        create_project: false,
        import_unassigned_anyway: false,
        explicit_target: None,
        identity_association: None,
        selection_set: SelectionSet::default(),
    });
    let published = publish_item(&conn, "job-undo", &item, &|| false).unwrap();
    let import_id = published.import_id.clone().unwrap();
    let mut job = JobRecord {
        job_id: "job-undo".into(),
        request_id: "req-undo".into(),
        plan_hash: "p".into(),
        plan_id: String::new(),
        state: JobState::Completed,
        state_version: 1,
        items: vec![published.clone()],
        created_at: chrono::Utc::now().to_rfc3339(),
        item_count: 1,
        next_item_cursor: None,
        progress: None,
    };
    save_job(&conn, &job).unwrap();
    let undone = undo_job(&conn, "job-undo").unwrap();
    assert_eq!(undone.items[0].phase, "undone");
    assert!(is_tombstoned(&conn, "inst-undo", "generic-file", "n-undo").unwrap());
    let _ = recover_incomplete(&conn);
    let again = publish_item(&conn, "job-undo-2", &item, &|| false).unwrap();
    assert_eq!(again.outcome.as_deref(), Some("tombstoned"));
    assert_eq!(again.import_id, None);
    let hidden = get_entry(&conn, &import_id)
        .unwrap()
        .expect("tombstoned import remains");
    assert!(hidden.hidden);
    assert_eq!(hidden.import_id, import_id);
    job.state = JobState::Running;
    save_job(&conn, &job).unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn undo_with_continuation_ref_reports_conflict_and_keeps_entry() {
    use crate::session_migration::catalog::{is_tombstoned, list_entries};
    use crate::session_migration::continuation::{
        save_continuation_ref, ContinuationSidecar, DispatchState,
    };
    let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
    let tmp = std::env::temp_dir().join(format!("undo-ref-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::env::set_var("GROK_APP_HOME", &tmp);
    let _ = crate::paths::ensure_app_dirs();
    let conn = open_catalog().unwrap();
    let item = prepare_test_commit_item(CommitItem {
        candidate_id: "c-ref".into(),
        installation_id: "inst-ref".into(),
        provider: "generic-file".into(),
        root_path: "/tmp".into(),
        adapter: sample_adapter("n-ref", "hello"),
        prepared: None,
        options: serde_json::json!({}),
        mapping_project_id: None,
        explicit_reimport: false,
        create_project: false,
        import_unassigned_anyway: false,
        explicit_target: None,
        identity_association: None,
        selection_set: SelectionSet::default(),
    });
    let published = publish_item(&conn, "job-ref", &item, &|| false).unwrap();
    let import_id = published.import_id.clone().unwrap();
    let rev = published.revision_id.clone().unwrap();
    save_continuation_ref(
        &conn,
        "sess-live",
        &ContinuationSidecar {
            import_id: import_id.clone(),
            revision_id: rev,
            branch_leaf: "e1".into(),
            allowed_selection_hash: "h".into(),
            allowed_event_ids: vec!["e1".into()],
            allowed_selection: None,
            handoff_hash: "hh".into(),
            request_id: "req-live".into(),
            dispatch_id: None,
            prompt_hash: None,
            agent_session_id: None,
            state: DispatchState::Prepared,
            grant_token: "g".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let job = JobRecord {
        job_id: "job-ref".into(),
        request_id: "req-ref".into(),
        plan_hash: "p".into(),
        plan_id: String::new(),
        state: JobState::Completed,
        state_version: 1,
        items: vec![published.clone()],
        created_at: chrono::Utc::now().to_rfc3339(),
        item_count: 1,
        next_item_cursor: None,
        progress: None,
    };
    save_job(&conn, &job).unwrap();
    let undone = undo_job(&conn, "job-ref").unwrap();
    assert_eq!(undone.items[0].phase, "conflict");
    let err = undone.items[0].error.clone().unwrap();
    assert!(err.contains("MAPPING_CONFLICT"), "{err}");
    assert!(!is_tombstoned(&conn, "inst-ref", "generic-file", "n-ref").unwrap());
    let entries = list_entries(&conn).unwrap();
    let same = entries.iter().find(|e| e.import_id == import_id).unwrap();
    assert!(!same.hidden);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn existing_revision_dedup_fails_closed_without_receipt_or_when_published_files_are_tampered() {
    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("dedup-auth");
    let conn = open_catalog().unwrap();
    let item = sample_item("dedup-native", "dedup-body");
    let first = publish_item(&conn, "job-dedup-a", &item, &|| false).unwrap();
    let import_id = first.import_id.clone().unwrap();
    let current = crate::session_migration::catalog::get_entry(&conn, &import_id)
        .unwrap()
        .unwrap()
        .current_revision
        .clone();

    conn.execute("DELETE FROM receipts", []).unwrap();
    let missing = publish_item(&conn, "job-dedup-missing-receipt", &item, &|| false);
    assert!(
        missing.is_err(),
        "dedup without a committed receipt must fail closed, got {missing:?}"
    );
    let after_missing = crate::session_migration::catalog::get_entry(&conn, &import_id)
        .unwrap()
        .unwrap();
    assert_eq!(after_missing.current_revision, current);
    drop(home);

    let home2 = crate::session_migration::test_home::IsolatedAppHome::acquire("dedup-hash");
    let conn2 = open_catalog().unwrap();
    let item2 = sample_item("dedup-hash-native", "dedup-hash-body");
    let published = publish_item(&conn2, "job-dedup-hash-a", &item2, &|| false).unwrap();
    let import2 = published.import_id.clone().unwrap();
    let rev2 = published.revision_id.clone().unwrap();
    let dir = revision_dir(&import2, &rev2).unwrap();
    let events = dir.join("events.jsonl");
    let original = fs::read(&events).unwrap();
    let mut tampered = original.clone();
    tampered.extend_from_slice(b"\n{\"tampered\":true}");
    fs::write(&events, tampered).unwrap();
    let before = crate::session_migration::catalog::get_entry(&conn2, &import2)
        .unwrap()
        .unwrap()
        .current_revision
        .clone();
    let hashed = publish_item(&conn2, "job-dedup-hash-b", &item2, &|| false);
    assert!(
        hashed.is_err(),
        "dedup after published file tamper must fail closed, got {hashed:?}"
    );
    let after = crate::session_migration::catalog::get_entry(&conn2, &import2)
        .unwrap()
        .unwrap();
    assert_eq!(after.current_revision, before);
    let _ = home2;
}

fn dir_bytes(path: &std::path::Path) -> u64 {
    let mut n = 0u64;
    let Ok(rd) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            n = n.saturating_add(dir_bytes(&p));
        } else if let Ok(meta) = p.metadata() {
            n = n.saturating_add(meta.len());
        }
    }
    n
}

#[test]
fn published_revision_records_per_file_byte_attribution() {
    use crate::session_migration::catalog::revision_dir;
    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("attr");
    let conn = open_catalog().unwrap();
    let body = "generic-flat-text-".repeat(2048);
    let source_bytes = body.len() as u64;
    let item = sample_item("attr-native", &body);
    let published = publish_item(&conn, "job-attr", &item, &|| false).unwrap();
    let import_id = published.import_id.clone().unwrap();
    let rev = published.revision_id.clone().unwrap();
    let dir = revision_dir(&import_id, &rev).unwrap();
    let names = [
        "events.jsonl",
        "manifest.json",
        "selection.json",
        "assets.json",
        "event-index.sqlite",
    ];
    let mut named = 0u64;
    for name in names {
        let n = dir.join(name).metadata().map(|m| m.len()).unwrap_or(0);
        named = named.saturating_add(n);
        eprintln!("ATTR {name}={n}");
    }
    let total = dir_bytes(&dir);
    let imports = dir_bytes(&crate::session_migration::catalog::imports_dir());
    eprintln!("ATTR source_bytes={source_bytes} revision_named={named} revision_total={total} imports_tree={imports}");
    assert!(
        total > 0 && named > 0,
        "published revision must have measurable files"
    );
    assert!(
        total >= source_bytes,
        "published tree {total} must not be smaller than source body {source_bytes}"
    );
    assert!(
        total <= source_bytes.saturating_mul(3),
        "published revision {total} exceeds 3× source {source_bytes}"
    );
    let _ = home;
}

#[test]
#[ignore]
fn publish_child_pauses_at_fault() {
    let native = std::env::var("GROK_PUBLISH_NATIVE").expect("GROK_PUBLISH_NATIVE");
    let crash_dir = std::path::PathBuf::from(
        std::env::var("GROK_PUBLISH_CRASH_DIR").expect("GROK_PUBLISH_CRASH_DIR"),
    );
    let conn = open_catalog().unwrap();
    let item = sample_item(&native, "kill-restart-body");
    let prepared = item.prepared.as_ref().expect("prepared");
    let meta = serde_json::json!({
        "path": prepared.path,
        "digest": prepared.digest,
        "file_hashes": prepared.file_hashes,
        "selection_set": item.selection_set,
        "candidate_id": item.candidate_id,
    });
    std::fs::write(crash_dir.join("prepared.json"), meta.to_string()).unwrap();
    let _ = publish_item(&conn, "job-kill-child", &item, &|| false);
}

pub(super) fn maybe_pause_publish(point: &'static str) {
    crate::session_migration::preview_lifecycle_tests::after_publish_point(point);
    if std::env::var("GROK_PUBLISH_CRASH_POINT").ok().as_deref() != Some(point) {
        return;
    }
    if let Ok(dir) = std::env::var("GROK_PUBLISH_CRASH_DIR") {
        let _ = std::fs::write(
            std::path::Path::new(&dir).join(format!("hit-{point}")),
            point.as_bytes(),
        );
    }
    loop {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

#[test]
fn twenty_kill_restarts_around_receipt_rename_and_catalog_commit() {
    let exe = std::env::current_exe().expect("test exe");
    let points = [
        "after_rename",
        "after_receipt_published",
        "after_catalog_commit",
    ];
    for i in 0..20 {
        let point = points[i % points.len()];
        let home =
            crate::session_migration::test_home::IsolatedAppHome::acquire(&format!("pub-kill-{i}"));
        let native = format!("kill-native-{i}");
        let crash_dir = home.path.join("crash");
        std::fs::create_dir_all(&crash_dir).unwrap();
        let marker = crash_dir.join(format!("hit-{point}"));
        let child_err = std::fs::File::create(crash_dir.join("child.err")).unwrap();
        let mut child = std::process::Command::new(&exe)
            .args([
                "--ignored",
                "--exact",
                "--test-threads=1",
                "--nocapture",
                "session_migration::jobs::tests::publish_child_pauses_at_fault",
            ])
            .env("GROK_APP_HOME", &home.path)
            .env("GROK_PUBLISH_NATIVE", &native)
            .env("GROK_PUBLISH_CRASH_POINT", point)
            .env("GROK_PUBLISH_CRASH_DIR", &crash_dir)
            .stdout(std::process::Stdio::from(
                child_err.try_clone().expect("clone child.err"),
            ))
            .stderr(std::process::Stdio::from(child_err))
            .spawn()
            .expect("spawn publish crash child");
        let started = std::time::Instant::now();
        while !marker.exists() {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(30),
                "child never reached {point} on cycle {i}; child.err={}",
                std::fs::read_to_string(crash_dir.join("child.err")).unwrap_or_default()
            );
            if child.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let _ = child.kill();
        let _ = child.wait();
        let conn = open_catalog().unwrap();
        let raw = std::fs::read_to_string(crash_dir.join("prepared.json")).unwrap_or_else(|e| {
            panic!(
                "cycle {i} {point} missing prepared.json: {e}; child.err={}",
                std::fs::read_to_string(crash_dir.join("child.err")).unwrap_or_default()
            )
        });
        let meta: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let path = std::path::PathBuf::from(meta["path"].as_str().unwrap());
        let digest = meta["digest"].as_str().unwrap().to_string();
        let hashes: std::collections::BTreeMap<String, String> =
            serde_json::from_value(meta["file_hashes"].clone()).unwrap();
        let mut item = sample_item(&native, "kill-restart-body");
        item.candidate_id = meta["candidate_id"]
            .as_str()
            .unwrap_or(&item.candidate_id)
            .to_string();
        item.selection_set = serde_json::from_value(meta["selection_set"].clone())
            .unwrap_or_else(|e| panic!("cycle {i} {point} selection_set: {e}"));
        item.prepared = Some(
            crate::session_migration::jobs::PreparedRevision::reopen(path, digest, hashes)
                .unwrap_or_else(|e| panic!("cycle {i} {point} reopen prepared: {e}")),
        );
        let published = publish_item(&conn, "job-kill-child", &item, &|| false)
            .unwrap_or_else(|e| panic!("cycle {i} {point} recover publish: {e}"));
        assert_eq!(
            published.phase, "committed",
            "cycle {i} {point} must finish committed, got {}",
            published.phase
        );
        let import_id = published.import_id.clone().expect("import id");
        let entries = crate::session_migration::catalog::list_entries(&conn).unwrap();
        let same: Vec<_> = entries
            .iter()
            .filter(|e| e.native_session_id == native)
            .collect();
        assert_eq!(
            same.len(),
            1,
            "cycle {i} {point} must not duplicate catalog entries"
        );
        let revs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM revisions WHERE import_id=?1",
                rusqlite::params![import_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            revs, 1,
            "cycle {i} {point} must not create duplicate revisions, got {revs}"
        );
        let dest = crate::session_migration::catalog::revision_dir(
            &import_id,
            published.revision_id.as_deref().unwrap(),
        )
        .unwrap();
        assert!(
            dest.join("events.jsonl").is_file() && dest.join("manifest.json").is_file(),
            "cycle {i} {point} dest must be a complete published revision"
        );
        let _ = home;
    }
}

crate::session_migration::test_home::fixture_home_contract_tests!(
    same_snapshot_and_options_with_different_selection_do_not_deduplicate,
    receipt_selection_options_identity_and_file_hash_tamper_fail_closed,
    published_selection_and_assets_tamper_fail_closed,
    selected_asset_blob_bytes_must_match_the_recorded_digest,
    same_request_and_content_do_not_duplicate,
    divergent_snapshots_are_selectable_revisions_not_concatenated,
    tombstone_blocks_auto_resurrect_until_explicit_reimport,
    undo_then_recover_does_not_resurrect_or_duplicate,
    undo_with_continuation_ref_reports_conflict_and_keeps_entry,
);
