//! Verifies the verifier and its owned pipes. NOT macOS native proof.
#[path = "support/macos_native_gates.rs"]
mod macos_native_gates;
#[path = "support/macos_native_pipe.rs"]
mod macos_native_pipe;
#[path = "support/macos_native_pointer.rs"]
mod macos_native_pointer;
#[path = "support/macos_native_pointer_gates.rs"]
mod macos_native_pointer_gates;
#[path = "support/macos_native_protocol.rs"]
mod macos_native_protocol;
use grok_computer_use_core::quartz_frame::{WindowInstance, WindowKey};
use grok_computer_use_core::{
    adapter::TargetInfo,
    protocol::{Observation, ObservationNode},
};
use macos_native_gates::{center, owned_target, Report, GATES};
use macos_native_pipe::Fixture;
use macos_native_protocol::{key_pair, parse, read_line, unchanged, KeyEvent, State, MAX_REPLY};
use serde_json::json;
use std::io::Cursor;
use std::path::Path;

#[test]
fn evidence_failure_never_skips_owned_cleanup_or_hides_original_failure() {
    for evidence_fails in [false, true] {
        for cleanup_fails in [false, true] {
            let calls = std::cell::Cell::new(0);
            let error = macos_native_gates::failure_with_cleanup(
                "native postcondition failed",
                if evidence_fails {
                    Err("disk full".into())
                } else {
                    Ok(())
                },
                || {
                    calls.set(calls.get() + 1);
                    if cleanup_fails {
                        Err("owned child not reaped".into())
                    } else {
                        Ok(())
                    }
                },
            );
            assert_eq!(calls.get(), 1);
            assert!(error.starts_with("native postcondition failed"));
            assert_eq!(
                error.contains("failure evidence: disk full"),
                evidence_fails
            );
            assert_eq!(
                error.contains("cleanup: owned child not reaped"),
                cleanup_fails
            );
            if !evidence_fails && !cleanup_fails {
                assert_eq!(error, "native postcondition failed");
            }
        }
    }
}

fn baseline() -> State {
    State {
        version: macos_native_protocol::VERSION,
        nonce: "fixture-nonce".into(),
        id: 0,
        pid: 7,
        architecture: std::env::consts::ARCH.into(),
        translated: false,
        window_id: 8,
        title: "GrokCuOwned-fixture-nonce".into(),
        active: true,
        focused: true,
        text: "原有😀text".into(),
        selection: [0, 0],
        keys: vec![],
        clicks: 0,
        pointer: macos_native_pointer::PointerState {
            width: 660.0,
            height: 180.0,
            scroll_offset: 500.0,
            box_rect: [290.0, 70.0, 80.0, 40.0],
            dragging: false,
            overflow: false,
            events: vec![],
        },
        error: None,
    }
}
fn bytes(state: &State) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(state).unwrap();
    bytes.push(b'\n');
    bytes
}
fn decode(state: &State) -> Result<State, String> {
    parse(&bytes(state), "fixture-nonce", 7, 0, Some(8))
}

fn pointer_event(
    kind: macos_native_pointer::PointerKind,
    x: f64,
    y: f64,
) -> macos_native_pointer::PointerEvent {
    macos_native_pointer::PointerEvent {
        kind,
        button: 0,
        click_count: if matches!(
            kind,
            macos_native_pointer::PointerKind::Wheel | macos_native_pointer::PointerKind::Drag
        ) {
            0
        } else {
            1
        },
        x,
        y,
        delta_y: 0.0,
        flags: 0,
    }
}

fn clicked(
    before: &macos_native_pointer::PointerState,
    button: u8,
    count: u32,
) -> macos_native_pointer::PointerState {
    use macos_native_pointer::PointerKind::{Down, Up};
    let mut after = before.clone();
    for n in 1..=count {
        for kind in [Down, Up] {
            let mut event = pointer_event(kind, 330.0, 90.0);
            event.button = button;
            event.click_count = n;
            after.events.push(event);
        }
    }
    after
}

