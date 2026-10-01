//! Production AX append path linked to deterministic doubles, NOT macOS proof.
#![cfg(not(target_os = "macos"))]
pub use grok_computer_use_core::{adapter, protocol};
#[path = "../../src/computer_use/macos_adapter.rs"]
mod macos_adapter;
#[path = "support/quartz_ffi.rs"]
mod quartz_ffi;

use adapter::{ActionScope, ComputerUseAdapter, DispatchRequest};
use macos_adapter::MacosAdapter;
use protocol::{ActionKind, ActionTarget, Observation, TEXT_MAX_CHARS};
use quartz_ffi::{inspect, reset, update};

fn setup() -> (MacosAdapter, Observation) {
    reset();
    update(|s| {
        s.ax_tree_enabled = true;
        s.ax_text_role = Some("AXTextArea");
        s.ax_focused = true;
        s.ax_press_available = false;
    });
    let adapter = MacosAdapter::new();
    let id = adapter.list_targets().unwrap().remove(0).target_id;
    let obs = adapter.observe_for_run("run", &id).unwrap();
    (adapter, obs)
}
fn request(obs: &Observation, text: &str) -> DispatchRequest {
    DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: obs.run_id.clone(),
        action_id: "append".into(),
        generation: 1,
        target_id: obs.target_id.clone(),
        target_generation: 1,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action: ActionKind::TypeText,
        target: ActionTarget::Element {
            element_ref: obs.nodes[1].node_ref.clone(),
        },
        parameters: serde_json::json!({"text": text}),
        scope: ActionScope::Directed,
    }
}
fn finish(adapter: MacosAdapter) {
    drop(adapter);
    inspect(|s| {
        assert!(s.ax_values.is_empty(), "append must not replace AXValue");
        assert!(s.key_posts.is_empty() && s.posts.is_empty() && s.ax_presses.is_empty());
        assert_eq!(s.ax_secure_reads, 0);
        assert_eq!(s.live_owned, 0);
        assert_eq!(s.ax_owned, 0);
    });
}
#[test]
fn focused_text_advertises_append_without_claiming_ime() {
    let (adapter, obs) = setup();
    macos_adapter::run_native_selftest().unwrap(); // Doubles, not native proof.
    assert!(obs.nodes[1].actions.iter().any(|a| a == "type_text"));
    assert!(adapter.capabilities().type_text.semantic);
    assert!(adapter.capabilities().type_text.coordinate);
    assert!(!adapter.capabilities().chinese_ime);
    finish(adapter);
}
#[test]
fn empty_append_is_a_validated_noop_not_a_value_replacement() {
    let (adapter, obs) = setup();
    let result = adapter.act(&request(&obs, "")).unwrap();
    assert!(!result.applied);
    assert!(!result.verifiable && !result.postcondition_ok);
    inspect(|s| {
        assert!(s.ax_append_writes.is_empty());
        assert_eq!(s.ax_selection, (1, 2));
        assert_eq!(s.ax_text, "原有😀text");
    });
    finish(adapter);
}

#[test]
fn append_preserves_prefix_replaces_no_selected_text_and_uses_native_unicode_endpoint() {
    for suffix in [
        "中文😀 e\u{301}\n第二行".into(),
        "🚀".repeat(TEXT_MAX_CHARS),
    ] {
        let (adapter, obs) = setup();
        let result = adapter.act(&request(&obs, &suffix)).unwrap();
        assert!(result.applied && !result.verifiable && !result.postcondition_ok);
        assert!(!result.detail.contains(&suffix));
        inspect(|s| {
            assert_eq!(s.ax_text, format!("原有😀text{suffix}"));
            assert_eq!(
                s.ax_selection,
                (s.ax_text.encode_utf16().count() as isize, 0)
            );
            assert_eq!(s.ax_append_writes, vec!["range", "text"]);
        });
        assert!(adapter.is_idle("run"));
        finish(adapter);
    }
}

#[test]
fn consecutive_appends_and_initially_empty_control_use_current_not_snapshot_length() {
    let (adapter, obs) = setup();
    update(|s| {
        s.ax_text.clear();
        s.ax_selection = (0, 0);
    });
    adapter.act(&request(&obs, "first😀")).unwrap();
    adapter.act(&request(&obs, "第二段")).unwrap();
    inspect(|s| {
        assert_eq!(s.ax_text, "first😀第二段");
        assert_eq!(s.ax_append_writes, vec!["range", "text", "range", "text"]);
    });
    finish(adapter);
}

