//! Consent boundaries use an owned fake adapter and an explicit synthetic Host decision.
use super::*;
use crate::fake::FakeAdapter;
use crate::task_consent::SYNTHETIC_DECISION_CLASS;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

fn pair() -> (ComputerUseBroker, Arc<FakeAdapter>) {
    let adapter = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: false,
            lease_path: std::env::temp_dir()
                .join(format!("cu-consent-{}.lock", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    );
    (broker, adapter)
}

fn args(purpose: &str, scope: &str, caps: &[&str]) -> Value {
    json!({
        "action": "request",
        "purpose": purpose,
        "surfaces": ["desktop"],
        "capabilities": caps,
        "scopeIntent": scope
    })
}

fn bind_token(broker: &ComputerUseBroker, token: &str) {
    if let crate::task_consent::BearerClass::Live(ctx) = broker.classify_bearer(token) {
        broker
            .bind_agent_session(&ctx.app_session_id, "synthetic-agent")
            .unwrap();
    }
}

fn request(broker: &ComputerUseBroker, token: &str) -> Value {
    bind_token(broker, token);
    broker
        .submit_access(
            token,
            args("read the fixture window", "GrokCuFixture", &["observe"]),
        )
        .unwrap()
}

fn approve(
    broker: &ComputerUseBroker,
    token: &str,
    target: &str,
    caps: &[&str],
) -> crate::task_consent::CommitOutcome {
    bind_token(broker, token);
    let pending = broker
        .submit_access(
            token,
            args("read the fixture window", "GrokCuFixture", caps),
        )
        .unwrap();
    let request_id = pending["requestId"].as_str().unwrap();
    broker.bind_synthetic_scope(request_id, target).unwrap();
    let outcome = broker
        .commit_access(token, request_id, pending["revision"].as_u64().unwrap())
        .unwrap();
    assert_eq!(outcome.decision_class, SYNTHETIC_DECISION_CLASS);
    outcome
}

#[test]
fn unapproved_transport_calls_no_adapter_and_rejects_bad_owners() {
    let (broker, adapter) = pair();
    let token = broker.issue_transport_credential("app-local").unwrap();
    for name in [
        "computer_status",
        "computer_list_targets",
        "computer_open_target",
        "computer_observe",
        "computer_act",
        "computer_wait",
        "computer_navigate",
        "computer_download",
        "browser_list_tabs",
        "browser_open",
        "browser_observe",
        "browser_act",
        "computer_request_handoff",
        "computer_stop",
    ] {
        let result =
            crate::tools::dispatch_with_transport(&broker, token.token(), name, json!({}), None);
        assert_eq!(result["isError"], true, "{name} {result}");
    }
    assert_eq!(adapter.list_calls(), 0);
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
    assert!(broker.issue_transport_credential("").is_err());
    assert!(broker.bind_agent_session("app-local", "").is_err());
    assert!(broker
        .submit_access(
            "not-a-token",
            args("read the fixture window", "GrokCuFixture", &["observe"])
        )
        .is_err());
    broker.bind_agent_session("app-local", "agent-1").unwrap();
    let same = broker.issue_transport_credential("app-local").unwrap();
    assert_eq!(same.token(), token.token());
    broker.bind_agent_session("app-local", "agent-2").unwrap();
    assert!(matches!(
        broker.classify_bearer(token.token()),
        crate::task_consent::BearerClass::Retired
    ));
    assert!(broker
        .submit_access(
            token.token(),
            args("read the fixture window", "Other", &["observe"])
        )
        .is_err());
    assert_eq!(broker.scope_memories("app-local").len(), 0);
    assert_eq!(adapter.list_calls(), 0);
}

#[test]
fn forged_fields_have_no_authority_effect() {
    let (broker, adapter) = pair();
    let token = broker.issue_transport_credential("app-local").unwrap();
    bind_token(&broker, token.token());
    let long = "x".repeat(201);
    let samples = [
        json!({"action": "request", "purpose": "read", "surfaces": ["desktop"], "capabilities": ["observe"], "scopeIntent": "one", "taskId": "forged"}),
        json!({"action": "request", "purpose": "read", "surfaces": ["desktop"], "capabilities": ["observe"], "scopeIntent": "one", "runId": "forged"}),
        json!({"action": "request", "purpose": "read", "surfaces": ["desktop"], "capabilities": ["observe"], "scopeIntent": "one", "requestId": "forged"}),
        json!({"action": "request", "purpose": "read", "surfaces": ["desktop"], "capabilities": ["observe"], "scopeIntent": "one", "revision": 9}),
        json!({"action": "request", "purpose": "read", "surfaces": ["desktop"], "capabilities": ["observe"], "scopeIntent": "one", "epoch": 3}),
        json!({"action": "request", "purpose": "read", "surfaces": ["browser"], "capabilities": ["observe"], "scopeIntent": "one"}),
        json!({"action": "request", "purpose": "read", "surfaces": ["desktop", "desktop"], "capabilities": ["observe"], "scopeIntent": "one"}),
        json!({"action": "request", "purpose": long, "surfaces": ["desktop"], "capabilities": ["observe"], "scopeIntent": "one"}),
        json!({"action": "request", "purpose": "read", "surfaces": ["desktop"], "capabilities": ["admin"], "scopeIntent": "one"}),
        json!({"action": "status", "purpose": "read"}),
        json!({"action": "cancel", "requestId": "forged"}),
    ];
    for sample in samples {
        assert!(broker.submit_access(token.token(), sample).is_err());
    }
    assert!(!broker.feature_enabled());
    assert_eq!(broker.live_request_count("app-local"), 0);
    assert_eq!(broker.executable_grant_count("app-local"), 0);
    assert_eq!(adapter.list_calls(), 0);
    assert_eq!(adapter.observe_calls(), 0);
}

#[test]
fn pending_scope_cannot_commit_and_capabilities_are_split() {
    let (broker, adapter) = pair();
    let token = broker.issue_transport_credential("app-local").unwrap();
    let pending = request(&broker, token.token());
    let request_id = pending["requestId"].as_str().unwrap();
    assert!(broker.commit_access(token.token(), request_id, 1).is_err());
    assert!(!broker.feature_enabled());
    assert_eq!(broker.executable_grant_count("app-local"), 0);
    assert_eq!(broker.continuation_count("app-local"), 0);
    assert_eq!(adapter.list_calls(), 0);

    let outcome = approve(&broker, token.token(), &adapter.fixture_id(), &["observe"]);
    assert_eq!(adapter.observe_calls(), 0);
    let act = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_act",
        json!({}),
        None,
    );
    assert_eq!(act["isError"], true, "{act}");
    assert!(adapter.executions().is_empty());
    let seen = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_observe",
        json!({"screenshot": false}),
        None,
    );
    assert_eq!(seen["isError"], false, "{seen}");
    assert_eq!(adapter.observe_calls(), 1);
    assert!(!seen.to_string().contains(token.token()));
    assert!(!outcome.run_id.is_empty());

    let (other, other_adapter) = pair();
    let other_token = other.issue_transport_credential("app-local").unwrap();
    approve(
        &other,
        other_token.token(),
        &other_adapter.fixture_id(),
        &["input"],
    );
    let reads = other_adapter.observe_calls();
    let lists = other_adapter.list_calls();
    let denied = crate::tools::dispatch_with_transport(
        &other,
        other_token.token(),
        "computer_observe",
        json!({"screenshot": false}),
        None,
    );
    assert_eq!(denied["isError"], true, "{denied}");
    assert_eq!(other_adapter.observe_calls(), reads);
    let listed = crate::tools::dispatch_with_transport(
        &other,
        other_token.token(),
        "computer_list_targets",
        json!({}),
        None,
    );
    assert_eq!(listed["isError"], true, "{listed}");
    assert_eq!(other_adapter.list_calls(), lists);
}

