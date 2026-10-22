// S64: second review may only reduce the published model-readable history.
#[test]
fn continuation_narrowing_regenerates_handoff_without_excluded_messages() {
    let home = IsolatedAppHome::acquire("continue-narrowing");
    let full = prefix_history(&home, false);
    let mut selection = full.selection.clone();
    selection.event_ids.retain(|id| id != "generic:1");
    selection.pinned_event_ids.retain(|id| id != "generic:1");
    let revised = engine::continue_prepare_with_selection(
        &full.import_id,
        Some(&full.revision_id),
        Some(&full.leaf),
        None,
        None,
        Some(selection.clone()),
    )
    .expect("a strict subset must regenerate the reviewed handoff");
    assert_eq!(revised.selection.hash(), selection.hash());
    assert!(!revised.text.contains("prefix answer"));
    assert!(revised.text.contains("prefix question"));
    assert!(engine::continue_restore(&full.draft_id, &full.draft_hash).is_err());
}

#[test]
fn continuation_narrowing_binds_mcp_and_preserves_the_published_archive() {
    let home = IsolatedAppHome::acquire("continue-narrow-mcp");
    let full = prefix_history(&home, false);
    let published =
        crate::session_migration::jobs::load_published_auth(&full.import_id, None, None).unwrap();
    let mut selected = full.selection.clone();
    selected.event_ids.retain(|id| id != "generic:1");
    selected.pinned_event_ids.retain(|id| id != "generic:1");
    let revised =
        engine::continue_selection(&full.draft_id, &full.draft_hash, selected.clone()).unwrap();
    assert!(engine::continue_restore(&full.draft_id, &full.draft_hash).is_err());
    assert!(
        engine::continue_create(&full.draft_id, &full.draft_hash, "stale-narrow", None).is_err()
    );
    assert!(!revised.text.contains("prefix answer"));
    let sid = engine::continue_create(
        &revised.draft_id,
        &revised.draft_hash,
        "narrow-create",
        Some("user edit".into()),
    )
    .unwrap();
    let grant = engine::grant_for_session(&sid).unwrap();
    assert_eq!(grant.granted_selection().unwrap().hash(), selected.hash());
    let call = |method, args| crate::session_migration::mcp_stdio::call_tool(&grant, method, &args);
    assert!(call("read", serde_json::json!({"citation":"generic:0"}))
        .unwrap()
        .to_string()
        .contains("prefix question"));
    assert!(call("read", serde_json::json!({"citation":"generic:1"})).is_err());
    assert!(
        !call("search", serde_json::json!({"query":"prefix answer"}))
            .unwrap()
            .to_string()
            .contains("prefix answer")
    );
    let mut forged = grant.clone();
    forged.selection = Some(full.selection.clone());
    assert!(!forged.matches_selection());
    forged.allowed_selection_hash = full.selection.hash();
    assert!(crate::session_migration::mcp_stdio::call_tool(
        &forged,
        "read",
        &serde_json::json!({"citation":"generic:1"})
    )
    .is_err());
    let mut missing = grant;
    missing.selection = None;
    assert!(!missing.matches_selection());
    let after =
        crate::session_migration::jobs::load_published_auth(&full.import_id, None, None).unwrap();
    assert_eq!(after.selection.hash(), published.selection.hash());
}

#[test]
fn continuation_narrowing_keeps_workspace_and_rejects_reexpansion_or_stale_review() {
    let home = IsolatedAppHome::acquire("continue-narrow-workspace");
    let full = prefix_history(&home, false);
    let target = workspace(&home);
    let full =
        engine::continue_workspace(&full.draft_id, &full.draft_hash, Some(&target.id)).unwrap();
    let mut selected = full.selection.clone();
    selected.event_ids.retain(|id| id != "generic:1");
    let revised = engine::continue_selection(&full.draft_id, &full.draft_hash, selected).unwrap();
    assert_eq!(revised.workspace.as_ref().unwrap().id, target.id);
    assert_eq!(revised.project_id, Some(target.primary_project_id));
    assert!(engine::continue_selection(
        &revised.draft_id,
        &revised.draft_hash,
        full.selection.clone()
    )
    .is_err());
    let replay =
        engine::continue_selection(&full.draft_id, &full.draft_hash, revised.selection.clone())
            .unwrap();
    assert_eq!(replay.draft_id, revised.draft_id);
    let mut changed_stale = revised.selection.clone();
    changed_stale.fields.message_text = false;
    assert!(engine::continue_selection(&full.draft_id, &full.draft_hash, changed_stale).is_err());
    assert_eq!(
        engine::continue_restore(&revised.draft_id, &revised.draft_hash)
            .unwrap()
            .selection
            .hash(),
        revised.selection.hash()
    );
}

