//! First-send integrity regression tests. All data is synthetic and isolated.
use super::{
    catalog, continuation::*, errors::ErrorCode, mcp_stdio::SessionMcpPolicy,
    test_home::IsolatedAppHome,
};

fn prepared() -> (String, ContinuationSidecar) {
    let sid = uuid::Uuid::new_v4().to_string();
    // Recovery of a legacy receipt repairs an existing native session only.
    crate::store::create_session_with_id(Some(sid.clone()), None, None, false).unwrap();
    let side = ContinuationSidecar {
        import_id: format!("imp:{}", uuid::Uuid::new_v4()),
        revision_id: "synthetic-revision".into(),
        branch_leaf: "synthetic-leaf".into(),
        request_id: format!("request-{sid}"),
        handoff_hash: "synthetic-draft-hash".into(),
        runtime_capability: "fragment-only".into(),
        grant_token: "synthetic-grant".into(),
        ..Default::default()
    };
    write_sidecar(&sid, &side).unwrap();
    write_draft_text(&sid, "Synthetic selected history").unwrap();
    (sid, side)
}

fn send(sid: &str) -> Result<ContinuationFirstSend, super::errors::ImportError> {
    apply_continuation_first_send(sid, "continue", SessionMcpPolicy::default())
}

#[test]
fn corrupt_sidecar_never_becomes_an_ordinary_send() {
    let _home = IsolatedAppHome::acquire("integrity-corrupt");
    let (sid, _) = prepared();
    std::fs::write(sidecar_path(&sid), b"{broken-json").unwrap();
    assert_eq!(send(&sid).unwrap_err().code, ErrorCode::SnapshotUnavailable);
}

#[test]
fn missing_handoff_blocks_instead_of_dropping_history() {
    let _home = IsolatedAppHome::acquire("integrity-handoff");
    let (sid, _) = prepared();
    std::fs::remove_file(sidecar_path(&sid).with_file_name("handoff_draft.txt")).unwrap();
    assert_eq!(
        send(&sid).unwrap_err().code,
        ErrorCode::HistoryAccessUnavailable
    );
}

#[test]
fn missing_sidecar_with_durable_reference_cannot_send_as_ordinary() {
    let _home = IsolatedAppHome::acquire("integrity-missing");
    let (sid, side) = prepared();
    save_continuation_ref(&catalog::open_catalog().unwrap(), &sid, &side).unwrap();
    std::fs::remove_file(sidecar_path(&sid)).unwrap();
    assert_eq!(
        send(&sid).unwrap_err().code,
        ErrorCode::HistoryAccessUnavailable
    );
}

#[test]
fn uncertain_or_inflight_dispatch_blocks_a_new_send() {
    let _home = IsolatedAppHome::acquire("integrity-state");
    let (sid, mut side) = prepared();
    for state in [
        DispatchState::Unknown,
        DispatchState::Dispatching,
        DispatchState::Running,
    ] {
        side.state = state;
        write_sidecar(&sid, &side).unwrap();
        assert_eq!(send(&sid).unwrap_err().code, ErrorCode::DispatchUnknown);
    }
}

#[test]
fn corrupt_evidence_is_not_replaced_by_a_tool_update() {
    let _home = IsolatedAppHome::acquire("integrity-evidence");
    let (sid, _) = prepared();
    std::fs::write(sidecar_path(&sid), b"{broken-json").unwrap();
    assert!(record_advertised_tools(
        &sid,
        vec![],
        super::capability::ProbeEvidence::SelfToolsList
    )
    .is_err());
    assert_eq!(std::fs::read(sidecar_path(&sid)).unwrap(), b"{broken-json");
}

#[test]
fn simultaneous_dispatch_claims_have_exactly_one_winner() {
    let _home = IsolatedAppHome::acquire("integrity-claims");
    for _ in 0..4 {
        let (sid, _) = prepared();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let winners = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let barrier = std::sync::Arc::clone(&barrier);
                    let sid = sid.clone();
                    scope.spawn(move || {
                        barrier.wait();
                        mark_dispatching(&sid, "synthetic prompt", Some("synthetic-agent")).is_ok()
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .filter(|winner| *winner)
                .count()
        });
        assert_eq!(winners, 1);
        assert_eq!(
            read_sidecar_strict(&sid).unwrap().unwrap().state,
            DispatchState::Dispatching
        );
    }
}