fn dragged(
    before: &macos_native_pointer::PointerState,
    reverse: bool,
) -> macos_native_pointer::PointerState {
    use macos_native_pointer::PointerKind::{Down, Drag, Up};
    let mut after = before.clone();
    let (start, end) = if reverse {
        ([475.2, 129.6], [330.0, 90.0])
    } else {
        ([330.0, 90.0], [475.2, 129.6])
    };
    after.events.extend([
        pointer_event(Down, start[0], start[1]),
        pointer_event(Drag, 402.6, 109.8),
        pointer_event(Drag, end[0], end[1]),
        pointer_event(Up, end[0], end[1]),
    ]);
    after.box_rect = if reverse {
        [290.0, 70.0, 80.0, 40.0]
    } else {
        [435.2, 109.6, 80.0, 40.0]
    };
    after
}

#[test]
fn pointer_protocol_requires_new_bounded_nontruncated_cocoa_evidence() {
    let good = baseline();
    assert!(good.pointer.fresh());
    for fault in 0..10 {
        let mut bad = serde_json::to_value(&good).unwrap();
        match fault {
            0 => {
                bad.as_object_mut().unwrap().remove("pointer");
            }
            1 => bad["version"] = json!(1),
            2 => bad["pointer"]["width"] = json!(-1),
            3 => bad["pointer"]["height"] = json!(5000),
            4 => bad["pointer"]["overflow"] = json!(true),
            5 => bad["pointer"]["boxRect"] = json!([650, 70, 80, 40]),
            6 => bad["pointer"]["scrollOffset"] = json!(1001),
            7 => bad["pointer"]["unexpected"] = json!(true),
            8 => bad["pointer"]["events"] = json!([{"kind":"invented"}]),
            _ => bad["pointer"]["width"] = serde_json::Value::Null,
        }
        let mut data = serde_json::to_vec(&bad).unwrap();
        data.push(b'\n');
        assert!(
            parse(&data, "fixture-nonce", 7, 0, Some(8)).is_err(),
            "fault {fault}"
        );
    }
    let mut huge = good.clone();
    huge.pointer.events =
        vec![pointer_event(macos_native_pointer::PointerKind::Down, 330.0, 90.0); 97];
    assert!(decode(&huge).is_err());
    for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut bad = good.pointer.clone();
        bad.width = number;
        assert!(bad.validate().is_err());
    }
}

#[test]
fn pointer_click_proof_checks_history_button_count_coordinates_and_zero_flags() {
    use macos_native_pointer::click;
    let before = clicked(&baseline().pointer, 0, 1);
    for button in 0..3 {
        for count in 1..=2 {
            let good = clicked(&before, button, count);
            assert!(click(&before, &good, [0.5, 0.5], button, count).is_ok());
            for fault in 0..10 {
                let mut bad = good.clone();
                let n = before.events.len();
                match fault {
                    0 => {
                        bad.events.pop();
                    }
                    1 => bad.events.swap(n, n + 1),
                    2 => bad.events[n].button = (button + 1) % 3,
                    3 => bad.events[n].click_count = 3,
                    4 => bad.events[n].flags = 0x100000,
                    5 => bad.events[n].x += 20.0,
                    6 => bad.scroll_offset += 10.0,
                    7 => bad.box_rect[0] += 10.0,
                    8 => bad.events[0].x += 10.0,
                    _ => bad.dragging = true,
                }
                assert!(
                    click(&before, &bad, [0.5, 0.5], button, count).is_err(),
                    "{button}/{count}/{fault}"
                );
            }
        }
    }
}

#[test]
fn pointer_scroll_requires_signed_pixels_and_independent_viewport_change() {
    use macos_native_pointer::{scroll, PointerKind::Wheel};
    let before = clicked(&baseline().pointer, 0, 1);
    for delta in [-120, 120] {
        let mut good = before.clone();
        let mut event = pointer_event(Wheel, 330.0, 90.0);
        event.delta_y = -f64::from(delta);
        good.events.push(event);
        good.scroll_offset += f64::from(delta);
        assert!(scroll(&before, &good, [0.5, 0.5], delta).is_ok());
        for fault in 0..8 {
            let mut bad = good.clone();
            match fault {
                0 => bad.events.last_mut().unwrap().delta_y *= -1.0,
                1 => bad.events.last_mut().unwrap().delta_y *= 0.5,
                2 => bad.scroll_offset = before.scroll_offset,
                3 => bad.events.push(bad.events.last().unwrap().clone()),
                4 => bad.events.last_mut().unwrap().x += 20.0,
                5 => bad.events.last_mut().unwrap().flags = 0x20000,
                6 => bad.box_rect[0] += 10.0,
                _ => bad.events[0].x += 10.0,
            }
            assert!(
                scroll(&before, &bad, [0.5, 0.5], delta).is_err(),
                "{delta}/{fault}"
            );
        }
    }
    assert!(scroll(&before, &before, [0.5, 0.5], 0).is_ok());
    assert!(scroll(&before, &clicked(&before, 0, 1), [0.5, 0.5], 0).is_err());
}

