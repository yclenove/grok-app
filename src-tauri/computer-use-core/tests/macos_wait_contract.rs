//! Retained native AX wait contracts. FFI doubles, NOT macOS native evidence.
#![cfg(not(target_os = "macos"))]
pub use grok_computer_use_core::{adapter, protocol};
#[path = "../../src/computer_use/macos_adapter.rs"]
mod macos_adapter;
#[path = "support/quartz_ffi.rs"]
mod quartz_ffi;
use adapter::{ActionScope, ComputerUseAdapter, DispatchRequest};
use macos_adapter::MacosAdapter;
use protocol::{ActionKind, ActionTarget, Observation};
use quartz_ffi::{inspect, reset, update};
use serde_json::json;
use std::sync::Arc;

fn setup() -> (Arc<MacosAdapter>, Observation) {
    reset();
    update(|s| s.ax_tree_enabled = true);
    let adapter = Arc::new(MacosAdapter::new());
    let id = adapter.list_targets().unwrap().remove(0).target_id;
    let obs = adapter.observe_for_run("run", &id).unwrap();
    (adapter, obs)
}
fn request(obs: &Observation, expected: &str) -> DispatchRequest {
    DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "run".into(),
        action_id: "wait".into(),
        generation: 1,
        target_id: obs.target_id.clone(),
        target_generation: 1,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action: ActionKind::Wait,
        target: ActionTarget::Element {
            element_ref: obs.nodes[1].node_ref.clone(),
        },
        parameters: json!({"nameEquals": expected, "timeoutMs": 100}),
        scope: ActionScope::Directed,
    }
}
fn finish(adapter: Arc<MacosAdapter>) {
    assert!(adapter.is_idle("run"));
    drop(adapter);
    inspect(|s| {
        assert_eq!(s.live_owned, 0);
        assert_eq!(s.ax_owned, 0);
        assert_eq!(s.ax_secure_reads, 0);
        assert!(s.posts.is_empty() && s.wheel_posts.is_empty() && s.key_posts.is_empty());
        assert!(s.ax_presses.is_empty() && s.ax_values.is_empty() && s.ax_append_writes.is_empty());
    });
}

#[test]
fn wait_verifies_the_original_control_without_recapture_or_native_input() {
    let (adapter, obs) = setup();
    macos_adapter::run_native_selftest().unwrap();
    let captures = inspect(|s| s.captures.len());
    let result = adapter.act(&request(&obs, &obs.nodes[1].name)).unwrap();
    assert!(result.verifiable && result.postcondition_ok);
    assert!(
        !result.applied,
        "read-only wait must not claim an input mutation"
    );
    inspect(|s| assert_eq!(s.captures.len(), captures));
    assert!(obs.nodes[1].actions.iter().any(|a| a == "wait"));
    finish(adapter);
}

#[test]
fn wait_allows_name_change_on_the_same_native_control() {
    let (adapter, obs) = setup();
    update(|s| s.ax_button_name = "完成 😀".into());
    let result = adapter.act(&request(&obs, "完成 😀")).unwrap();
    assert!(result.postcondition_ok && result.verifiable);
    finish(adapter);
}

#[test]
fn replacement_with_matching_name_is_not_the_observed_control() {
    let (adapter, obs) = setup();
    update(|s| {
        s.ax_button_generation += 1;
        s.ax_button_name = "complete".into();
    });
    assert!(adapter.act(&request(&obs, "complete")).is_err());
    finish(adapter);
}

#[test]
fn name_change_between_polls_keeps_the_same_reference() {
    let (adapter, obs) = setup();
    let before = inspect(|s| s.ax_name_reads);
    update(|s| {
        s.ax_on_name_read_at = Some(before + 1);
        s.ax_on_name_read = Some(Box::new(|| update(|s| s.ax_button_name = "ready".into())));
    });
    assert!(
        adapter
            .act(&request(&obs, "ready"))
            .unwrap()
            .postcondition_ok
    );
    inspect(|s| assert_eq!(s.ax_name_reads, before + 2));
    finish(adapter);
}

