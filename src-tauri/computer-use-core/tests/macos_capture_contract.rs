//! Executes the production Rust adapter against deterministic Quartz/ImageIO
//! FFI doubles. This is NOT macOS/native/permission/installed-App evidence.
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

fn target(adapter: &MacosAdapter) -> String {
    adapter.list_targets().unwrap().remove(0).target_id
}

fn request(observation: &Observation) -> DispatchRequest {
    DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "run".into(),
        action_id: "click".into(),
        generation: 1,
        target_id: observation.target_id.clone(),
        target_generation: 1,
        snapshot_id: observation.snapshot_id.clone(),
        geometry_revision: observation.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Coord { x: 600.0, y: 400.0 },
        parameters: serde_json::json!({}),
        scope: ActionScope::Directed,
    }
}

#[test]
fn production_capture_selects_only_one_window_and_encodes_through_imageio() {
    reset();
    let adapter = MacosAdapter::new();
    let observation = adapter.observe_for_run("run", &target(&adapter)).unwrap();
    inspect(|s| {
        assert_eq!(s.captures, vec![(8, 11, 9)]);
        assert!(s.null_capture_bounds);
        assert_eq!(s.encodes, 1);
        assert_eq!(s.live_owned, 0);
    });
    assert_eq!(
        (observation.image.width, observation.image.height),
        (1200, 800)
    );
    assert_eq!((observation.origin_x, observation.origin_y), (-1000, 80));
    assert_eq!(observation.scale, 2.0);
    assert_eq!(observation.nodes.len(), 1);
    assert_eq!(observation.nodes[0].role, "AXWindow");
    assert!(observation.nodes[0].node_ref.starts_with("mac-ax-"));
    assert_eq!(observation.nodes[0].actions, vec!["wait"]);
    assert!(observation.image.content_id.starts_with("sha256-"));
    assert!(adapter.capabilities().observe_ax);
    assert!(adapter.capabilities().set_value.semantic);
    assert!(!adapter.capabilities().set_value.coordinate);
    macos_adapter::run_native_selftest().unwrap(); // FFI doubles only.
}

#[test]
fn retina_click_uses_bound_pixels_not_raw_screen_pixels() {
    reset();
    let adapter = MacosAdapter::new();
    let observation = adapter.observe_for_run("run", &target(&adapter)).unwrap();
    let result = adapter.act(&request(&observation)).unwrap();
    assert!(result.applied);
    assert!(!result.verifiable);
    inspect(|s| {
        assert_eq!(
            s.posts,
            vec![(7, 1, -700.0, 280.0, 1), (7, 2, -700.0, 280.0, 1)]
        );
        assert_eq!(s.live_owned, 0);
    });
}

#[test]
fn preview_cannot_mint_or_retire_model_input_authority() {
    reset();
    let adapter = MacosAdapter::new();
    let preview = adapter
        .capture_for_run(
            "run",
            &target(&adapter),
            CaptureOptions {
                for_model: false,
                ..CaptureOptions::model(true)
            },
        )
        .unwrap();
    assert!(adapter.act(&request(&preview)).is_err());
    let model = adapter.observe_for_run("run", &target(&adapter)).unwrap();
    adapter.observe(&target(&adapter)).unwrap();
    adapter.act(&request(&model)).unwrap();
    inspect(|s| assert_eq!(s.posts.len(), 2));
}

#[test]
fn changed_or_failed_capture_does_not_keep_old_coordinate_authority() {
    reset();
    let adapter = MacosAdapter::new();
    let model = adapter.observe_for_run("run", &target(&adapter)).unwrap();
    update(|s| s.move_during_capture = true);
    assert!(adapter.observe_for_run("run", &target(&adapter)).is_err());
    update(|s| {
        s.move_during_capture = false;
        s.x = -1000.0;
    });
    assert!(adapter.act(&request(&model)).is_err());
    inspect(|s| {
        assert!(s.posts.is_empty());
        assert_eq!(s.live_owned, 0);
    });
}

#[test]
fn encoder_and_allocation_errors_release_all_owned_native_objects() {
    for fault in 0..4 {
        reset();
        let adapter = MacosAdapter::new();
        update(|s| match fault {
            0 => s.fail_capture = true,
            1 => s.fail_finalize = true,
            2 => s.fail_destination = true,
            _ => s.image_size = (usize::MAX, 800),
        });
        assert!(adapter.observe_for_run("run", &target(&adapter)).is_err());
        inspect(|s| assert_eq!(s.live_owned, 0, "fault {fault}"));
    }
}

