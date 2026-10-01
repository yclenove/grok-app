//! Production AX value path linked to deterministic FFI doubles, NOT macOS proof.
#![cfg(not(target_os = "macos"))]
pub use grok_computer_use_core::{adapter, protocol};
#[path = "../../src/computer_use/macos_adapter.rs"]
mod macos_adapter;
#[path = "support/quartz_ffi.rs"]
mod quartz_ffi;

use adapter::{ActionScope, CaptureOptions, ComputerUseAdapter, DispatchRequest};
use macos_adapter::MacosAdapter;
use protocol::{ActionKind, ActionTarget, Observation, TEXT_MAX_CHARS};
use quartz_ffi::{inspect, reset, update};

fn setup(role: &'static str) -> (MacosAdapter, String) {
    reset();
    update(|s| {
        s.ax_tree_enabled = true;
        s.ax_text_role = Some(role);
        s.ax_press_available = false;
        s.ax_button_name = "编辑区域".into();
    });
    let adapter = MacosAdapter::new();
    let id = adapter.list_targets().unwrap().remove(0).target_id;
    (adapter, id)
}

fn request(observation: &Observation, text: &str) -> DispatchRequest {
    DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: observation.run_id.clone(),
        action_id: "value".into(),
        generation: 1,
        target_id: observation.target_id.clone(),
        target_generation: 1,
        snapshot_id: observation.snapshot_id.clone(),
        geometry_revision: observation.geometry_revision,
        action: ActionKind::SetValue,
        target: ActionTarget::Element {
            element_ref: observation.nodes[1].node_ref.clone(),
        },
        parameters: serde_json::json!({"text":text}),
        scope: ActionScope::Directed,
    }
}

fn finish(adapter: MacosAdapter, expected: Vec<String>) {
    drop(adapter);
    inspect(|s| {
        assert_eq!(
            s.ax_values,
            expected.into_iter().map(|s| (1, s)).collect::<Vec<_>>()
        );
        assert!(s.posts.is_empty(), "no coordinate/keyboard fallback");
        assert!(
            s.ax_presses.is_empty(),
            "setting a value does not focus by clicking"
        );
        assert_eq!(
            s.ax_secure_reads, 0,
            "no AXValue reads or protected field access"
        );
        assert_eq!(s.live_owned, 0);
        assert_eq!(s.ax_owned, 0);
    });
}

#[test]
fn editable_text_roles_advertise_exact_set_value_without_pretending_to_be_ime() {
    for role in ["AXTextField", "AXTextArea", "AXComboBox"] {
        let (adapter, id) = setup(role);
        macos_adapter::run_native_selftest().unwrap(); // FFI doubles, not native proof.
        let observation = adapter.observe_for_run("run", &id).unwrap();
        assert_eq!(observation.nodes[1].actions, vec!["wait", "set_value"]);
        assert!(adapter.capabilities().set_value.semantic);
        assert!(!adapter.capabilities().set_value.coordinate);
        assert!(adapter.capabilities().type_text.semantic);
        assert!(!adapter.capabilities().chinese_ime);
        finish(adapter, vec![]);
    }
}

#[test]
fn unicode_empty_and_protocol_limit_values_are_written_once_without_reading_old_text() {
    for text in [
        "中文😀 e\u{301}\n第二行".into(),
        String::new(),
        "🚀".repeat(TEXT_MAX_CHARS),
    ] {
        let (adapter, id) = setup("AXTextArea");
        let observation = adapter
            .capture_for_run("run", &id, CaptureOptions::model(false))
            .unwrap();
        assert!(observation.image.png_base64.is_none());
        let result = adapter.act(&request(&observation, &text)).unwrap();
        assert!(result.applied);
        assert!(!result.verifiable && !result.postcondition_ok);
        assert!(adapter.is_idle("run"));
        assert!(!result.detail.contains(&text) || text.is_empty());
        finish(adapter, vec![text]);
    }
}

#[test]
fn readonly_nontext_disabled_hidden_and_protected_nodes_never_gain_value_authority() {
    for fault in 0..7 {
        let (adapter, id) = setup("AXTextField");
        update(|s| match fault {
            0 => s.ax_value_settable = 0,
            1 => s.ax_text_role = Some("AXSlider"),
            2 => s.ax_button_enabled = false,
            3 => s.ax_button_hidden = true,
            4 => s.ax_subrole_secure = true,
            5 => s.ax_text_role = Some("AXSecureTextField"),
            _ => s.ax_settable_status = -25205,
        });
        let observation = adapter.observe_for_run("run", &id).unwrap();
        assert!(observation
            .nodes
            .iter()
            .all(|n| !n.actions.iter().any(|a| a == "set_value")));
        finish(adapter, vec![]);
    }
}

