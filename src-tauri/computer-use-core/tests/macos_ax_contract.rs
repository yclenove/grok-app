//! Production adapter with deterministic AX/Quartz doubles, not native macOS proof.
#![cfg(not(target_os = "macos"))]
pub use grok_computer_use_core::{adapter, protocol};
#[path = "../../src/computer_use/macos_adapter.rs"]
mod macos_adapter;
#[path = "support/quartz_ffi.rs"]
mod quartz_ffi;

use adapter::{ActionScope, CaptureOptions, ComputerUseAdapter, DispatchRequest};
use macos_adapter::MacosAdapter;
use protocol::{ActionKind, ActionTarget, Observation};
use quartz_ffi::{inspect, reset, update, State};

fn setup() -> (MacosAdapter, String) {
    reset();
    update(|s| s.ax_tree_enabled = true);
    let adapter = MacosAdapter::new();
    let id = adapter.list_targets().unwrap().remove(0).target_id;
    (adapter, id)
}

fn request(observation: &Observation) -> DispatchRequest {
    DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: observation.run_id.clone(),
        action_id: "press".into(),
        generation: 1,
        target_id: observation.target_id.clone(),
        target_generation: 1,
        snapshot_id: observation.snapshot_id.clone(),
        geometry_revision: observation.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: observation
                .nodes
                .iter()
                .find(|n| n.role == "AXButton")
                .expect("fixture button")
                .node_ref
                .clone(),
        },
        parameters: serde_json::json!({}),
        scope: ActionScope::Directed,
    }
}

fn finish(adapter: MacosAdapter, expected_presses: usize) {
    drop(adapter);
    inspect(|s| {
        assert_eq!(s.ax_presses.len(), expected_presses);
        assert!(
            s.posts.is_empty(),
            "semantic actions must never fall back to CGEvent"
        );
        assert_eq!(
            s.ax_secure_reads, 0,
            "protected names/actions/values must not be read"
        );
        assert_eq!(s.live_owned, 0, "CF Create/Copy values leaked");
        assert_eq!(s.ax_owned, 0, "retained AX elements leaked");
    });
}

#[test]
fn observation_contains_real_window_children_and_retina_geometry_without_secure_fields() {
    let (adapter, id) = setup();
    let observation = adapter.observe_for_run("run", &id).unwrap();
    assert!(adapter.capabilities().observe_ax);
    assert!(adapter.capabilities().semantic_click);
    assert_eq!(observation.nodes.len(), 2);
    assert_eq!(observation.nodes[0].role, "AXWindow");
    assert_eq!(observation.nodes[0].actions, vec!["wait"]);
    let button = &observation.nodes[1];
    assert_eq!(button.role, "AXButton");
    assert_eq!(button.name, "执行按钮 🚀");
    assert_eq!(button.actions, vec!["wait", "click"]);
    assert_eq!(
        (button.x, button.y, button.width, button.height),
        (Some(40.0), Some(60.0), Some(200.0), Some(80.0))
    );
    assert!(!observation.truncated);
    assert!(observation
        .nodes
        .iter()
        .all(|n| n.node_ref.starts_with("mac-ax-")));
    assert_ne!(observation.nodes[0].node_ref, button.node_ref);
    macos_adapter::run_native_selftest().unwrap(); // Doubles only, not native proof.
    finish(adapter, 0);
}

#[test]
fn semantic_click_uses_one_axpress_and_does_not_claim_verified_postcondition() {
    let (adapter, id) = setup();
    let observation = adapter.observe_for_run("run", &id).unwrap();
    let result = adapter.act(&request(&observation)).unwrap();
    assert!(result.applied);
    assert!(!result.verifiable);
    assert!(!result.postcondition_ok);
    assert!(adapter.is_idle("run"));
    inspect(|s| assert_eq!(s.ax_presses, vec![1]));
    finish(adapter, 1);
}

#[test]
fn unknown_cross_run_cross_target_and_stale_snapshot_references_are_rejected() {
    let (adapter, id) = setup();
    let observation = adapter.observe_for_run("run", &id).unwrap();
    let original = request(&observation);
    for case in 0..5 {
        let mut bad = request(&observation);
        match case {
            0 => bad.run_id = "another-run".into(),
            1 => bad.target_id.push('0'),
            2 => bad.snapshot_id.push('0'),
            3 => bad.geometry_revision = bad.geometry_revision.wrapping_add(1),
            _ => {
                bad.target = ActionTarget::Element {
                    element_ref: "mac-ax-forged".into(),
                }
            }
        }
        assert!(adapter.act(&bad).is_err(), "case {case}");
    }
    adapter.act(&original).unwrap();
    finish(adapter, 1);
}