#[test]
fn idempotent_create_does_not_treat_corruption_as_a_missing_sidecar() {
    let _home = IsolatedAppHome::acquire("integrity-repair-corrupt");
    let (sid, side) = prepared();
    save_continuation_ref(&catalog::open_catalog().unwrap(), &sid, &side).unwrap();
    std::fs::write(sidecar_path(&sid), b"{broken-json").unwrap();
    let error = super::engine::continue_create(
        "already-created",
        &side.handoff_hash,
        &side.request_id,
        None,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::SnapshotUnavailable);
    assert_eq!(std::fs::read(sidecar_path(&sid)).unwrap(), b"{broken-json");
    assert_eq!(
        std::fs::read_to_string(sidecar_path(&sid).with_file_name("handoff_draft.txt")).unwrap(),
        "Synthetic selected history"
    );
}

#[test]
fn ordinary_send_and_callbacks_do_not_create_import_or_session_files() {
    let _home = IsolatedAppHome::acquire("integrity-ordinary");
    let sid = uuid::Uuid::new_v4().to_string();
    assert!(matches!(
        send(&sid).unwrap(),
        ContinuationFirstSend::Passthrough
    ));
    on_continuation_turn_success(&sid).unwrap();
    on_continuation_turn_failure(&sid).unwrap();
    mark_failed_before_dispatch(&sid).unwrap();
    mark_running(&sid).unwrap();
    record_advertised_tools(
        &sid,
        vec![],
        super::capability::ProbeEvidence::SelfToolsList,
    )
    .unwrap();
    assert!(!catalog::catalog_path().exists());
    assert!(!sidecar_path(&sid).parent().unwrap().exists());
}

#[test]
fn recovery_after_dispatch_never_rebuilds_a_prepared_session() {
    let _home = IsolatedAppHome::acquire("integrity-repair-dispatch");
    let (sid, initial) = prepared();
    save_continuation_ref(&catalog::open_catalog().unwrap(), &sid, &initial).unwrap();
    mark_dispatching(&sid, "prompt", Some("synthetic-agent")).unwrap();
    let dispatch_id = read_sidecar_strict(&sid).unwrap().unwrap().dispatch_id;
    std::fs::remove_file(sidecar_path(&sid)).unwrap();
    let recovered = super::engine::continue_create(
        "already-created",
        &initial.handoff_hash,
        &initial.request_id,
        None,
    )
    .unwrap();
    assert_eq!(recovered, sid);
    assert_eq!(send(&sid).unwrap_err().code, ErrorCode::DispatchUnknown);
    assert_eq!(
        read_sidecar_strict(&sid).unwrap().unwrap().dispatch_id,
        dispatch_id
    );
}

#[test]
fn stale_sidecar_cannot_override_persisted_dispatch_evidence() {
    let _home = IsolatedAppHome::acquire("integrity-stale-file");
    let (sid, initial) = prepared();
    save_continuation_ref(&catalog::open_catalog().unwrap(), &sid, &initial).unwrap();
    mark_dispatching(&sid, "prompt", None).unwrap();
    // Simulate a crash between the write-ahead receipt and sidecar replacement.
    std::fs::write(sidecar_path(&sid), serde_json::to_vec(&initial).unwrap()).unwrap();
    assert_eq!(send(&sid).unwrap_err().code, ErrorCode::DispatchUnknown);
    assert_eq!(
        mark_dispatching(&sid, "duplicate", None).unwrap_err().code,
        ErrorCode::DispatchUnknown
    );
    assert!(write_draft_text(&sid, "changed history").is_err());
}

#[test]
fn running_cannot_be_downgraded_to_safe_retry() {
    let _home = IsolatedAppHome::acquire("integrity-running");
    let (sid, _) = prepared();
    mark_dispatching(&sid, "prompt", None).unwrap();
    mark_running(&sid).unwrap();
    mark_failed_before_dispatch(&sid).unwrap();
    assert_eq!(send(&sid).unwrap_err().code, ErrorCode::DispatchUnknown);
}

#[test]
fn corrupt_sidecar_is_reported_by_turn_callbacks() {
    let _home = IsolatedAppHome::acquire("integrity-corrupt-callback");
    let (sid, _) = prepared();
    std::fs::write(sidecar_path(&sid), b"{broken-json").unwrap();
    assert!(on_continuation_turn_success(&sid).is_err());
    assert!(on_continuation_turn_failure(&sid).is_err());
}