#[test]
fn invalid_clipboard_and_oversize_parameters_never_move_selection() {
    for params in [
        serde_json::json!({"text":"x", "via":"clipboard"}),
        serde_json::json!({"text":"x", "force":true}),
        serde_json::json!({"text":17}),
        serde_json::json!({"text":"\0"}),
        serde_json::json!({"text":"x".repeat(TEXT_MAX_CHARS+1)}),
        serde_json::json!({}),
        serde_json::json!([]),
    ] {
        let (adapter, obs) = setup();
        let mut req = request(&obs, "suffix");
        req.parameters = params;
        assert!(adapter.act(&req).is_err());
        inspect(|s| assert!(s.ax_append_writes.is_empty()));
        assert!(adapter.is_idle("run"));
        finish(adapter);
    }
}

#[test]
fn unsupported_selection_attributes_and_unfocused_text_do_not_advertise_append() {
    for fault in 0..7 {
        let (adapter, obs) = setup();
        update(|s| match fault {
            0 => s.ax_append_settable.0 = 0,
            1 => s.ax_append_settable.1 = 0,
            2 => s.ax_focused = false,
            3 => s.ax_text_attributes_missing = true,
            4 => s.ax_button_enabled = false,
            5 => s.ax_text_role = Some("AXButton"),
            _ => s.ax_frontmost = false,
        });
        let current = adapter.observe_for_run("run", &obs.target_id).unwrap();
        assert!(!current.nodes[1].actions.iter().any(|a| a == "type_text"));
        assert!(adapter.act(&request(&current, "x")).is_err());
        inspect(|s| assert!(s.ax_append_writes.is_empty()));
        finish(adapter);
    }
}

#[test]
fn malformed_native_count_or_range_never_reaches_selection_write() {
    for fault in 0..12 {
        let (adapter, obs) = setup();
        update(|s| match fault {
            0 => s.ax_count_wrong_type = true,
            1 => s.ax_count_override = Some(-1.0),
            2 => s.ax_count_override = Some(2.5),
            3 => s.ax_count_override = Some(f64::NAN),
            4 => s.ax_count_override = Some(f64::INFINITY),
            5 => s.ax_count_override = Some(isize::MAX as f64),
            6 => s.ax_range_wrong_type = true,
            7 => s.ax_selection = (-1, 0),
            8 => s.ax_selection = (1, -1),
            9 => s.ax_selection = (isize::MAX, 1),
            10 => s.ax_selection = (100, 0),
            _ => s.ax_append_settable.1 = 2,
        });
        assert!(adapter.act(&request(&obs, "x")).is_err(), "fault {fault}");
        inspect(|s| assert!(s.ax_append_writes.is_empty()));
        finish(adapter);
    }
}

#[test]
fn stale_cross_run_retired_preview_background_and_desktop_requests_cannot_append() {
    for fault in 0..9 {
        let (adapter, obs) = setup();
        let mut req = request(&obs, "x");
        match fault {
            0 => req.run_id = "other".into(),
            1 => req.snapshot_id = "old".into(),
            2 => req.geometry_revision += 1,
            3 => req.scope = ActionScope::Desktop,
            4 => req.cancellation.cancel(),
            5 => adapter.release_target_for_run("run", &obs.target_id),
            6 => req = request(&adapter.observe(&obs.target_id).unwrap(), "x"),
            7 => req.target = ActionTarget::Coord { x: 0.0, y: 0.0 },
            _ => {
                adapter.observe_for_run("run", &obs.target_id).unwrap();
            }
        }
        assert!(adapter.act(&req).is_err(), "fault {fault}");
        inspect(|s| assert!(s.ax_append_writes.is_empty()));
        finish(adapter);
    }
}

