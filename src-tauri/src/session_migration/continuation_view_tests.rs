// Included in continuation_create::tests to reuse the real import/create fixture.
use crate::session_migration::{continuation_lifecycle as lifecycle, continuation_view as review};

#[test]
fn draft_view_ordinary_and_invalid_ids_have_no_storage_side_effects() {
    let _home = IsolatedAppHome::acquire("draft-view-ordinary");
    let sid = uuid::Uuid::new_v4().to_string();
    assert!(review::get(&sid).unwrap().is_none());
    assert!(!crate::paths::session_dir(&sid).exists());
    assert!(!catalog::catalog_path().exists());
    for invalid in ["../outside", "", "imp:123", "C:\\outside"] {
        assert!(review::get(invalid).is_err());
        assert!(review::save(invalid, "hash", "text").is_err());
    }
    assert!(!catalog::catalog_path().exists());
    let ordinary =
        store::create_session(None, Some("Synthetic ordinary chat".into()), false).unwrap();
    let side = continuation::sidecar_path(&ordinary.id);
    let before = std::fs::read_dir(crate::paths::session_dir(&ordinary.id))
        .unwrap()
        .count();
    assert!(review::get(&ordinary.id).unwrap().is_none());
    assert!(review::save(&ordinary.id, "hash", "not a continuation").is_err());
    assert!(!side.exists());
    assert_eq!(
        std::fs::read_dir(crate::paths::session_dir(&ordinary.id))
            .unwrap()
            .count(),
        before
    );
}

#[test]
fn draft_view_edit_survives_reopen_and_is_injected_once_without_dispatch() {
    let home = IsolatedAppHome::acquire("draft-view-edit");
    let prepared = draft(&home);
    let sid = create(&prepared, "review-edit").unwrap();
    let before = review::get(&sid).unwrap().unwrap();
    assert_eq!(before.text.as_deref(), Some("edited input"));
    assert_eq!(before.import_id, prepared.import_id);
    assert_eq!(before.revision_id, prepared.revision_id);
    assert_eq!(before.leaf, prepared.leaf);
    let side_before = continuation::read_sidecar(&sid).unwrap();
    let saved = review::save(
        &sid,
        before.text_hash.as_deref().unwrap(),
        "Reviewed context 中文",
    )
    .unwrap();
    assert_eq!(saved.state, DispatchState::Prepared);
    assert_ne!(saved.text_hash, before.text_hash);
    assert_eq!(review::get(&sid).unwrap().unwrap().text, saved.text);
    assert_eq!(create(&prepared, "review-edit").unwrap(), sid);
    assert_eq!(review::get(&sid).unwrap().unwrap().text, saved.text);
    let side_after = continuation::read_sidecar(&sid).unwrap();
    assert_eq!(
        serde_json::to_value(side_before).unwrap(),
        serde_json::to_value(side_after).unwrap()
    );
    assert!(store::load_messages(&sid).is_empty());
    // Synthetic advertised-tool evidence only; no CLI or network is started.
    continuation::record_advertised_tools(
        &sid,
        super::super::capability::REQUIRED_IMPORT_TOOLS
            .iter()
            .map(|s| s.to_string())
            .collect(),
        super::super::capability::ProbeEvidence::AgentAdvertised,
    )
    .unwrap();
    match continuation::apply_continuation_first_send(&sid, "New instruction", Default::default())
        .unwrap()
    {
        continuation::ContinuationFirstSend::Handoff {
            prompt,
            handoff_text_hash,
        } => {
            assert_eq!(prompt, "Reviewed context 中文\nNew instruction");
            assert_eq!(Some(handoff_text_hash), saved.text_hash);
        }
        _ => panic!("missing first-send context"),
    }
    assert_eq!(
        review::get(&sid).unwrap().unwrap().state,
        DispatchState::Prepared
    );
    assert!(store::load_messages(&sid).is_empty());
}

#[test]
fn draft_view_compare_and_swap_rejects_stale_edits_but_accepts_identical_retry() {
    let home = IsolatedAppHome::acquire("draft-view-cas");
    let sid = create(&draft(&home), "review-cas").unwrap();
    let hash = review::get(&sid).unwrap().unwrap().text_hash.unwrap();
    let first = review::save(&sid, &hash, "First edit").unwrap();
    assert_eq!(
        review::save(&sid, &hash, "Different edit")
            .unwrap_err()
            .code,
        ErrorCode::HandoffChanged
    );
    let retry = review::save(&sid, &hash, "First edit").unwrap();
    assert_eq!(retry.text_hash, first.text_hash);
    assert_eq!(retry.text, first.text);
}

