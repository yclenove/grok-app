//! Production screenshot-to-AX input binding with doubles, NOT Mac native proof.
#![cfg(not(target_os = "macos"))]
pub use grok_computer_use_core::{adapter, protocol};
#[path = "../../src/computer_use/macos_adapter.rs"]
mod macos_adapter;
#[path = "support/quartz_ffi.rs"]
mod quartz_ffi;
use adapter::{ActionScope, CaptureOptions, ComputerUseAdapter, DispatchRequest};
use macos_adapter::MacosAdapter;
use protocol::{ActionKind, ActionTarget, Observation};
use quartz_ffi::{inspect, reset, update};
use serde_json::json;

fn setup() -> (MacosAdapter, Observation) {
    reset();
    update(|s| {
        s.ax_tree_enabled = true;
        s.ax_focused = true;
        s.ax_text_role = Some("AXTextArea");
    });
    let adapter = MacosAdapter::new();
    let id = adapter.list_targets().unwrap().remove(0).target_id;
    let obs = adapter.observe_for_run("run", &id).unwrap();
    (adapter, obs)
}
fn request(obs: &Observation, action: ActionKind) -> DispatchRequest {
    DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: obs.run_id.clone(),
        action_id: "coordinate-input".into(),
        generation: 1,
        target_id: obs.target_id.clone(),
        target_generation: 1,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action,
        target: ActionTarget::Coord { x: 60.0, y: 80.0 },
        parameters: if action == ActionKind::TypeText {
            json!({"text":"追加😀"})
        } else {
            json!({"key":"enter"})
        },
        scope: ActionScope::Directed,
    }
}
fn finish(adapter: MacosAdapter) {
    drop(adapter);
    inspect(|s| {
        assert!(
            s.posts.is_empty() && s.ax_presses.is_empty(),
            "no focus click or pointer fallback"
        );
        assert!(
            s.ax_values.is_empty(),
            "coordinate append never replaces AXValue"
        );
        assert_eq!(s.ax_secure_reads, 0);
        assert_eq!(s.live_owned, 0);
        assert_eq!(s.ax_owned, 0);
    });
}
#[test]
fn screenshot_point_binds_exact_focused_text_for_unicode_append() {
    let (adapter, obs) = setup();
    macos_adapter::run_native_selftest().unwrap();
    let result = adapter.act(&request(&obs, ActionKind::TypeText)).unwrap();
    assert!(result.applied && !result.verifiable && !result.postcondition_ok);
    inspect(|s| {
        assert_eq!(s.ax_text, "原有😀text追加😀");
        assert_eq!(s.ax_append_writes, vec!["range", "text"]);
    });
    assert!(adapter.capabilities().type_text.coordinate);
    finish(adapter);
}
#[test]
fn screenshot_point_binds_exact_focused_control_for_named_key_pair() {
    let (adapter, obs) = setup();
    let result = adapter.act(&request(&obs, ActionKind::Key)).unwrap();
    assert!(result.applied && !result.verifiable && !result.postcondition_ok);
    inspect(|s| assert_eq!(s.key_posts, vec![(7, 36, true, 0), (7, 36, false, 0)]));
    assert!(adapter.capabilities().key.coordinate);
    finish(adapter);
}

fn untouched() {
    inspect(|s| assert!(s.key_posts.is_empty() && s.ax_append_writes.is_empty()));
}

#[test]
fn negative_origin_and_image_scaling_hit_the_requested_native_point() {
    for (size, x, y) in [
        ((1200, 800), 60.0, 80.0),
        ((600, 400), 30.0, 40.0),
        ((900, 600), 45.0, 60.0),
    ] {
        let (adapter, obs) = setup();
        update(|s| s.image_size = size);
        let fresh = adapter.observe_for_run("run", &obs.target_id).unwrap();
        let mut req = request(&fresh, ActionKind::Key);
        req.target = ActionTarget::Coord { x, y };
        adapter.act(&req).unwrap();
        inspect(|s| {
            assert!(s.ax_hit_points.contains(&(-970.0, 120.0)));
            assert!(s.ax_point_hit_calls >= 2);
        });
        finish(adapter);
    }
}

#[test]
fn distorted_screenshot_never_mints_coordinate_authority() {
    let (adapter, obs) = setup();
    update(|s| s.image_size = (900, 1000));
    assert!(adapter.observe_for_run("run", &obs.target_id).is_err());
    untouched();
    finish(adapter);
}

#[test]
fn screenshot_boundaries_and_float_rounding_never_select_an_adjacent_control() {
    for (x, y) in [
        (f64::NAN, 80.0),
        (60.0, f64::INFINITY),
        (-1.0, 80.0),
        (1200.0, 80.0),
        (60.0, 800.0),
        (0.0, 0.0),
        (39.999999, 80.0),
        (60.0, 59.999999),
    ] {
        let (adapter, obs) = setup();
        let mut req = request(&obs, ActionKind::Key);
        req.target = ActionTarget::Coord { x, y };
        assert!(adapter.act(&req).is_err(), "({x},{y}) must not dispatch");
        untouched();
        finish(adapter);
    }
}

