use super::*;

#[test]
fn codex_positive_control_observe_grant_allows_read_and_rejects_valid_input() {
    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    let outcome = review_commit(
        &broker,
        token.token(),
        "A",
        &["observe"],
        &adapter.fixture_id(),
    );
    let seen = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_observe",
        json!({"screenshot": false}),
        None,
    );
    assert_eq!(seen["isError"], false, "positive read failed: {seen}");
    assert!(adapter.observe_calls() > 0);
    let obs = broker.observe(&outcome.run_id).unwrap();
    let (target_id, generation) = broker.authorized_target(&outcome.run_id).unwrap();
    let valid_action = crate::broker::gates::click_req(
        &outcome.run_id,
        &target_id,
        generation,
        &obs.snapshot_id,
        obs.geometry_revision,
        "review-valid-input",
    );
    let input = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_act",
        serde_json::to_value(valid_action).unwrap(),
        None,
    );
    assert_eq!(
        input["isError"], true,
        "observe-only grant accepted valid input: {input}"
    );
    assert!(adapter.executions().is_empty());
}

#[test]
fn codex_incremental_input_scope_does_not_borrow_a_read_grant() {
    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    let a = review_commit(
        &broker,
        token.token(),
        "A",
        &["observe"],
        &adapter.fixture_id(),
    );
    let initial = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_observe",
        json!({"screenshot": false}),
        None,
    );
    assert_eq!(
        initial["isError"], false,
        "A positive control failed: {initial}"
    );
    let old_target = broker.authorized_target(&a.run_id).unwrap().0;
    adapter.rotate_identity();
    let target_b = adapter.fixture_id();
    assert_ne!(old_target, target_b);
    let b = review_commit(&broker, token.token(), "B", &["input"], &target_b);
    assert_ne!(a.request_id, b.request_id);
    let before = adapter.observe_calls();
    let result = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_observe",
        json!({"screenshot": false}),
        None,
    );
    let actual_reads = adapter.observe_calls() - before;
    assert!(
        result["isError"] == true && actual_reads == 0,
        "input-only B was read using A authority; actual_reads={actual_reads}, result={result}"
    );
}

#[test]
fn both_targets_alive_read_dispatches_only_to_the_observe_member() {
    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    let id_a = adapter.fixture_id();
    adapter.pin_synthetic_member(&id_a);
    let a = review_commit(&broker, token.token(), "A", &["observe"], &id_a);
    adapter.rotate_identity();
    let id_b = adapter.fixture_id();
    assert_ne!(id_a, id_b);
    let b = review_commit(&broker, token.token(), "B", &["input"], &id_b);
    let read_a = model_observe(&broker, token.token(), &a.run_id, &id_a);
    assert_eq!(read_a["isError"], false, "A read failed: {read_a}");
    assert_eq!(adapter.last_observe_target_id(), id_a);
    assert_eq!(adapter.observe_calls(), 1);
    let before = adapter.observe_calls();
    let read_b = model_observe(&broker, token.token(), &b.run_id, &id_b);
    assert_eq!(read_b["isError"], true, "input-only B was read: {read_b}");
    assert_eq!(adapter.observe_calls(), before);
    assert_eq!(adapter.last_observe_target_id(), id_a);
    assert!(adapter.executions().is_empty());
    assert_eq!(broker.authorized_target(&a.run_id).unwrap().0, id_a);
    assert_eq!(broker.executable_grant_count("review-app"), 2);
}