#[test]
fn incorrect_target_permission_and_occlusion_never_post_input() {
    reset();
    let adapter = MacosAdapter::new();
    assert!(adapter.observe_for_run("run", "mac:8:11").is_err());
    assert!(adapter.observe_for_run("run", "mac:7:11:extra").is_err());
    update(|s| s.screen_allowed = false);
    assert!(adapter.observe_for_run("run", &target(&adapter)).is_err());
    update(|s| s.screen_allowed = true);
    let model = adapter.observe_for_run("run", &target(&adapter)).unwrap();
    let mut req = request(&model);
    req.target = ActionTarget::Element {
        element_ref: "root".into(),
    };
    assert!(adapter.act(&req).is_err());
    update(|s| s.occluded = true);
    assert!(adapter.act(&request(&model)).is_err());
    inspect(|s| {
        assert!(s.posts.is_empty());
        assert_eq!(s.live_owned, 0);
    });
}

#[test]
fn cancelled_request_posts_nothing_and_stop_retires_the_frame() {
    reset();
    let adapter = MacosAdapter::new();
    let model = adapter.observe_for_run("run", &target(&adapter)).unwrap();
    let req = request(&model);
    req.cancellation.cancel();
    assert!(adapter.act(&req).is_err());
    adapter.abort("run", 2).unwrap();
    assert!(adapter.act(&request(&model)).is_err());
    assert!(adapter.is_idle("run"));
    inspect(|s| assert!(s.posts.is_empty()));
}

#[test]
fn cancellation_between_down_and_up_releases_only_owned_input_and_stops_double_click() {
    reset();
    let adapter = MacosAdapter::new();
    let model = adapter.observe_for_run("run", &target(&adapter)).unwrap();
    let mut req = request(&model);
    req.parameters = serde_json::json!({"count": 2});
    update(|s| s.cancel_on_down = Some(req.cancellation.clone()));
    assert!(adapter.act(&req).is_err());
    inspect(|s| {
        assert_eq!(s.posts.len(), 2);
        assert_eq!(s.live_owned, 0);
    });
    assert!(adapter.is_idle("run"));
}

#[test]
fn uncertain_release_remains_occupied_after_abort_and_never_targets_replacement() {
    reset();
    let adapter = MacosAdapter::new();
    let model = adapter.observe_for_run("run", &target(&adapter)).unwrap();
    update(|s| s.occlude_on_down = true);
    assert!(adapter.act(&request(&model)).is_err());
    assert!(!adapter.is_idle("run"));
    adapter.abort("run", 2).unwrap();
    assert!(!adapter.is_idle("run"));
    inspect(|s| {
        assert_eq!(s.posts.len(), 1);
        assert_eq!(s.live_owned, 0);
    });
}

#[test]
fn process_birth_reuse_retires_discovered_target_and_existing_model_frame() {
    for change_microseconds in [false, true] {
        reset();
        let adapter = MacosAdapter::new();
        let old = adapter.list_targets().unwrap().remove(0);
        let model = adapter.observe_for_run("run", &old.target_id).unwrap();
        let mut req = request(&model);
        req.target_id = old.target_id.clone();
        update(|s| {
            if change_microseconds {
                s.birth.1 += 1;
            } else {
                s.birth.0 += 1;
            }
        });
        assert!(
            !adapter.target_alive(&old.target_id),
            "reused PID must not preserve authorization"
        );
        assert!(adapter.observe(&old.target_id).is_err());
        assert_eq!(adapter.current_geometry_revision_for(&old.target_id), 0);
        assert!(adapter.act(&req).is_err());
        let replacement = adapter.list_targets().unwrap().remove(0);
        assert_ne!(old.target_id, replacement.target_id);
        assert_ne!(old.lifecycle_stamp, replacement.lifecycle_stamp);
        assert!(adapter.target_alive(&replacement.target_id));
        let fresh = adapter
            .observe_for_run("run", &replacement.target_id)
            .unwrap();
        req.target_id = replacement.target_id;
        assert!(
            adapter.act(&req).is_err(),
            "old snapshot must not bind to replacement"
        );
        req.snapshot_id = fresh.snapshot_id;
        req.geometry_revision = fresh.geometry_revision;
        adapter.act(&req).unwrap();
        inspect(|s| {
            assert_eq!(s.posts.len(), 2);
            assert_eq!(s.live_owned, 0);
        });
    }
}