#[test]
fn draft_view_concurrent_edits_have_exactly_one_winner() {
    let home = IsolatedAppHome::acquire("draft-view-concurrent");
    let sid = create(&draft(&home), "review-race").unwrap();
    let hash = review::get(&sid).unwrap().unwrap().text_hash.unwrap();
    let barrier = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            barrier.wait();
            review::save(&sid, &hash, "Window A")
        });
        barrier.wait();
        let b = review::save(&sid, &hash, "Window B");
        [a.join().unwrap(), b]
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.iter().find_map(|r| r.as_ref().err()).unwrap().code,
        ErrorCode::HandoffChanged
    );
    let saved = review::get(&sid).unwrap().unwrap();
    assert_eq!(
        saved.text,
        results.into_iter().find_map(Result::ok).unwrap().text
    );
}

#[test]
fn draft_view_dispatch_receipt_locks_edits_even_if_sidecar_is_stale() {
    let home = IsolatedAppHome::acquire("draft-view-receipt");
    let sid = create(&draft(&home), "review-receipt").unwrap();
    let initial = review::get(&sid).unwrap().unwrap();
    let sidecar = continuation::sidecar_path(&sid);
    let prepared_bytes = std::fs::read(&sidecar).unwrap();
    lifecycle::mark_dispatching(&sid, "synthetic prompt", None).unwrap();
    for state in [
        DispatchState::Dispatching,
        DispatchState::Running,
        DispatchState::Completed,
    ] {
        if state == DispatchState::Running {
            lifecycle::mark_running(&sid).unwrap();
        }
        if state == DispatchState::Completed {
            lifecycle::mark_completed(&sid).unwrap();
        }
        // Simulate the durable receipt succeeding before its file projection.
        std::fs::write(&sidecar, &prepared_bytes).unwrap();
        let locked = review::get(&sid).unwrap().unwrap();
        assert_eq!(locked.state, state);
        assert!(locked.text.is_none() && locked.text_hash.is_none());
        assert_eq!(
            review::save(&sid, initial.text_hash.as_deref().unwrap(), "replacement")
                .unwrap_err()
                .code,
            ErrorCode::DispatchUnknown
        );
    }
    let second = create(&draft(&home), "review-unknown").unwrap();
    lifecycle::mark_dispatching(&second, "synthetic prompt", None).unwrap();
    lifecycle::mark_unknown(&second).unwrap();
    assert_eq!(
        review::get(&second).unwrap().unwrap().state,
        DispatchState::Unknown
    );
    assert!(review::save(&second, "stale", "replacement").is_err());
}

#[test]
fn draft_view_pre_dispatch_failure_remains_editable() {
    let home = IsolatedAppHome::acquire("draft-view-preflight");
    let sid = create(&draft(&home), "review-preflight").unwrap();
    lifecycle::mark_failed_before_dispatch(&sid).unwrap();
    let before = review::get(&sid).unwrap().unwrap();
    assert_eq!(before.state, DispatchState::FailedBeforeDispatch);
    let after = review::save(
        &sid,
        before.text_hash.as_deref().unwrap(),
        "Corrected context",
    )
    .unwrap();
    assert_eq!(after.state, DispatchState::FailedBeforeDispatch);
    assert_eq!(after.text.as_deref(), Some("Corrected context"));
}

#[test]
fn draft_view_deleted_session_cannot_be_recreated_by_a_late_save() {
    let home = IsolatedAppHome::acquire("draft-view-deleted");
    let sid = create(&draft(&home), "review-deleted").unwrap();
    let hash = review::get(&sid).unwrap().unwrap().text_hash.unwrap();
    store::delete_session(&sid).unwrap();
    assert!(review::save(&sid, &hash, "Late edit").is_err());
    assert!(!crate::paths::session_dir(&sid).exists());
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
}