#[test]
fn both_targets_alive_act_dispatches_only_to_the_input_member() {
    // A legal act payload is an observation the model actually saw, so the
    // member that may act also has observe on its own target. B is observe-only.
    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    let id_a = adapter.fixture_id();
    adapter.pin_synthetic_member(&id_a);
    let a = review_commit(&broker, token.token(), "A", &["observe", "input"], &id_a);
    adapter.rotate_identity();
    let id_b = adapter.fixture_id();
    let b = review_commit(&broker, token.token(), "B", &["observe"], &id_b);
    let read_a = model_observe(&broker, token.token(), &a.run_id, &id_a);
    assert_eq!(read_a["isError"], false, "{read_a}");
    let acted = model_act(
        &broker,
        token.token(),
        legal_click(&broker, &a.run_id, "act-a"),
    );
    assert_eq!(acted["isError"], false, "A input failed: {acted}");
    assert_eq!(adapter.executions(), vec!["act-a".to_string()]);
    assert_eq!(adapter.last_dispatch_target_id(), id_a);
    let read_b = model_observe(&broker, token.token(), &b.run_id, &id_b);
    assert_eq!(read_b["isError"], false, "B observe failed: {read_b}");
    assert_eq!(adapter.last_observe_target_id(), id_b);
    let before = adapter.executions().len();
    let denied = model_act(
        &broker,
        token.token(),
        legal_click(&broker, &b.run_id, "act-b"),
    );
    assert_eq!(
        denied["isError"], true,
        "observe-only B accepted input: {denied}"
    );
    assert_eq!(adapter.executions().len(), before);
    assert_eq!(adapter.last_dispatch_target_id(), id_a);
}

#[test]
fn retired_observe_member_does_not_receive_the_input_grant_action() {
    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    let id_a = adapter.fixture_id();
    let a = review_commit(&broker, token.token(), "A", &["observe", "input"], &id_a);
    let stale = legal_click(&broker, &a.run_id, "act-retired-a");
    adapter.rotate_identity();
    let id_b = adapter.fixture_id();
    assert_ne!(id_a, id_b);
    let b = review_commit(&broker, token.token(), "B", &["observe"], &id_b);
    let before = adapter.executions().len();
    let denied = model_act(&broker, token.token(), stale);
    assert_eq!(denied["isError"], true, "{denied}");
    assert_eq!(adapter.executions().len(), before);
    let read_b = model_observe(&broker, token.token(), &b.run_id, &id_b);
    assert_eq!(read_b["isError"], false, "live B was not read: {read_b}");
    assert_eq!(adapter.last_observe_target_id(), id_b);
}

#[test]
fn failed_increment_leaves_the_original_grant_executable() {
    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    let id_a = adapter.fixture_id();
    let a = review_commit(&broker, token.token(), "A", &["observe"], &id_a);
    let first = model_observe(&broker, token.token(), &a.run_id, &id_a);
    assert_eq!(first["isError"], false, "{first}");
    let pending = broker
        .submit_access(token.token(), review_args("B-pending", &["input"]))
        .unwrap();
    let still = model_observe(&broker, token.token(), &a.run_id, &id_a);
    assert_eq!(still["isError"], false, "pending B blocked A: {still}");
    let snapshot = broker.inner.lock().runs[&a.run_id].snapshot_id.clone();
    broker
        .deny_access(
            token.token(),
            pending["requestId"].as_str().unwrap(),
            pending["revision"].as_u64().unwrap(),
        )
        .unwrap();
    let cancelled = broker
        .submit_access(token.token(), review_args("B-cancel", &["input"]))
        .unwrap();
    broker
        .cancel_access(
            token.token(),
            cancelled["requestId"].as_str().unwrap(),
            cancelled["revision"].as_u64().unwrap(),
        )
        .unwrap();
    let missing = broker
        .submit_access(token.token(), review_args("B-missing", &["input"]))
        .unwrap();
    broker
        .bind_synthetic_scope(
            missing["requestId"].as_str().unwrap(),
            "fake:window:missing",
        )
        .unwrap();
    assert!(broker
        .commit_access(
            token.token(),
            missing["requestId"].as_str().unwrap(),
            missing["revision"].as_u64().unwrap(),
        )
        .is_err());
    assert_eq!(broker.executable_grant_count("review-app"), 1);
    assert_eq!(broker.authorized_target(&a.run_id).unwrap().0, id_a);
    assert_eq!(broker.stop_state(&a.run_id).unwrap(), StopState::Running);
    assert_eq!(broker.inner.lock().runs[&a.run_id].snapshot_id, snapshot);
    let again = model_observe(&broker, token.token(), &a.run_id, &id_a);
    assert_eq!(
        again["isError"], false,
        "A was not executable after B failed: {again}"
    );
    assert_eq!(adapter.last_observe_target_id(), id_a);
}