#[test]
fn process_replacement_during_capture_cannot_publish_pixels_or_keep_old_frame() {
    reset();
    let adapter = MacosAdapter::new();
    let id = adapter.list_targets().unwrap().remove(0).target_id;
    let model = adapter.observe_for_run("run", &id).unwrap();
    update(|s| s.rebirth_during_capture = true);
    assert!(adapter.observe_for_run("run", &id).is_err());
    update(|s| {
        s.rebirth_during_capture = false;
        s.birth.1 = 123_456;
    });
    let mut req = request(&model);
    req.target_id = id;
    assert!(adapter.act(&req).is_err());
    inspect(|s| {
        assert!(s.posts.is_empty());
        assert_eq!(s.live_owned, 0);
    });
}

#[test]
fn process_replacement_after_down_never_releases_into_new_process() {
    reset();
    let adapter = MacosAdapter::new();
    let id = adapter.list_targets().unwrap().remove(0).target_id;
    let model = adapter.observe_for_run("run", &id).unwrap();
    let mut req = request(&model);
    req.target_id = id;
    update(|s| s.rebirth_on_down = true);
    assert!(adapter.act(&req).is_err());
    adapter.abort("run", 2).unwrap();
    assert!(!adapter.is_idle("run"));
    inspect(|s| {
        assert_eq!(s.posts.len(), 1);
        assert_eq!(s.live_owned, 0);
    });
}

#[test]
fn unavailable_process_identity_never_becomes_a_discoverable_target() {
    reset();
    update(|s| s.process_info_bytes = 0);
    assert!(MacosAdapter::new().list_targets().unwrap().is_empty());
    inspect(|s| {
        assert!(s.posts.is_empty());
        assert!(s.captures.is_empty());
        assert_eq!(s.live_owned, 0);
    });
}

#[test]
fn incomplete_or_inconsistent_process_info_is_rejected_at_every_boundary() {
    for fault in 0..8 {
        reset();
        let adapter = MacosAdapter::new();
        let id = adapter.list_targets().unwrap().remove(0).target_id;
        let model = adapter.observe_for_run("run", &id).unwrap();
        update(|s| match fault {
            0 => s.process_info_bytes = -1,
            1 => s.process_info_bytes = 135,
            2 => s.process_info_bytes = 137,
            3 => s.process_info_pid = 8,
            4 => s.process_status = 5,
            5 => s.process_status = 0,
            6 => s.birth.0 = 0,
            _ => s.birth.1 = 1_000_000,
        });
        assert!(adapter.list_targets().unwrap().is_empty(), "fault {fault}");
        assert!(!adapter.target_alive(&id), "fault {fault}");
        assert!(
            adapter.observe_for_run("run", &id).is_err(),
            "fault {fault}"
        );
        assert!(adapter.act(&request(&model)).is_err(), "fault {fault}");
        inspect(|s| {
            assert_eq!(s.captures.len(), 1);
            assert!(s.posts.is_empty());
            assert_eq!(s.live_owned, 0);
        });
    }
}

#[test]
fn replacement_during_window_enumeration_is_not_a_live_target() {
    reset();
    let adapter = MacosAdapter::new();
    let id = adapter.list_targets().unwrap().remove(0).target_id;
    update(|s| s.rebirth_on_window_read = Some(s.window_reads + 1));
    assert!(!adapter.target_alive(&id));
    inspect(|s| {
        assert!(s.posts.is_empty());
        assert!(s.captures.is_empty());
        assert_eq!(s.live_owned, 0);
    });
}

#[test]
fn legacy_pid_only_ids_are_not_silently_upgraded() {
    reset();
    let adapter = MacosAdapter::new();
    for legacy in [
        "mac:7:11",
        "mac:7:11:1700000000",
        "mac:7:11:1700000000:123456",
    ] {
        assert!(!adapter.target_alive(legacy));
        assert!(adapter.observe_for_run("run", legacy).is_err());
    }
    inspect(|s| {
        assert!(s.posts.is_empty());
        assert!(s.captures.is_empty());
        assert_eq!(s.live_owned, 0);
    });
}

#[test]
fn process_identity_is_checked_after_event_preparation_before_first_post() {
    reset();
    let adapter = MacosAdapter::new();
    let id = adapter.list_targets().unwrap().remove(0).target_id;
    let model = adapter.observe_for_run("run", &id).unwrap();
    update(|s| s.rebirth_on_event_configure = true);
    assert!(adapter.act(&request(&model)).is_err());
    inspect(|s| {
        assert!(s.posts.is_empty());
        assert_eq!(s.live_owned, 0);
    });
    assert!(adapter.is_idle("run"));
}