#[test]
fn invalid_settable_reply_fails_the_observation_instead_of_minting_a_write() {
    for reply in [0, 1, 2] {
        let (adapter, id) = setup("AXTextField");
        update(|s| match reply {
            0 => s.ax_settable_status = -25204,
            1 => s.ax_settable_status = -25202,
            _ => s.ax_value_settable = 9,
        });
        assert!(adapter.observe_for_run("run", &id).is_err());
        finish(adapter, vec![]);
    }
}

#[test]
fn stale_replaced_or_changed_text_control_never_receives_a_value() {
    for fault in 0..10 {
        let (adapter, id) = setup("AXTextField");
        let observation = adapter.observe_for_run("run", &id).unwrap();
        update(|s| match fault {
            0 => s.ax_value_settable = 0,
            1 => s.ax_button_generation += 1,
            2 => s.ax_role_changed = true,
            3 => s.ax_subrole_secure = true,
            4 => s.ax_parent_escape = true,
            5 => s.ax_button_enabled = false,
            6 => s.ax_button_hidden = true,
            7 => s.ax_settable_status = -25204,
            8 => s.ax_allowed = false,
            _ => s.ax_button_name = "another control".into(),
        });
        assert!(adapter
            .act(&request(&observation, "not dispatched"))
            .is_err());
        assert!(adapter.is_idle("run"));
        finish(adapter, vec![]);
    }
}

#[test]
fn preview_cross_run_and_old_snapshot_cannot_write_but_fresh_model_can() {
    let (adapter, id) = setup("AXTextField");
    let model = adapter.observe_for_run("run", &id).unwrap();
    let preview = adapter.observe(&id).unwrap();
    let mut req = request(&preview, "preview");
    req.run_id = "run".into();
    assert!(adapter.act(&req).is_err());
    let mut req = request(&model, "foreign");
    req.run_id = "other".into();
    assert!(adapter.act(&req).is_err());
    let fresh = adapter.observe_for_run("run", &id).unwrap();
    assert!(adapter.act(&request(&model, "old")).is_err());
    adapter.act(&request(&fresh, "current")).unwrap();
    finish(adapter, vec!["current".into()]);
}

#[test]
fn invalid_text_and_coordinate_or_append_requests_fail_without_native_write() {
    let (adapter, id) = setup("AXTextField");
    let observation = adapter.observe_for_run("run", &id).unwrap();
    for parameters in [
        serde_json::json!({}),
        serde_json::json!({"text":42}),
        serde_json::json!({"text":"x\u{0}y"}),
        serde_json::json!({"text":"x".repeat(TEXT_MAX_CHARS+1)}),
        serde_json::json!({"text":"x","via":"clipboard"}),
    ] {
        let mut req = request(&observation, "");
        req.parameters = parameters;
        assert!(adapter.act(&req).is_err());
    }
    let mut req = request(&observation, "append");
    req.action = ActionKind::TypeText;
    assert!(adapter.act(&req).is_err());
    req.action = ActionKind::SetValue;
    req.target = ActionTarget::Coord { x: 50.0, y: 80.0 };
    assert!(adapter.act(&req).is_err());
    finish(adapter, vec![]);
}

#[test]
fn cancellation_and_release_during_settable_query_prevent_native_value_write() {
    for stop in [true, false] {
        let (adapter, id) = setup("AXTextField");
        let adapter = std::sync::Arc::new(adapter);
        let observation = adapter.observe_for_run("run", &id).unwrap();
        // An uninvoked hook must not retain native objects until TLS teardown
        // when this test runs against an implementation missing the feature.
        let stopping = std::sync::Arc::downgrade(&adapter);
        update(|s| {
            s.ax_on_settable = Some(Box::new(move || {
                let stopping = stopping.upgrade().expect("test adapter is alive");
                if stop {
                    stopping.abort("run", 2).unwrap();
                } else {
                    stopping.release_target_for_run("run", &id);
                }
            }))
        });
        assert!(adapter
            .act(&request(&observation, "must not write"))
            .is_err());
        inspect(|s| {
            assert!(
                s.ax_on_settable.is_none(),
                "settable query hook did not run"
            )
        });
        assert!(adapter.is_idle("run"));
        finish(std::sync::Arc::try_unwrap(adapter).ok().unwrap(), vec![]);
    }
}