#[test]
fn draft_view_corrupt_or_missing_files_fail_closed_and_empty_save_preserves_text() {
    let home = IsolatedAppHome::acquire("draft-view-corrupt");
    let sid = create(&draft(&home), "review-corrupt").unwrap();
    let hash = review::get(&sid).unwrap().unwrap().text_hash.unwrap();
    assert!(review::save(&sid, &hash, " \n ").is_err());
    assert_eq!(
        review::get(&sid).unwrap().unwrap().text.as_deref(),
        Some("edited input")
    );
    let path = continuation::sidecar_path(&sid).with_file_name("handoff_draft.txt");
    std::fs::write(&path, [0xff, 0xfe]).unwrap();
    assert!(review::get(&sid).is_err());
    assert!(review::save(&sid, &hash, "repair without review").is_err());
    std::fs::remove_file(&path).unwrap();
    assert!(review::get(&sid).is_err());
    std::fs::write(&path, "restored synthetic context").unwrap();
    std::fs::remove_file(continuation::sidecar_path(&sid)).unwrap();
    assert!(review::get(&sid).is_err());
    assert!(review::save(&sid, &hash, "replacement").is_err());
}

#[test]
fn draft_view_edit_invalidates_an_already_preflighted_send() {
    let home = IsolatedAppHome::acquire("draft-view-send-race");
    let sid = create(&draft(&home), "review-send-race").unwrap();
    continuation::record_advertised_tools(
        &sid,
        super::super::capability::REQUIRED_IMPORT_TOOLS
            .iter()
            .map(|s| s.to_string())
            .collect(),
        super::super::capability::ProbeEvidence::AgentAdvertised,
    )
    .unwrap();
    let before = review::get(&sid).unwrap().unwrap();
    let saved = review::save(
        &sid,
        before.text_hash.as_deref().unwrap(),
        "New reviewed context",
    )
    .unwrap();
    let turn = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        lifecycle::mark_dispatching_checked(
            &sid,
            "old context and instruction",
            None,
            before.text_hash.as_deref().unwrap(),
            Default::default(),
            &turn
        )
        .unwrap_err()
        .code,
        ErrorCode::HandoffChanged
    );
    assert_eq!(
        review::get(&sid).unwrap().unwrap().state,
        DispatchState::Prepared
    );
    lifecycle::mark_dispatching_checked(
        &sid,
        "New reviewed context\nNew instruction",
        None,
        saved.text_hash.as_deref().unwrap(),
        Default::default(),
        &turn,
    )
    .unwrap();
    assert_eq!(
        review::save(&sid, saved.text_hash.as_deref().unwrap(), "Too late")
            .unwrap_err()
            .code,
        ErrorCode::DispatchUnknown
    );
    assert!(store::load_messages(&sid).is_empty());
}

#[test]
fn draft_view_delete_racing_with_edit_never_resurrects_a_session() {
    let home = IsolatedAppHome::acquire("draft-view-delete-race");
    let sid = create(&draft(&home), "review-delete-race").unwrap();
    let hash = review::get(&sid).unwrap().unwrap().text_hash.unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let edit = scope.spawn(|| {
            barrier.wait();
            review::save(&sid, &hash, "Racing edit")
        });
        barrier.wait();
        store::delete_session(&sid).unwrap();
        let _ = edit.join().unwrap();
    });
    assert!(!crate::paths::session_dir(&sid).exists());
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
}

#[test]
fn draft_view_oversized_text_is_bounded_and_read_only_files_are_preserved() {
    let home = IsolatedAppHome::acquire("draft-view-budget");
    let sid = create(&draft(&home), "review-budget").unwrap();
    let hash = review::get(&sid).unwrap().unwrap().text_hash.unwrap();
    let path = continuation::sidecar_path(&sid).with_file_name("handoff_draft.txt");
    let original = std::fs::metadata(&path).unwrap().permissions();
    let mut read_only = original.clone();
    read_only.set_readonly(true);
    std::fs::set_permissions(&path, read_only).unwrap();
    let failed = review::save(&sid, &hash, "Must not replace");
    std::fs::set_permissions(&path, original).unwrap();
    assert!(failed.is_err());
    assert_eq!(
        review::get(&sid).unwrap().unwrap().text.as_deref(),
        Some("edited input")
    );
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(super::super::budget::JSON_RECORD_MAX_BYTES as u64 + 1)
        .unwrap();
    assert_eq!(
        review::get(&sid).unwrap_err().code,
        ErrorCode::ResourceLimit
    );
    assert_eq!(
        review::save(
            &sid,
            &hash,
            "x".repeat(super::super::budget::JSON_RECORD_MAX_BYTES + 1)
                .as_str()
        )
        .unwrap_err()
        .code,
        ErrorCode::ResourceLimit
    );
}