#[test]
fn revoked_permissions_replaced_or_protected_controls_never_write() {
    for fault in 0..13 {
        let (adapter, obs) = setup();
        update(|s| match fault {
            0 => s.ax_allowed = false,
            1 => s.screen_allowed = false,
            2 => s.ax_button_generation += 1,
            3 => s.ax_subrole_secure = true,
            4 => s.ax_parent_escape = true,
            5 => s.ax_window_escape = true,
            6 => s.ax_focused = false,
            7 => s.ax_frontmost = false,
            8 => s.ax_focus_node = 2,
            9 => s.ax_focus_window_escape = true,
            10 => s.birth.1 += 1,
            11 => s.x += 4.0,
            _ => s.ax_button_hidden = true,
        });
        assert!(adapter.act(&request(&obs, "x")).is_err(), "fault {fault}");
        inspect(|s| assert!(s.ax_append_writes.is_empty()));
        finish(adapter);
    }
}

#[test]
fn allocations_fail_before_native_selection_and_drop_all_retained_objects() {
    for range in [false, true] {
        let (adapter, obs) = setup();
        update(|s| {
            if range {
                s.ax_fail_range_create = true;
            } else {
                s.cf_watch_string = Some("suffix".into());
                s.cf_fail_string = true;
            }
        });
        assert!(adapter.act(&request(&obs, "suffix")).is_err());
        inspect(|s| assert!(s.ax_append_writes.is_empty()));
        finish(adapter);
    }
}

#[test]
fn selection_allocation_callbacks_cannot_hide_revocation_or_endpoint_changes() {
    for fault in 0..7 {
        let (adapter, obs) = setup();
        let req = request(&obs, "x");
        let cancel = req.cancellation.clone();
        update(|s| {
            s.ax_on_range_create = Some(Box::new(move || {
                update(|s| match fault {
                    0 => s.ax_text.push('a'),
                    1 => s.ax_append_settable.1 = 0,
                    2 => s.ax_focused = false,
                    3 => s.ax_subrole_secure = true,
                    4 => s.ax_allowed = false,
                    5 => s.birth.1 += 1,
                    _ => cancel.cancel(),
                });
            }))
        });
        assert!(adapter.act(&req).is_err(), "fault {fault}");
        inspect(|s| assert!(s.ax_append_writes.is_empty()));
        finish(adapter);
    }
}

#[test]
fn ignored_or_changed_selection_never_replaces_existing_text() {
    for fault in 0..5 {
        let (adapter, obs) = setup();
        update(|s| {
            if fault == 0 {
                s.ax_selection_ignored = true;
            } else {
                s.ax_on_range_set = Some(Box::new(move || {
                    update(|s| match fault {
                        1 => s.ax_selection = (0, 0),
                        2 => s.ax_selection = (0, 2),
                        3 => s.ax_text.push_str("user edit"),
                        _ => s.ax_selection = (isize::MAX, 1),
                    })
                }));
            }
        });
        assert!(
            adapter.act(&request(&obs, "suffix")).is_err(),
            "fault {fault}"
        );
        inspect(|s| {
            assert_eq!(s.ax_append_writes, vec!["range"]);
            assert_eq!(
                s.ax_text,
                if fault == 3 {
                    "原有😀textuser edit"
                } else {
                    "原有😀text"
                }
            );
        });
        assert!(adapter.is_idle("run"));
        finish(adapter);
    }
}

#[test]
fn cancellation_or_loss_of_focus_after_selection_never_sends_text() {
    for fault in 0..6 {
        let (adapter, obs) = setup();
        let req = request(&obs, "suffix");
        let cancel = req.cancellation.clone();
        update(|s| {
            s.ax_on_range_set = Some(Box::new(move || {
                update(|s| match fault {
                    0 => cancel.cancel(),
                    1 => s.ax_focused = false,
                    2 => s.ax_allowed = false,
                    3 => s.ax_subrole_secure = true,
                    4 => s.ax_focus_window_escape = true,
                    _ => s.ax_button_generation += 1,
                })
            }))
        });
        assert!(adapter.act(&req).is_err(), "fault {fault}");
        inspect(|s| {
            assert_eq!(s.ax_append_writes, vec!["range"]);
            assert_eq!(s.ax_text, "原有😀text");
        });
        finish(adapter);
    }
}