#[test]
fn native_value_result_unknown_retains_occupancy_after_stop_and_forbids_retry() {
    let (adapter, id) = setup("AXTextField");
    let observation = adapter.observe_for_run("run", &id).unwrap();
    update(|s| s.ax_value_status = -25204);
    let req = request(&observation, "one uncertain value");
    let error = adapter.act(&req).unwrap_err();
    assert!(error.contains("completion unknown"), "{error}");
    assert!(!error.contains("one uncertain value"));
    assert!(!adapter.is_idle("run"));
    adapter.abort("run", 2).unwrap();
    assert!(!adapter.is_idle("run"));
    let fresh = adapter.observe_for_run("next", &id).unwrap();
    assert!(adapter.act(&request(&fresh, "do not replay")).is_err());
    finish(adapter, vec!["one uncertain value".into()]);
}

#[test]
fn cancellation_inside_successful_value_call_never_replays_the_write() {
    let (adapter, id) = setup("AXTextField");
    let observation = adapter.observe_for_run("run", &id).unwrap();
    let req = request(&observation, "single write");
    update(|s| s.ax_cancel_on_value = Some(req.cancellation.clone()));
    assert!(adapter.act(&req).is_err());
    assert!(adapter.is_idle("run"));
    assert!(adapter.act(&req).is_err());
    finish(adapter, vec!["single write".into()]);
}

#[test]
fn value_allocation_failure_or_late_permission_and_identity_change_prevent_write() {
    for fault in 0..5 {
        let (adapter, id) = setup("AXTextField");
        let observation = adapter.observe_for_run("run", &id).unwrap();
        update(|s| {
            s.cf_watch_string = Some("payload🚀".into());
            if fault == 0 {
                s.cf_fail_string = true;
            } else {
                s.cf_on_string_create = Some(Box::new(move || {
                    update(|s| match fault {
                        1 => s.ax_subrole_secure = true,
                        2 => s.ax_value_settable = 0,
                        3 => s.ax_allowed = false,
                        _ => s.x += 10.0,
                    })
                }));
            }
        });
        assert!(adapter.act(&request(&observation, "payload🚀")).is_err());
        assert!(adapter.is_idle("run"));
        finish(adapter, vec![]);
    }
}

#[test]
fn role_permission_and_geometry_changes_in_final_settable_query_block_the_write() {
    for fault in 0..6 {
        let (adapter, id) = setup("AXTextField");
        let observation = adapter.observe_for_run("run", &id).unwrap();
        update(|s| {
            s.ax_on_settable = Some(Box::new(move || {
                update(|s| match fault {
                    0 => s.ax_subrole_secure = true,
                    1 => s.ax_role_changed = true,
                    2 => s.ax_button_enabled = false,
                    3 => s.ax_button_hidden = true,
                    4 => s.ax_allowed = false,
                    _ => s.x += 10.0,
                });
            }));
        });
        assert!(adapter
            .act(&request(&observation, "sensitive payload"))
            .is_err());
        inspect(|s| assert!(s.ax_on_settable.is_none()));
        assert!(adapter.is_idle("run"));
        finish(adapter, vec![]);
    }
}

#[test]
fn a_readonly_snapshot_cannot_gain_writes_without_a_fresh_model_observation() {
    let (adapter, id) = setup("AXTextField");
    update(|s| s.ax_value_settable = 0);
    let old = adapter.observe_for_run("run", &id).unwrap();
    update(|s| s.ax_value_settable = 1);
    assert!(adapter.act(&request(&old, "stale capability")).is_err());
    let fresh = adapter.observe_for_run("run", &id).unwrap();
    let mut click = request(&fresh, "");
    click.action = ActionKind::Click;
    click.parameters = serde_json::json!({});
    assert!(adapter.act(&click).is_err());
    adapter.act(&request(&fresh, "fresh capability")).unwrap();
    finish(adapter, vec!["fresh capability".into()]);
}

#[test]
fn value_dispatch_revalidates_scope_and_cancellation_before_preparing_text() {
    let (adapter, id) = setup("AXTextField");
    let observation = adapter.observe_for_run("run", &id).unwrap();
    let req = request(&observation, "cancelled");
    req.cancellation.cancel();
    assert!(adapter.act(&req).is_err());
    let mut req = request(&observation, "wrong reference");
    req.target = ActionTarget::Element {
        element_ref: observation.nodes[0].node_ref.clone(),
    };
    assert!(adapter.act(&req).is_err());
    let mut req = request(&observation, "wrong snapshot");
    req.snapshot_id.push_str("-forged");
    assert!(adapter.act(&req).is_err());
    let mut req = request(&observation, "wrong geometry");
    req.geometry_revision += 1;
    assert!(adapter.act(&req).is_err());
    finish(adapter, vec![]);
}