#[test]
fn duplicate_request_and_commit_happen_once() {
    let (broker, adapter) = pair();
    let token = broker.issue_transport_credential("app-local").unwrap();
    let first = request(&broker, token.token());
    let second = request(&broker, token.token());
    assert_eq!(first["requestId"], second["requestId"]);
    assert_eq!(broker.live_request_count("app-local"), 1);
    let request_id = first["requestId"].as_str().unwrap();
    broker
        .bind_synthetic_scope(request_id, &adapter.fixture_id())
        .unwrap();
    let once = broker.commit_access(token.token(), request_id, 1).unwrap();
    let lists = adapter.list_calls();
    let twice = broker.commit_access(token.token(), request_id, 1).unwrap();
    assert_eq!(twice.continuation_id, once.continuation_id);
    assert_eq!(adapter.list_calls(), lists);
    assert_eq!(broker.continuation_count("app-local"), 1);
    let observes = adapter.observe_calls();
    let status = broker
        .submit_access(token.token(), json!({"action": "status"}))
        .unwrap();
    assert_eq!(status["state"], "approved");
    assert_eq!(adapter.observe_calls(), observes);
    assert_eq!(broker.continuation_count("app-local"), 1);
    let claim = broker.claim_continuation("app-local").unwrap();
    assert!(broker.consume_continuation(&claim).is_ok());
    assert!(broker.consume_continuation(&claim).is_err());
}