#[test]
fn ambiguous_selection_and_text_native_failures_hold_occupancy_and_forbid_replay() {
    for text_failure in [false, true] {
        let (adapter, obs) = setup();
        update(|s| {
            if text_failure {
                s.ax_append_status.1 = -25204
            } else {
                s.ax_append_status.0 = -25204
            }
        });
        let error = adapter.act(&request(&obs, "suffix")).unwrap_err();
        assert!(error.contains("unknown") && error.contains("replay forbidden"));
        assert!(!adapter.is_idle("run"));
        let writes = inspect(|s| s.ax_append_writes.len());
        assert_eq!(writes, if text_failure { 2 } else { 1 });
        assert!(adapter.act(&request(&obs, "suffix")).is_err());
        inspect(|s| assert_eq!(s.ax_append_writes.len(), writes));
        finish(adapter);
    }
}

#[test]
fn cancellation_after_text_does_not_report_success_or_automatically_replay() {
    let (adapter, obs) = setup();
    let req = request(&obs, "suffix");
    let cancel = req.cancellation.clone();
    update(|s| s.ax_on_text_set = Some(Box::new(move || cancel.cancel())));
    assert!(adapter.act(&req).is_err());
    inspect(|s| {
        assert_eq!(s.ax_text, "原有😀textsuffix");
        assert_eq!(s.ax_append_writes, vec!["range", "text"]);
    });
    finish(adapter);
}

#[test]
fn stop_during_native_selection_invalidates_retained_observation_without_deadlock() {
    let (adapter, obs) = setup();
    let adapter = std::sync::Arc::new(adapter);
    let weak = std::sync::Arc::downgrade(&adapter);
    let id = obs.target_id.clone();
    update(|s| {
        s.ax_on_range_set = Some(Box::new(move || {
            if let Some(adapter) = weak.upgrade() {
                adapter.release_target_for_run("run", &id);
            }
        }))
    });
    assert!(adapter.act(&request(&obs, "suffix")).is_err());
    inspect(|s| assert_eq!(s.ax_append_writes, vec!["range"]));
    finish(std::sync::Arc::try_unwrap(adapter).ok().unwrap());
}

#[test]
fn selection_change_during_final_focus_validation_must_not_replace_old_text() {
    let (adapter, obs) = setup();
    update(|s| {
        s.ax_on_range_set = Some(Box::new(|| {
            update(|s| {
                // After selection: available() entry, available() exit, final before().
                s.ax_on_focus_read_at = Some(s.ax_focus_reads + 3);
                s.ax_on_focus_read = Some(Box::new(|| update(|s| s.ax_selection = (0, 2))));
            })
        }))
    });
    assert!(adapter.act(&request(&obs, "suffix")).is_err());
    inspect(|s| {
        assert_eq!(s.ax_text, "原有😀text");
        assert_eq!(s.ax_append_writes, vec!["range"]);
    });
    finish(adapter);
}

#[test]
fn snapshot_retirement_during_final_selection_read_forbids_text() {
    let (adapter, obs) = setup();
    let adapter = std::sync::Arc::new(adapter);
    let weak = std::sync::Arc::downgrade(&adapter);
    let id = obs.target_id.clone();
    update(|s| {
        s.ax_on_range_set = Some(Box::new(move || {
            update(|s| {
                s.ax_on_range_read = Some(Box::new(move || {
                    if let Some(adapter) = weak.upgrade() {
                        adapter.release_target_for_run("run", &id);
                    }
                }));
            })
        }))
    });
    assert!(adapter.act(&request(&obs, "suffix")).is_err());
    inspect(|s| assert_eq!(s.ax_append_writes, vec!["range"]));
    finish(std::sync::Arc::try_unwrap(adapter).ok().unwrap());
}

#[test]
fn protection_change_inside_writability_query_prevents_further_text_metadata_reads() {
    let (adapter, obs) = setup();
    update(|s| s.ax_on_settable = Some(Box::new(|| update(|s| s.ax_subrole_secure = true))));
    assert!(adapter.act(&request(&obs, "suffix")).is_err());
    inspect(|s| assert!(s.ax_append_writes.is_empty()));
    finish(adapter);
}

#[test]
fn protection_change_during_count_read_never_reads_selection_or_writes_text() {
    let (adapter, obs) = setup();
    update(|s| s.ax_on_count_read = Some(Box::new(|| update(|s| s.ax_subrole_secure = true))));
    assert!(adapter.act(&request(&obs, "suffix")).is_err());
    inspect(|s| assert!(s.ax_append_writes.is_empty()));
    finish(adapter);
}