#[test]
fn liveness_preserves_the_same_window_eligibility_as_discovery() {
    for fault in 0..3 {
        reset();
        let adapter = MacosAdapter::new();
        let id = adapter.list_targets().unwrap().remove(0).target_id;
        let model = adapter.observe_for_run("run", &id).unwrap();
        update(|s| match fault {
            0 => s.window_layer = 1,
            1 => s.window_title.clear(),
            _ => s.window_owner = "Window Server".into(),
        });
        assert!(adapter.list_targets().unwrap().is_empty());
        assert!(!adapter.target_alive(&id));
        assert!(adapter.observe(&id).is_err());
        assert!(adapter.act(&request(&model)).is_err());
        inspect(|s| {
            assert!(s.posts.is_empty());
            assert_eq!(s.live_owned, 0);
        });
    }
}

#[test]
fn repeated_discovery_keeps_one_owned_witness_and_adapter_drop_releases_it() {
    reset();
    let adapter = MacosAdapter::new();
    let id = target(&adapter);
    for _ in 0..8 {
        assert_eq!(target(&adapter), id);
        assert!(adapter.target_alive(&id));
        inspect(|s| {
            assert_eq!(s.ax_owned, 1);
            assert_eq!(s.live_owned, 0);
        });
    }
    drop(adapter);
    inspect(|s| assert_eq!(s.ax_owned, 0));
}

#[test]
fn same_process_window_replacement_cannot_inherit_identity_or_model_authority() {
    reset();
    let adapter = MacosAdapter::new();
    let old = adapter.list_targets().unwrap().remove(0);
    let model = adapter.observe_for_run("run", &old.target_id).unwrap();
    update(|s| s.ax_instance += 1); // PID, birth, WID, title and bounds unchanged.
    assert!(!adapter.target_alive(&old.target_id));
    assert!(adapter.observe(&old.target_id).is_err());
    assert!(adapter.act(&request(&model)).is_err());
    assert_eq!(adapter.current_geometry_revision_for(&old.target_id), 0);
    let new = adapter.list_targets().unwrap().remove(0);
    assert_ne!(new.target_id, old.target_id);
    assert_ne!(new.lifecycle_stamp, old.lifecycle_stamp);
    assert!(adapter.target_alive(&new.target_id));
    let mut old_snapshot = request(&model);
    old_snapshot.target_id = new.target_id.clone();
    assert!(adapter.act(&old_snapshot).is_err());
    let fresh = adapter.observe_for_run("run", &new.target_id).unwrap();
    assert!(adapter.act(&old_snapshot).is_err());
    adapter.act(&request(&fresh)).unwrap();
    inspect(|s| {
        assert_eq!(s.posts.len(), 2);
        assert_eq!(s.ax_owned, 2); // Window binding plus retained model AX root.
        assert_eq!(s.live_owned, 0);
    });
    drop(adapter);
    inspect(|s| assert_eq!(s.ax_owned, 0));
}

#[test]
fn replacement_discovery_itself_retires_old_nonce_without_a_liveness_probe() {
    reset();
    let adapter = MacosAdapter::new();
    let old = target(&adapter);
    update(|s| s.ax_instance += 1);
    let new = target(&adapter);
    assert_ne!(old, new);
    assert!(!adapter.target_alive(&old));
    update(|s| s.ax_instance = 1); // A retired nonce can never revive.
    assert!(!adapter.target_alive(&old));
    inspect(|s| assert_eq!(s.ax_owned, 1));
    drop(adapter);
    inspect(|s| assert_eq!(s.ax_owned, 0));
}

#[test]
fn fabricated_nonce_and_another_adapter_cannot_resolve_a_native_window() {
    use grok_computer_use_core::quartz_frame::WindowInstance;
    reset();
    let adapter = MacosAdapter::new();
    let id = target(&adapter);
    let fake = WindowInstance::fresh(WindowInstance::parse(&id).unwrap().window).target_id();
    assert!(!adapter.target_alive(&fake));
    assert!(adapter.observe_for_run("run", &fake).is_err());
    let other = MacosAdapter::new();
    assert!(!other.target_alive(&id));
    assert!(other.observe_for_run("run", &id).is_err());
    inspect(|s| {
        assert!(s.captures.is_empty());
        assert!(s.posts.is_empty());
        assert_eq!(s.live_owned, 0);
    });
    drop(adapter);
    drop(other);
    inspect(|s| assert_eq!(s.ax_owned, 0));
}

