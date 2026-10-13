//! Synthetic Host decisions and owned fake effects, never native/human consent.
use super::*;
use crate::fake::FakeAdapter;
use crate::protocol::Observation;
use crate::task_consent::SYNTHETIC_DECISION_CLASS;
use serde_json::{json, Value};

fn fixture() -> (
    Arc<ComputerUseBroker>,
    Arc<FakeAdapter>,
    String,
    String,
    String,
    String,
) {
    let adapter = Arc::new(FakeAdapter::new());
    let a = adapter.fixture_id();
    adapter.pin_synthetic_member(&a);
    adapter.rotate_identity();
    let b = adapter.fixture_id();
    let broker = Arc::new(ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: false,
            lease_path: std::env::temp_dir()
                .join(format!("cu-scope-switch-{}.lock", Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    ));
    let transport = broker.issue_transport_credential("group-app").unwrap();
    broker
        .bind_agent_session("group-app", "synthetic-agent")
        .unwrap();
    let token = transport.token().to_owned();
    let pending = broker
        .submit_access(
            &token,
            json!({"action":"request", "purpose":"use two owned fixtures",
        "scopeIntent":"A and B", "surfaces":["desktop"], "capabilities":["observe","input"]}),
        )
        .unwrap();
    let request = pending["requestId"].as_str().unwrap();
    broker
        .inner
        .lock()
        .consent
        .bind_synthetic_group(request, &[&a, &b])
        .unwrap();
    let outcome = broker.commit_access(&token, request, 1).unwrap();
    assert_eq!(outcome.decision_class, SYNTHETIC_DECISION_CLASS);
    assert_eq!(broker.executable_grant_count("group-app"), 1);
    assert_eq!(broker.continuation_count("group-app"), 1);
    (broker, adapter, token, outcome.run_id, a, b)
}

fn call(broker: &ComputerUseBroker, token: &str, name: &str, args: Value) -> Value {
    crate::tools::dispatch_with_transport(broker, token, name, args, None)
}

fn body(result: &Value) -> Value {
    assert_eq!(result["isError"], false, "{result}");
    let text = result["content"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["type"] == "text")
        .unwrap();
    serde_json::from_str(text["text"].as_str().unwrap()).unwrap()
}

fn observe(broker: &ComputerUseBroker, token: &str, run: &str) -> Observation {
    serde_json::from_value(body(&call(
        broker,
        token,
        "computer_observe",
        json!({"runId":run,"screenshot":false}),
    )))
    .unwrap()
}

fn finish(broker: &ComputerUseBroker) {
    broker.fence_task_consent("group-app");
    for ticket in broker.take_cleanup_tickets() {
        assert_eq!(
            broker.finish_stop_cleanup(&ticket).unwrap(),
            StopState::Stopped
        );
    }
}

#[test]
fn once_approved_group_lists_both_members_and_switches_five_times_with_real_fake_effects() {
    let (broker, adapter, token, run, a, b) = fixture();
    let rows = body(&call(&broker, &token, "computer_list_targets", json!({})));
    assert_eq!(rows.as_array().unwrap().len(), 2);
    let mut old = observe(&broker, &token, &run);
    for (index, target) in [&b, &a, &b, &a, &b].into_iter().enumerate() {
        let opened = body(&call(
            &broker,
            &token,
            "computer_open_target",
            json!({"runId":run,"targetId":target}),
        ));
        assert_eq!(opened["targetId"], *target);
        let stale = gates::click_req(
            &run,
            &old.target_id,
            old.target_generation,
            &old.snapshot_id,
            old.geometry_revision,
            &format!("stale-{index}"),
        );
        assert_eq!(
            call(
                &broker,
                &token,
                "computer_act",
                serde_json::to_value(stale).unwrap()
            )["isError"],
            true
        );
        old = observe(&broker, &token, &run);
        assert_eq!(old.target_id, *target);
        let fresh = gates::click_req(
            &run,
            &old.target_id,
            old.target_generation,
            &old.snapshot_id,
            old.geometry_revision,
            &format!("fresh-{index}"),
        );
        assert_eq!(
            call(
                &broker,
                &token,
                "computer_act",
                serde_json::to_value(fresh).unwrap()
            )["isError"],
            false
        );
        assert_eq!(adapter.last_dispatch_target_id(), *target);
    }
    assert_eq!(adapter.executions().len(), 5);
    assert_eq!(broker.executable_grant_count("group-app"), 1);
    assert_eq!(broker.continuation_count("group-app"), 1);
    assert_eq!(broker.test_run_count(), 1);
    finish(&broker);
}

#[test]
fn outside_group_cannot_read_or_switch_and_does_not_retire_a() {
    let (broker, adapter, token, run, a, _) = fixture();
    let before = (
        adapter.list_calls(),
        adapter.observe_calls(),
        adapter.abort_called(),
    );
    for name in [
        "computer_open_target",
        "computer_observe",
        "computer_list_targets",
    ] {
        let result = call(
            &broker,
            &token,
            name,
            json!({"runId":run,"targetId":"fake:outside:C"}),
        );
        assert_eq!(result["isError"], true, "{result}");
    }
    assert_eq!(
        before,
        (
            adapter.list_calls(),
            adapter.observe_calls(),
            adapter.abort_called()
        )
    );
    assert_eq!(observe(&broker, &token, &run).target_id, a);
    assert!(adapter.executions().is_empty());
    finish(&broker);
}

#[test]
fn returned_to_a_does_not_revive_a_pre_switch_permit_or_action_id() {
    let (broker, adapter, token, run, a, b) = fixture();
    let old_permit = broker.begin_model_call(&token, "computer_observe").unwrap();
    let old = observe(&broker, &token, &run);
    let original = gates::click_req(
        &run,
        &a,
        old.target_generation,
        &old.snapshot_id,
        old.geometry_revision,
        "once",
    );
    assert_eq!(
        call(
            &broker,
            &token,
            "computer_act",
            serde_json::to_value(original).unwrap()
        )["isError"],
        false
    );
    for target in [&b, &a] {
        body(&call(
            &broker,
            &token,
            "computer_open_target",
            json!({"runId":run,"targetId":target}),
        ));
    }
    assert!(broker.observe_for_permit(&old_permit, false).is_err());
    let fresh = observe(&broker, &token, &run);
    let replay = gates::click_req(
        &run,
        &a,
        fresh.target_generation,
        &fresh.snapshot_id,
        fresh.geometry_revision,
        "once",
    );
    let cached = body(&call(
        &broker,
        &token,
        "computer_act",
        serde_json::to_value(replay).unwrap(),
    ));
    // The existing idempotency contract returns the historical outcome. Its
    // old generation and the independent effect counter prove no new dispatch.
    assert_eq!(cached["generation"], old.target_generation);
    assert_eq!(adapter.executions(), ["once"]);
    finish(&broker);
}

#[test]
fn preflight_and_reopening_current_member_do_not_retire_an_observation() {
    let (broker, adapter, token, run, a, b) = fixture();
    let old = observe(&broker, &token, &run);
    let before = (
        adapter.list_calls(),
        adapter.observe_calls(),
        adapter.abort_called(),
    );
    broker
        .preflight_tool_permission(
            &token,
            "computer_open_target",
            &json!({"runId":run,"targetId":b}),
        )
        .unwrap();
    let reopened = body(&call(
        &broker,
        &token,
        "computer_open_target",
        json!({"runId":run,"targetId":a}),
    ));
    assert_eq!(reopened["targetGeneration"], old.target_generation);
    assert_eq!(
        before,
        (
            adapter.list_calls(),
            adapter.observe_calls(),
            adapter.abort_called()
        )
    );
    let action = gates::click_req(
        &run,
        &a,
        old.target_generation,
        &old.snapshot_id,
        old.geometry_revision,
        "still-fresh",
    );
    assert_eq!(
        call(
            &broker,
            &token,
            "computer_act",
            serde_json::to_value(action).unwrap()
        )["isError"],
        false
    );
    finish(&broker);
}

#[test]
fn stop_during_blocked_cleanup_is_bounded_and_late_switch_never_publishes_b() {
    let (broker, adapter, token, run, _, b) = fixture();
    observe(&broker, &token, &run);
    adapter.set_abort_blocked(true);
    let worker_broker = broker.clone();
    let worker_token = token.clone();
    let worker_run = run.clone();
    let worker = std::thread::spawn(move || {
        call(
            &worker_broker,
            &worker_token,
            "computer_open_target",
            json!({"runId":worker_run,"targetId":b}),
        )
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while adapter.abort_called() == 0 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(adapter.abort_called() > 0);
    assert_eq!(
        call(&broker, &token, "computer_observe", json!({"runId":run}))["isError"],
        true
    );
    let start = Instant::now();
    let tickets = broker.consent_stop("group-app");
    assert!(start.elapsed() < Duration::from_millis(100));
    assert_eq!(broker.executable_grant_count("group-app"), 0);
    adapter.set_abort_blocked(false);
    assert_eq!(worker.join().unwrap()["isError"], true);
    for ticket in tickets {
        assert_eq!(
            broker.finish_stop_cleanup(&ticket).unwrap(),
            StopState::Stopped
        );
    }
    assert_eq!(adapter.observe_calls(), 1);
    assert!(adapter.executions().is_empty());
    assert_eq!(broker.stop_state(&run).unwrap(), StopState::Stopped);
}

#[test]
fn unknown_old_work_blocks_switch_until_original_worker_is_terminal() {
    let (broker, adapter, token, run, a, b) = fixture();
    let old = observe(&broker, &token, &run);
    adapter.set_abort_does_not_quiesce(true);
    adapter.set_hang(true);
    let action = gates::click_req(
        &run,
        &a,
        old.target_generation,
        &old.snapshot_id,
        old.geometry_revision,
        "slow-original",
    );
    let result = call(
        &broker,
        &token,
        "computer_act",
        serde_json::to_value(action).unwrap(),
    );
    assert_eq!(result["isError"], true);
    assert!(adapter.wait_for_execution("slow-original", Duration::from_secs(2)));
    let rejected = call(
        &broker,
        &token,
        "computer_open_target",
        json!({"runId":run,"targetId":b}),
    );
    assert_eq!(rejected["code"], "lease_held");
    assert_eq!(
        call(&broker, &token, "computer_observe", json!({"runId":run}))["isError"],
        true
    );
    assert_eq!(adapter.last_dispatch_target_id(), a);
    adapter.finish_in_flight();
    let deadline = Instant::now() + Duration::from_secs(2);
    while broker.inner.lock().runs[&run]
        .in_flight
        .load(Ordering::SeqCst)
        && Instant::now() < deadline
    {
        std::thread::yield_now();
    }
    assert!(!broker.inner.lock().runs[&run]
        .in_flight
        .load(Ordering::SeqCst));
    body(&call(
        &broker,
        &token,
        "computer_open_target",
        json!({"runId":run,"targetId":b}),
    ));
    assert_eq!(observe(&broker, &token, &run).target_id, b);
    assert_eq!(adapter.executions(), ["slow-original"]);
    finish(&broker);
}

#[test]
fn switch_cannot_steal_another_sessions_desktop_lease() {
    let (broker, adapter, token, run, _, b) = fixture();
    broker.open_run("other-app", "other-run").unwrap();
    let other = adapter.fixture_id();
    broker.authorize_target("other-run", &other).unwrap();
    let observation = broker.observe("other-run").unwrap();
    let action = gates::click_req(
        "other-run",
        &other,
        observation.target_generation,
        &observation.snapshot_id,
        observation.geometry_revision,
        "other-effect",
    );
    assert_eq!(broker.act(action).kind, OutcomeKind::Verified);
    let rejected = call(
        &broker,
        &token,
        "computer_open_target",
        json!({"runId":run,"targetId":b}),
    );
    assert_eq!(rejected["code"], "lease_held");
    assert_eq!(broker.lease.holder().unwrap().run_id, "other-run");
    assert_eq!(adapter.executions(), ["other-effect"]);
    broker.request_stop("other-run").unwrap();
    body(&call(
        &broker,
        &token,
        "computer_open_target",
        json!({"runId":run,"targetId":b}),
    ));
    assert_eq!(observe(&broker, &token, &run).target_id, b);
    finish(&broker);
}