#[test]
fn late_approve_after_deny_cancel_ttl_or_stop_creates_nothing() {
    let (broker, adapter) = pair();
    let token = broker.issue_transport_credential("app-local").unwrap();
    let pending = request(&broker, token.token());
    let request_id = pending["requestId"].as_str().unwrap().to_string();
    broker.deny_access(token.token(), &request_id, 1).unwrap();
    broker
        .bind_synthetic_scope(&request_id, &adapter.fixture_id())
        .unwrap_err();
    assert!(broker.commit_access(token.token(), &request_id, 1).is_err());
    assert!(!broker.feature_enabled());
    assert_eq!(broker.executable_grant_count("app-local"), 0);
    assert!(broker.no_authority_reasons("app-local").contains(&"denied"));
    assert_eq!(broker.continuation_count("app-local"), 0);

    let pending = request(&broker, token.token());
    let request_id = pending["requestId"].as_str().unwrap().to_string();
    broker.cancel_access(token.token(), &request_id, 1).unwrap();
    assert!(broker.commit_access(token.token(), &request_id, 1).is_err());
    assert!(broker
        .no_authority_reasons("app-local")
        .contains(&"cancelled"));

    let pending = request(&broker, token.token());
    let request_id = pending["requestId"].as_str().unwrap().to_string();
    broker.advance_consent_clock(Duration::from_secs(10 * 60 + 1));
    assert!(broker.commit_access(token.token(), &request_id, 1).is_err());
    assert!(broker
        .no_authority_reasons("app-local")
        .contains(&"expired"));
    assert!(!broker.feature_enabled());
    assert_eq!(adapter.list_calls(), 0);

    let pending = request(&broker, token.token());
    let request_id = pending["requestId"].as_str().unwrap().to_string();
    broker.consent_stop("app-local");
    assert!(broker.commit_access(token.token(), &request_id, 1).is_err());
    assert_eq!(broker.executable_grant_count("app-local"), 0);
    assert!(!broker.feature_enabled());
    assert_eq!(adapter.observe_calls(), 0);
}

#[test]
fn stop_during_prepare_and_old_epoch_do_not_enable_or_roll_back() {
    let (broker, adapter) = pair();
    let broker = Arc::new(broker);
    let token = broker.issue_transport_credential("app-local").unwrap();
    let pending = request(&broker, token.token());
    let request_id = pending["requestId"].as_str().unwrap().to_string();
    broker
        .bind_synthetic_scope(&request_id, &adapter.fixture_id())
        .unwrap();
    let racing = broker.clone();
    broker.arm_commit_interrupt_before_enable(Box::new(move || {
        racing.consent_stop("app-local");
    }));
    assert!(broker.commit_access(token.token(), &request_id, 1).is_err());
    assert!(!broker.feature_enabled());
    assert_eq!(broker.executable_grant_count("app-local"), 0);
    assert_eq!(adapter.list_calls(), 0);

    let (broker, adapter) = pair();
    let broker = Arc::new(broker);
    let token = broker.issue_transport_credential("app-local").unwrap();
    let pending = request(&broker, token.token());
    let request_id = pending["requestId"].as_str().unwrap().to_string();
    broker
        .bind_synthetic_scope(&request_id, &adapter.fixture_id())
        .unwrap();
    let racing = broker.clone();
    broker.arm_commit_interrupt_before_enable(Box::new(move || {
        racing.set_feature_enabled(true);
    }));
    assert!(broker.commit_access(token.token(), &request_id, 1).is_err());
    assert!(broker.feature_enabled());
    assert!(broker.feature_epoch() >= 1);
    assert_eq!(broker.executable_grant_count("app-local"), 0);
    assert_eq!(adapter.list_calls(), 0);

    let (broker, adapter) = pair();
    let broker = Arc::new(broker);
    let token = broker.issue_transport_credential("app-local").unwrap();
    let pending = request(&broker, token.token());
    let request_id = pending["requestId"].as_str().unwrap().to_string();
    broker
        .bind_synthetic_scope(&request_id, "missing-window")
        .unwrap();
    let racing = broker.clone();
    broker.arm_commit_interrupt_after_enable(Box::new(move || {
        racing.set_feature_enabled(false);
        racing.set_feature_enabled(true);
    }));
    assert!(broker.commit_access(token.token(), &request_id, 1).is_err());
    assert!(broker.feature_enabled());
    assert!(broker.feature_epoch() > 1);
    assert_eq!(broker.executable_grant_count("app-local"), 0);
    assert_eq!(adapter.observe_calls(), 0);
    assert_eq!(adapter.list_calls(), 0);
}

