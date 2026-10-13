use super::*;

#[test]
fn codex_real_model_activity_restarts_idle_deadline() {
    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    review_commit(
        &broker,
        token.token(),
        "A",
        &["observe"],
        &adapter.fixture_id(),
    );
    broker.advance_consent_clock(Duration::from_secs(29 * 60));
    let before = adapter.observe_calls();
    let seen = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_observe",
        json!({"screenshot": false}),
        None,
    );
    assert_eq!(
        seen["isError"], false,
        "activity positive control failed: {seen}"
    );
    assert!(
        adapter.observe_calls() > before,
        "no real fake-adapter read completed"
    );
    broker.advance_consent_clock(Duration::from_secs(2 * 60));
    assert_eq!(
        broker.executable_grant_count("review-app"),
        1,
        "only two minutes idle after a completed model read, but grant expired"
    );
    assert!(broker
        .execution_run(token.token(), "computer_observe")
        .is_ok());
}

#[test]
fn act_wait_and_browser_work_restart_idle_and_non_work_does_not() {
    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    let outcome = review_commit(
        &broker,
        token.token(),
        "Work",
        &["observe", "input"],
        &adapter.fixture_id(),
    );
    let obs = broker.observe(&outcome.run_id).unwrap();
    let (target_id, generation) = broker.authorized_target(&outcome.run_id).unwrap();
    broker.advance_consent_clock(Duration::from_secs(29 * 60));
    let acted = model_act(
        &broker,
        token.token(),
        crate::broker::gates::click_req(
            &outcome.run_id,
            &target_id,
            generation,
            &obs.snapshot_id,
            obs.geometry_revision,
            "idle-act",
        ),
    );
    assert_eq!(acted["isError"], false, "{acted}");
    assert_eq!(adapter.executions(), vec!["idle-act".to_string()]);
    assert_eq!(adapter.last_dispatch_target_id(), target_id);
    broker.advance_consent_clock(Duration::from_secs(2 * 60));
    assert_eq!(broker.executable_grant_count("review-app"), 1);
    broker.advance_consent_clock(Duration::from_secs(30 * 60));
    assert_eq!(broker.executable_grant_count("review-app"), 0);
    assert!(broker.feature_enabled());

    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    let outcome = review_commit(
        &broker,
        token.token(),
        "Wait",
        &["observe", "input"],
        &adapter.fixture_id(),
    );
    let obs = broker.observe(&outcome.run_id).unwrap();
    let (target_id, generation) = broker.authorized_target(&outcome.run_id).unwrap();
    let before = adapter.observe_calls();
    broker.advance_consent_clock(Duration::from_secs(29 * 60));
    let waited = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_wait",
        json!({
            "version": crate::protocol::PROTOCOL_VERSION,
            "actionId": "idle-wait",
            "runId": outcome.run_id,
            "targetId": target_id,
            "targetGeneration": generation,
            "snapshotId": obs.snapshot_id,
            "geometryRevision": obs.geometry_revision,
            "elementRef": "n1",
            "nameEquals": "Count",
            "timeoutMs": 200
        }),
        None,
    );
    assert_eq!(waited["isError"], false, "{waited}");
    assert!(adapter.observe_calls() > before);
    broker.advance_consent_clock(Duration::from_secs(2 * 60));
    assert_eq!(broker.executable_grant_count("review-app"), 1);
    broker.advance_consent_clock(Duration::from_secs(30 * 60));
    assert_eq!(broker.executable_grant_count("review-app"), 0);

    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    let outcome = review_commit(
        &broker,
        token.token(),
        "Preview",
        &["observe"],
        &adapter.fixture_id(),
    );
    broker.advance_consent_clock(Duration::from_secs(29 * 60));
    broker.observe_preview(&outcome.run_id).unwrap();
    let status = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_status",
        json!({}),
        None,
    );
    assert_eq!(status["isError"], false, "{status}");
    let issued = broker.issue_transport_credential("review-app").unwrap();
    let BearerClass::Live(old) = broker.classify_bearer(issued.token()) else {
        panic!("old");
    };
    broker
        .bind_agent_session("review-app", "synthetic-review-agent")
        .unwrap();
    // The captured context above was taken after issue. Re-capture the pre-bind
    // identity by issuing a second session and using a stale agent-less copy.
    let stale = crate::task_consent::TransportContext {
        agent_session_id: None,
        ..old
    };
    assert!(broker
        .execution_with_context(&stale, "computer_observe")
        .is_err());
    broker
        .submit_access(token.token(), review_args("no-refresh", &["input"]))
        .unwrap();
    broker.advance_consent_clock(Duration::from_secs(2 * 60));
    assert_eq!(
        broker.executable_grant_count("review-app"),
        0,
        "preview, status, or a rejected context refreshed the grant"
    );
}