#[test]
fn same_member_reauthorize_requires_a_fresh_observe_before_act() {
    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    let outcome = review_commit(
        &broker,
        token.token(),
        "A",
        &["observe", "input"],
        &adapter.fixture_id(),
    );
    let stale = legal_click(&broker, &outcome.run_id, "stale-act");
    let before = adapter.executions().len();
    broker
        .authorize_desktop_window(&outcome.run_id, &adapter.fixture_id())
        .unwrap();
    let rejected = model_act(&broker, token.token(), stale);
    assert_eq!(
        rejected["isError"], true,
        "stale geometry was replayed: {rejected}"
    );
    assert_eq!(adapter.executions().len(), before);
    let seen = model_observe(
        &broker,
        token.token(),
        &outcome.run_id,
        &adapter.fixture_id(),
    );
    assert_eq!(seen["isError"], false, "{seen}");
    let acted = model_act(
        &broker,
        token.token(),
        legal_click(&broker, &outcome.run_id, "fresh-act"),
    );
    assert_eq!(acted["isError"], false, "fresh act failed: {acted}");
    assert_eq!(adapter.last_dispatch_target_id(), adapter.fixture_id());
    assert_eq!(adapter.executions(), vec!["fresh-act".to_string()]);
}

#[test]
fn cut_points_do_not_dispatch_after_stop_replacement_rotation_or_retirement() {
    for cut in ["barrier", "switch", "admit"] {
        for kind in ["stop", "agent", "rotate", "task"] {
            let (broker, adapter) = review_pair();
            let broker = Arc::new(broker);
            let token = review_token(&broker);
            let outcome = review_commit(
                &broker,
                token.token(),
                "A",
                &["observe"],
                &adapter.fixture_id(),
            );
            let seen = model_observe(
                &broker,
                token.token(),
                &outcome.run_id,
                &adapter.fixture_id(),
            );
            assert_eq!(seen["isError"], false, "{cut}/{kind} positive: {seen}");
            let reads = adapter.observe_calls();
            let execs = adapter.executions().len();
            let racing = Arc::clone(&broker);
            let interrupt_kind = kind.to_string();
            let hook = Box::new(move || apply_interrupt(&racing, &interrupt_kind));
            match cut {
                "barrier" => broker.arm_execution_barrier(hook),
                "switch" => broker.arm_after_target_switch(hook),
                "admit" => broker.arm_before_final_admit(hook),
                _ => unreachable!(),
            }
            if cut == "switch" {
                adapter.pin_synthetic_member(&adapter.fixture_id());
                adapter.rotate_identity();
                let pending = broker
                    .submit_access(token.token(), review_args("B", &["observe"]))
                    .unwrap();
                broker
                    .bind_synthetic_scope(
                        pending["requestId"].as_str().unwrap(),
                        &adapter.fixture_id(),
                    )
                    .unwrap();
                let committed = broker.commit_access(
                    token.token(),
                    pending["requestId"].as_str().unwrap(),
                    pending["revision"].as_u64().unwrap(),
                );
                assert!(committed.is_err(), "{cut}/{kind} still committed");
            } else {
                let again = model_observe(
                    &broker,
                    token.token(),
                    &outcome.run_id,
                    &adapter.fixture_id(),
                );
                assert_eq!(again["isError"], true, "{cut}/{kind} dispatched: {again}");
            }
            assert_eq!(adapter.observe_calls(), reads, "{cut}/{kind}");
            assert_eq!(adapter.executions().len(), execs, "{cut}/{kind}");
            assert!(
                broker
                    .execution_run(token.token(), "computer_observe")
                    .is_err(),
                "{cut}/{kind} authority survived"
            );
        }
    }
}