#[test]
fn zero_pointer_scroll_does_not_validate_an_invalid_coordinate() {
    let state = baseline().pointer;
    for point in [[f64::NAN, 0.5], [-0.1, 0.5], [0.5, 1.0]] {
        assert!(macos_native_pointer::scroll(&state, &state, point, 0).is_err());
    }
}

#[test]
fn pointer_drag_needs_box_motion_ordered_path_and_final_owned_release() {
    use macos_native_pointer::{drag, PointerKind::Drag};
    let before = clicked(&baseline().pointer, 0, 1);
    let good = dragged(&before, false);
    assert!(drag(&before, &good, [0.5, 0.5], [0.72, 0.72]).is_ok());
    let back = dragged(&good, true);
    assert!(drag(&good, &back, [0.72, 0.72], [0.5, 0.5]).is_ok());
    let n = before.events.len();
    for fault in 0..13 {
        let mut bad = good.clone();
        match fault {
            0 => {
                bad.events.pop();
            }
            1 => bad.events.last_mut().unwrap().kind = Drag,
            2 => bad.events[n + 1].x += 25.0,
            3 => {
                bad.events[n + 1].x = 300.0;
                bad.events[n + 1].y = 70.0;
            }
            4 => bad.events[n + 2].x -= 10.0,
            5 => bad.box_rect = before.box_rect,
            6 => bad.events.last_mut().unwrap().x -= 10.0,
            7 => bad.events[n].button = 1,
            8 => {
                for _ in 0..20 {
                    bad.events.insert(n + 1, bad.events[n + 1].clone());
                }
            }
            9 => bad.events[n].flags = 0x40000,
            10 => bad.scroll_offset += 10.0,
            11 => bad.dragging = true,
            _ => bad.events[0].x += 10.0,
        }
        assert!(
            drag(&before, &bad, [0.5, 0.5], [0.72, 0.72]).is_err(),
            "fault {fault}"
        );
    }
    let mut coalesced = good.clone();
    coalesced.events.remove(n + 1);
    assert!(drag(&before, &coalesced, [0.5, 0.5], [0.72, 0.72]).is_ok());
}

#[test]
fn pointer_drag_must_not_use_position_tolerance_to_accept_a_resized_box() {
    let before = baseline().pointer;
    for axis in [2, 3] {
        let mut after = dragged(&before, false);
        after.box_rect[axis] += 0.5;
        assert!(macos_native_pointer::drag(&before, &after, [0.5, 0.5], [0.72, 0.72]).is_err());
    }
}

#[test]
fn native_drag_motion_has_no_click_count_and_slow_release_can_report_zero() {
    let before = baseline().pointer;
    let mut after = dragged(&before, false);
    after.events.last_mut().unwrap().click_count = 0;
    assert!(macos_native_pointer::drag(&before, &after, [0.5, 0.5], [0.72, 0.72]).is_ok());
    after.events[1].click_count = 1;
    assert!(macos_native_pointer::drag(&before, &after, [0.5, 0.5], [0.72, 0.72]).is_err());
    let mut click = clicked(&before, 0, 1);
    click.events.last_mut().unwrap().click_count = 0;
    assert!(macos_native_pointer::click(&before, &click, [0.5, 0.5], 0, 1).is_err());
}

