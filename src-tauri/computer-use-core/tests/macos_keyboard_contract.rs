//! Production keyboard path with FFI doubles; NOT macOS native proof.
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
    update(|s| {
        s.ax_tree_enabled = true;
        s.ax_focused = true;
    });
    let adapter = Arc::new(MacosAdapter::new());
    let id = adapter.list_targets().unwrap().remove(0).target_id;
    let obs = adapter.observe_for_run("run", &id).unwrap();
    (adapter, obs)
}
fn request(obs: &Observation, key: &str) -> DispatchRequest {
    DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "run".into(),
        action_id: "key".into(),
        generation: 1,
        target_id: obs.target_id.clone(),
        target_generation: 1,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action: ActionKind::Key,
        target: ActionTarget::Element {
            element_ref: obs.nodes[1].node_ref.clone(),
        },
        parameters: json!({"key":key}),
        scope: ActionScope::Directed,
    }
}
fn finish(adapter: Arc<MacosAdapter>) {
    drop(adapter);
    inspect(|s| {
        assert_eq!(s.live_owned, 0);
        assert_eq!(s.ax_owned, 0);
        assert_eq!(s.ax_secure_reads, 0);
        assert!(s.posts.is_empty() && s.wheel_posts.is_empty());
        assert!(s.ax_presses.is_empty() && s.ax_values.is_empty());
    });
}
#[test]
fn focused_node_advertises_named_keys_without_ime_claims() {
    let (adapter, obs) = setup();
    macos_adapter::run_native_selftest().unwrap();
    assert!(adapter.capabilities().key.semantic);
    assert!(adapter.capabilities().key.coordinate && !adapter.capabilities().chinese_ime);
    assert!(obs.nodes[1].actions.iter().any(|a| a == "key"));
    assert!(!obs.nodes[0].actions.iter().any(|a| a == "key"));
    finish(adapter);
}
#[test]
fn all_named_keys_post_one_unmodified_down_up_to_the_original_pid() {
    // Independent Apple HIToolbox virtual-key values, not the implementation table.
    for (name, code) in [
        ("return", 36),
        ("tab", 48),
        ("escape", 53),
        ("space", 49),
        ("backspace", 51),
        ("delete", 117),
        ("home", 115),
        ("end", 119),
        ("pageup", 116),
        ("pagedown", 121),
        ("left", 123),
        ("right", 124),
        ("down", 125),
        ("up", 126),
    ] {
        let (adapter, obs) = setup();
        let result = adapter.act(&request(&obs, name)).unwrap();
        assert!(result.applied && !result.verifiable && !result.postcondition_ok);
        inspect(|s| {
            assert_eq!(s.key_posts, vec![(7, code, true, 0), (7, code, false, 0)]);
            assert_eq!(s.key_sources, vec![u32::MAX]);
        });
        assert!(adapter.is_idle("run"));
        finish(adapter);
    }
}
#[test]
fn cancellation_after_down_releases_only_our_key_and_never_replays() {
    let (adapter, obs) = setup();
    let req = request(&obs, "tab");
    let cancel = req.cancellation.clone();
    update(|s| s.on_key_down = Some(Box::new(move || cancel.cancel())));
    assert!(adapter.act(&req).is_err());
    inspect(|s| assert_eq!(s.key_posts, vec![(7, 48, true, 0), (7, 48, false, 0)]));
    assert!(adapter.is_idle("run"));
    finish(adapter);
}
#[test]
fn focus_change_within_same_window_still_releases_our_tab() {
    let (adapter, obs) = setup();
    update(|s| {
        s.on_key_down = Some(Box::new(|| {
            update(|s| {
                s.ax_focused = false;
                s.ax_focus_node = 2;
            })
        }))
    });
    assert!(adapter.act(&request(&obs, "tab")).unwrap().applied);
    inspect(|s| assert_eq!(s.key_posts, vec![(7, 48, true, 0), (7, 48, false, 0)]));
    finish(adapter);
}
#[test]
fn changed_window_after_down_retains_occupancy_and_never_releases_to_replacement() {
    let (adapter, obs) = setup();
    update(|s| s.on_key_down = Some(Box::new(|| update(|s| s.ax_focus_window_escape = true))));
    assert!(adapter.act(&request(&obs, "enter")).is_err());
    inspect(|s| assert_eq!(s.key_posts, vec![(7, 36, true, 0)]));
    assert!(!adapter.is_idle("run"));
    adapter.abort("run", 2).unwrap();
    assert!(!adapter.is_idle("run"));
    finish(adapter);
}

