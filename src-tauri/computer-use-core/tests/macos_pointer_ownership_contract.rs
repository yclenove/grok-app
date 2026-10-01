//! Production pointer path with deterministic FFI doubles, not macOS acceptance.
#![cfg(not(target_os = "macos"))]
pub use grok_computer_use_core::{adapter, protocol};
#[path = "../../src/computer_use/macos_adapter.rs"]
mod macos_adapter;
#[path = "support/quartz_ffi.rs"]
mod quartz_ffi;

use adapter::{ActionScope, ComputerUseAdapter, DispatchRequest};
use macos_adapter::MacosAdapter;
use protocol::{ActionKind, ActionTarget};
use quartz_ffi::{inspect, reset, update};
use serde_json::json;
use std::sync::Arc;

fn setup(action: ActionKind) -> (Arc<MacosAdapter>, DispatchRequest) {
    reset();
    let adapter = Arc::new(MacosAdapter::new());
    macos_adapter::run_native_selftest().unwrap(); // FFI double, not native proof.
    let target_id = adapter.list_targets().unwrap().remove(0).target_id;
    let obs = adapter.observe_for_run("run", &target_id).unwrap();
    let req = DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "run".into(),
        action_id: "pointer-ownership".into(),
        generation: 1,
        target_id,
        target_generation: 1,
        snapshot_id: obs.snapshot_id,
        geometry_revision: obs.geometry_revision,
        action,
        target: ActionTarget::Coord { x: 100.0, y: 120.0 },
        parameters: match action {
            ActionKind::Click => json!({}),
            ActionKind::Scroll => json!({"delta":120}),
            _ => json!({"toX":900,"toY":600}),
        },
        scope: ActionScope::Directed,
    };
    (adapter, req)
}

fn finish(adapter: Arc<MacosAdapter>) {
    drop(adapter);
    inspect(|s| {
        assert_eq!(s.live_owned, 0);
        assert_eq!(s.ax_owned, 0);
        assert!(s.ax_presses.is_empty() && s.ax_values.is_empty());
    });
}

#[test]
fn pointer_refuses_user_or_foreign_synthetic_buttons_before_input() {
    for action in [ActionKind::Click, ActionKind::Scroll, ActionKind::Drag] {
        for button in 0..3 {
            for synthetic in [false, true] {
                let (adapter, req) = setup(action);
                update(|s| {
                    if synthetic {
                        s.synthetic_button = Some(button)
                    } else {
                        s.held_button = Some(button)
                    }
                });
                assert!(
                    adapter.act(&req).is_err(),
                    "{action:?}/{button}/{synthetic}"
                );
                inspect(|s| {
                    assert!(s.posts.is_empty() && s.wheel_posts.is_empty());
                    assert_eq!(s.event_allocations, 0);
                });
                assert!(adapter.is_idle("run"));
                finish(adapter);
            }
        }
    }
}

#[test]
fn pointer_refuses_physical_and_synthetic_modifier_state() {
    for action in [ActionKind::Click, ActionKind::Scroll, ActionKind::Drag] {
        for synthetic in [false, true] {
            let (adapter, req) = setup(action);
            update(|s| {
                if synthetic {
                    s.synthetic_flags = 0x40000
                } else {
                    s.keyboard_flags = 0x20000
                }
            });
            assert!(adapter.act(&req).is_err(), "{action:?}/{synthetic}");
            inspect(|s| assert!(s.posts.is_empty() && s.wheel_posts.is_empty()));
            assert!(adapter.is_idle("run"));
            finish(adapter);
        }
    }
}

#[test]
fn button_pressed_during_event_configuration_prevents_first_post() {
    for action in [ActionKind::Click, ActionKind::Scroll, ActionKind::Drag] {
        let (adapter, req) = setup(action);
        update(|s| s.on_event_configure = Some(Box::new(|| update(|s| s.held_button = Some(0)))));
        assert!(adapter.act(&req).is_err(), "{action:?}");
        inspect(|s| {
            assert!(s.on_event_configure.is_none());
            assert!(s.posts.is_empty() && s.wheel_posts.is_empty());
        });
        assert!(adapter.is_idle("run"));
        finish(adapter);
    }
}