#[test]
fn rejected_native_action_must_leave_pointer_events_and_visual_state_unchanged() {
    let before = baseline();
    for fault in 0..5 {
        let mut after = before.clone();
        match fault {
            0 => after.pointer = clicked(&before.pointer, 0, 1),
            1 => after.pointer.scroll_offset += 1.0,
            2 => after.pointer.box_rect[0] += 1.0,
            3 => after.pointer.dragging = true,
            _ => after.pointer.overflow = true,
        }
        assert!(unchanged(&before, &after).is_err(), "fault {fault}");
    }
    let old: Vec<_> = GATES
        .iter()
        .copied()
        .filter(|g| !g.starts_with("pointer-") && !g.starts_with("retained-name-"))
        .collect();
    assert_eq!(old.len(), 18);
    assert_eq!(GATES.len(), 29);
    let mut report = Report::not_run("test only");
    report.passed = old;
    report.finish(Ok(()));
    assert_eq!(report.status, "failed");
}

#[test]
fn native_wait_gate_rejects_mutating_or_unverified_return_flags_and_incomplete_matrix() {
    use grok_computer_use_core::adapter::AdapterActResult;
    let good = AdapterActResult {
        applied: false,
        outcome: None,
        postcondition_ok: true,
        verifiable: true,
        detail: "read-only matched".into(),
    };
    macos_native_gates::verified_read_only_wait(&good).unwrap();
    for fault in 0..4 {
        let mut bad = good.clone();
        match fault {
            0 => bad.applied = true,
            1 => bad.verifiable = false,
            2 => bad.postcondition_ok = false,
            _ => bad.outcome = Some(grok_computer_use_core::protocol::OutcomeKind::Rejected),
        }
        assert!(macos_native_gates::verified_read_only_wait(&bad).is_err());
    }
    let waits: Vec<_> = GATES
        .iter()
        .copied()
        .filter(|g| g.starts_with("retained-name-"))
        .collect();
    assert_eq!(
        waits,
        vec![
            "retained-name-wait",
            "retained-name-wait-timeout",
            "retained-name-wait-stale-reference"
        ]
    );
    let mut report = Report::not_run("test only");
    report.passed = GATES
        .iter()
        .copied()
        .filter(|g| !g.starts_with("retained-name-"))
        .collect();
    assert_eq!(report.passed.len(), 26);
    report.finish(Ok(()));
    assert_eq!(report.status, "failed");
}

#[test]
fn accepts_only_matching_version_nonce_process_window_title_and_request() {
    assert_eq!(decode(&baseline()).unwrap(), baseline());
    for fault in 0..11 {
        let mut state = baseline();
        match fault {
            0 => state.version = 1,
            1 => state.nonce = "wrong".into(),
            2 => state.pid = 9,
            3 => state.id = 1,
            4 => state.window_id = 9,
            5 => state.window_id = 0,
            6 => state.window_id = u32::MAX,
            7 => state.title = "unowned".into(),
            8 => state.error = Some("failed".into()),
            9 => state.architecture = "different-architecture".into(),
            _ => state.translated = true,
        }
        assert!(decode(&state).is_err(), "fault {fault}");
    }
}