#[test]
fn unsupported_focus_states_never_advertise_keys() {
    for fault in 0..8 {
        let (adapter, obs) = setup();
        update(|s| match fault {
            0 => s.ax_focused = false,
            1 => s.ax_frontmost = false,
            2 => s.ax_focus_window_escape = true,
            3 => s.ax_focus_node = 2,
            4 => s.ax_button_enabled = false,
            5 => s.ax_button_hidden = true,
            6 => s.ax_subrole_secure = true,
            _ => s.ax_text_role = Some("AXSecureTextField"),
        });
        let now = adapter.observe_for_run("run", &obs.target_id).unwrap();
        assert!(now
            .nodes
            .iter()
            .all(|n| !n.actions.iter().any(|a| a == "key")));
        finish(adapter);
    }
}
#[test]
fn malformed_or_forbidden_keys_cannot_allocate_or_dispatch() {
    for params in [
        json!({}),
        json!({"key":""}),
        json!({"key":null}),
        json!({"key":3}),
        json!({"key":"cmd+a"}),
        json!({"key":"alt+f4"}),
        json!({"key":"win+l"}),
        json!({"key":"F1"}),
        json!({"key":"a"}),
        json!({"key":"😀"}),
        json!({"key":"enter","force":true}),
        json!({"key":"space","text":"hello"}),
        json!({"key":"a".repeat(100)}),
        json!([]),
    ] {
        let (adapter, obs) = setup();
        let mut req = request(&obs, "enter");
        req.parameters = params;
        assert!(adapter.act(&req).is_err());
        inspect(|s| {
            assert!(s.key_posts.is_empty());
            assert_eq!(s.event_allocations, 0);
        });
        assert!(adapter.is_idle("run"));
        finish(adapter);
    }
}
#[test]
fn changed_authority_or_identity_cannot_dispatch_from_a_retained_reference() {
    for fault in 0..17 {
        let (adapter, obs) = setup();
        let mut req = request(&obs, "enter");
        match fault {
            0 => req.snapshot_id = "stale".into(),
            1 => req.run_id = "another".into(),
            2 => req.geometry_revision += 1,
            3 => req.scope = ActionScope::Desktop,
            4 => req.cancellation.cancel(),
            5 => adapter.release_target_for_run("run", &obs.target_id),
            _ => update(|s| match fault {
                6 => s.ax_button_generation += 1,
                7 => s.ax_parent_escape = true,
                8 => s.ax_window_escape = true,
                9 => s.ax_button_name = "replacement".into(),
                10 => s.ax_focused = false,
                11 => s.ax_frontmost = false,
                12 => s.ax_focus_node = 2,
                13 => s.ax_focus_window_escape = true,
                14 => s.ax_subrole_secure = true,
                15 => s.ax_allowed = false,
                _ => s.screen_allowed = false,
            }),
        }
        assert!(adapter.act(&req).is_err(), "fault {fault}");
        inspect(|s| assert!(s.key_posts.is_empty(), "fault {fault}"));
        assert!(adapter.is_idle("run"));
        finish(adapter);
    }
}
#[test]
fn preview_and_background_coordinates_never_gain_keyboard_authority() {
    let (adapter, obs) = setup();
    let preview = adapter.observe(&obs.target_id).unwrap();
    assert!(adapter.act(&request(&preview, "enter")).is_err());
    let mut req = request(&obs, "enter");
    req.target = ActionTarget::Coord { x: 0.0, y: 0.0 };
    assert!(adapter.act(&req).is_err());
    inspect(|s| assert!(s.key_posts.is_empty()));
    finish(adapter);
}
#[test]
fn all_allocations_precede_input_and_failures_release_every_owned_object() {
    for number in 0..3 {
        let (adapter, obs) = setup();
        update(|s| {
            if number == 0 {
                s.fail_event_source = true
            } else {
                s.fail_event_number = Some(number)
            }
        });
        assert!(adapter.act(&request(&obs, "enter")).is_err());
        inspect(|s| assert!(s.key_posts.is_empty()));
        assert!(adapter.is_idle("run"));
        finish(adapter);
    }
}
#[test]
fn event_configuration_cannot_hide_revocation_or_changed_focus() {
    for fault in 0..7 {
        let (adapter, obs) = setup();
        let weak = Arc::downgrade(&adapter);
        let target = obs.target_id.clone();
        update(|s| {
            s.on_key_configure = Some(Box::new(move || {
                if fault == 0 {
                    weak.upgrade()
                        .unwrap()
                        .release_target_for_run("run", &target);
                } else {
                    update(|s| match fault {
                        1 => s.ax_allowed = false,
                        2 => s.screen_allowed = false,
                        3 => s.ax_focus_window_escape = true,
                        4 => s.ax_focused = false,
                        5 => s.ax_subrole_secure = true,
                        _ => s.birth.1 += 1,
                    });
                }
            }))
        });
        assert!(
            adapter.act(&request(&obs, "enter")).is_err(),
            "fault {fault}"
        );
        inspect(|s| assert!(s.key_posts.is_empty(), "fault {fault}"));
        finish(adapter);
    }
}
#[test]
fn held_physical_or_synthetic_keys_and_modifiers_are_not_borrowed_or_released() {
    for fault in 0..4 {
        let (adapter, obs) = setup();
        update(|s| match fault {
            0 => s.held_key = Some(36),
            1 => s.keyboard_flags = 1 << 20,
            2 => s.synthetic_key = Some(36),
            _ => s.synthetic_flags = 1 << 17,
        });
        assert!(
            adapter.act(&request(&obs, "enter")).is_err(),
            "fault {fault}"
        );
        inspect(|s| assert!(s.key_posts.is_empty(), "fault {fault}"));
        finish(adapter);
    }
}
#[test]
fn late_focus_reads_cannot_retire_the_snapshot_or_redirect_input() {
    for fault in 0..5 {
        let (adapter, obs) = setup();
        let weak = Arc::downgrade(&adapter);
        let target = obs.target_id.clone();
        update(|s| {
            s.ax_on_focus_read_at = Some(s.ax_focus_reads + 2);
            s.ax_on_focus_read = Some(Box::new(move || {
                if fault == 0 {
                    weak.upgrade()
                        .unwrap()
                        .release_target_for_run("run", &target);
                } else {
                    update(|s| match fault {
                        1 => s.ax_focus_window_escape = true,
                        2 => s.ax_subrole_secure = true,
                        3 => s.ax_button_generation += 1,
                        _ => s.x += 10.0,
                    });
                }
            }));
        });
        assert!(
            adapter.act(&request(&obs, "enter")).is_err(),
            "fault {fault}"
        );
        inspect(|s| assert!(s.key_posts.is_empty(), "fault {fault}"));
        finish(adapter);
    }
}
#[test]
fn unconfirmed_key_release_retains_occupancy_across_stop_and_blocks_other_runs() {
    for fault in 0..7 {
        let (adapter, obs) = setup();
        update(|s| {
            s.on_key_down = Some(Box::new(move || {
                update(|s| match fault {
                    0 => s.ax_allowed = false,
                    1 => s.screen_allowed = false,
                    2 => s.birth.1 += 1,
                    3 => s.ax_instance += 1,
                    4 => s.ax_frontmost = false,
                    5 => s.x += 1.0,
                    _ => s.held_key = Some(36),
                })
            }))
        });
        assert!(
            adapter.act(&request(&obs, "enter")).is_err(),
            "fault {fault}"
        );
        inspect(|s| assert_eq!(s.key_posts, vec![(7, 36, true, 0)], "fault {fault}"));
        assert!(!adapter.is_idle("run"));
        adapter.abort("run", 2).unwrap();
        assert!(!adapter.is_idle("other"));
        let mut next = request(&obs, "enter");
        next.run_id = "other".into();
        next.generation = 2;
        assert!(adapter.act(&next).is_err());
        finish(adapter);
    }
}
#[test]
fn retirement_after_down_permits_only_owned_release_and_reports_changed_authority() {
    let (adapter, obs) = setup();
    let weak = Arc::downgrade(&adapter);
    let target = obs.target_id.clone();
    update(|s| {
        s.on_key_down = Some(Box::new(move || {
            weak.upgrade()
                .unwrap()
                .release_target_for_run("run", &target)
        }))
    });
    assert!(adapter.act(&request(&obs, "enter")).is_err());
    inspect(|s| assert_eq!(s.key_posts, vec![(7, 36, true, 0), (7, 36, false, 0)]));
    assert!(adapter.is_idle("run"));
    finish(adapter);
}

