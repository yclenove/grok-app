//! U01-R2 authority regressions. Synthetic Host decisions are test data.
//! These predicates are the four Codex counterexamples plus the positive
//! control. They describe the rejected contract; they are not a patch of the
//! ignored review snapshot.
use super::*;
use crate::fake::FakeAdapter;
use crate::task_consent::BearerClass;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn review_pair() -> (ComputerUseBroker, Arc<FakeAdapter>) {
    let adapter = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir()
                .join(format!("cu-u01r2-{}.lock", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    );
    (broker, adapter)
}

fn review_token(broker: &ComputerUseBroker) -> crate::task_consent::IssuedTransport {
    let token = broker.issue_transport_credential("review-app").unwrap();
    broker
        .bind_agent_session("review-app", "synthetic-review-agent")
        .unwrap();
    token
}

fn review_args(scope: &str, caps: &[&str]) -> Value {
    json!({
        "action": "request",
        "purpose": "review synthetic fixture",
        "surfaces": ["desktop"],
        "scopeIntent": scope,
        "capabilities": caps
    })
}

fn review_commit(
    broker: &ComputerUseBroker,
    token: &str,
    scope: &str,
    caps: &[&str],
    target_id: &str,
) -> crate::task_consent::CommitOutcome {
    let pending = broker
        .submit_access(token, review_args(scope, caps))
        .unwrap();
    let request_id = pending["requestId"].as_str().unwrap();
    broker.bind_synthetic_scope(request_id, target_id).unwrap();
    broker
        .commit_access(token, request_id, pending["revision"].as_u64().unwrap())
        .unwrap()
}

fn review_pair_enabled(enabled: bool) -> (ComputerUseBroker, Arc<FakeAdapter>) {
    let adapter = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: enabled,
            lease_path: std::env::temp_dir()
                .join(format!("cu-u01r2-{}.lock", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    );
    (broker, adapter)
}

fn model_observe(broker: &ComputerUseBroker, token: &str, run_id: &str, target_id: &str) -> Value {
    crate::tools::dispatch_with_transport(
        broker,
        token,
        "computer_observe",
        json!({"screenshot": false, "runId": run_id, "targetId": target_id}),
        None,
    )
}

fn model_act(
    broker: &ComputerUseBroker,
    token: &str,
    request: crate::protocol::ActionRequest,
) -> Value {
    crate::tools::dispatch_with_transport(
        broker,
        token,
        "computer_act",
        serde_json::to_value(request).unwrap(),
        None,
    )
}

fn legal_click(
    broker: &ComputerUseBroker,
    run_id: &str,
    action_id: &str,
) -> crate::protocol::ActionRequest {
    let obs = broker
        .observe(run_id)
        .expect("model observe for a legal payload");
    let (target_id, generation) = broker.authorized_target(run_id).unwrap();
    crate::broker::gates::click_req(
        run_id,
        &target_id,
        generation,
        &obs.snapshot_id,
        obs.geometry_revision,
        action_id,
    )
}

fn apply_interrupt(broker: &ComputerUseBroker, kind: &str) {
    match kind {
        "stop" => {
            let _ = broker.consent_stop("review-app");
        }
        "agent" => {
            broker
                .bind_agent_session("review-app", "synthetic-review-agent-2")
                .expect("replacement");
        }
        "rotate" => {
            broker.rotate_connection("review-app").expect("rotation");
        }
        "task" => {
            broker
                .begin_new_task("review-app")
                .expect("scope retirement");
        }
        other => panic!("unknown interrupt {other}"),
    }
}

fn stop_during_commit(
    feature_on: bool,
    gap: &str,
) -> (
    Arc<ComputerUseBroker>,
    Arc<FakeAdapter>,
    Vec<StopCleanupTicket>,
) {
    let (broker, adapter) = review_pair_enabled(feature_on);
    let broker = Arc::new(broker);
    let token = review_token(&broker);
    let pending = broker
        .submit_access(token.token(), review_args("A", &["observe"]))
        .unwrap();
    broker
        .bind_synthetic_scope(
            pending["requestId"].as_str().unwrap(),
            &adapter.fixture_id(),
        )
        .unwrap();
    let tickets = Arc::new(Mutex::new(Vec::new()));
    let racing = Arc::clone(&broker);
    let sink = Arc::clone(&tickets);
    let hook = Box::new(move || {
        *sink.lock().expect("tickets") = racing.consent_stop("review-app");
    });
    match gap {
        "open" => broker.arm_commit_interrupt_after_open(hook),
        "bind" => broker.arm_commit_interrupt_after_bind(hook),
        "finalize" => broker.arm_commit_interrupt_before_finalize(hook),
        _ => panic!("gap"),
    }
    let epoch = broker.feature_epoch();
    let committed = broker.commit_access(
        token.token(),
        pending["requestId"].as_str().unwrap(),
        pending["revision"].as_u64().unwrap(),
    );
    assert!(
        committed.is_err(),
        "{gap} feature_on={feature_on} committed"
    );
    assert_eq!(broker.executable_grant_count("review-app"), 0);
    assert_eq!(broker.continuation_count("review-app"), 0);
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
    if feature_on {
        assert!(broker.feature_enabled());
        assert_eq!(broker.feature_epoch(), epoch);
    } else {
        assert!(!broker.feature_enabled());
        assert!(broker.feature_epoch() > epoch);
    }
    let tickets = tickets.lock().expect("tickets").clone();
    assert!(!tickets.is_empty(), "{gap} produced no cleanup ticket");
    (broker, adapter, tickets)
}

#[path = "tests_task_consent_rework2_scope.rs"]
mod scope;

#[path = "tests_task_consent_rework2_resources.rs"]
mod resources;

#[path = "tests_task_consent_rework2_capacity.rs"]
mod capacity;

#[path = "tests_task_consent_rework2_idle.rs"]
mod idle;

#[path = "tests_task_scope_inventory.rs"]
mod inventory;
