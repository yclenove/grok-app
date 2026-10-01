//! Production adapter + deterministic FFI doubles; NOT macOS native acceptance.
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
use std::sync::Arc;

fn setup() -> (Arc<MacosAdapter>, Observation) {
    reset();
    let adapter = Arc::new(MacosAdapter::new());
    let target = adapter.list_targets().unwrap().remove(0).target_id;
    let obs = adapter.observe_for_run("run", &target).unwrap();
    (adapter, obs)
}
fn request(obs: &Observation, action: ActionKind) -> DispatchRequest {
    DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "run".into(),
        action_id: "pointer".into(),
        generation: 1,
        target_id: obs.target_id.clone(),
        target_generation: 1,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action,
        target: ActionTarget::Coord { x: 100.0, y: 120.0 },
        parameters: if action == ActionKind::Scroll {
            json!({"delta":120})
        } else {
            json!({"toX":900,"toY":600})
        },
        scope: ActionScope::Directed,
    }
}
fn finish(adapter: Arc<MacosAdapter>) {
    drop(adapter);
    inspect(|s| {
        assert_eq!(s.live_owned, 0);
        assert_eq!(s.ax_owned, 0);
        assert!(s.ax_presses.is_empty());
        assert!(s.ax_values.is_empty());
        assert_eq!(s.ax_secure_reads, 0);
    });
}

#[test]
fn capabilities_advertise_only_coordinate_scroll_and_drag_with_both_permissions() {
    let (adapter, _) = setup();
    macos_adapter::run_native_selftest().unwrap(); // doubles, not host proof
    for permission in 0..3 {
        update(|s| {
            s.screen_allowed = permission != 1;
            s.ax_allowed = permission != 2;
        });
        let caps = adapter.capabilities();
        assert_eq!(caps.scroll.coordinate, permission == 0);
        assert_eq!(caps.drag.coordinate, permission == 0);
        assert!(!caps.scroll.semantic && !caps.drag.semantic);
        assert_eq!(caps.type_text.coordinate, permission == 0);
        assert_eq!(caps.key.coordinate, permission == 0);
        assert!(!caps.chinese_ime);
    }
    finish(adapter);
}

#[test]
fn protocol_down_and_up_translate_to_opposite_quartz_delta_without_click() {
    let (adapter, obs) = setup();
    for delta in [-2400, -120, -1, 1, 120, 2400] {
        let mut req = request(&obs, ActionKind::Scroll);
        req.parameters = json!({"delta":delta});
        let result = adapter.act(&req).unwrap();
        assert!(result.applied && !result.verifiable && !result.postcondition_ok);
        inspect(|s| {
            // choice.rs scroll_down uses positive; native Quartz is positive UP.
            assert_eq!(s.wheel_creations.last(), Some(&(0, 1, -delta, 0, 0)));
            assert_eq!(s.wheel_posts.last(), Some(&(7, -950.0, 140.0, -delta)));
            assert!(
                s.posts.is_empty(),
                "wheel must not click to focus or move globally"
            );
        });
    }
    assert!(adapter.is_idle("run"));
    finish(adapter);
}

#[test]
fn zero_wheel_is_explicit_noop_without_native_event() {
    let (adapter, obs) = setup();
    let mut req = request(&obs, ActionKind::Scroll);
    req.parameters = json!({"delta":0});
    assert!(!adapter.act(&req).unwrap().applied);
    inspect(|s| {
        assert!(s.wheel_creations.is_empty());
        assert!(s.wheel_posts.is_empty());
    });
    finish(adapter);
}

#[test]
fn drag_posts_one_down_interpolated_path_and_one_up_in_same_window() {
    for legacy in [false, true] {
        let (adapter, obs) = setup();
        let mut req = request(&obs, ActionKind::Drag);
        if legacy {
            req.parameters = json!({"x1":900,"y1":600});
        }
        let result = adapter.act(&req).unwrap();
        assert!(result.applied && !result.verifiable && !result.postcondition_ok);
        inspect(|s| {
            assert_eq!(s.posts.len(), 18);
            assert_eq!(s.posts[0], (7, 1, -950.0, 140.0, 1));
            for step in 1..=16 {
                assert_eq!(
                    s.posts[step],
                    (
                        7,
                        6,
                        -950.0 + 25.0 * step as f64,
                        140.0 + 15.0 * step as f64,
                        1
                    )
                );
            }
            assert_eq!(s.posts[17], (7, 2, -550.0, 380.0, 1));
            assert!(s.wheel_posts.is_empty());
        });
        assert!(adapter.is_idle("run"));
        finish(adapter);
    }
}