#[test]
fn native_window_replacement_during_capture_retires_the_nonce_and_old_frame() {
    reset();
    let adapter = MacosAdapter::new();
    let id = target(&adapter);
    let model = adapter.observe_for_run("run", &id).unwrap();
    update(|s| s.ax_replaced_during_capture = true);
    assert!(adapter.observe_for_run("run", &id).is_err());
    update(|s| {
        s.ax_replaced_during_capture = false;
        s.ax_instance = 1;
    });
    assert!(!adapter.target_alive(&id));
    assert!(adapter.act(&request(&model)).is_err());
    inspect(|s| {
        assert!(s.posts.is_empty());
        assert_eq!(s.ax_owned, 0);
        assert_eq!(s.live_owned, 0);
    });
}

#[test]
fn native_window_replacement_after_down_never_receives_an_owned_release() {
    reset();
    let adapter = MacosAdapter::new();
    let id = target(&adapter);
    let model = adapter.observe_for_run("run", &id).unwrap();
    update(|s| s.ax_replaced_on_down = true);
    assert!(adapter.act(&request(&model)).is_err());
    assert!(!adapter.is_idle("run"));
    adapter.abort("run", 2).unwrap();
    assert!(!adapter.is_idle("run"));
    inspect(|s| {
        assert_eq!(s.posts.len(), 1);
        assert_eq!(s.ax_owned, 0);
        assert_eq!(s.live_owned, 0);
    });
}

#[test]
fn native_window_identity_is_rechecked_after_event_preparation() {
    reset();
    let adapter = MacosAdapter::new();
    let id = target(&adapter);
    let model = adapter.observe_for_run("run", &id).unwrap();
    update(|s| s.ax_replaced_on_event_configure = true);
    assert!(adapter.act(&request(&model)).is_err());
    assert!(adapter.is_idle("run"));
    inspect(|s| {
        assert!(s.posts.is_empty());
        assert_eq!(s.ax_owned, 0);
        assert_eq!(s.live_owned, 0);
    });
}

fn ax_fault(fault: usize) {
    update(|s| match fault {
        0 => s.ax_allowed = false,
        1 => s.ax_fail_timeout = true,
        2 => s.ax_wrong_pid = true,
        3 => s.ax_wrong_type = true,
        4 => s.ax_wrong_bounds = true,
        5 => s.ax_fail_attribute = true,
        _ => s.occluded = true,
    });
}

#[test]
fn unprovable_ax_window_identity_never_enters_the_target_picker() {
    for fault in 0..7 {
        reset();
        ax_fault(fault);
        let adapter = MacosAdapter::new();
        assert!(adapter.list_targets().unwrap().is_empty(), "fault {fault}");
        if fault == 0 {
            assert!(!adapter.capabilities().observe_screenshot);
            assert!(!adapter.capabilities().coordinate_click);
        }
        drop(adapter);
        inspect(|s| {
            assert_eq!(s.ax_owned, 0, "fault {fault}");
            assert_eq!(s.live_owned, 0, "fault {fault}");
            assert!(s.captures.is_empty());
            assert!(s.posts.is_empty());
        });
    }
}

#[test]
fn ax_failures_after_authorization_do_not_capture_or_dispatch_and_do_not_leak() {
    for fault in 0..7 {
        reset();
        let adapter = MacosAdapter::new();
        let id = target(&adapter);
        let model = adapter.observe_for_run("run", &id).unwrap();
        ax_fault(fault);
        assert!(!adapter.target_alive(&id), "fault {fault}");
        assert!(
            adapter.observe_for_run("run", &id).is_err(),
            "fault {fault}"
        );
        assert!(adapter.act(&request(&model)).is_err(), "fault {fault}");
        inspect(|s| {
            assert_eq!(s.captures.len(), 1);
            assert!(s.posts.is_empty());
            assert_eq!(s.ax_owned, 1, "fault {fault}");
            assert_eq!(s.live_owned, 0, "fault {fault}");
        });
        drop(adapter);
        inspect(|s| assert_eq!(s.ax_owned, 0, "fault {fault}"));
    }
}