#[test]
fn recovery_keeps_terminal_and_safe_retry_states_and_request_identity() {
    let _home = IsolatedAppHome::acquire("integrity-repair-terminal");
    for state in [
        DispatchState::Completed,
        DispatchState::Unknown,
        DispatchState::FailedBeforeDispatch,
    ] {
        let (sid, initial) = prepared();
        save_continuation_ref(&catalog::open_catalog().unwrap(), &sid, &initial).unwrap();
        mark_dispatching(&sid, "prompt", Some("synthetic-agent")).unwrap();
        match state {
            DispatchState::Completed => mark_completed(&sid).unwrap(),
            DispatchState::Unknown => mark_unknown(&sid).unwrap(),
            _ => mark_failed_before_dispatch(&sid).unwrap(),
        }
        std::fs::remove_file(sidecar_path(&sid)).unwrap();
        assert_eq!(
            super::engine::continue_create(
                "already-created",
                &initial.handoff_hash,
                &initial.request_id,
                None
            )
            .unwrap(),
            sid
        );
        assert_eq!(read_sidecar_strict(&sid).unwrap().unwrap().state, state);
        match state {
            DispatchState::Completed => assert!(matches!(
                send(&sid).unwrap(),
                ContinuationFirstSend::Passthrough
            )),
            DispatchState::Unknown => {
                assert_eq!(send(&sid).unwrap_err().code, ErrorCode::DispatchUnknown)
            }
            _ => {
                assert!(matches!(
                    send(&sid).unwrap(),
                    ContinuationFirstSend::Handoff { .. }
                ));
                mark_dispatching(&sid, "safe retry", None).unwrap();
            }
        }
        assert_eq!(
            read_sidecar_strict(&sid).unwrap().unwrap().request_id,
            initial.request_id
        );
    }
}

#[test]
fn missing_legacy_sidecar_is_not_assumed_safe_to_rebuild() {
    let _home = IsolatedAppHome::acquire("integrity-legacy");
    let (sid, side) = prepared();
    let conn = catalog::open_catalog().unwrap();
    save_continuation_ref(&conn, &sid, &side).unwrap();
    conn.execute("UPDATE continuation_refs SET json=json_remove(json, '$.dispatchReceiptVersion') WHERE app_session_id=?1", [&sid]).unwrap();
    std::fs::remove_file(sidecar_path(&sid)).unwrap();
    assert_eq!(
        super::engine::continue_create(
            "already-created",
            &side.handoff_hash,
            &side.request_id,
            None
        )
        .unwrap_err()
        .code,
        ErrorCode::DispatchUnknown
    );
    assert!(!sidecar_path(&sid).exists());
}

#[test]
fn legacy_sidecar_state_survives_receipt_upgrade() {
    let _home = IsolatedAppHome::acquire("integrity-legacy-upgrade");
    let (sid, mut side) = prepared();
    let conn = catalog::open_catalog().unwrap();
    save_continuation_ref(&conn, &sid, &side).unwrap();
    conn.execute("UPDATE continuation_refs SET json=json_remove(json, '$.dispatchReceiptVersion') WHERE app_session_id=?1", [&sid]).unwrap();
    side.state = DispatchState::Running;
    side.dispatch_id = Some("old-dispatch".into());
    write_sidecar(&sid, &side).unwrap();
    assert_eq!(send(&sid).unwrap_err().code, ErrorCode::DispatchUnknown);
    on_continuation_turn_failure(&sid).unwrap();
    let (_, receipt) = load_ref_by_request(&conn, &side.request_id)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.state, DispatchState::Unknown);
    assert_eq!(receipt.dispatch_id.as_deref(), Some("old-dispatch"));
}