#[test]
fn preview_refs_neither_grant_authority_nor_replace_model_refs() {
    let (adapter, id) = setup();
    let options = CaptureOptions {
        for_model: false,
        ..CaptureOptions::model(true)
    };
    let preview = adapter
        .capture_for_run("run", &id, options.clone())
        .unwrap();
    assert!(adapter.act(&request(&preview)).is_err());
    let model = adapter.observe_for_run("run", &id).unwrap();
    let preview = adapter.capture_for_run("run", &id, options).unwrap();
    assert!(adapter.act(&request(&preview)).is_err());
    adapter.act(&request(&model)).unwrap();
    finish(adapter, 1);
}

#[test]
fn ax_only_model_observation_grants_semantic_but_not_coordinate_authority() {
    let (adapter, id) = setup();
    let observation = adapter
        .capture_for_run("run", &id, CaptureOptions::model(false))
        .unwrap();
    assert!(observation.image.png_base64.is_none());
    let mut coord = request(&observation);
    coord.target = ActionTarget::Coord { x: 60.0, y: 70.0 };
    assert!(adapter.act(&coord).is_err());
    adapter.act(&request(&observation)).unwrap();
    finish(adapter, 1);
}

#[test]
fn replacement_control_with_identical_role_name_bounds_cannot_inherit_old_reference() {
    let (adapter, id) = setup();
    let old = adapter.observe_for_run("run", &id).unwrap();
    update(|s| s.ax_button_generation += 1);
    assert!(adapter.act(&request(&old)).is_err());
    let new = adapter.observe_for_run("run", &id).unwrap();
    assert_eq!(old.nodes[1].name, new.nodes[1].name);
    assert_ne!(old.nodes[1].node_ref, new.nodes[1].node_ref);
    assert!(adapter.act(&request(&old)).is_err());
    adapter.act(&request(&new)).unwrap();
    inspect(|s| assert_eq!(s.ax_presses, vec![2]));
    finish(adapter, 1);
}

#[test]
fn live_element_presentation_ancestry_permissions_and_action_changes_fail_closed() {
    let changes: [fn(&mut State); 12] = [
        |s| s.ax_button_name.push('!'),
        |s| s.ax_role_changed = true,
        |s| s.ax_parent_escape = true,
        |s| s.ax_window_escape = true,
        |s| s.ax_child_wrong_pid = true,
        |s| s.ax_button_enabled = false,
        |s| s.ax_button_hidden = true,
        |s| s.ax_subrole_secure = true,
        |s| s.ax_button_size.0 += 1.0,
        |s| s.ax_press_available = false,
        |s| s.ax_allowed = false,
        |s| s.screen_allowed = false,
    ];
    for (case, change) in changes.into_iter().enumerate() {
        let (adapter, id) = setup();
        let observation = adapter.observe_for_run("run", &id).unwrap();
        update(change);
        assert!(adapter.act(&request(&observation)).is_err(), "case {case}");
        finish(adapter, 0);
    }
}

#[test]
fn disabled_hidden_or_nonpressable_nodes_do_not_advertise_click() {
    let changes: [fn(&mut State); 3] = [
        |s| s.ax_button_enabled = false,
        |s| s.ax_button_hidden = true,
        |s| s.ax_press_available = false,
    ];
    for change in changes {
        let (adapter, id) = setup();
        update(change);
        let observation = adapter.observe_for_run("run", &id).unwrap();
        let hidden = inspect(|s| s.ax_button_hidden);
        assert_eq!(
            observation.nodes[1].actions,
            if hidden { vec![] } else { vec!["wait"] }
        );
        assert!(adapter.act(&request(&observation)).is_err());
        finish(adapter, 0);
    }
}

#[test]
fn zero_sized_nodes_remain_readable_but_cannot_advertise_click() {
    let (adapter, id) = setup();
    update(|s| s.ax_button_size.0 = 0.0);
    let observation = adapter.observe_for_run("run", &id).unwrap();
    assert_eq!(observation.nodes[1].name, "执行按钮 🚀");
    assert!(observation.nodes[1].width.is_none());
    assert!(observation.nodes[1].actions.is_empty());
    assert!(adapter.act(&request(&observation)).is_err());
    finish(adapter, 0);
}