#[test]
fn timeout_preserves_original_model_refs_without_recapture() {
    let (adapter, obs) = setup();
    let mut req = request(&obs, "not present");
    req.parameters["timeoutMs"] = json!(10);
    let before = inspect(|s| s.captures.len());
    assert!(adapter.act(&req).unwrap_err().contains("timed out"));
    assert!(
        adapter
            .act(&request(&obs, &obs.nodes[1].name))
            .unwrap()
            .postcondition_ok
    );
    inspect(|s| assert_eq!(s.captures.len(), before));
    finish(adapter);
}

#[test]
fn disabled_unfocused_controls_can_be_waited_on_without_focus_stealing() {
    let (adapter, original) = setup();
    update(|s| {
        s.ax_button_enabled = false;
        s.ax_focused = false;
        s.ax_frontmost = false;
    });
    let obs = adapter.observe_for_run("run", &original.target_id).unwrap();
    assert_eq!(obs.nodes[1].actions, vec!["wait"]);
    assert!(
        adapter
            .act(&request(&obs, &obs.nodes[1].name))
            .unwrap()
            .postcondition_ok
    );
    finish(adapter);
}

#[test]
fn preview_refs_reject_without_displacing_the_model_and_imageless_wait_works() {
    let (adapter, original) = setup();
    let obs = adapter
        .capture_for_run(
            "run",
            &original.target_id,
            adapter::CaptureOptions::model(false),
        )
        .unwrap();
    let preview = adapter
        .capture_for_run(
            "run",
            &obs.target_id,
            adapter::CaptureOptions {
                for_model: false,
                ..adapter::CaptureOptions::model(true)
            },
        )
        .unwrap();
    assert_ne!(preview.nodes[1].node_ref, obs.nodes[1].node_ref);
    assert!(adapter
        .act(&request(&preview, &preview.nodes[1].name))
        .is_err());
    assert!(
        adapter
            .act(&request(&obs, &obs.nodes[1].name))
            .unwrap()
            .postcondition_ok
    );
    finish(adapter);
}

#[test]
fn malformed_parameters_reject_before_reading_a_native_name() {
    for params in [
        json!({}),
        json!([]),
        json!({"nameEquals": null}),
        json!({"nameEquals": 4}),
        json!({"nameEquals": " "}),
        json!({"nameEquals": "x".repeat(257)}),
        json!({"nameEquals": "x", "timeoutMs": 0}),
        json!({"nameEquals": "x", "timeoutMs": 10001}),
        json!({"nameEquals": "x", "timeoutMs": -1}),
        json!({"nameEquals": "x", "timeoutMs": 1.5}),
        json!({"nameEquals": "x", "timeoutMs": null}),
        json!({"nameEquals": "x", "timeoutMs": "1"}),
        json!({"nameEquals": "x", "force": true}),
        json!({"nameEquals": "x", "via": "clipboard"}),
    ] {
        let (adapter, obs) = setup();
        let reads = inspect(|s| s.ax_name_reads);
        let mut req = request(&obs, "x");
        req.parameters = params;
        assert!(adapter.act(&req).is_err(), "{:?}", req.parameters);
        inspect(|s| assert_eq!(s.ax_name_reads, reads));
        finish(adapter);
    }
}

#[test]
fn stale_cross_run_cross_target_coordinate_and_cancelled_authority_reject() {
    for fault in 0..10 {
        let (adapter, obs) = setup();
        let mut req = request(&obs, &obs.nodes[1].name);
        match fault {
            0 => req.snapshot_id.push('x'),
            1 => req.geometry_revision += 1,
            2 => req.run_id.push('x'),
            3 => req.target_id.push('x'),
            4 => {
                req.target = ActionTarget::Element {
                    element_ref: "unknown".into(),
                }
            }
            5 => req.target = ActionTarget::Coord { x: 60.0, y: 70.0 },
            6 => req.scope = ActionScope::Desktop,
            7 => req.cancellation.cancel(),
            8 => adapter.release_target_for_run("run", &obs.target_id),
            _ => {
                adapter.observe_for_run("run", &obs.target_id).unwrap();
            }
        }
        assert!(adapter.act(&req).is_err(), "fault {fault}");
        finish(adapter);
    }
}