#[test]
fn failed_receipt_commit_aborts_dispatch_without_changing_sidecar() {
    let _home = IsolatedAppHome::acquire("integrity-receipt-write");
    let (sid, initial) = prepared();
    let conn = catalog::open_catalog().unwrap();
    save_continuation_ref(&conn, &sid, &initial).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_receipt BEFORE UPDATE ON continuation_refs BEGIN SELECT RAISE(ABORT, 'synthetic disk failure'); END;").unwrap();
    assert!(mark_dispatching(&sid, "prompt", None).is_err());
    assert_eq!(
        read_sidecar_strict(&sid).unwrap().unwrap().state,
        DispatchState::Prepared
    );
    assert_eq!(
        load_ref_by_request(&conn, &initial.request_id)
            .unwrap()
            .unwrap()
            .1
            .state,
        DispatchState::Prepared
    );
}

#[test]
fn receipt_commit_survives_failed_sidecar_replacement() {
    let _home = IsolatedAppHome::acquire("integrity-projection-write");
    let (sid, initial) = prepared();
    let conn = catalog::open_catalog().unwrap();
    save_continuation_ref(&conn, &sid, &initial).unwrap();
    // A directory at the temp file path makes replacement fail on every OS.
    let temp = sidecar_path(&sid).with_file_name("import_continuation.json.tmp");
    std::fs::create_dir(&temp).unwrap();
    assert!(mark_dispatching(&sid, "prompt", None).is_err());
    assert_eq!(
        read_sidecar_strict(&sid).unwrap().unwrap().state,
        DispatchState::Prepared
    );
    assert_eq!(
        load_ref_by_request(&conn, &initial.request_id)
            .unwrap()
            .unwrap()
            .1
            .state,
        DispatchState::Dispatching
    );
    assert_eq!(send(&sid).unwrap_err().code, ErrorCode::DispatchUnknown);
    std::fs::remove_dir(&temp).unwrap();
    assert_eq!(
        super::engine::continue_create(
            "already-created",
            &initial.handoff_hash,
            &initial.request_id,
            None
        )
        .unwrap(),
        sid
    );
    assert_eq!(
        read_sidecar_strict(&sid).unwrap().unwrap().state,
        DispatchState::Dispatching
    );
}

#[test]
fn tool_updates_cannot_erase_a_concurrent_dispatch() {
    let _home = IsolatedAppHome::acquire("integrity-tools-race");
    let (sid, _) = prepared();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let claim = scope.spawn(|| {
            barrier.wait();
            mark_dispatching(&sid, "prompt", Some("synthetic-agent")).unwrap();
        });
        let tools = scope.spawn(|| {
            barrier.wait();
            for _ in 0..16 {
                record_advertised_tools(
                    &sid,
                    vec![],
                    super::capability::ProbeEvidence::SelfToolsList,
                )
                .unwrap();
            }
        });
        claim.join().unwrap();
        tools.join().unwrap();
    });
    let side = read_sidecar_strict(&sid).unwrap().unwrap();
    assert_eq!(side.state, DispatchState::Dispatching);
    assert!(side.dispatch_id.is_some());
    assert_eq!(side.agent_session_id.as_deref(), Some("synthetic-agent"));
}

#[test]
fn unreadable_catalog_cannot_prove_an_ordinary_send() {
    let _home = IsolatedAppHome::acquire("integrity-catalog-corrupt");
    let (sid, _) = prepared();
    std::fs::remove_file(sidecar_path(&sid)).unwrap();
    std::fs::remove_file(sidecar_path(&sid).with_file_name("handoff_draft.txt")).unwrap();
    std::fs::write(catalog::catalog_path(), b"not a SQLite database").unwrap();
    assert!(send(&sid).is_err());
}

#[test]
fn missing_file_recovery_does_not_restore_old_tool_exposure() {
    let _home = IsolatedAppHome::acquire("integrity-recover-tools");
    let (sid, initial) = prepared();
    record_advertised_tools(
        &sid,
        vec!["old-tool".into()],
        super::capability::ProbeEvidence::AgentAdvertised,
    )
    .unwrap();
    mark_dispatching(&sid, "prompt", None).unwrap();
    mark_failed_before_dispatch(&sid).unwrap();
    std::fs::remove_file(sidecar_path(&sid)).unwrap();
    super::engine::continue_create(
        "already-created",
        &initial.handoff_hash,
        &initial.request_id,
        None,
    )
    .unwrap();
    let recovered = read_sidecar_strict(&sid).unwrap().unwrap();
    assert!(recovered.advertised_tool_names.is_empty());
    assert_eq!(
        recovered.probe_evidence,
        Some(super::capability::ProbeEvidence::SelfToolsList)
    );
}