#[test]
fn preview_imageless_and_retired_model_observations_never_authorize_coordinates() {
    for fault in 0..5 {
        let (adapter, obs) = setup();
        let mut req = request(&obs, ActionKind::TypeText);
        match fault {
            0 => {
                req = request(
                    &adapter.observe(&obs.target_id).unwrap(),
                    ActionKind::TypeText,
                )
            }
            1 => {
                req = request(
                    &adapter
                        .capture_for_run("run", &obs.target_id, CaptureOptions::model(false))
                        .unwrap(),
                    ActionKind::TypeText,
                )
            }
            2 => adapter.release_target_for_run("run", &obs.target_id),
            3 => {
                adapter.observe_for_run("run", &obs.target_id).unwrap();
            }
            _ => req.run_id = "other".into(),
        }
        assert!(adapter.act(&req).is_err(), "fault {fault}");
        untouched();
        finish(adapter);
    }
}

#[test]
fn native_hit_must_identify_the_exact_observed_control_not_nearest_or_parent() {
    for node in [0, 2, 9, 999] {
        let (adapter, obs) = setup();
        update(|s| s.ax_point_hit_node = node);
        assert!(
            adapter.act(&request(&obs, ActionKind::TypeText)).is_err(),
            "node {node}"
        );
        untouched();
        finish(adapter);
    }
}

#[test]
fn native_hit_errors_null_and_wrong_types_release_all_retained_objects() {
    for fault in 0..4 {
        let (adapter, obs) = setup();
        update(|s| match fault {
            0 => s.ax_point_hit_status = -25204,
            1 => s.ax_point_hit_null = true,
            2 => s.ax_wrong_type = true,
            _ => s.ax_child_wrong_pid = true,
        });
        assert!(adapter.act(&request(&obs, ActionKind::Key)).is_err());
        untouched();
        finish(adapter);
    }
}

#[test]
fn occlusion_focus_permissions_geometry_scope_and_secure_changes_block_coordinates() {
    for fault in 0..17 {
        let (adapter, obs) = setup();
        let mut req = request(&obs, ActionKind::TypeText);
        match fault {
            0 => req.scope = ActionScope::Desktop,
            1 => req.cancellation.cancel(),
            2 => req.geometry_revision += 1,
            3 => req.snapshot_id = "stale".into(),
            _ => update(|s| match fault {
                4 => s.ax_focused = false,
                5 => s.ax_frontmost = false,
                6 => s.ax_focus_node = 2,
                7 => s.ax_focus_window_escape = true,
                8 => s.ax_subrole_secure = true,
                9 => s.ax_allowed = false,
                10 => s.screen_allowed = false,
                11 => s.x += 3.0,
                12 => s.birth.1 += 1,
                13 => s.ax_button_generation += 1,
                14 => s.ax_parent_escape = true,
                15 => s.ax_window_escape = true,
                _ => s.occlusion_rect = Some((-975.0, 115.0, 10.0, 10.0)),
            }),
        }
        assert!(adapter.act(&req).is_err(), "fault {fault}");
        untouched();
        finish(adapter);
    }
}

#[test]
fn requested_point_cannot_gain_focus_or_action_not_present_in_observation() {
    for action in [ActionKind::Key, ActionKind::TypeText] {
        let (adapter, obs) = setup();
        update(|s| s.ax_focused = false);
        let unfocused = adapter.observe_for_run("run", &obs.target_id).unwrap();
        update(|s| s.ax_focused = true);
        assert!(adapter.act(&request(&unfocused, action)).is_err());
        untouched();
        finish(adapter);
    }
}

#[test]
fn native_hit_callbacks_revoke_authority_without_sending_input() {
    for call in [1, 2] {
        for fault in 0..6 {
            let (adapter, obs) = setup();
            let req = request(&obs, ActionKind::Key);
            let cancel = req.cancellation.clone();
            update(|s| {
                s.ax_on_point_hit_at = Some(call);
                s.ax_on_point_hit = Some(Box::new(move || {
                    update(|s| match fault {
                        0 => cancel.cancel(),
                        1 => s.ax_subrole_secure = true,
                        2 => s.ax_focused = false,
                        3 => s.ax_allowed = false,
                        4 => s.birth.1 += 1,
                        _ => s.ax_button_size.0 += 1.0,
                    })
                }));
            });
            assert!(adapter.act(&req).is_err(), "call {call} fault {fault}");
            untouched();
            finish(adapter);
        }
    }
}

#[test]
fn changed_hit_after_key_allocation_is_not_rebound_or_clicked() {
    let (adapter, obs) = setup();
    update(|s| s.on_key_configure = Some(Box::new(|| update(|s| s.ax_point_hit_node = 2))));
    assert!(adapter.act(&request(&obs, ActionKind::Key)).is_err());
    untouched();
    finish(adapter);
}

