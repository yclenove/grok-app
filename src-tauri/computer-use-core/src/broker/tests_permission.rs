//! Synthetic Host/FakeAdapter preflight. No native or human grant evidence.
use super::*;
use crate::fake::FakeAdapter;
use crate::protocol::{ActionKind, ActionRequest, ActionTarget, PROTOCOL_VERSION};
use serde_json::json;

fn granted() -> (ComputerUseBroker, Arc<FakeAdapter>, String, String) {
    let adapter = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: false,
            lease_path: std::env::temp_dir()
                .join(format!("cu-permission-{}.lease", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    );
    let token = broker.reserve_transport_connection("app").unwrap();
    broker
        .bind_transport_agent(token.token(), "synthetic-agent")
        .unwrap();
    let pending = broker.submit_access(token.token(), json!({"action":"request","purpose":"permission regression","scopeIntent":"fixture","surfaces":["desktop"],"capabilities":["observe","input"]})).unwrap();
    let request = pending["requestId"].as_str().unwrap();
    broker
        .bind_synthetic_scope(request, &adapter.fixture_id())
        .unwrap();
    let grant = broker
        .commit_access(
            token.token(),
            request,
            pending["revision"].as_u64().unwrap(),
        )
        .unwrap();
    (broker, adapter, token.token().into(), grant.run_id)
}

#[test]
fn executable_reads_need_grant_but_no_prior_snapshot_and_preflight_has_zero_effect() {
    let (broker, adapter, token, _) = granted();
    for tool in [
        "computer_status",
        "computer_list_targets",
        "computer_observe",
    ] {
        broker
            .preflight_tool_permission(&token, tool, &json!({}))
            .unwrap();
    }
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
    broker.consent_stop("app");
    assert!(broker
        .preflight_tool_permission(&token, "computer_status", &json!({"runId":"stopped"}))
        .is_err());
    broker
        .preflight_tool_permission(&token, "computer_status", &json!({}))
        .unwrap();
}

#[test]
fn malformed_target_selectors_never_fall_back_to_the_only_grant() {
    let (broker, adapter, token, run) = granted();
    for args in [
        json!({"runId":null,"screenshot":false}),
        json!({"runId":7,"screenshot":false}),
        json!({"targetId":"","screenshot":false}),
        json!({"target_id":" ","screenshot":false}),
        json!({"runId":run,"run_id":run,"screenshot":false}),
        json!({"targetId":adapter.fixture_id(),"target_id":adapter.fixture_id(),"screenshot":false}),
        json!({"runId":"x".repeat(257),"screenshot":false}),
    ] {
        assert!(
            broker
                .preflight_tool_permission(&token, "computer_observe", &args)
                .is_err(),
            "malformed selector gained a permission: {args}"
        );
        let result =
            crate::tools::dispatch_with_transport(&broker, &token, "computer_observe", args, None);
        assert_eq!(result["isError"], true, "{result}");
    }
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
    broker.fence_task_consent("app");
    for ticket in broker.take_cleanup_tickets() {
        broker.finish_stop_cleanup(&ticket).unwrap();
    }
}

#[test]
fn actions_require_original_observation_and_preflight_never_dispatches() {
    let (broker, adapter, token, run) = granted();
    let observation = broker.observe(&run).unwrap();
    let (target, generation) = broker.authorized_target(&run).unwrap();
    let action = ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: "permission-action".into(),
        run_id: run,
        target_id: target,
        target_generation: generation,
        snapshot_id: observation.snapshot_id,
        geometry_revision: observation.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: observation.nodes[0].node_ref.clone(),
        },
        parameters: json!({}),
    };
    let args = serde_json::to_value(action).unwrap();
    broker
        .preflight_tool_permission(&token, "computer_act", &args)
        .unwrap();
    let before = adapter.observe_calls();
    for (key, value) in [
        ("snapshotId", json!("stale")),
        ("targetGeneration", json!(999)),
        ("geometryRevision", json!(999)),
        ("runId", json!("foreign-run")),
    ] {
        let mut invalid = args.clone();
        invalid[key] = value;
        assert!(broker
            .preflight_tool_permission(&token, "computer_act", &invalid)
            .is_err());
    }
    assert_eq!(adapter.observe_calls(), before);
    assert!(adapter.executions().is_empty());
}
