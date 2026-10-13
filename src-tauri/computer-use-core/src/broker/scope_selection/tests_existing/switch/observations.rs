use super::*;

#[test]
fn existing_task_group_pause_control_rejects_stale_generation_request_and_foreign_owner() {
    let f = Fixture::new();
    let outcome = approve_group(&f);
    let old = io::address(&f, &outcome.run_id);
    open(&f, &outcome.run_id, "102").unwrap();
    assert!(f.broker.pause_host_execution(&f.token, &old).is_err());
    assert!(f.broker.preview_host_execution(&f.token, &old).is_err());
    let current = io::address(&f, &outcome.run_id);
    let mut wrong = current.clone();
    wrong.key.revision += 1;
    assert!(f.broker.pause_host_execution(&f.token, &wrong).is_err());
    let foreign = f.broker.issue_transport_credential("foreign-app").unwrap();
    f.broker
        .bind_agent_session("foreign-app", "synthetic-foreign")
        .unwrap();
    assert!(f
        .broker
        .pause_host_execution(foreign.token(), &current)
        .is_err());
    assert!(!f.broker.is_paused(&outcome.run_id).unwrap());
    assert_eq!(f.broker.executable_grant_count("existing-app"), 1);
    f.no_io();
    f.broker.pause_host_execution(&f.token, &current).unwrap();
    assert!(f.broker.is_paused(&outcome.run_id).unwrap());
    assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
    f.no_io();
}

#[test]
fn existing_task_group_quiesced_original_capture_cannot_publish_during_failed_switch() {
    let f = Fixture::new();
    let outcome = approve_group(&f);
    let permit = f
        .broker
        .begin_model_call_claiming(&f.token, "computer_observe", Some(&outcome.run_id), None)
        .unwrap();
    let broker = f.broker.clone();
    let worker = std::thread::spawn(move || broker.observe_for_permit(&permit, false));
    let original = io::poll(&f);
    assert_eq!(original.tab_id, "101");
    // Whether the cancelled waiter has exited by the final switch check is
    // scheduler-dependent. Both results must retire the old capture.
    let switched = open(&f, &outcome.run_id, "102");
    assert!(io::try_complete(&f, original).is_err());
    assert!(worker.join().unwrap().is_err());
    if let Err(error) = switched {
        assert!(matches!(error, BrokerError::LeaseHeld { .. }));
        open(&f, &outcome.run_id, "102").unwrap();
    }
    assert!(f.broker.inner.lock().runs[&outcome.run_id]
        .snapshot_id
        .is_none());
    assert_eq!(
        f.broker.tabs().existing_target_info("102").unwrap().run_id,
        outcome.run_id
    );
    assert!(f.broker.tabs().existing_target_info("101").is_none());
    let fresh = io::observe(&f, &outcome.run_id);
    assert_eq!(fresh.target_id, "tab:102");
    let (worker, dispatch) = action::start(&f, &fresh, "fresh-after-late-capture");
    action::finish(&f, worker, dispatch);
    f.no_io();
}

#[test]
fn existing_task_group_switch_requires_fresh_observe_and_never_revives_old_refs() {
    let f = Fixture::new();
    let outcome = approve_group(&f);
    let old = io::observe(&f, &outcome.run_id);
    let (worker, dispatch) = action::start(&f, &old, "original-positive");
    action::finish(&f, worker, dispatch);
    open(&f, &outcome.run_id, "102").unwrap();
    io::stale_action(&f, &old, "old-a");
    let second = io::observe(&f, &outcome.run_id);
    assert_eq!(second.target_id, "tab:102");
    assert_ne!(second.snapshot_id, old.snapshot_id);
    let (worker, dispatch) = action::start(&f, &second, "second-positive");
    action::finish(&f, worker, dispatch);
    open(&f, &outcome.run_id, "101").unwrap();
    io::stale_action(&f, &old, "returned-a");
    io::stale_action(&f, &second, "old-b");
    let returned = io::observe(&f, &outcome.run_id);
    assert_eq!(returned.target_id, "tab:101");
    assert!(returned.target_generation > second.target_generation);
    assert_ne!(returned.snapshot_id, old.snapshot_id);
    assert_eq!(f.broker.executable_grant_count("existing-app"), 1);
    assert_eq!(f.broker.continuation_count("existing-app"), 1);
    f.no_io();
}

#[test]
fn existing_task_group_same_member_and_preflight_preserve_original_observation() {
    let f = Fixture::new();
    let outcome = approve_group(&f);
    let old = io::observe(&f, &outcome.run_id);
    f.broker
        .preflight_tool_permission(
            &f.token,
            "computer_open_target",
            &json!({"runId":outcome.run_id,"targetId":"tab:102"}),
        )
        .unwrap();
    assert!(f.broker.tabs().existing_target_info("102").is_none());
    let same = open(&f, &outcome.run_id, "101").unwrap();
    assert_eq!(same["targetGeneration"], old.target_generation);
    let inner = f.broker.inner.lock();
    assert_eq!(
        inner.runs[&outcome.run_id].snapshot_id.as_deref(),
        Some(old.snapshot_id.as_str())
    );
    drop(inner);
    f.no_io();
}