#[test]
fn long_action_and_unknown_result_do_not_expire_before_physical_completion() {
    let (broker, adapter) = review_pair();
    let broker = Arc::new(broker);
    let token = review_token(&broker);
    let outcome = review_commit(
        &broker,
        token.token(),
        "Hang",
        &["observe", "input"],
        &adapter.fixture_id(),
    );
    let click = legal_click(&broker, &outcome.run_id, "hung-act");
    adapter.set_hang(true);
    let acting = Arc::clone(&broker);
    let token_string = token.token().to_string();
    let act_thread = thread::spawn(move || model_act(&acting, &token_string, click));
    let started = Instant::now();
    while adapter.executions().is_empty() && started.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!adapter.executions().is_empty());
    broker.advance_consent_clock(Duration::from_secs(1900));
    assert_eq!(broker.executable_grant_count("review-app"), 1);
    adapter.set_hang(false);
    let finished = act_thread.join().unwrap();
    assert_eq!(finished["isError"], false, "{finished}");
    broker.advance_consent_clock(Duration::from_secs(1700));
    assert_eq!(broker.executable_grant_count("review-app"), 1);
    broker.advance_consent_clock(Duration::from_secs(200));
    assert_eq!(broker.executable_grant_count("review-app"), 0);
    assert!(broker.feature_enabled());
}

#[test]
fn timed_out_action_starts_idle_when_the_original_worker_finishes() {
    let adapter = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: true,
            action_timeout: Duration::from_millis(25),
            lease_path: std::env::temp_dir()
                .join(format!("cu-unknown-idle-{}.lock", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    );
    let token = review_token(&broker);
    let grant = review_commit(
        &broker,
        token.token(),
        "timeout",
        &["observe", "input"],
        &adapter.fixture_id(),
    );
    let click = legal_click(&broker, &grant.run_id, "timed-out-click");
    adapter.set_hang(true);
    let result = model_act(&broker, token.token(), click);
    let outcome: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(outcome["kind"], "unknown", "{result}");
    assert_eq!(adapter.executions(), vec!["timed-out-click"]);
    broker.advance_consent_clock(Duration::from_secs(1900));
    assert_eq!(broker.executable_grant_count("review-app"), 1);
    adapter.set_hang(false);
    let start = Instant::now();
    while broker.inner.lock().runs[&grant.run_id]
        .in_flight
        .load(Ordering::SeqCst)
    {
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "original worker did not finish"
        );
        thread::yield_now();
    }
    broker.advance_consent_clock(Duration::from_secs(1700));
    assert_eq!(broker.executable_grant_count("review-app"), 1);
    broker.advance_consent_clock(Duration::from_secs(200));
    assert_eq!(broker.executable_grant_count("review-app"), 0);
    assert_eq!(adapter.executions(), vec!["timed-out-click"]);
    for ticket in broker.take_cleanup_tickets() {
        broker.finish_stop_cleanup(&ticket).unwrap();
    }
    assert_eq!(
        broker.stop_state(&grant.run_id).unwrap(),
        StopState::Stopped
    );
}

#[test]
fn late_completion_does_not_refresh_a_replacement_or_restore_feature_off() {
    let (broker, adapter) = review_pair();
    let broker = Arc::new(broker);
    let token = review_token(&broker);
    let outcome = review_commit(
        &broker,
        token.token(),
        "Old",
        &["observe", "input"],
        &adapter.fixture_id(),
    );
    let click = legal_click(&broker, &outcome.run_id, "late-act");
    adapter.set_hang(true);
    let acting = Arc::clone(&broker);
    let token_string = token.token().to_string();
    let act_thread = thread::spawn(move || model_act(&acting, &token_string, click));
    let started = Instant::now();
    while adapter.executions().is_empty() && started.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(10));
    }
    broker.begin_new_task("review-app").unwrap();
    assert_eq!(broker.executable_grant_count("review-app"), 0);
    let replacement = review_commit(
        &broker,
        token.token(),
        "Replacement",
        &["observe"],
        &adapter.fixture_id(),
    );
    broker.advance_consent_clock(Duration::from_secs(1700));
    assert_eq!(broker.executable_grant_count("review-app"), 1);
    adapter.set_hang(false);
    let _ = act_thread.join().unwrap();
    broker.advance_consent_clock(Duration::from_secs(200));
    assert_eq!(broker.executable_grant_count("review-app"), 0);
    assert_ne!(replacement.run_id, outcome.run_id);
    assert!(broker.feature_enabled());

    let (broker, adapter) = review_pair();
    let broker = Arc::new(broker);
    let token = review_token(&broker);
    let outcome = review_commit(
        &broker,
        token.token(),
        "Off",
        &["observe", "input"],
        &adapter.fixture_id(),
    );
    let click = legal_click(&broker, &outcome.run_id, "late-off");
    adapter.set_hang(true);
    let acting = Arc::clone(&broker);
    let token_string = token.token().to_string();
    let act_thread = thread::spawn(move || model_act(&acting, &token_string, click));
    let started = Instant::now();
    while adapter.executions().is_empty() && started.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(10));
    }
    broker.set_feature_enabled(false);
    let _ = act_thread.join().unwrap();
    adapter.set_hang(false);
    assert!(!broker.feature_enabled());
    assert_eq!(broker.executable_grant_count("review-app"), 0);
    broker.advance_consent_clock(Duration::from_secs(60));
    assert!(!broker.feature_enabled());
    assert!(broker
        .execution_run(token.token(), "computer_observe")
        .is_err());
}