#[test]
fn scope_intent_does_not_merge_and_old_card_cannot_approve() {
    let (broker, adapter) = pair();
    let token = broker.issue_transport_credential("app-local").unwrap();
    bind_token(&broker, token.token());
    let chrome = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "Chrome", &["observe"]),
        )
        .unwrap();
    let excel = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "Excel", &["observe"]),
        )
        .unwrap();
    assert_ne!(chrome["requestId"], excel["requestId"]);
    assert_eq!(broker.live_request_count("app-local"), 1);
    assert!(broker
        .commit_access(token.token(), chrome["requestId"].as_str().unwrap(), 1)
        .is_err());
    assert_eq!(broker.scope_memories("app-local").len(), 0);
    assert_eq!(adapter.list_calls(), 0);
    broker.rotate_connection("app-local").unwrap();
    assert!(matches!(
        broker.classify_bearer(token.token()),
        crate::task_consent::BearerClass::Retired
    ));
    assert!(broker
        .submit_access(token.token(), json!({"action": "status"}))
        .is_err());
    assert!(broker.claim_continuation("app-local").is_none());
    assert!(broker.scope_memories("app-local").is_empty());
}

#[test]
fn request_state_is_bounded_and_completion_drops_the_grant() {
    let (broker, adapter) = pair();
    let token = broker.issue_transport_credential("app-local").unwrap();
    bind_token(&broker, token.token());
    for index in 0..12 {
        let result = broker.submit_access(
            token.token(),
            args(&format!("purpose {index}"), "GrokCuFixture", &["observe"]),
        );
        assert!(result.is_ok(), "{index}");
    }
    assert_eq!(broker.live_request_count("app-local"), 1);
    assert!(broker.stored_request_count() <= 8);
    assert_eq!(broker.executable_grant_count("app-local"), 0);
    assert_eq!(adapter.list_calls(), 0);

    let fresh = broker.issue_transport_credential("fresh").unwrap();
    approve(&broker, fresh.token(), &adapter.fixture_id(), &["observe"]);
    broker.advance_consent_clock(Duration::from_secs(30 * 60 + 1));
    assert!(broker
        .execution_run(fresh.token(), "computer_observe")
        .is_err());
    assert_eq!(broker.executable_grant_count("fresh"), 0);
    assert!(broker.claim_continuation("fresh").is_none());
    assert!(!broker.take_cleanup_tickets().is_empty());
    assert!(broker.feature_enabled());

    let again_token = broker.issue_transport_credential("next").unwrap();
    approve(
        &broker,
        again_token.token(),
        &adapter.fixture_id(),
        &["observe"],
    );
    let before = broker.scope_memories("next").len();
    broker.complete_task("next").unwrap();
    assert_eq!(broker.executable_grant_count("next"), 0);
    assert_eq!(broker.scope_memories("next").len(), before);
    let again = broker
        .submit_access(
            again_token.token(),
            args("read the fixture window", "GrokCuFixture", &["observe"]),
        )
        .unwrap();
    assert_eq!(again["state"], "pending");
    assert_eq!(broker.executable_grant_count("next"), 0);
}

#[test]
fn failed_bind_restores_a_feature_this_commit_enabled() {
    let (broker, adapter) = pair();
    let token = broker.issue_transport_credential("app-local").unwrap();
    let pending = request(&broker, token.token());
    broker
        .bind_synthetic_scope(pending["requestId"].as_str().unwrap(), "missing-window")
        .unwrap();
    assert!(broker
        .commit_access(token.token(), pending["requestId"].as_str().unwrap(), 1)
        .is_err());
    assert!(!broker.feature_enabled());
    assert!(broker.feature_epoch() > 1);
    assert_eq!(broker.executable_grant_count("app-local"), 0);
    assert_eq!(adapter.list_calls(), 0);
    assert_eq!(adapter.observe_calls(), 0);
}