#[test]
fn continuation_narrowing_cannot_replace_an_unknown_creation_intent() {
    let home = IsolatedAppHome::acquire("continue-narrow-intent");
    let full = prefix_history(&home, false);
    FAIL_AT.with(|fail| *fail.borrow_mut() = Some("intent"));
    let interrupted =
        engine::continue_create(&full.draft_id, &full.draft_hash, "narrow-unknown", None);
    FAIL_AT.with(|fail| *fail.borrow_mut() = None);
    assert!(interrupted.is_err());
    let mut selected = full.selection.clone();
    selected.fields.message_text = false;
    assert!(
        engine::continue_selection(&full.draft_id, &full.draft_hash, selected.clone()).is_err()
    );
    assert!(engine::continue_prepare_with_selection(
        &full.import_id,
        Some(&full.revision_id),
        Some(&full.leaf),
        None,
        None,
        Some(selected)
    )
    .is_err());
    let restored = engine::continue_restore(&full.draft_id, &full.draft_hash).unwrap();
    assert_eq!(
        restored.creation_request_id.as_deref(),
        Some("narrow-unknown")
    );
    assert_eq!(restored.selection.hash(), full.selection.hash());
}

#[test]
fn continuation_narrowing_rejects_each_authority_dimension_and_malformed_ids() {
    use crate::session_migration::continuation_scope::narrow;
    let mut ceiling =
        crate::session_migration::selection::AllowedSelection::for_events(vec!["event".into()]);
    ceiling.fields = crate::session_migration::selection::SelectionFields {
        message_text: false,
        tool_call_name: false,
        tool_call_input: false,
        tool_result_content: false,
        compaction_summary: false,
        visible_reasoning: false,
    };
    for index in 0..12 {
        let mut candidate = ceiling.clone();
        match index {
            0 => candidate.event_ids.push("unknown".into()),
            1 => candidate.asset_ids.push("unknown".into()),
            2 => candidate.pinned_event_ids.push("event".into()),
            3 => candidate.include_subagents = true,
            4 => candidate.fields.message_text = true,
            5 => candidate.fields.tool_call_name = true,
            6 => candidate.fields.tool_call_input = true,
            7 => candidate.fields.tool_result_content = true,
            8 => candidate.fields.compaction_summary = true,
            9 => candidate.fields.visible_reasoning = true,
            10 => candidate.event_ids.push("event".into()),
            _ => candidate.version = 3,
        }
        assert!(narrow(&ceiling, &candidate).is_err(), "dimension {index}");
    }
}

#[test]
fn continuation_narrowing_empty_scope_and_legacy_exact_grants_fail_closed() {
    let home = IsolatedAppHome::acquire("continue-narrow-empty");
    let full = prefix_history(&home, false);
    let mut empty = full.selection.clone();
    empty.event_ids.clear();
    empty.pinned_event_ids.clear();
    empty.asset_ids.clear();
    let revised =
        engine::continue_selection(&full.draft_id, &full.draft_hash, empty.clone()).unwrap();
    assert!(revised.readable_event_ids.is_empty());
    assert!(!revised.text.contains("prefix question"));
    let auth = crate::session_migration::continuation_scope::bound_selection(
        &full.import_id,
        &full.revision_id,
        &full.leaf,
        None,
        &full.selection.hash(),
    )
    .unwrap();
    assert_eq!(auth.hash(), full.selection.hash());
    assert!(
        crate::session_migration::continuation_scope::bound_selection(
            &full.import_id,
            &full.revision_id,
            &full.leaf,
            None,
            &empty.hash()
        )
        .is_err()
    );
}

#[test]
fn continuation_narrowing_replays_a_lost_response_with_the_same_identity() {
    let home = IsolatedAppHome::acquire("continue-narrow-replay");
    let full = prefix_history(&home, false);
    let mut selected = full.selection.clone();
    selected.fields.message_text = false;
    let first =
        engine::continue_selection(&full.draft_id, &full.draft_hash, selected.clone()).unwrap();
    let second = engine::continue_selection(&full.draft_id, &full.draft_hash, selected).unwrap();
    assert_eq!(first.draft_id, second.draft_id);
    assert_eq!(first.draft_hash, second.draft_hash);
    assert_eq!(
        catalog::open_catalog()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM drafts", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn continuation_narrowing_concurrent_replays_converge_to_one_replacement() {
    let home = IsolatedAppHome::acquire("continue-narrow-concurrent");
    let full = prefix_history(&home, false);
    let mut selected = full.selection.clone();
    selected.fields.message_text = false;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let old = full.clone();
            let selected = selected.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                engine::continue_selection(&old.draft_id, &old.draft_hash, selected)
            })
        })
        .collect();
    let outcomes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(outcomes.iter().any(Result::is_ok));
    let replay = engine::continue_selection(&full.draft_id, &full.draft_hash, selected).unwrap();
    for completed in outcomes.into_iter().filter_map(Result::ok) {
        assert_eq!(completed.draft_id, replay.draft_id);
        assert_eq!(completed.draft_hash, replay.draft_hash);
    }
    assert_eq!(
        catalog::open_catalog()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM drafts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(engine::continue_restore(&full.draft_id, &full.draft_hash).is_err());
}