#[test]
fn identity_geometry_protection_and_permissions_fail_closed() {
    for fault in 0..16 {
        let (adapter, obs) = setup();
        update(|s| match fault {
            0 => s.ax_instance += 1,
            1 => s.ax_button_generation += 1,
            2 => s.birth.1 += 1,
            3 => s.ax_parent_escape = true,
            4 => s.ax_window_escape = true,
            5 => s.ax_role_changed = true,
            6 => s.ax_subrole_secure = true,
            7 => s.ax_button_hidden = true,
            8 => s.ax_button_size.0 += 1.0,
            9 => s.x += 1.0,
            10 => s.window_present = false,
            11 => s.ax_allowed = false,
            12 => s.screen_allowed = false,
            13 => s.process_info_bytes = 0,
            14 => s.ax_fail_attribute = true,
            _ => s.ax_text_role = Some("AXSecureTextField"),
        });
        assert!(
            adapter.act(&request(&obs, &obs.nodes[1].name)).is_err(),
            "fault {fault}"
        );
        finish(adapter);
    }
}

#[test]
fn matching_name_cannot_hide_retirement_or_mutation_during_the_read() {
    for fault in 0..9 {
        let (adapter, obs) = setup();
        let req = request(&obs, &obs.nodes[1].name);
        let cancel = req.cancellation.clone();
        let weak = Arc::downgrade(&adapter);
        let target = obs.target_id.clone();
        update(|s| {
            s.ax_on_name_read_at = Some(s.ax_name_reads + 1);
            s.ax_on_name_read = Some(Box::new(move || match fault {
                0 => cancel.cancel(),
                1 => weak
                    .upgrade()
                    .unwrap()
                    .release_target_for_run("run", &target),
                2 => weak.upgrade().unwrap().abort("run", 2).unwrap(),
                3 => update(|s| s.ax_button_generation += 1),
                4 => update(|s| s.ax_parent_escape = true),
                5 => update(|s| s.ax_subrole_secure = true),
                6 => update(|s| s.screen_allowed = false),
                7 => update(|s| s.birth.1 += 1),
                _ => update(|s| s.x += 1.0),
            }));
        });
        assert!(adapter.act(&req).is_err(), "fault {fault}");
        finish(adapter);
    }
}

#[test]
fn a_native_read_returning_after_deadline_is_not_a_match() {
    let (adapter, obs) = setup();
    let mut req = request(&obs, &obs.nodes[1].name);
    req.parameters["timeoutMs"] = json!(10);
    update(|s| {
        s.ax_on_name_read_at = Some(s.ax_name_reads + 1);
        s.ax_on_name_read = Some(Box::new(|| {
            std::thread::sleep(std::time::Duration::from_millis(20))
        }));
    });
    assert!(adapter.act(&req).is_err());
    finish(adapter);
}

#[test]
fn waiting_does_not_clear_an_unknown_native_input() {
    let (adapter, obs) = setup();
    let mut click = request(&obs, "unused");
    click.action = ActionKind::Click;
    click.parameters = json!({});
    update(|s| s.ax_press_status = -25204);
    assert!(adapter.act(&click).is_err());
    assert!(!adapter.is_idle("run"));
    assert!(adapter.act(&request(&obs, &obs.nodes[1].name)).is_err());
    adapter.abort("run", 2).unwrap();
    assert!(!adapter.is_idle("run"));
    drop(adapter);
    inspect(|s| {
        assert_eq!(s.ax_presses.len(), 1);
        assert_eq!(s.ax_owned, 0);
        assert_eq!(s.live_owned, 0);
    });
}

#[test]
fn wait_capability_requires_both_permissions_and_no_coordinate_mode() {
    let (adapter, _) = setup();
    assert!(adapter.wait_uses_retained_reference());
    assert!(adapter.capabilities().wait.semantic && !adapter.capabilities().wait.coordinate);
    update(|s| s.ax_allowed = false);
    assert!(!adapter.capabilities().wait.semantic);
    update(|s| {
        s.ax_allowed = true;
        s.screen_allowed = false;
    });
    assert!(!adapter.capabilities().wait.semantic);
    finish(adapter);
}