#[test]
fn invalid_parameters_or_outside_destination_never_begin_input() {
    let (adapter, obs) = setup();
    for (action, values) in [
        (
            ActionKind::Scroll,
            vec![
                json!({}),
                json!(null),
                json!({"delta":2401}),
                json!({"delta":-2401}),
                json!({"delta":1.5}),
                json!({"delta":"120"}),
                json!({"delta":120,"extra":true}),
            ],
        ),
        (
            ActionKind::Drag,
            vec![
                json!({}),
                json!({"toX":20}),
                json!({"toX":20,"toY":30,"x1":20,"y1":30}),
                json!({"toX":-1,"toY":30}),
                json!({"toX":1200,"toY":30}),
                json!({"toX":20,"toY":800}),
                json!({"toX":20,"toY":30,"extra":true}),
            ],
        ),
    ] {
        for value in values {
            let mut req = request(&obs, action);
            req.parameters = value;
            assert!(
                adapter.act(&req).is_err(),
                "{:?}: {:?}",
                action,
                req.parameters
            );
        }
    }
    inspect(|s| {
        assert!(s.posts.is_empty());
        assert!(s.wheel_posts.is_empty());
    });
    finish(adapter);
}

#[test]
fn neither_preview_semantic_only_nor_cross_run_can_authorize_pointer_input() {
    for action in [ActionKind::Scroll, ActionKind::Drag] {
        let (adapter, obs) = setup();
        let preview = adapter.observe(&obs.target_id).unwrap();
        assert!(adapter.act(&request(&preview, action)).is_err());
        let mut req = request(&obs, action);
        req.run_id = "other".into();
        assert!(adapter.act(&req).is_err());
        req = request(&obs, action);
        req.target = ActionTarget::Element {
            element_ref: obs.nodes[0].node_ref.clone(),
        };
        assert!(adapter.act(&req).is_err());
        let semantic = adapter
            .capture_for_run("run", &obs.target_id, CaptureOptions::model(false))
            .unwrap();
        assert!(adapter.act(&request(&obs, action)).is_err());
        assert!(adapter.act(&request(&semantic, action)).is_err());
        inspect(|s| {
            assert!(s.posts.is_empty());
            assert!(s.wheel_posts.is_empty());
        });
        finish(adapter);
    }
}

#[test]
fn source_or_any_drag_event_allocation_failure_precedes_all_input() {
    // Down + 16 drag samples each have a preallocated owned release.
    for fault in 0..=34 {
        let (adapter, obs) = setup();
        update(|s| {
            s.fail_event_source = fault == 0;
            s.fail_event_number = Some(fault);
        });
        assert!(adapter.act(&request(&obs, ActionKind::Drag)).is_err());
        inspect(|s| assert!(s.posts.is_empty(), "allocation {fault}"));
        assert!(adapter.is_idle("run"));
        finish(adapter);
    }
}

#[test]
fn configuration_time_cancel_release_permission_or_identity_change_never_posts() {
    for action in [ActionKind::Scroll, ActionKind::Drag] {
        for fault in 0..7 {
            let (adapter, obs) = setup();
            let req = request(&obs, action);
            let cancel = req.cancellation.clone();
            let weak = Arc::downgrade(&adapter);
            let target = obs.target_id.clone();
            update(|s| {
                s.on_event_configure = Some(Box::new(move || match fault {
                    0 => cancel.cancel(),
                    1 => weak
                        .upgrade()
                        .unwrap()
                        .release_target_for_run("run", &target),
                    2 => update(|s| s.ax_allowed = false),
                    3 => update(|s| s.screen_allowed = false),
                    4 => update(|s| s.ax_instance += 1),
                    5 => update(|s| s.birth.1 += 1),
                    _ => update(|s| s.x += 10.0),
                }))
            });
            assert!(adapter.act(&req).is_err(), "{action:?}/{fault}");
            inspect(|s| {
                assert!(s.on_event_configure.is_none(), "hook must run");
                assert!(s.posts.is_empty());
                assert!(s.wheel_posts.is_empty());
            });
            assert!(adapter.is_idle("run"));
            finish(adapter);
        }
    }
}

