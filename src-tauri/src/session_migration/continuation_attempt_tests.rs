//! Callback ownership at the safe-retry boundary. All data is synthetic.
use super::{continuation::*, mcp_stdio::SessionMcpPolicy, test_home::IsolatedAppHome};

fn prepared() -> String {
    let sid = uuid::Uuid::new_v4().to_string();
    write_sidecar(
        &sid,
        &ContinuationSidecar {
            import_id: format!("imp:{}", uuid::Uuid::new_v4()),
            revision_id: "synthetic-revision".into(),
            branch_leaf: "synthetic-leaf".into(),
            request_id: format!("request-{sid}"),
            handoff_hash: "synthetic-draft".into(),
            runtime_capability: "fragment-only".into(),
            grant_token: "synthetic-grant".into(),
            ..Default::default()
        },
    )
    .unwrap();
    write_draft_text(&sid, "synthetic selected history").unwrap();
    sid
}

fn claim(sid: &str) -> String {
    let policy = SessionMcpPolicy::default();
    let ContinuationFirstSend::Handoff {
        prompt,
        handoff_text_hash,
    } = apply_continuation_first_send(sid, "continue", policy).unwrap()
    else {
        panic!("expected first handoff")
    };
    let turn_id = uuid::Uuid::new_v4().to_string();
    mark_dispatching_checked(sid, &prompt, None, &handoff_text_hash, policy, &turn_id).unwrap();
    assert_eq!(
        read_sidecar_strict(sid)
            .unwrap()
            .unwrap()
            .dispatch_id
            .as_deref(),
        Some(turn_id.as_str())
    );
    turn_id
}

fn fail_before_dispatch(sid: &str, attempt: &str) {
    notify_failed_before_dispatch(sid, attempt);
}

fn late_failure(sid: &str, attempt: &str) {
    notify_turn_failure(sid, attempt);
}

#[test]
fn delayed_failure_after_safe_retry_cannot_poison_a_new_claim() {
    let _home = IsolatedAppHome::acquire("attempt-late-error");
    let sid = prepared();
    let first = claim(&sid);
    // turn.rs: missing agent session id -> failed-before-dispatch, then the
    // common Err path releases runtime flags and persists before notifying.
    fail_before_dispatch(&sid, &first);
    let second = claim(&sid);
    assert_ne!(first, second);
    late_failure(&sid, &first);
    let side = read_sidecar_strict(&sid).unwrap().unwrap();
    assert_eq!(side.dispatch_id.as_deref(), Some(second.as_str()));
    assert_eq!(side.state, DispatchState::Dispatching);
}

#[test]
fn delayed_failure_after_safe_retry_cannot_poison_the_new_running_turn() {
    let _home = IsolatedAppHome::acquire("attempt-late-running-error");
    let sid = prepared();
    let first = claim(&sid);
    fail_before_dispatch(&sid, &first);
    let second = claim(&sid);
    mark_running_for_turn(&sid, &second).unwrap();
    late_failure(&sid, &first);
    let side = read_sidecar_strict(&sid).unwrap().unwrap();
    assert_eq!(side.dispatch_id.as_deref(), Some(second.as_str()));
    assert_eq!(side.state, DispatchState::Running);
}

#[test]
fn callbacks_without_a_claim_do_not_consume_prepared_or_ordinary_sessions() {
    let _home = IsolatedAppHome::acquire("attempt-unclaimed");
    let ordinary = uuid::Uuid::new_v4().to_string();
    notify_turn_success(&ordinary, "unclaimed");
    notify_turn_failure(&ordinary, "unclaimed");
    notify_failed_before_dispatch(&ordinary, "unclaimed");
    assert!(!super::catalog::catalog_path().exists());
    assert!(!sidecar_path(&ordinary).parent().unwrap().exists());
    let sid = prepared();
    notify_turn_success(&sid, "unclaimed");
    notify_turn_failure(&sid, "unclaimed");
    notify_failed_before_dispatch(&sid, "unclaimed");
    assert_eq!(
        read_sidecar_strict(&sid).unwrap().unwrap().state,
        DispatchState::Prepared
    );
}

#[test]
fn stale_callbacks_and_start_cannot_take_over_a_retry() {
    let _home = IsolatedAppHome::acquire("attempt-stale-start");
    let sid = prepared();
    let old = claim(&sid);
    fail_before_dispatch(&sid, &old);
    let current = claim(&sid);
    notify_turn_success(&sid, &old);
    notify_failed_before_dispatch(&sid, &old);
    assert!(mark_running_for_turn(&sid, &old).is_err());
    assert!(mark_running_for_turn(&sid, "").is_err());
    assert_eq!(
        read_sidecar_strict(&sid).unwrap().unwrap().state,
        DispatchState::Dispatching
    );
    mark_running_for_turn(&sid, &current).unwrap();
    assert!(
        mark_running_for_turn(&sid, &current).is_err(),
        "cannot authorize ACP twice"
    );
    notify_failed_before_dispatch(&sid, &current);
    assert_eq!(
        read_sidecar_strict(&sid).unwrap().unwrap().state,
        DispatchState::Running
    );
    notify_turn_failure(&sid, &current);
    assert_eq!(
        read_sidecar_strict(&sid).unwrap().unwrap().state,
        DispatchState::Unknown
    );
    assert!(apply_continuation_first_send(&sid, "retry", SessionMcpPolicy::default()).is_err());
}

