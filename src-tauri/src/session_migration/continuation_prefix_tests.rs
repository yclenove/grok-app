// Reader "continue from here" must narrow the published scope, not reject an
// ancestor or grant the events after it. All fixtures use an isolated App Home.
fn prefix_history(home: &IsolatedAppHome, omit_first: bool) -> HandoffDraft {
    let file = home.path.join("prefix-history.json");
    std::fs::write(&file, r#"[{"role":"user","content":"prefix question"},{"role":"assistant","content":"prefix answer"},{"role":"user","content":"FUTURE_SENTINEL"}]"#).unwrap();
    let scanned =
        scan::scan_start(vec![(Provider::GenericFile, file.to_string_lossy().into())]).unwrap();
    let candidate = &scanned.candidates[0];
    let (preview, details) =
        engine::preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let mappings = engine::propose_mappings(std::slice::from_ref(&preview), None).unwrap();
    let mut selection = details.default_selection;
    if omit_first {
        selection.event_ids.retain(|id| id != "generic:0");
    }
    let plan = engine::plan_with_selection(vec![preview], mappings, Some(&selection)).unwrap();
    let job = engine::commit(
        &plan.plan_id,
        &plan.plan_hash,
        &uuid::Uuid::new_v4().to_string(),
    )
    .unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let job = engine::job_get(&job.job_id).unwrap();
    let import = job.items[0].import_id.as_ref().unwrap();
    engine::continue_prepare(import, None, None, None, None).unwrap()
}

#[test]
fn continuation_prefix_creates_native_chat_and_bounds_all_model_history_reads() {
    let home = IsolatedAppHome::acquire("continue-prefix");
    let full = prefix_history(&home, false);
    let prefix = engine::continue_prepare(
        &full.import_id,
        Some(&full.revision_id),
        Some("generic:1"),
        None,
        None,
    )
    .unwrap();
    assert_eq!(prefix.leaf, "generic:1");
    assert_eq!(prefix.selection.event_ids, vec!["generic:0", "generic:1"]);
    assert!(prefix.text.contains("prefix question"));
    assert!(prefix.text.contains("prefix answer"));
    assert!(!prefix.text.contains("FUTURE_SENTINEL"));
    assert!(!prefix.readable_event_ids.contains(&"generic:2".into()));
    let sid = engine::continue_create(&prefix.draft_id, &prefix.draft_hash, "prefix-create", None)
        .unwrap();
    assert_eq!(
        engine::continue_create(&prefix.draft_id, &prefix.draft_hash, "prefix-create", None)
            .unwrap(),
        sid
    );
    assert_eq!(store::load_sessions_snapshot().unwrap().len(), 1);
    assert_eq!(
        continuation::read_sidecar(&sid).unwrap().state,
        continuation::DispatchState::Prepared
    );
    assert_eq!(
        std::fs::read_to_string(continuation::sidecar_path(&sid).with_file_name("messages.json"))
            .unwrap(),
        "[]"
    );
    let grant = engine::grant_for_session(&sid).unwrap();
    assert!(grant.matches_selection());
    assert_eq!(
        grant.granted_selection().unwrap().hash(),
        prefix.selection.hash()
    );
    let read = |id: &str| {
        crate::session_migration::mcp_stdio::call_tool(
            &grant,
            "read",
            &serde_json::json!({"citation": id}),
        )
    };
    assert!(read("generic:0")
        .unwrap()
        .to_string()
        .contains("prefix question"));
    assert!(read("generic:1")
        .unwrap()
        .to_string()
        .contains("prefix answer"));
    assert!(read("generic:2").is_err());
    let search = crate::session_migration::mcp_stdio::call_tool(
        &grant,
        "search",
        &serde_json::json!({"query": "FUTURE_SENTINEL"}),
    )
    .unwrap();
    let body: serde_json::Value =
        serde_json::from_str(search["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(body["hits"].as_array().unwrap().is_empty());
    let (_, _, total) = engine::events_page(
        &full.import_id,
        Some(&full.revision_id),
        None,
        None,
        Some("generic:1"),
    )
    .unwrap();
    assert_eq!(total, 2);
    // Returning to the prefix reader must also keep its export action working.
    let markdown =
        engine::export_markdown(&full.import_id, Some(&full.revision_id), Some("generic:1"))
            .unwrap();
    assert!(markdown.contains("prefix question"));
    assert!(markdown.contains("prefix answer"));
    assert!(!markdown.contains("FUTURE_SENTINEL"));
    assert!(engine::export_for_model(
        &full.import_id,
        Some(&full.revision_id),
        Some("generic:1"),
        &full.selection
    )
    .is_err());
    let model_text = engine::export_for_model(
        &full.import_id,
        Some(&full.revision_id),
        Some("generic:1"),
        &prefix.selection,
    )
    .unwrap();
    assert!(model_text.contains("prefix answer"));
    assert!(!model_text.contains("FUTURE_SENTINEL"));
    // A serialized caller-provided full selection cannot enlarge this grant.
    let mut forged = grant.clone();
    forged.selection = Some(full.selection.clone());
    forged.allowed_event_ids = full.selection.event_ids.clone();
    // New explicit scopes reject forged serialized selections instead of
    // ignoring them and silently falling back to the complete published scope.
    assert!(forged.granted_selection().is_err());
    forged.allowed_selection_hash = full.selection.hash();
    assert!(!forged.matches_selection());
    assert!(crate::session_migration::mcp_stdio::call_tool(
        &forged,
        "read",
        &serde_json::json!({"citation": "generic:2"})
    )
    .is_err());
    catalog::open_catalog()
        .unwrap()
        .execute("UPDATE entries SET hidden=1", [])
        .unwrap();
    assert!(!grant.matches_selection());
    assert!(read("generic:0").is_err());
}

#[test]
fn continuation_prefix_first_message_is_supported_without_rewriting_published_scope() {
    let home = IsolatedAppHome::acquire("continue-prefix-first");
    let full = prefix_history(&home, false);
    let before =
        crate::session_migration::jobs::load_published_auth(&full.import_id, None, None).unwrap();
    let prefix =
        engine::continue_prepare(&full.import_id, None, Some("generic:0"), None, None).unwrap();
    assert_eq!(prefix.selection.event_ids, vec!["generic:0"]);
    assert!(prefix.text.contains("prefix question"));
    assert!(!prefix.text.contains("prefix answer"));
    assert!(!prefix.text.contains("FUTURE_SENTINEL"));
    let after =
        crate::session_migration::jobs::load_published_auth(&full.import_id, None, None).unwrap();
    assert_eq!(before.selection.hash(), after.selection.hash());
    assert_eq!(after.leaf_id, "generic:2");
    assert!(engine::continue_prepare_with_selection(
        &full.import_id,
        None,
        Some("generic:0"),
        None,
        None,
        Some(full.selection)
    )
    .is_err());
}

#[test]
fn continuation_prefix_denies_unselected_or_missing_endpoint_without_creating_a_session() {
    let home = IsolatedAppHome::acquire("continue-prefix-denied");
    let full = prefix_history(&home, true);
    for leaf in ["generic:0", "not-an-event"] {
        assert_eq!(
            engine::continue_prepare(&full.import_id, None, Some(leaf), None, None)
                .unwrap_err()
                .code,
            ErrorCode::HistoryAccessUnavailable
        );
    }
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
}

#[test]
fn continuation_prefix_recovers_same_scope_and_request_after_interrupted_creation() {
    let home = IsolatedAppHome::acquire("continue-prefix-recovery");
    let full = prefix_history(&home, false);
    let prefix =
        engine::continue_prepare(&full.import_id, None, Some("generic:0"), None, None).unwrap();
    FAIL_AT.with(|fail| *fail.borrow_mut() = Some("intent"));
    let interrupted = engine::continue_create(
        &prefix.draft_id,
        &prefix.draft_hash,
        "prefix-recovery",
        None,
    );
    FAIL_AT.with(|fail| *fail.borrow_mut() = None);
    assert!(interrupted.is_err());
    let restored =
        engine::continue_prepare(&full.import_id, None, Some("generic:0"), None, None).unwrap();
    assert_eq!(restored.draft_id, prefix.draft_id);
    assert_eq!(
        restored.creation_request_id.as_deref(),
        Some("prefix-recovery")
    );
    assert_eq!(restored.selection.hash(), prefix.selection.hash());
    assert_eq!(
        engine::continue_prepare(&full.import_id, None, Some("generic:1"), None, None)
            .unwrap_err()
            .code,
        ErrorCode::PlanStale
    );
    let sid = engine::continue_create(
        &restored.draft_id,
        &restored.draft_hash,
        "prefix-recovery",
        None,
    )
    .unwrap();
    let grant = engine::grant_for_session(&sid).unwrap();
    assert_eq!(grant.allowed_event_ids, vec!["generic:0"]);
    assert!(grant.matches_selection());
    let view = crate::session_migration::continuation_view::get(&sid)
        .unwrap()
        .unwrap();
    assert_eq!(view.leaf, "generic:0");
    assert_eq!(view.text.as_deref(), Some(prefix.text.as_str()));
}