#[test]
fn stop_inside_final_hit_retires_coordinate_authority_without_deadlock() {
    let (adapter, obs) = setup();
    let adapter = std::sync::Arc::new(adapter);
    let weak = std::sync::Arc::downgrade(&adapter);
    let id = obs.target_id.clone();
    update(|s| {
        s.ax_on_point_hit_at = Some(2);
        s.ax_on_point_hit = Some(Box::new(move || {
            if let Some(adapter) = weak.upgrade() {
                adapter.release_target_for_run("run", &id);
            }
        }));
    });
    assert!(adapter.act(&request(&obs, ActionKind::Key)).is_err());
    untouched();
    finish(std::sync::Arc::try_unwrap(adapter).ok().unwrap());
}

#[test]
fn coordinate_key_cancellation_cleans_only_its_original_pair() {
    let (adapter, obs) = setup();
    let req = request(&obs, ActionKind::Key);
    let cancel = req.cancellation.clone();
    update(|s| s.on_key_down = Some(Box::new(move || cancel.cancel())));
    assert!(adapter.act(&req).is_err());
    inspect(|s| assert_eq!(s.key_posts, vec![(7, 36, true, 0), (7, 36, false, 0)]));
    assert!(adapter.is_idle("run"));
    finish(adapter);
}

#[test]
fn coordinate_text_cancellation_after_selection_does_not_append() {
    let (adapter, obs) = setup();
    let req = request(&obs, ActionKind::TypeText);
    let cancel = req.cancellation.clone();
    update(|s| s.ax_on_range_set = Some(Box::new(move || cancel.cancel())));
    assert!(adapter.act(&req).is_err());
    inspect(|s| {
        assert_eq!(s.ax_append_writes, vec!["range"]);
        assert_eq!(s.ax_text, "原有😀text");
    });
    finish(adapter);
}

#[test]
fn failed_semantic_reference_never_falls_back_to_a_coordinate_hit() {
    let (adapter, obs) = setup();
    let mut req = request(&obs, ActionKind::Key);
    req.target = ActionTarget::Element {
        element_ref: "unknown".into(),
    };
    assert!(adapter.act(&req).is_err());
    untouched();
    inspect(|s| assert_eq!(s.ax_point_hit_calls, 0));
    finish(adapter);
}

#[test]
fn coordinate_set_value_is_not_silently_treated_as_append() {
    let (adapter, obs) = setup();
    let mut req = request(&obs, ActionKind::TypeText);
    req.action = ActionKind::SetValue;
    assert!(adapter.act(&req).is_err());
    untouched();
    finish(adapter);
}

#[test]
fn changing_coordinate_hit_or_occlusion_after_selection_prevents_text() {
    for fault in 0..4 {
        let (adapter, obs) = setup();
        update(|s| {
            s.ax_on_range_set = Some(Box::new(move || {
                update(|s| match fault {
                    0 => s.ax_point_hit_node = 2,
                    1 => s.occlusion_rect = Some((-975.0, 115.0, 10.0, 10.0)),
                    2 => s.ax_button_hidden = true,
                    _ => s.ax_button_size.0 += 2.0,
                })
            }))
        });
        assert!(
            adapter.act(&request(&obs, ActionKind::TypeText)).is_err(),
            "fault {fault}"
        );
        inspect(|s| {
            assert_eq!(s.ax_append_writes, vec!["range"]);
            assert_eq!(s.ax_text, "原有😀text");
        });
        finish(adapter);
    }
}

#[test]
fn unknown_coordinate_input_completion_retains_occupancy_and_forbids_replay() {
    for action in [ActionKind::Key, ActionKind::TypeText] {
        let (adapter, obs) = setup();
        update(|s| {
            if action == ActionKind::Key {
                s.on_key_down = Some(Box::new(|| update(|s| s.ax_focus_window_escape = true)));
            } else {
                s.ax_append_status.1 = -25204;
            }
        });
        let req = request(&obs, action);
        assert!(adapter.act(&req).is_err());
        assert!(!adapter.is_idle("run"));
        let counts = inspect(|s| (s.key_posts.len(), s.ax_append_writes.len()));
        assert!(adapter.act(&req).is_err());
        inspect(|s| assert_eq!((s.key_posts.len(), s.ax_append_writes.len()), counts));
        finish(adapter);
    }
}

#[test]
fn coordinate_empty_text_is_noop_and_explicit_clipboard_is_not_substituted() {
    let (adapter, obs) = setup();
    let mut req = request(&obs, ActionKind::TypeText);
    req.parameters = json!({"text":""});
    let result = adapter.act(&req).unwrap();
    assert!(!result.applied && !result.verifiable);
    untouched();
    req.parameters = json!({"text":"x","via":"clipboard"});
    assert!(adapter.act(&req).is_err());
    untouched();
    finish(adapter);
}

#[test]
fn coordinate_tab_can_move_focus_but_releases_only_inside_original_window() {
    let (adapter, obs) = setup();
    let mut req = request(&obs, ActionKind::Key);
    req.parameters = json!({"key":"tab"});
    update(|s| s.on_key_down = Some(Box::new(|| update(|s| s.ax_focus_node = 2))));
    assert!(adapter.act(&req).unwrap().applied);
    inspect(|s| assert_eq!(s.key_posts, vec![(7, 48, true, 0), (7, 48, false, 0)]));
    assert!(adapter.is_idle("run"));
    finish(adapter);
}