#[test]
fn current_success_is_terminal_and_does_not_repeat_handoff() {
    let _home = IsolatedAppHome::acquire("attempt-success");
    let sid = prepared();
    let current = claim(&sid);
    mark_running_for_turn(&sid, &current).unwrap();
    notify_turn_success(&sid, &current);
    notify_turn_failure(&sid, &current);
    notify_failed_before_dispatch(&sid, &current);
    assert_eq!(
        read_sidecar_strict(&sid).unwrap().unwrap().state,
        DispatchState::Completed
    );
    assert!(matches!(
        apply_continuation_first_send(&sid, "next", SessionMcpPolicy::default()).unwrap(),
        ContinuationFirstSend::Passthrough
    ));
}

#[test]
fn failed_before_dispatch_attempt_cannot_start_after_cancellation() {
    let _home = IsolatedAppHome::acquire("attempt-cancel-start");
    let sid = prepared();
    let current = claim(&sid);
    fail_before_dispatch(&sid, &current);
    assert!(mark_running_for_turn(&sid, &current).is_err());
    notify_turn_failure(&sid, &current);
    assert_eq!(
        read_sidecar_strict(&sid).unwrap().unwrap().state,
        DispatchState::FailedBeforeDispatch
    );
}

#[test]
fn running_receipt_write_failure_is_returned_before_acp_can_start() {
    let _home = IsolatedAppHome::acquire("attempt-running-write");
    let sid = prepared();
    let current = claim(&sid);
    let conn = super::catalog::open_catalog().unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_running BEFORE UPDATE ON continuation_refs
         BEGIN SELECT RAISE(FAIL, 'synthetic running persistence failure'); END;",
    )
    .unwrap();
    assert!(mark_running_for_turn(&sid, &current).is_err());
    assert_eq!(
        read_sidecar_strict(&sid).unwrap().unwrap().state,
        DispatchState::Dispatching
    );
    conn.execute_batch("DROP TRIGGER reject_running").unwrap();
    mark_running_for_turn(&sid, &current).unwrap();
    assert_eq!(
        read_sidecar_strict(&sid).unwrap().unwrap().state,
        DispatchState::Running
    );
}

#[test]
fn legacy_dispatch_ids_are_not_adopted_by_a_new_turn() {
    let _home = IsolatedAppHome::acquire("attempt-legacy");
    let sid = prepared();
    // Pre-V25 claims generated a different random id from the Host turn id.
    mark_dispatching(&sid, "legacy handoff", None).unwrap();
    let old_id = read_sidecar_strict(&sid).unwrap().unwrap().dispatch_id;
    notify_turn_success(&sid, "new-host-turn");
    notify_failed_before_dispatch(&sid, "new-host-turn");
    assert!(mark_running_for_turn(&sid, "new-host-turn").is_err());
    let side = read_sidecar_strict(&sid).unwrap().unwrap();
    assert_eq!(side.dispatch_id, old_id);
    assert_eq!(side.state, DispatchState::Dispatching);
    assert!(apply_continuation_first_send(&sid, "retry", SessionMcpPolicy::default()).is_err());
}

#[test]
fn concurrent_stale_failure_and_current_start_preserve_current_owner() {
    let _home = IsolatedAppHome::acquire("attempt-concurrent");
    for _ in 0..4 {
        let sid = prepared();
        let old = claim(&sid);
        fail_before_dispatch(&sid, &old);
        let current = claim(&sid);
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let late = scope.spawn(|| {
                barrier.wait();
                notify_turn_failure(&sid, &old);
            });
            barrier.wait();
            mark_running_for_turn(&sid, &current).unwrap();
            late.join().unwrap();
        });
        let side = read_sidecar_strict(&sid).unwrap().unwrap();
        assert_eq!(side.dispatch_id.as_deref(), Some(current.as_str()));
        assert_eq!(side.state, DispatchState::Running);
    }
}

#[test]
fn final_wire_prompt_replaces_the_provisional_claim_hash() {
    let _home = IsolatedAppHome::acquire("attempt-wire-hash");
    let sid = prepared();
    let turn = claim(&sid);
    let provisional = read_sidecar_strict(&sid)
        .unwrap()
        .unwrap()
        .prompt_hash
        .unwrap();
    let final_prompt = "[session lookup hint]\n<SYSTEM>selected history</SYSTEM>\nuser request";
    finalize_prompt_hash_for_turn(&sid, &turn, final_prompt).unwrap();
    let side = read_sidecar_strict(&sid).unwrap().unwrap();
    assert_ne!(side.prompt_hash.as_deref(), Some(provisional.as_str()));
    assert_eq!(
        side.prompt_hash.as_deref(),
        Some(super::hash::sha256_bytes(final_prompt.as_bytes()).as_str())
    );
    mark_running_for_turn(&sid, &turn).unwrap();
}

#[test]
fn final_wire_hash_failure_blocks_the_model_call() {
    let _home = IsolatedAppHome::acquire("attempt-wire-hash-failure");
    let sid = prepared();
    let turn = claim(&sid);
    let conn = super::catalog::open_catalog().unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_final_hash BEFORE UPDATE ON continuation_refs
         BEGIN SELECT RAISE(FAIL, 'synthetic final hash persistence failure'); END;",
    )
    .unwrap();
    assert!(finalize_prompt_hash_for_turn(&sid, &turn, "final prompt").is_err());
    assert_eq!(
        read_sidecar_strict(&sid).unwrap().unwrap().state,
        DispatchState::Dispatching
    );
    conn.execute_batch("DROP TRIGGER reject_final_hash")
        .unwrap();
    finalize_prompt_hash_for_turn(&sid, &turn, "final prompt").unwrap();
}
