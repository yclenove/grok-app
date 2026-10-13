//! Read-only consent metadata is not a screen-read or execution permit.
//! Host approvals here are synthetic; this is not native/model acceptance.
use super::*;
use crate::fake::FakeAdapter;
use serde_json::{json, Value};

fn pending() -> (ComputerUseBroker, Arc<FakeAdapter>, String, Value) {
    let adapter = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: false,
            lease_path: std::env::temp_dir()
                .join(format!("cu-access-status-{}.lease", uuid::Uuid::new_v4())),
            ..Default::default()
        },
    );
    let token = broker.reserve_transport_connection("app").unwrap();
    broker.bind_transport_agent(token.token(), "agent").unwrap();
    let request = broker.submit_access(token.token(), json!({
        "action":"request", "purpose":"consent status regression", "scopeIntent":"owned fixture",
        "surfaces":["desktop"], "capabilities":["observe","input"]
    })).unwrap();
    (broker, adapter, token.token().into(), request)
}

fn call(broker: &ComputerUseBroker, token: &str, args: Value) -> Value {
    crate::tools::dispatch_with_transport(broker, token, "computer_status", args, None)
}

fn text(result: Value) -> Value {
    assert_ne!(result["isError"], true, "{result}");
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn no_effects(broker: &ComputerUseBroker, adapter: &FakeAdapter) {
    assert!(!broker.feature_enabled());
    assert_eq!(broker.executable_grant_count("app"), 0);
    assert_eq!(broker.continuation_count("app"), 0);
    assert_eq!(adapter.list_calls(), 0);
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
}

#[test]
fn pending_status_is_original_metadata_without_grant_discovery_or_execution() {
    let (broker, adapter, token, request) = pending();
    let census = broker.authority_census();
    for _ in 0..3 {
        broker
            .preflight_tool_permission(&token, "computer_status", &json!({}))
            .unwrap();
        let status = text(call(&broker, &token, json!({})));
        assert_eq!(status["state"], "pending");
        assert_eq!(status["statusKind"], "access-request");
        assert_eq!(status["authority"], "none");
        assert_eq!(status["requestId"], request["requestId"]);
        assert_eq!(status["taskId"], request["taskId"]);
        assert_eq!(status["revision"], request["revision"]);
        assert_eq!(status["nextAction"], "wait-for-host");
        for key in [
            "runId",
            "targetId",
            "backend",
            "traces",
            "timings",
            "token",
            "screenshot",
        ] {
            assert!(status.get(key).is_none(), "status leaked {key}");
        }
        assert_eq!(broker.authority_census(), census);
        no_effects(&broker, &adapter);
    }
    // A successful consent read cannot be reused as executable authority.
    for tool in [
        "computer_observe",
        "computer_list_targets",
        "computer_upload",
        "computer_act",
        "browser_observe",
    ] {
        assert!(broker
            .preflight_tool_permission(&token, tool, &json!({}))
            .is_err());
    }
    no_effects(&broker, &adapter);
}

#[test]
fn pending_status_never_hides_forged_or_malformed_execution_selectors() {
    let (broker, adapter, token, _) = pending();
    for args in [
        json!({"runId":"foreign"}),
        json!({"targetId":"foreign"}),
        json!({"runId":null}),
        json!({"targetId":7}),
        json!({"targetId":""}),
        json!({"runId":"a","run_id":"a"}),
        json!({"screenshot":false}),
        json!({"appSessionId":"another-app"}),
        json!({"alwaysApprove":true}),
        json!({"taskId":"forged"}),
        Value::Null,
    ] {
        assert!(
            broker
                .preflight_tool_permission(&token, "computer_status", &args)
                .is_err(),
            "{args}"
        );
        assert_eq!(call(&broker, &token, args)["isError"], true);
    }
    no_effects(&broker, &adapter);
}

#[test]
fn terminal_status_reports_no_authority_without_reopening_the_request() {
    for terminal in ["denied", "cancelled", "expired", "completed"] {
        let (broker, adapter, token, request) = pending();
        let id = request["requestId"].as_str().unwrap();
        match terminal {
            "denied" => broker.deny_access(&token, id, 1).unwrap(),
            "cancelled" => broker.cancel_access(&token, id, 1).unwrap(),
            "expired" => broker.advance_consent_clock(std::time::Duration::from_secs(601)),
            "completed" => broker.complete_task("app").unwrap(),
            _ => unreachable!(),
        }
        let census = broker.authority_census();
        broker
            .preflight_tool_permission(&token, "computer_status", &json!({}))
            .unwrap();
        let status = text(call(&broker, &token, json!({})));
        assert_eq!(status["state"], terminal);
        assert_eq!(status["authority"], "none");
        assert_eq!(status["nextAction"], "explain-decision");
        assert_eq!(status["requestId"], request["requestId"]);
        assert_eq!(broker.authority_census(), census);
        assert_eq!(broker.live_request_count("app"), 0);
        no_effects(&broker, &adapter);
    }
}

#[test]
fn status_never_borrows_another_owner_or_survives_transport_retirement() {
    let (broker, adapter, token, _) = pending();
    let other = broker.reserve_transport_connection("other-app").unwrap();
    for invalid in ["unknown", other.token()] {
        assert!(broker
            .preflight_tool_permission(invalid, "computer_status", &json!({}))
            .is_err());
        assert_eq!(call(&broker, invalid, json!({}))["isError"], true);
    }
    broker
        .bind_transport_agent(other.token(), "other-agent")
        .unwrap();
    assert_eq!(call(&broker, other.token(), json!({}))["isError"], true);
    broker.retire_transport_connection(&token);
    assert_eq!(call(&broker, &token, json!({}))["isError"], true);
    no_effects(&broker, &adapter);
}

#[test]
fn approved_status_keeps_the_original_executable_grant_gate() {
    let (broker, adapter, token, request) = pending();
    let id = request["requestId"].as_str().unwrap();
    broker
        .bind_synthetic_scope(id, &adapter.fixture_id())
        .unwrap();
    let grant = broker.commit_access(&token, id, 1).unwrap();
    broker
        .preflight_tool_permission(&token, "computer_status", &json!({}))
        .unwrap();
    let status = text(call(&broker, &token, json!({})));
    assert_eq!(status["runId"], grant.run_id);
    assert_eq!(status["taskId"], request["taskId"]);
    assert_eq!(status["stopState"], "running");
    assert!(status.get("nextAction").is_none());
    assert_eq!(adapter.observe_calls(), 0);
    broker.fence_task_consent("app");
    for ticket in broker.take_cleanup_tickets() {
        broker.finish_stop_cleanup(&ticket).unwrap();
    }
}

#[test]
fn incremental_request_status_does_not_borrow_or_revoke_the_old_approved_scope() {
    let (broker, adapter, token, first) = pending();
    let id = first["requestId"].as_str().unwrap();
    broker
        .bind_synthetic_scope(id, &adapter.fixture_id())
        .unwrap();
    let grant = broker.commit_access(&token, id, 1).unwrap();
    let second = broker.submit_access(&token, json!({
        "action":"request", "purpose":"additional scope", "scopeIntent":"a different fixture",
        "surfaces":["desktop"], "capabilities":["observe"]
    })).unwrap();
    let status = text(call(&broker, &token, json!({})));
    assert_eq!(status["requestId"], second["requestId"]);
    assert_eq!(status["authority"], "none");
    assert!(status.get("runId").is_none());
    let scope = json!({"runId":grant.run_id, "targetId":adapter.fixture_id()});
    assert_eq!(
        text(call(&broker, &token, scope.clone()))["runId"],
        grant.run_id
    );
    broker
        .deny_access(&token, second["requestId"].as_str().unwrap(), 1)
        .unwrap();
    assert_eq!(text(call(&broker, &token, json!({})))["state"], "denied");
    broker
        .preflight_tool_permission(&token, "computer_observe", &scope)
        .unwrap();
    assert_eq!(broker.executable_grant_count("app"), 1);
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
    broker.fence_task_consent("app");
    for ticket in broker.take_cleanup_tickets() {
        broker.finish_stop_cleanup(&ticket).unwrap();
    }
}