#[test]
fn failed_model_capture_retires_previous_refs_but_failed_preview_does_not() {
    let (adapter, id) = setup();
    let old = adapter.observe_for_run("run", &id).unwrap();
    update(|s| s.fail_capture = true);
    assert!(adapter.observe(&id).is_err());
    update(|s| s.fail_capture = false);
    adapter.act(&request(&old)).unwrap();
    update(|s| s.fail_capture = true);
    assert!(adapter.observe_for_run("run", &id).is_err());
    update(|s| s.fail_capture = false);
    assert!(adapter.act(&request(&old)).is_err());
    let fresh = adapter.observe_for_run("run", &id).unwrap();
    adapter.act(&request(&fresh)).unwrap();
    finish(adapter, 2);
}

#[test]
fn cancelled_or_replaced_tree_read_cannot_publish_and_releases_native_objects() {
    for replace in [false, true] {
        let (adapter, id) = setup();
        let old = adapter.observe_for_run("run", &id).unwrap();
        let options = CaptureOptions::model(true);
        update(|s| {
            if replace {
                s.ax_replace_on_children = true;
            } else {
                s.ax_cancel_on_children = Some(options.cancellation.clone());
            }
        });
        assert!(adapter.capture_for_run("run", &id, options).is_err());
        update(|s| {
            s.ax_replace_on_children = false;
            s.ax_cancel_on_children = None;
        });
        assert!(adapter.act(&request(&old)).is_err());
        finish(adapter, 0);
    }
}

#[test]
fn malformed_or_foreign_children_are_not_published() {
    let changes: [fn(&mut State); 4] = [
        |s| s.ax_wrong_child_type = true,
        |s| s.ax_child_wrong_pid = true,
        |s| s.ax_window_escape = true,
        |s| s.ax_parent_escape = true,
    ];
    for change in changes {
        let (adapter, id) = setup();
        update(change);
        assert!(adapter.observe_for_run("run", &id).is_err());
        finish(adapter, 0);
    }
}

#[test]
fn cycles_and_oversized_child_lists_are_bounded_and_report_truncation() {
    for cycle in [false, true] {
        let (adapter, id) = setup();
        update(|s| {
            if cycle {
                s.ax_cycle = true;
            } else {
                s.ax_child_count_override = Some(1_000_000);
            }
        });
        let observation = adapter.observe_for_run("run", &id).unwrap();
        assert!(observation.truncated);
        assert_eq!(observation.nodes.len(), 2);
        inspect(|s| assert!(s.ax_children_requests.iter().all(|&count| count <= 256)));
        finish(adapter, 0);
    }
}

#[test]
fn unknown_axpress_completion_keeps_occupancy_even_after_stop_and_forbids_retry() {
    let (adapter, id) = setup();
    let observation = adapter.observe_for_run("run", &id).unwrap();
    update(|s| s.ax_press_status = -25204);
    let req = request(&observation);
    assert!(adapter
        .act(&req)
        .unwrap_err()
        .contains("completion unknown"));
    assert!(!adapter.is_idle("run"));
    adapter.abort("run", 2).unwrap();
    assert!(!adapter.is_idle("run"));
    let fresh = adapter.observe_for_run("other-run", &id).unwrap();
    assert!(adapter.act(&request(&fresh)).is_err());
    finish(adapter, 1);
}

#[test]
fn cancellation_inside_axpress_never_causes_a_second_press_or_coordinate_fallback() {
    let (adapter, id) = setup();
    let observation = adapter.observe_for_run("run", &id).unwrap();
    let req = request(&observation);
    update(|s| s.ax_cancel_on_press = Some(req.cancellation.clone()));
    assert!(adapter.act(&req).is_err());
    assert!(adapter.is_idle("run")); // Synchronous AXPress actually returned success.
    assert!(adapter.act(&req).is_err());
    adapter.abort("run", 2).unwrap();
    assert!(adapter.act(&request(&observation)).is_err());
    finish(adapter, 1);
}

#[test]
fn stop_and_target_release_retire_only_the_owning_run_references() {
    let (adapter, id) = setup();
    let one = adapter.observe_for_run("one", &id).unwrap();
    let two = adapter.observe_for_run("two", &id).unwrap();
    adapter.release_target_for_run("one", &id);
    assert!(adapter.act(&request(&one)).is_err());
    adapter.act(&request(&two)).unwrap();
    adapter.abort("two", 2).unwrap();
    assert!(adapter.act(&request(&two)).is_err());
    finish(adapter, 1);
}