#[test]
fn combined_session_state_containing_our_down_does_not_forbid_the_owned_up() {
    // Apple documents combinedSessionState as ALL posting sources. Treating a
    // synthetic down here as proof of an unrelated owner would deadlock our
    // own pair. Foreign synthetic senders racing admission need native proof;
    // the public table cannot identify the owner. HID-held input stays rejected.
    let (adapter, obs) = setup();
    update(|s| s.on_key_down = Some(Box::new(|| update(|s| s.synthetic_key = Some(36)))));
    assert!(adapter.act(&request(&obs, "enter")).unwrap().applied);
    inspect(|s| assert_eq!(s.key_posts, vec![(7, 36, true, 0), (7, 36, false, 0)]));
    assert!(adapter.is_idle("run"));
    finish(adapter);
}
#[test]
fn stop_during_down_keeps_occupancy_until_owned_release_and_blocks_a_new_run() {
    use std::cell::RefCell;
    use std::rc::Rc;
    let (adapter, obs) = setup();
    let mut competing = request(&obs, "enter");
    competing.run_id = "competing".into();
    let result = Rc::new(RefCell::new(None));
    let record = result.clone();
    let weak = Arc::downgrade(&adapter);
    update(|s| {
        s.on_key_down = Some(Box::new(move || {
            let adapter = weak.upgrade().unwrap();
            let stop = adapter.abort("run", 2);
            *record.borrow_mut() = Some((
                stop.is_ok(),
                adapter.is_idle("run"),
                adapter.act(&competing).is_err(),
            ));
        }))
    });
    assert!(adapter.act(&request(&obs, "enter")).is_err());
    assert_eq!(*result.borrow(), Some((true, false, true)));
    inspect(|s| assert_eq!(s.key_posts, vec![(7, 36, true, 0), (7, 36, false, 0)]));
    assert!(adapter.is_idle("run"));
    finish(adapter);
}
