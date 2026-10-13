//! Synthetic Host decisions with actual FakeAdapter reads and cleanup.
use super::*;
use crate::fake::FakeAdapter;
use serde_json::{json, Value};
use std::time::Duration;

fn pair() -> (
    ComputerUseBroker,
    Arc<FakeAdapter>,
    crate::task_consent::IssuedTransport,
) {
    let adapter = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir()
                .join(format!("cu-completion-{}.lock", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    );
    let token = broker.issue_transport_credential("completion-app").unwrap();
    broker
        .bind_agent_session("completion-app", "synthetic-agent")
        .unwrap();
    (broker, adapter, token)
}

fn grant(
    broker: &ComputerUseBroker,
    token: &str,
    scope: &str,
    target: &str,
) -> crate::task_consent::CommitOutcome {
    let pending = broker
        .submit_access(
            token,
            json!({
                "action": "request", "purpose": "completion regression", "surfaces": ["desktop"],
                "scopeIntent": scope, "capabilities": ["observe"]
            }),
        )
        .unwrap();
    let request = pending["requestId"].as_str().unwrap();
    broker.bind_synthetic_scope(request, target).unwrap();
    broker
        .commit_access(token, request, pending["revision"].as_u64().unwrap())
        .unwrap()
}

fn drain(broker: &ComputerUseBroker) {
    for ticket in broker.take_cleanup_tickets() {
        broker.finish_stop_cleanup(&ticket).unwrap();
    }
}

#[test]
fn task_complete_fences_all_incremental_runs() {
    let (broker, adapter, token) = pair();
    let a = grant(&broker, token.token(), "A", &adapter.fixture_id());
    let b = grant(&broker, token.token(), "B", &adapter.fixture_id());
    assert_ne!(a.run_id, b.run_id);
    broker.observe(&a.run_id).unwrap();
    broker.observe(&b.run_id).unwrap();
    assert!(adapter.observe_calls() >= 2);
    broker.complete_task("completion-app").unwrap();
    assert_eq!(broker.executable_grant_count("completion-app"), 0);
    for run in [&a.run_id, &b.run_id] {
        assert_ne!(broker.stop_state(run).unwrap(), StopState::Running);
    }
    drain(&broker);
    assert_eq!(broker.stop_state(&a.run_id).unwrap(), StopState::Stopped);
    assert_eq!(broker.stop_state(&b.run_id).unwrap(), StopState::Stopped);
    assert!(adapter.executions().is_empty());
}

#[test]
fn rejected_expired_tool_still_hands_off_original_run() {
    let (broker, adapter, token) = pair();
    let approved = grant(&broker, token.token(), "A", &adapter.fixture_id());
    let positive = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_observe",
        json!({"screenshot": false}),
        None,
    );
    assert_eq!(positive["isError"], false, "{positive}");
    assert!(adapter.observe_calls() > 0);
    broker
        .inner
        .lock()
        .consent
        .advance_clock_without_sweep(Duration::from_secs(31 * 60));
    let before = adapter.observe_calls();
    let rejected: Value = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_observe",
        json!({"screenshot": false}),
        None,
    );
    assert_eq!(rejected["isError"], true);
    assert_eq!(adapter.observe_calls(), before);
    assert_ne!(
        broker.stop_state(&approved.run_id).unwrap(),
        StopState::Running
    );
    drain(&broker);
    assert_eq!(
        broker.stop_state(&approved.run_id).unwrap(),
        StopState::Stopped
    );
    assert!(adapter.executions().is_empty());
}

#[test]
fn model_completion_fences_every_scope_and_waits_for_original_cleanup() {
    let (broker, adapter, token) = pair();
    let a = grant(&broker, token.token(), "A", &adapter.fixture_id());
    let b = grant(&broker, token.token(), "B", &adapter.fixture_id());
    broker.observe(&a.run_id).unwrap();
    broker.observe(&b.run_id).unwrap();
    let before = adapter.observe_calls();
    let completed = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_complete_task",
        json!({"taskId": a.binding.task_id}),
        None,
    );
    assert_eq!(completed["isError"], false, "{completed}");
    assert!(!completed.to_string().contains(token.token()));
    let result: Value =
        serde_json::from_str(completed["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(result["state"], "completion_requested");
    assert_eq!(result["authority"], "none");
    assert_eq!(broker.executable_grant_count("completion-app"), 0);
    assert!(broker.feature_enabled());
    for run in [&a.run_id, &b.run_id] {
        assert_ne!(broker.stop_state(run).unwrap(), StopState::Running);
    }
    let denied = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_observe",
        json!({"screenshot": false}),
        None,
    );
    assert_eq!(denied["isError"], true);
    assert_eq!(adapter.observe_calls(), before);
    let again = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_complete_task",
        json!({"taskId": a.binding.task_id}),
        None,
    );
    assert_eq!(again["isError"], false, "{again}");
    drain(&broker);
    let view = broker.host_task_view(token.token()).unwrap();
    assert!(view.task_id.is_none());
    assert!(view.grants.is_empty());
    assert_eq!(
        view.request.unwrap().phase,
        crate::task_consent::HostRequestPhase::Completed
    );
    assert_eq!(view.executions.len(), 2);
    assert!(view
        .executions
        .iter()
        .all(|run| run.phase == HostExecutionPhase::Stopped && !run.cleanup_pending));
    assert!(adapter.executions().is_empty());
}

#[test]
fn model_completion_rejects_pending_forged_and_replaced_tasks() {
    let (broker, adapter, token) = pair();
    let pending = broker
        .submit_access(
            token.token(),
            json!({
                "action": "request", "purpose": "pending completion", "surfaces": ["desktop"],
                "scopeIntent": "fixture", "capabilities": ["observe"]
            }),
        )
        .unwrap();
    let task_id = pending["taskId"]
        .as_str()
        .expect("request must expose correlation taskId");
    let complete = |args| {
        crate::tools::dispatch_with_transport(
            &broker,
            token.token(),
            "computer_complete_task",
            args,
            None,
        )
    };
    for args in [
        json!({"taskId": task_id}),
        json!({}),
        Value::Null,
        json!({"taskId": ""}),
        json!({"taskId": task_id, "runId": "forged"}),
        json!({"taskId": "forged"}),
    ] {
        let denied = complete(args);
        assert_eq!(denied["isError"], true, "{denied}");
    }
    assert_eq!(broker.live_request_count("completion-app"), 1);
    let approved = grant(&broker, token.token(), "A", &adapter.fixture_id());
    assert_eq!(approved.binding.task_id, task_id);
    for args in [
        json!({}),
        Value::Null,
        json!({"taskId": ""}),
        json!({"taskId": task_id, "runId": approved.run_id}),
        json!({"taskId": "forged"}),
    ] {
        assert_eq!(complete(args)["isError"], true);
        assert_eq!(broker.executable_grant_count("completion-app"), 1);
    }
    broker.begin_new_task("completion-app").unwrap();
    drain(&broker);
    let next = grant(&broker, token.token(), "next", &adapter.fixture_id());
    assert_ne!(next.binding.task_id, task_id);
    assert_eq!(complete(json!({"taskId": task_id}))["isError"], true);
    assert_eq!(broker.executable_grant_count("completion-app"), 1);
    assert_eq!(broker.stop_state(&next.run_id).unwrap(), StopState::Running);
    broker.rotate_connection("completion-app").unwrap();
    assert_eq!(
        complete(json!({"taskId": next.binding.task_id}))["isError"],
        true
    );
    drain(&broker);
}