#[test]
fn semantic_click_does_not_emulate_double_click_right_click_or_other_input() {
    let (adapter, id) = setup();
    let observation = adapter.observe_for_run("run", &id).unwrap();
    for parameters in [
        serde_json::json!({"count":2}),
        serde_json::json!({"button":"right"}),
    ] {
        let mut req = request(&observation);
        req.parameters = parameters;
        assert!(adapter.act(&req).is_err());
    }
    let mut req = request(&observation);
    req.action = ActionKind::TypeText;
    req.parameters = serde_json::json!({"text":"do not type"});
    assert!(adapter.act(&req).is_err());
    finish(adapter, 0);
}

#[test]
fn geometry_change_after_final_action_query_prevents_axpress() {
    let (adapter, id) = setup();
    let observation = adapter.observe_for_run("run", &id).unwrap();
    update(|s| s.ax_move_on_action_names = true);
    assert!(adapter.act(&request(&observation)).is_err());
    finish(adapter, 0);
}

#[test]
fn snapshot_cache_eviction_and_fresh_observation_retire_old_authority() {
    let (adapter, id) = setup();
    let oldest = adapter.observe_for_run("run", &id).unwrap();
    let newer = adapter.observe_for_run("run", &id).unwrap();
    assert!(adapter.act(&request(&oldest)).is_err());
    for i in 0..8 {
        adapter.observe_for_run(&format!("other-{i}"), &id).unwrap();
    }
    assert!(adapter.act(&request(&newer)).is_err());
    let fresh = adapter.observe_for_run("run", &id).unwrap();
    adapter.act(&request(&fresh)).unwrap();
    finish(adapter, 1);
}

#[test]
fn native_ancestry_walk_supports_nested_controls_and_bounds_deep_trees() {
    for depth in [3, 80] {
        let (adapter, id) = setup();
        update(|s| s.ax_chain_depth = depth);
        let observation = adapter.observe_for_run("run", &id).unwrap();
        if depth == 3 {
            assert_eq!(observation.nodes.len(), 5);
            assert!(!observation.truncated);
            adapter.act(&request(&observation)).unwrap();
            finish(adapter, 1);
        } else {
            assert_eq!(observation.nodes.len(), 32);
            assert!(observation.truncated);
            assert!(observation
                .nodes
                .iter()
                .all(|n| n.actions.iter().all(|a| a == "wait")));
            finish(adapter, 0);
        }
    }
}

#[test]
fn hidden_ancestor_prevents_observation_and_dispatch_of_its_descendants() {
    let (adapter, id) = setup();
    update(|s| s.ax_chain_depth = 3);
    let observation = adapter.observe_for_run("run", &id).unwrap();
    update(|s| s.ax_group_hidden = true);
    assert!(adapter.act(&request(&observation)).is_err());
    let fresh = adapter.observe_for_run("run", &id).unwrap();
    assert!(!fresh.nodes.iter().any(|n| n.role == "AXButton"));
    finish(adapter, 0);
}

#[test]
fn stop_during_ax_read_rejects_late_publication_without_waiting_for_window_lock() {
    for screenshot in [true, false] {
        let (adapter, id) = setup();
        let adapter = std::sync::Arc::new(adapter);
        let old = adapter.observe_for_run("run", &id).unwrap();
        let stopping = adapter.clone();
        update(|s| {
            s.ax_on_children = Some(Box::new(move || {
                stopping.abort("run", 2).unwrap();
            }))
        });
        let error = adapter
            .capture_for_run("run", &id, CaptureOptions::model(screenshot))
            .unwrap_err();
        assert!(error.contains("retired"), "{error}");
        assert!(adapter.act(&request(&old)).is_err());
        let adapter = std::sync::Arc::try_unwrap(adapter)
            .ok()
            .expect("hook released adapter");
        finish(adapter, 0);
    }
}

#[test]
fn release_during_ax_validation_cannot_dispatch_an_already_loaded_reference() {
    let (adapter, id) = setup();
    let adapter = std::sync::Arc::new(adapter);
    let observation = adapter.observe_for_run("run", &id).unwrap();
    let releasing = adapter.clone();
    update(|s| {
        s.ax_on_children = Some(Box::new(move || {
            releasing.release_target_for_run("run", &id);
        }))
    });
    assert!(adapter.act(&request(&observation)).is_err());
    let adapter = std::sync::Arc::try_unwrap(adapter)
        .ok()
        .expect("hook released adapter");
    finish(adapter, 0);
}