#[test]
fn cancellation_during_ax_resolution_retires_the_model_ticket_and_releases_copies() {
    reset();
    let adapter = MacosAdapter::new();
    let id = target(&adapter);
    let model = adapter.observe_for_run("run", &id).unwrap();
    let options = CaptureOptions::model(true);
    update(|s| s.ax_cancel_on_hit = Some(options.cancellation.clone()));
    assert!(adapter.capture_for_run("run", &id, options).is_err());
    update(|s| s.ax_cancel_on_hit = None);
    assert!(adapter.act(&request(&model)).is_err());
    inspect(|s| {
        assert_eq!(s.captures.len(), 1);
        assert!(s.posts.is_empty());
        assert_eq!(s.ax_owned, 1);
        assert_eq!(s.live_owned, 0);
    });
    drop(adapter);
    inspect(|s| assert_eq!(s.ax_owned, 0));
}

#[test]
fn moving_the_same_ax_window_during_event_preparation_cannot_use_old_coordinates() {
    reset();
    let adapter = MacosAdapter::new();
    let id = target(&adapter);
    let model = adapter.observe_for_run("run", &id).unwrap();
    update(|s| s.move_on_event_configure = true);
    assert!(adapter.act(&request(&model)).is_err());
    inspect(|s| {
        assert!(
            s.posts.is_empty(),
            "geometry must be rechecked, not only AX identity"
        );
        assert_eq!(s.live_owned, 0);
    });
    assert!(adapter.is_idle("run"));
}

#[test]
fn observing_a_disappeared_window_permanently_retires_its_nonce() {
    reset();
    let adapter = MacosAdapter::new();
    let old = target(&adapter);
    let model = adapter.observe_for_run("run", &old).unwrap();
    update(|s| s.window_present = false);
    assert!(!adapter.target_alive(&old));
    inspect(|s| assert_eq!(s.ax_owned, 0, "dead identity must be released"));
    update(|s| s.window_present = true);
    assert!(
        !adapter.target_alive(&old),
        "an observed dead target cannot revive"
    );
    assert!(adapter.act(&request(&model)).is_err());
    assert_ne!(target(&adapter), old);
    drop(adapter);
    inspect(|s| {
        assert_eq!(s.ax_owned, 0);
        assert_eq!(s.live_owned, 0);
        assert!(s.posts.is_empty());
    });
}

#[test]
fn snapshot_eviction_retires_coordinate_and_ax_authority_together() {
    reset();
    let adapter = MacosAdapter::new();
    let id = target(&adapter);
    let model = adapter.observe_for_run("run", &id).unwrap();
    for i in 0..8 {
        adapter.observe_for_run(&format!("other-{i}"), &id).unwrap();
    }
    assert!(adapter.act(&request(&model)).is_err());
    drop(adapter);
    inspect(|s| {
        assert!(s.posts.is_empty());
        assert_eq!(s.ax_owned, 0);
        assert_eq!(s.live_owned, 0);
    });
}

#[test]
fn release_during_event_preparation_cannot_post_using_a_loaded_frame() {
    reset();
    let adapter = std::sync::Arc::new(MacosAdapter::new());
    let id = target(&adapter);
    let model = adapter.observe_for_run("run", &id).unwrap();
    let releasing = adapter.clone();
    update(|s| {
        s.on_event_configure = Some(Box::new(move || {
            releasing.release_target_for_run("run", &id);
        }))
    });
    assert!(adapter.act(&request(&model)).is_err());
    drop(adapter);
    inspect(|s| {
        assert!(s.posts.is_empty());
        assert_eq!(s.ax_owned, 0);
        assert_eq!(s.live_owned, 0);
    });
}

#[test]
fn release_after_down_allows_owned_up_but_not_a_second_down() {
    reset();
    let adapter = std::sync::Arc::new(MacosAdapter::new());
    let id = target(&adapter);
    let model = adapter.observe_for_run("run", &id).unwrap();
    let releasing = adapter.clone();
    update(|s| {
        s.on_mouse_down = Some(Box::new(move || {
            releasing.release_target_for_run("run", &id);
        }))
    });
    let mut req = request(&model);
    req.parameters = serde_json::json!({"count":2});
    assert!(adapter.act(&req).is_err());
    assert!(adapter.is_idle("run"));
    drop(adapter);
    inspect(|s| {
        assert_eq!(
            s.posts,
            vec![(7, 1, -700.0, 280.0, 1), (7, 2, -700.0, 280.0, 1)]
        );
        assert_eq!(s.ax_owned, 0);
        assert_eq!(s.live_owned, 0);
    });
}