#[test]
fn drag_cancel_or_observation_retirement_releases_only_last_posted_point() {
    for after_drag in [false, true] {
        for retire in [false, true] {
            let (adapter, obs) = setup();
            let req = request(&obs, ActionKind::Drag);
            let cancel = req.cancellation.clone();
            let weak = Arc::downgrade(&adapter);
            let target = obs.target_id.clone();
            let hook = Box::new(move || {
                if retire {
                    weak.upgrade()
                        .unwrap()
                        .release_target_for_run("run", &target)
                } else {
                    cancel.cancel()
                }
            });
            update(|s| {
                if after_drag {
                    s.on_mouse_drag = Some(hook)
                } else {
                    s.on_mouse_down = Some(hook)
                }
            });
            assert!(adapter.act(&req).is_err());
            inspect(|s| {
                assert!(s.on_mouse_drag.is_none() && s.on_mouse_down.is_none());
                assert_eq!(s.posts.len(), if after_drag { 3 } else { 2 });
                let last = &s.posts[s.posts.len() - 1];
                let prior = &s.posts[s.posts.len() - 2];
                assert_eq!(last.1, 2);
                assert_eq!((last.2, last.3), (prior.2, prior.3));
            });
            assert!(adapter.is_idle("run"));
            finish(adapter);
        }
    }
}

#[test]
fn unknown_drag_release_retains_occupancy_and_stop_does_not_replay() {
    for fault in 0..6 {
        let (adapter, obs) = setup();
        let req = request(&obs, ActionKind::Drag);
        update(|s| {
            s.on_mouse_drag = Some(Box::new(move || {
                update(|s| match fault {
                    0 => s.ax_instance += 1,
                    1 => s.birth.1 += 1,
                    2 => s.occluded = true,
                    3 => s.x += 10.0,
                    4 => s.ax_allowed = false,
                    _ => s.screen_allowed = false,
                })
            }))
        });
        let error = adapter.act(&req).unwrap_err();
        assert!(error.contains("release unconfirmed"), "{fault}: {error}");
        inspect(|s| {
            assert!(s.on_mouse_drag.is_none());
            assert_eq!(s.posts.len(), 2);
            assert_eq!(s.posts[1].1, 6);
        });
        assert!(!adapter.is_idle("run"));
        adapter.abort("run", 2).unwrap();
        assert!(!adapter.is_idle("other"));
        assert!(adapter.act(&req).is_err());
        inspect(|s| assert_eq!(s.posts.len(), 2));
        finish(adapter);
    }
}

#[test]
fn cancellation_during_wheel_post_does_not_repeat_or_invent_mouse_release() {
    let (adapter, obs) = setup();
    let req = request(&obs, ActionKind::Scroll);
    let cancel = req.cancellation.clone();
    update(|s| s.on_wheel_post = Some(Box::new(move || cancel.cancel())));
    assert!(adapter.act(&req).is_err());
    assert!(adapter.act(&req).is_err());
    inspect(|s| {
        assert!(s.on_wheel_post.is_none());
        assert_eq!(s.wheel_posts.len(), 1);
        assert!(s.posts.is_empty());
    });
    assert!(adapter.is_idle("run"));
    finish(adapter);
}

#[test]
fn path_only_occlusion_is_rejected_before_drag_down_including_after_configuration() {
    for during_configuration in [false, true] {
        let (adapter, obs) = setup();
        if during_configuration {
            update(|s| {
                s.on_event_configure = Some(Box::new(|| {
                    update(|s| s.occlusion_rect = Some((-755.0, 255.0, 10.0, 10.0)))
                }))
            });
        } else {
            update(|s| s.occlusion_rect = Some((-755.0, 255.0, 10.0, 10.0)));
        }
        assert!(adapter.act(&request(&obs, ActionKind::Drag)).is_err());
        inspect(|s| {
            assert!(s.posts.is_empty());
            assert!(s.on_event_configure.is_none());
        });
        assert!(adapter.is_idle("run"));
        finish(adapter);
    }
}

#[test]
fn new_obstruction_on_next_point_allows_release_at_previous_visible_point() {
    let (adapter, obs) = setup();
    update(|s| {
        s.on_mouse_drag = Some(Box::new(|| {
            update(|s| s.occlusion_rect = Some((-901.0, 169.0, 2.0, 2.0)))
        }))
    });
    assert!(adapter.act(&request(&obs, ActionKind::Drag)).is_err());
    inspect(|s| {
        assert_eq!(
            s.posts,
            vec![
                (7, 1, -950.0, 140.0, 1),
                (7, 6, -925.0, 155.0, 1),
                (7, 2, -925.0, 155.0, 1)
            ]
        );
        assert!(s.on_mouse_drag.is_none());
    });
    assert!(adapter.is_idle("run"));
    finish(adapter);
}

#[test]
fn wheel_source_and_event_failure_do_not_post_or_leak() {
    for source in [false, true] {
        let (adapter, obs) = setup();
        update(|s| {
            s.fail_event_source = source;
            s.fail_event_number = Some(1);
        });
        assert!(adapter.act(&request(&obs, ActionKind::Scroll)).is_err());
        inspect(|s| {
            assert!(s.wheel_posts.is_empty());
            assert!(s.posts.is_empty());
        });
        assert!(adapter.is_idle("run"));
        finish(adapter);
    }
}