#[test]
fn nested_wait_revalidates_the_entire_native_ancestry() {
    for hidden in [false, true] {
        let (adapter, original) = setup();
        update(|s| s.ax_chain_depth = 3);
        let obs = adapter.observe_for_run("run", &original.target_id).unwrap();
        let child = obs.nodes.iter().find(|n| n.role == "AXButton").unwrap();
        let mut req = request(&obs, &child.name);
        req.target = ActionTarget::Element {
            element_ref: child.node_ref.clone(),
        };
        if hidden {
            update(|s| s.ax_group_hidden = true);
        }
        let result = adapter.act(&req);
        if hidden {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().postcondition_ok);
        }
        finish(adapter);
    }
}

#[test]
fn failed_wait_getter_does_not_reobserve_or_turn_into_a_name_search() {
    let (adapter, obs) = setup();
    let captures = inspect(|s| s.captures.len());
    update(|s| {
        s.ax_on_name_read_at = Some(s.ax_name_reads + 1);
        s.ax_on_name_read = Some(Box::new(|| update(|s| s.ax_fail_attribute = true)));
    });
    assert!(adapter.act(&request(&obs, &obs.nodes[1].name)).is_err());
    inspect(|s| assert_eq!(s.captures.len(), captures));
    finish(adapter);
}