#[test]
fn native_proof_requires_this_process_not_parent_shell_to_be_untranslated() {
    use macos_native_protocol::native_translation_readback as check;
    assert!(check(0, 0, 4, false).is_ok());
    assert!(check(-1, 0, 4, true).is_ok());
    for (status, value, size, missing) in [
        (0, 1, 4, false),
        (0, 99, 4, false),
        (0, 0, 0, false),
        (-1, 0, 4, false),
        (1, 0, 4, true),
        (0, 1, 4, true),
    ] {
        assert!(check(status, value, size, missing).is_err());
    }
}
#[test]
fn requires_framed_strict_json_not_a_log_line_or_extra_fields() {
    let mut extra = serde_json::to_value(baseline()).unwrap();
    extra["verified"] = json!(true);
    let mut extra = serde_json::to_vec(&extra).unwrap();
    extra.push(b'\n');
    for input in [
        b"native PASS\n".to_vec(),
        serde_json::to_vec(&baseline()).unwrap(),
        extra,
        vec![b'x'; MAX_REPLY + 1],
        b"{}\n".to_vec(),
    ] {
        assert!(parse(&input, "fixture-nonce", 7, 0, Some(8)).is_err());
    }
}
#[test]
fn line_reader_bounds_allocation_and_rejects_eof_before_newline() {
    let valid = bytes(&baseline());
    let mut both = valid.clone();
    both.extend(&valid);
    let mut reader = Cursor::new(both);
    assert_eq!(read_line(&mut reader).unwrap(), valid);
    assert_eq!(read_line(&mut reader).unwrap(), valid);
    assert!(read_line(&mut reader).is_err());
    for input in [vec![b'x'; MAX_REPLY + 1], b"partial".to_vec(), vec![]] {
        assert!(read_line(&mut Cursor::new(input)).is_err());
    }
}
#[test]
fn selection_uses_utf16_and_overflow_or_truncated_evidence_fails() {
    let mut state = baseline();
    state.selection = [state.text.encode_utf16().count(), 0];
    assert!(decode(&state).is_ok());
    for range in [
        [usize::MAX, 1],
        [0, 100],
        [state.text.encode_utf16().count(), 1],
    ] {
        state.selection = range;
        assert!(decode(&state).is_err());
    }
    state = baseline();
    state.keys = vec![
        KeyEvent {
            code: 123,
            down: true
        };
        65
    ];
    assert!(decode(&state).is_err());
    state = baseline();
    state.text = "x".repeat(8193);
    assert!(decode(&state).is_err());
}
#[test]
fn rejection_needs_unchanged_text_selection_keys_and_clicks() {
    let before = baseline();
    assert!(unchanged(&before, &before).is_ok());
    for fault in 0..4 {
        let mut after = before.clone();
        match fault {
            0 => after.text.push('x'),
            1 => after.selection = [1, 0],
            2 => after.keys.push(KeyEvent {
                code: 123,
                down: true,
            }),
            _ => after.clicks += 1,
        }
        assert!(unchanged(&before, &after).is_err());
    }
}
#[test]
fn missing_release_duplicates_wrong_keys_and_forged_prefix_fail() {
    let mut state = baseline();
    let previous = vec![
        KeyEvent {
            code: 124,
            down: true,
        },
        KeyEvent {
            code: 124,
            down: false,
        },
    ];
    state.keys = previous.clone();
    state.keys.extend([
        KeyEvent {
            code: 123,
            down: true,
        },
        KeyEvent {
            code: 123,
            down: false,
        },
    ]);
    assert!(key_pair(&state, &previous, 123).is_ok());
    for fault in 0..4 {
        let mut bad = state.clone();
        match fault {
            0 => {
                bad.keys.pop();
            }
            1 => bad.keys.push(KeyEvent {
                code: 123,
                down: false,
            }),
            2 => bad.keys[2].code = 124,
            _ => bad.keys[0].code = 123,
        }
        assert!(key_pair(&bad, &previous, 123).is_err());
    }
}
fn target() -> TargetInfo {
    let instance = WindowInstance::fresh(WindowKey::new(7, 8, 100, 1).unwrap());
    TargetInfo {
        target_id: instance.target_id(),
        title: baseline().title,
        app_name: "OwnedFixture".into(),
        kind: "window".into(),
        pid: Some(7),
        backend: "macos".into(),
        execution_mode: "exclusive".into(),
        replay_policy: "never".into(),
        lifecycle_stamp: instance.lifecycle_stamp(),
        display_id: "quartz".into(),
        coordinate_space: "image-pixels".into(),
        scope_label: "owned".into(),
    }
}
#[test]
fn discovery_cannot_substitute_same_title_or_pid_or_duplicate_windows() {
    let t = target();
    assert_eq!(
        owned_target(std::slice::from_ref(&t), &baseline()).unwrap(),
        t.target_id
    );
    assert!(owned_target(&[t.clone(), t.clone()], &baseline()).is_err());
    for fault in 0..7 {
        let mut bad = t.clone();
        match fault {
            0 => bad.pid = Some(9),
            1 => bad.title = "other".into(),
            2 => bad.target_id = "mac:7:8".into(),
            3 => bad.kind = "desktop".into(),
            4 => bad.backend = "fake".into(),
            5 => bad.lifecycle_stamp += 1,
            _ => bad.coordinate_space = "screen-points".into(),
        }
        assert!(owned_target(&[bad], &baseline()).is_err());
    }
}
#[test]
fn coordinate_target_comes_from_complete_image_bounds_not_screen_points() {
    let obs: Observation = serde_json::from_value(json!({"version":1,"runId":"r","targetId":"t",
        "targetGeneration":1,"snapshotId":"s","capturedAt":"fixture","geometryRevision":1,
        "coordinateSpace":"image-pixels","image":{"width":1200,"height":800,"contentId":"c"},
        "nodes":[],"truncated":false}))
    .unwrap();
    let node = ObservationNode {
        x: Some(40.0),
        y: Some(60.0),
        width: Some(100.0),
        height: Some(120.0),
        ..Default::default()
    };
    assert_eq!(
        center(&node, &obs).unwrap(),
        grok_computer_use_core::protocol::ActionTarget::Coord { x: 90.0, y: 120.0 }
    );
    for fault in 0..6 {
        let mut bad = node.clone();
        match fault {
            0 => bad.x = None,
            1 => bad.x = Some(-1.0),
            2 => bad.y = Some(f64::NAN),
            3 => bad.width = Some(0.0),
            4 => bad.width = Some(1200.0),
            _ => bad.height = Some(f64::INFINITY),
        }
        assert!(center(&bad, &obs).is_err());
    }
}
#[test]
fn no_platform_no_adapter_or_incomplete_matrix_is_never_native_pass() {
    let report = macos_native_gates::execute(None, Path::new("absent"), Path::new("absent"));
    assert_eq!(report.status, "not_run");
    assert!(report.passed.is_empty());
    let mut report = Report::not_run("test");
    report.finish(Ok(()));
    assert_eq!(report.status, "failed");
    report.passed = GATES.to_vec();
    report.finish(Err("cleanup unconfirmed".into()));
    assert_eq!(report.status, "failed");
    report.passed.swap(0, 1);
    report.finish(Ok(()));
    assert_eq!(report.status, "failed");
    report.passed = GATES.to_vec();
    report.finish(Ok(()));
    assert_eq!(report.status, "passed");
}
#[test]
fn evidence_must_be_new_and_never_overwrites_existing_data() {
    let path = std::env::temp_dir().join(format!("cu-native-proof-{}.json", uuid::Uuid::new_v4()));
    macos_native_gates::write_new(&path, b"original").unwrap();
    assert!(macos_native_gates::write_new(&path, b"replacement").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    std::fs::remove_file(path).unwrap();
}
#[test]
fn owned_pipe_handshake_roundtrip_and_clean_exit() {
    let mut fixture = Fixture::contract_child("normal").unwrap();
    let state = fixture.request("state").unwrap();
    assert_eq!(state.id, 1);
    assert!(fixture.request("type").is_err());
    assert!(fixture.request("key").is_err());
    assert!(fixture.request("setExpected").is_err());
    assert_eq!(fixture.request("focus").unwrap().id, 2);
    fixture.wait(|s| s.active && s.focused).unwrap();
    fixture.quiet(&state).unwrap();
    fixture.request("quit").unwrap();
    fixture.shutdown().unwrap();
}
#[test]
fn owned_pipe_rejects_bad_receipts_instead_of_retrying_an_input() {
    for mode in [
        "bad-json",
        "oversized",
        "partial-eof",
        "wrong-nonce",
        "wrong-pid",
        "wrong-window",
        "stale-id",
    ] {
        let mut fixture = Fixture::contract_child(mode).unwrap();
        assert!(fixture.request("state").is_err(), "{mode}");
    }
    assert!(Fixture::contract_child("startup-eof").is_err());
}
#[test]
fn owned_pipe_times_out_and_nonzero_child_exit_is_not_a_pass() {
    let start = std::time::Instant::now();
    {
        let mut fixture = Fixture::contract_child("reply-timeout").unwrap();
        assert!(fixture.request("state").is_err());
    }
    assert!(start.elapsed() < std::time::Duration::from_secs(8));
    let mut fixture = Fixture::contract_child("bad-exit").unwrap();
    fixture.request("quit").unwrap();
    assert!(fixture.shutdown().is_err());
}