#[test]
fn target_coordinate_scope_snapshot_and_pre_cancel_checks_precede_events() {
    for action in [ActionKind::Scroll, ActionKind::Drag] {
        for fault in 0..10 {
            let (adapter, obs) = setup();
            let mut req = request(&obs, action);
            match fault {
                0 => req.scope = ActionScope::Desktop,
                1 => req.snapshot_id = "stale".into(),
                2 => req.geometry_revision += 1,
                3 => req.cancellation.cancel(),
                4 => {
                    req.target = ActionTarget::Coord {
                        x: f64::NAN,
                        y: 10.0,
                    }
                }
                5 => {
                    req.target = ActionTarget::Coord {
                        x: 10.0,
                        y: f64::INFINITY,
                    }
                }
                6 => req.target = ActionTarget::Coord { x: -1.0, y: 10.0 },
                7 => req.target = ActionTarget::Coord { x: 1200.0, y: 10.0 },
                8 => req.target = ActionTarget::Coord { x: 10.0, y: 800.0 },
                _ => req.target_id = "mac:7:11".into(),
            }
            assert!(adapter.act(&req).is_err(), "{action:?}/{fault}");
            inspect(|s| {
                assert!(s.posts.is_empty());
                assert!(s.wheel_posts.is_empty());
                assert_eq!(s.event_allocations, 0);
            });
            finish(adapter);
        }
    }
}

#[test]
fn native_post_remains_exclusive_during_reentrant_stop_and_other_run_attempt() {
    for action in [ActionKind::Scroll, ActionKind::Drag] {
        let (adapter, obs) = setup();
        let req = request(&obs, action);
        let weak = Arc::downgrade(&adapter);
        let mut other = req.clone();
        other.run_id = "other".into();
        other.cancellation = Default::default();
        let observed = Arc::new(std::sync::Mutex::new(None));
        let evidence = observed.clone();
        let hook = Box::new(move || {
            let adapter = weak.upgrade().unwrap();
            let before = adapter.is_idle("other");
            adapter.abort("run", 2).unwrap();
            let after = adapter.is_idle("run");
            *evidence.lock().unwrap() = Some((before, after, adapter.act(&other).err()));
        });
        update(|s| {
            if action == ActionKind::Scroll {
                s.on_wheel_post = Some(hook)
            } else {
                s.on_mouse_down = Some(hook)
            }
        });
        assert!(adapter.act(&req).is_err());
        let (before, after, error) = observed
            .lock()
            .unwrap()
            .take()
            .expect("native post hook must run");
        assert!(!before && !after);
        assert!(error.unwrap().contains("occupied"));
        inspect(|s| {
            assert!(s.on_mouse_down.is_none() && s.on_wheel_post.is_none());
            assert_eq!(
                s.posts.len(),
                if action == ActionKind::Scroll { 0 } else { 2 }
            );
            assert_eq!(
                s.wheel_posts.len(),
                if action == ActionKind::Scroll { 1 } else { 0 }
            );
        });
        assert!(adapter.is_idle("run"));
        finish(adapter);
    }
}

#[test]
fn click_configuration_cannot_bypass_late_permission_revocation() {
    for screen in [false, true] {
        let (adapter, obs) = setup();
        let mut req = request(&obs, ActionKind::Click);
        req.parameters = json!({});
        update(|s| {
            s.on_event_configure = Some(Box::new(move || {
                update(|s| {
                    if screen {
                        s.screen_allowed = false
                    } else {
                        s.ax_allowed = false
                    }
                })
            }))
        });
        assert!(adapter.act(&req).is_err(), "screen={screen}");
        inspect(|s| {
            assert!(s.on_event_configure.is_none());
            assert!(s.posts.is_empty());
        });
        assert!(adapter.is_idle("run"));
        finish(adapter);
    }
}

#[test]
fn click_release_permission_loss_does_not_claim_cleanup_success() {
    for screen in [false, true] {
        let (adapter, obs) = setup();
        let mut req = request(&obs, ActionKind::Click);
        req.parameters = json!({});
        update(|s| {
            s.on_mouse_down = Some(Box::new(move || {
                update(|s| {
                    if screen {
                        s.screen_allowed = false
                    } else {
                        s.ax_allowed = false
                    }
                })
            }))
        });
        assert!(adapter
            .act(&req)
            .unwrap_err()
            .contains("release unconfirmed"));
        inspect(|s| {
            assert!(s.on_mouse_down.is_none());
            assert_eq!(s.posts.len(), 1);
        });
        assert!(!adapter.is_idle("run"));
        adapter.abort("run", 2).unwrap();
        assert!(!adapter.is_idle("run"));
        finish(adapter);
    }
}