fn broker_setup() -> (
    Arc<grok_computer_use_core::broker::ComputerUseBroker>,
    Arc<MacosAdapter>,
    protocol::ActionRequest,
) {
    use grok_computer_use_core::broker::{BrokerOptions, ComputerUseBroker};
    let (adapter, original) = setup();
    let broker = Arc::new(ComputerUseBroker::new(
        Arc::new(grok_computer_use_core::owned::HostOwnedAdapter::new(
            adapter.clone(),
        )),
        BrokerOptions {
            feature_enabled: true,
            observe_budget: 2,
            lease_path: std::env::temp_dir()
                .join(format!("mac-wait-{}.lease", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    ));
    broker.open_run("owner", "run").unwrap();
    let generation = broker.authorize_target("run", &original.target_id).unwrap();
    let obs = broker.observe("run").unwrap();
    let req = protocol::ActionRequest {
        version: protocol::PROTOCOL_VERSION,
        action_id: "wait".into(),
        run_id: "run".into(),
        target_id: obs.target_id,
        target_generation: generation,
        snapshot_id: obs.snapshot_id,
        geometry_revision: obs.geometry_revision,
        action: ActionKind::Wait,
        target: ActionTarget::Element {
            element_ref: obs.nodes[1].node_ref.clone(),
        },
        parameters: json!({"nameEquals": obs.nodes[1].name, "timeoutMs": 100}),
    };
    (broker, adapter, req)
}

#[test]
fn broker_routes_opaque_desktop_refs_to_native_wait_and_counts_one_attempt() {
    let (broker, adapter, req) = broker_setup();
    let captures = inspect(|s| s.captures.len());
    let result = broker.act(req.clone());
    assert_eq!(
        result.kind,
        protocol::OutcomeKind::Verified,
        "{:?}",
        result.reason
    );
    inspect(|s| assert_eq!(s.captures.len(), captures));
    assert_eq!(
        broker.model_snapshot_id("run").as_deref(),
        Some(req.snapshot_id.as_str())
    );
    let reads = inspect(|s| s.ax_name_reads);
    assert_eq!(
        broker.act(req.clone()).kind,
        protocol::OutcomeKind::Verified
    );
    inspect(|s| assert_eq!(s.ax_name_reads, reads));
    let mut next = req;
    next.action_id = "second-wait".into();
    let rejected = broker.act(next);
    assert_eq!(rejected.kind, protocol::OutcomeKind::Rejected);
    assert!(rejected
        .reason
        .unwrap()
        .contains("observe budget exhausted"));
    drop(broker);
    finish(adapter);
}

#[test]
fn broker_wait_miss_is_rejected_without_claiming_input_or_advancing_snapshot() {
    let (broker, adapter, mut req) = broker_setup();
    req.parameters = json!({"nameEquals":"absent", "timeoutMs":10});
    let snapshot = req.snapshot_id.clone();
    let result = broker.act(req);
    assert_eq!(result.kind, protocol::OutcomeKind::Rejected);
    assert!(!result.executed);
    assert!(result.reason.unwrap().contains("timed out"));
    assert_eq!(
        broker.model_snapshot_id("run").as_deref(),
        Some(snapshot.as_str())
    );
    drop(broker);
    finish(adapter);
}

#[test]
fn broker_stop_fence_during_matching_read_rejects_late_wait_success() {
    let (broker, adapter, req) = broker_setup();
    let weak = Arc::downgrade(&broker);
    update(|s| {
        s.ax_on_name_read_at = Some(s.ax_name_reads + 1);
        s.ax_on_name_read = Some(Box::new(move || {
            weak.upgrade().unwrap().fence_stop("run").unwrap();
        }));
    });
    let result = broker.act(req);
    assert_eq!(result.kind, protocol::OutcomeKind::Rejected);
    assert!(!result.executed);
    assert!(broker.model_snapshot_id("run").is_none());
    broker.request_stop("run").unwrap();
    assert_eq!(
        broker
            .wait_stopped("run", std::time::Duration::from_secs(1))
            .unwrap(),
        grok_computer_use_core::broker::StopState::Stopped
    );
    drop(broker);
    finish(adapter);
}

#[test]
fn computer_wait_tool_uses_the_same_native_identity_and_session_binding() {
    let (broker, adapter, req) = broker_setup();
    let ActionTarget::Element { element_ref } = &req.target else {
        unreachable!()
    };
    let args = json!({
        "version": req.version, "actionId": req.action_id, "runId":req.run_id,
        "targetId":req.target_id, "targetGeneration":req.target_generation,
        "snapshotId":req.snapshot_id, "geometryRevision":req.geometry_revision,
        "elementRef":element_ref, "nameEquals":req.parameters["nameEquals"], "timeoutMs":100
    });
    let wrong = grok_computer_use_core::ipc::SessionBinding {
        session_id: "foreign".into(),
        run_id: "run".into(),
    };
    let denied =
        grok_computer_use_core::tools::dispatch(&broker, &wrong, "computer_wait", args.clone());
    assert_eq!(denied["isError"], true);
    let binding = grok_computer_use_core::ipc::SessionBinding {
        session_id: "owner".into(),
        run_id: "run".into(),
    };
    let result = grok_computer_use_core::tools::dispatch(&broker, &binding, "computer_wait", args);
    assert_eq!(result["isError"], false, "{result}");
    let text: serde_json::Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["matched"], true);
    assert_eq!(text["outcome"]["kind"], "verified");
    drop(broker);
    finish(adapter);
}

#[test]
fn host_owned_wrapper_preserves_capture_purpose_wait_strategy_and_run_retirement() {
    use grok_computer_use_core::owned::HostOwnedAdapter;
    let (adapter, original) = setup();
    let owned = HostOwnedAdapter::new(adapter.clone());
    assert!(owned.wait_uses_retained_reference());
    let obs = owned
        .capture_for_run(
            "run",
            &original.target_id,
            adapter::CaptureOptions::model(false),
        )
        .unwrap();
    assert!(obs.image.png_base64.is_none());
    let req = request(&obs, &obs.nodes[1].name);
    assert!(owned.act(&req).unwrap().postcondition_ok);
    let preview = owned
        .capture_for_run(
            "run",
            &obs.target_id,
            adapter::CaptureOptions {
                for_model: false,
                ..adapter::CaptureOptions::model(true)
            },
        )
        .unwrap();
    assert!(owned
        .act(&request(&preview, &preview.nodes[1].name))
        .is_err());
    assert!(owned.act(&req).unwrap().postcondition_ok);
    let other = owned.observe_for_run("other", &obs.target_id).unwrap();
    let mut other_req = request(&other, &other.nodes[1].name);
    other_req.run_id = "other".into();
    owned.release_target_for_run("run", &obs.target_id);
    assert!(owned.act(&req).is_err());
    assert!(owned.act(&other_req).unwrap().postcondition_ok);
    drop(owned);
    finish(adapter);
}