#[test]
fn user_button_after_down_does_not_receive_a_synthetic_up_or_false_idle() {
    for action in [ActionKind::Click, ActionKind::Drag] {
        let (adapter, req) = setup(action);
        update(|s| s.on_mouse_down = Some(Box::new(|| update(|s| s.held_button = Some(0)))));
        let result = adapter.act(&req);
        assert!(result.is_err(), "{action:?}");
        assert!(result.unwrap_err().contains("release unconfirmed"));
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

#[test]
fn own_queued_down_allows_drag_motion_release_and_double_click() {
    for action in [ActionKind::Click, ActionKind::Drag] {
        let (adapter, mut req) = setup(action);
        if action == ActionKind::Click {
            req.parameters = json!({"count":2});
        }
        update(|s| s.on_mouse_down = Some(Box::new(|| update(|s| s.synthetic_button = Some(0)))));
        assert!(adapter.act(&req).unwrap().applied);
        inspect(|s| {
            assert_eq!(
                s.posts.len(),
                if action == ActionKind::Click { 4 } else { 18 }
            )
        });
        assert!(adapter.is_idle("run"));
        finish(adapter);
    }
}

#[test]
fn pointer_events_use_private_source_not_hid_state() {
    for action in [ActionKind::Click, ActionKind::Scroll, ActionKind::Drag] {
        let (adapter, req) = setup(action);
        assert!(adapter.act(&req).unwrap().applied);
        inspect(|s| assert_eq!(s.key_sources, vec![u32::MAX]));
        finish(adapter);
    }
}

#[test]
fn caps_lock_is_not_a_held_pointer_modifier_and_events_are_explicitly_neutral() {
    for action in [ActionKind::Click, ActionKind::Scroll, ActionKind::Drag] {
        let (adapter, req) = setup(action);
        update(|s| s.keyboard_flags = 0x10000);
        assert!(adapter.act(&req).unwrap().applied);
        inspect(|s| {
            assert!(!s.pointer_post_flags.is_empty());
            assert!(s.pointer_post_flags.iter().all(|flags| *flags == 0));
        });
        finish(adapter);
    }
}

#[test]
fn right_middle_and_double_click_keep_their_own_button_identity() {
    for (name, button, down, up) in [("left", 0, 1, 2), ("right", 1, 3, 4), ("middle", 2, 25, 26)] {
        let (adapter, mut req) = setup(ActionKind::Click);
        req.parameters = json!({"button":name,"count":2});
        update(|s| {
            s.on_mouse_down = Some(Box::new(move || {
                update(|s| s.synthetic_button = Some(button))
            }))
        });
        assert!(adapter.act(&req).unwrap().applied);
        inspect(|s| {
            assert_eq!(
                s.posts.iter().map(|p| (p.1, p.4)).collect::<Vec<_>>(),
                vec![(down, 1), (up, 1), (down, 2), (up, 2)]
            )
        });
        assert!(adapter.is_idle("run"));
        finish(adapter);
    }
}

#[test]
fn foreign_button_or_modifier_during_drag_stops_motion_without_false_release() {
    for foreign_modifier in [false, true] {
        let (adapter, req) = setup(ActionKind::Drag);
        update(|s| {
            s.on_mouse_drag = Some(Box::new(move || {
                update(|s| {
                    if foreign_modifier {
                        s.synthetic_flags = 0x100000
                    } else {
                        s.synthetic_button = Some(1)
                    }
                })
            }))
        });
        assert!(adapter
            .act(&req)
            .unwrap_err()
            .contains("release unconfirmed"));
        inspect(|s| {
            assert!(s.on_mouse_drag.is_none());
            assert_eq!(s.posts.len(), 2);
            assert_eq!(s.posts.last().unwrap().1, 6);
        });
        assert!(!adapter.is_idle("run"));
        adapter.abort("run", 2).unwrap();
        assert!(!adapter.is_idle("run"));
        finish(adapter);
    }
}