#[test]
fn display_name_truncation_is_explicit_and_oversized_native_strings_fail_closed() {
    for length in [600, 9000] {
        let (adapter, id) = setup();
        update(|s| s.ax_button_name = "中".repeat(length));
        let result = adapter.observe_for_run("run", &id);
        if length == 600 {
            let observation = result.unwrap();
            assert_eq!(observation.nodes[1].name.chars().count(), 512);
            assert!(observation.nodes[1].truncated && observation.truncated);
            adapter.act(&request(&observation)).unwrap();
            finish(adapter, 1);
        } else {
            assert!(result.is_err());
            finish(adapter, 0);
        }
    }
}

#[test]
fn malformed_nonfinite_or_negative_child_geometry_is_not_silently_normalized() {
    for width in [-1.0, f64::NAN, f64::INFINITY] {
        let (adapter, id) = setup();
        update(|s| s.ax_button_size.0 = width);
        assert!(adapter.observe_for_run("run", &id).is_err());
        finish(adapter, 0);
    }
}

#[test]
fn semantic_only_replacement_during_coordinate_preparation_retires_old_image() {
    let (adapter, id) = setup();
    let adapter = std::sync::Arc::new(adapter);
    let old = adapter.observe_for_run("run", &id).unwrap();
    let replacing = adapter.clone();
    let result = std::rc::Rc::new(std::cell::RefCell::new(None));
    let captured = result.clone();
    update(|s| {
        s.on_event_configure = Some(Box::new(move || {
            *captured.borrow_mut() =
                Some(replacing.capture_for_run("run", &id, CaptureOptions::model(false)));
        }))
    });
    let mut coordinate = request(&old);
    coordinate.target = ActionTarget::Coord { x: 100.0, y: 100.0 };
    let error = adapter.act(&coordinate).unwrap_err();
    assert!(error.contains("retired"), "{error}");
    assert!(adapter.is_idle("run"));
    let fresh = result
        .borrow_mut()
        .take()
        .expect("capture hook ran")
        .unwrap();
    assert!(fresh.image.png_base64.is_none());
    assert_ne!(fresh.snapshot_id, old.snapshot_id);
    assert!(adapter.act(&request(&old)).is_err());
    let mut fresh_coordinate = request(&fresh);
    fresh_coordinate.target = coordinate.target;
    assert!(adapter.act(&fresh_coordinate).is_err());
    adapter.act(&request(&fresh)).unwrap();
    let adapter = std::sync::Arc::try_unwrap(adapter)
        .ok()
        .expect("hook released adapter");
    finish(adapter, 1);
}

#[test]
fn replacement_attempt_during_ax_validation_revokes_loaded_nodes_and_pixels() {
    for screenshot in [true, false] {
        let (adapter, id) = setup();
        let adapter = std::sync::Arc::new(adapter);
        let old = adapter.observe_for_run("run", &id).unwrap();
        let replacing = adapter.clone();
        let target = id.clone();
        let result = std::rc::Rc::new(std::cell::RefCell::new(None));
        let captured = result.clone();
        update(|s| {
            s.ax_on_children = Some(Box::new(move || {
                *captured.borrow_mut() = Some(replacing.capture_for_run(
                    "run",
                    &target,
                    CaptureOptions::model(screenshot),
                ));
            }))
        });
        let error = adapter.act(&request(&old)).unwrap_err();
        assert!(error.contains("retired"), "{error}");
        let capture_error = result
            .borrow_mut()
            .take()
            .expect("capture hook ran")
            .unwrap_err();
        assert!(capture_error.contains("busy"), "{capture_error}");
        let mut coordinate = request(&old);
        coordinate.target = ActionTarget::Coord { x: 100.0, y: 100.0 };
        assert!(adapter.act(&coordinate).is_err());
        assert!(adapter.act(&request(&old)).is_err());
        assert!(adapter.is_idle("run"));
        let fresh = adapter.observe_for_run("run", &id).unwrap();
        adapter.act(&request(&fresh)).unwrap();
        let adapter = std::sync::Arc::try_unwrap(adapter)
            .ok()
            .expect("hook released adapter");
        finish(adapter, 1);
    }
}
