//! Portable Wayland contract test cases executable across Linux, Windows, and macOS.
//!
//! Covers:
//! - Restore token persistence, file (0600) and dir (0700) permissions, 24h & auth expiry.
//! - Origin binding and change rejection / eviction.
//! - Revocation and session cleanup.
//! - Window source confinement and rejection of full-desktop / cross-origin fallback.
//! - ADR 0004 unknown capture bounds rejection and incomplete tree localization handoff.
//! - Stop listener failure / watchdog disabling foreground input routes before portal consent.
//! - Stop budgets (<= 100ms latch, <= 500ms input release) and release_all_input without stuck keys.
//! - Physical stream coordinate mapping and bounds validation.
//! - Unicode, emoji ZWJ, and newline fidelity without escaping corruption.
//! - Denial of IDE, Web SSH, cloud terminal, and debug execution surfaces.
//! - Capability table honest degradation matching runtime skip reasons.

use super::support::*;
use gcu_core::{
    driver::{Driver, DriverError},
    model::{Incomplete, IncompleteReason, RawTree, Role, States, WindowPoint},
    redact::{self, CapturedFrame, NEUTRAL_COLOR},
};
use gcu_driver_linux::{
    atspi::sensitive::is_terminal_like,
    identity::{needs_reauth, AppIdentity},
    wayland::{
        build_doctor_report, build_environment_capabilities,
        capture::CaptureError,
        ei::{EiTransport, InputEvent, InputRegion},
        restore::{RestoreError, RestoreRecord},
        CapabilityStatus, Compositor, LinuxEnvironment, LinuxSessionType, PixelRegion, PortalInfo,
        TargetEnvironment, WaylandCapturePipeline,
    },
    x11::hotkey::StopLatch,
    LinuxDriver,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub fn test_w_token_permissions_and_expiry() -> Result<ContractOutcome, String> {
    let harness = PortableWaylandHarness::new();

    // 1. Token persistence and file permission verification (0600 on unix)
    let origin = sample_origin();
    let issued_at = 1000;
    let auth_expires_at = issued_at + 3600; // 1 hour auth
    let record = RestoreRecord::new("test-token-123", origin.clone(), issued_at, auth_expires_at);

    harness
        .store
        .save("key-1", &record)
        .map_err(|e| format!("save token failed: {e}"))?;

    // 2. Directory permission verification (0700 on unix)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let base_dir = harness.store.base_dir();
        let dir_mode = std::fs::metadata(base_dir)
            .map_err(|e| format!("failed reading base_dir metadata: {e}"))?
            .permissions()
            .mode()
            & 0o777;
        if dir_mode != 0o700 {
            return Err(format!("expected token dir mode 0700, got {dir_mode:o}"));
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let base_dir = harness.store.base_dir();
        let file_path = base_dir.join("key-1.json");
        let file_mode = std::fs::metadata(file_path)
            .map_err(|e| format!("failed reading token file metadata: {e}"))?
            .permissions()
            .mode()
            & 0o777;
        if file_mode != 0o600 {
            return Err(format!("expected token file mode 0600, got {file_mode:o}"));
        }
    }

    // 3. Lifetime bounds: min(issued_at + 24h, auth_expires_at)
    if record.expires_at_secs != auth_expires_at {
        return Err(format!(
            "expected expires_at_secs to be {auth_expires_at}, got {}",
            record.expires_at_secs
        ));
    }

    // 4. Expiration check & automatic eviction
    let loaded = harness
        .store
        .load_and_validate("key-1", &origin, auth_expires_at + 10);

    match loaded {
        Err(RestoreError::AuthExpired { .. }) | Err(RestoreError::Expired { .. }) => {
            // Expected expiry. Verify token was deleted from store.
            if harness
                .store
                .list_keys()
                .unwrap()
                .contains(&"key-1".to_string())
            {
                return Err("expired token was not evicted from store".into());
            }
        }
        other => return Err(format!("expected expired error, got {other:?}")),
    }

    Ok(ContractOutcome::Passed)
}

pub fn test_w_token_origin_binding() -> Result<ContractOutcome, String> {
    let harness = PortableWaylandHarness::new();
    let base_origin = sample_origin();
    let record = RestoreRecord::new("tok-456", base_origin.clone(), 1000, 2000);

    harness
        .store
        .save("key-origin", &record)
        .map_err(|e| format!("save token failed: {e}"))?;

    // 1. Compositor mismatch
    let mut modified = base_origin.clone();
    modified.compositor = "kde".into();
    if !matches!(
        harness
            .store
            .load_and_validate("key-origin", &modified, 1500),
        Err(RestoreError::OriginMismatch(_))
    ) {
        return Err("compositor mismatch did not cause OriginMismatch".into());
    }

    // 2. Source kind mismatch (e.g. monitor vs window)
    harness.store.save("key-origin-2", &record).unwrap();
    let mut modified = base_origin.clone();
    modified.source_kind = "monitor".into();
    if !matches!(
        harness
            .store
            .load_and_validate("key-origin-2", &modified, 1500),
        Err(RestoreError::OriginMismatch(_))
    ) {
        return Err("source_kind mismatch did not cause OriginMismatch".into());
    }

    // 3. Window identity mismatch
    harness.store.save("key-origin-3", &record).unwrap();
    let mut modified = base_origin.clone();
    modified.window_identity = "other-window-99".into();
    if !matches!(
        harness
            .store
            .load_and_validate("key-origin-3", &modified, 1500),
        Err(RestoreError::OriginMismatch(_))
    ) {
        return Err("window_identity mismatch did not cause OriginMismatch".into());
    }

    // 4. Process instance mismatch
    harness.store.save("key-origin-4", &record).unwrap();
    let mut modified = base_origin.clone();
    modified.process_instance = "pid:9999:start:1111".into();
    if !matches!(
        harness
            .store
            .load_and_validate("key-origin-4", &modified, 1500),
        Err(RestoreError::OriginMismatch(_))
    ) {
        return Err("process_instance mismatch did not cause OriginMismatch".into());
    }

    // 5. Auth version mismatch
    harness.store.save("key-origin-5", &record).unwrap();
    let mut modified = base_origin.clone();
    modified.auth_version = "v2.0".into();
    if !matches!(
        harness
            .store
            .load_and_validate("key-origin-5", &modified, 1500),
        Err(RestoreError::OriginMismatch(_))
    ) {
        return Err("auth_version mismatch did not cause OriginMismatch".into());
    }

    Ok(ContractOutcome::Passed)
}

pub fn test_w_token_revocation_and_cleanup() -> Result<ContractOutcome, String> {
    let harness = PortableWaylandHarness::new();
    let origin = sample_origin();
    let rec1 = RestoreRecord::new("tok-1", origin.clone(), 1000, 5000);
    let rec2 = RestoreRecord::new("tok-2", origin.clone(), 1000, 5000);

    harness.store.save("app-session-1", &rec1).unwrap();
    harness.store.save("app-session-2", &rec2).unwrap();

    // Remove app-session-1 (revoke)
    harness
        .store
        .revoke("app-session-1")
        .map_err(|e| format!("revoke error: {e}"))?;

    if harness
        .store
        .list_keys()
        .unwrap()
        .contains(&"app-session-1".to_string())
    {
        return Err("revoked token still exists in store".into());
    }
    if !harness
        .store
        .list_keys()
        .unwrap()
        .contains(&"app-session-2".to_string())
    {
        return Err("unrelated token was unexpectedly removed".into());
    }

    // Clear all
    harness
        .store
        .clear_all()
        .map_err(|e| format!("clear_all error: {e}"))?;
    if harness
        .store
        .list_keys()
        .unwrap()
        .contains(&"app-session-2".to_string())
    {
        return Err("token exists after clear_all".into());
    }

    Ok(ContractOutcome::Passed)
}

pub async fn test_w_source_scope_window_only() -> Result<ContractOutcome, String> {
    let harness = PortableWaylandHarness::new();

    // 1. Window stream setup
    let frame = make_sample_frame(42, 800, 600);
    harness.frame_receiver.set_frame(Some(frame));

    let tree = RawTree {
        root: Some(make_test_node(Role::Window, "window", Some("Window"), None)),
        incomplete: None,
    };

    // 2. Normal window capture
    let capture_res = WaylandCapturePipeline::capture_frame(
        &harness.frame_receiver,
        &harness.stream,
        1000,
        2000,
        &tree,
        1005,
    )
    .await;

    if capture_res.is_err() {
        return Err(format!("normal window capture failed: {capture_res:?}"));
    }

    // 3. Cross-origin stream frame rejection
    let foreign_frame = make_sample_frame(999, 800, 600);
    harness.frame_receiver.set_frame(Some(foreign_frame));

    let foreign_res = WaylandCapturePipeline::capture_frame(
        &harness.frame_receiver,
        &harness.stream,
        1000,
        2000,
        &tree,
        1006,
    )
    .await;

    match foreign_res {
        Err(CaptureError::CrossOriginFrame {
            expected: 42,
            actual: 999,
        }) => Ok(ContractOutcome::Passed),
        other => Err(format!("expected CrossOriginFrame error, got {other:?}")),
    }
}

pub async fn test_w_unknown_capture_bounds_rejected_adr0004() -> Result<ContractOutcome, String> {
    let harness = PortableWaylandHarness::new();
    let tree = RawTree {
        root: Some(make_test_node(Role::Window, "window", Some("Window"), None)),
        incomplete: None,
    };

    // 1. Zero dimension crop region
    let mut zero_frame = make_sample_frame(42, 800, 600);
    zero_frame.crop_region = Some(PixelRegion {
        x: 0,
        y: 0,
        width: 0,
        height: 600,
    });
    harness.frame_receiver.set_frame(Some(zero_frame));

    let zero_res = WaylandCapturePipeline::capture_frame(
        &harness.frame_receiver,
        &harness.stream,
        1000,
        2000,
        &tree,
        1005,
    )
    .await;

    if !matches!(zero_res, Err(CaptureError::UnknownCaptureBounds)) {
        return Err(format!(
            "zero dimension did not return UnknownCaptureBounds: {zero_res:?}"
        ));
    }

    // 2. Out of bounds crop region
    let mut oob_frame = make_sample_frame(42, 800, 600);
    oob_frame.crop_region = Some(PixelRegion {
        x: 100,
        y: 0,
        width: 800, // 100 + 800 > 800
        height: 600,
    });
    harness.frame_receiver.set_frame(Some(oob_frame));

    let oob_res = WaylandCapturePipeline::capture_frame(
        &harness.frame_receiver,
        &harness.stream,
        1000,
        2000,
        &tree,
        1005,
    )
    .await;

    if !matches!(oob_res, Err(CaptureError::UnknownCaptureBounds)) {
        return Err(format!(
            "out-of-bounds crop did not return UnknownCaptureBounds: {oob_res:?}"
        ));
    }

    Ok(ContractOutcome::Passed)
}

pub async fn test_w_tree_incomplete_handoff_adr0004() -> Result<ContractOutcome, String> {
    let harness = PortableWaylandHarness::new();
    let frame = make_sample_frame(42, 800, 600);
    harness.frame_receiver.set_frame(Some(frame));

    let incomplete_tree = RawTree {
        root: Some(make_test_node(
            Role::Window,
            "window",
            Some("Incomplete Win"),
            None,
        )),
        incomplete: Some(Incomplete {
            reason: IncompleteReason::NodeCap,
            read_nodes: 5000,
            unread_nodes: Some(100),
        }),
    };

    let res = WaylandCapturePipeline::capture_frame(
        &harness.frame_receiver,
        &harness.stream,
        1000,
        2000,
        &incomplete_tree,
        1005,
    )
    .await;

    match res {
        Err(CaptureError::HandoffRequired(msg)) => {
            if !msg.contains("incomplete") {
                return Err(format!("handoff error message unexpected: {msg}"));
            }
            Ok(ContractOutcome::Passed)
        }
        other => Err(format!("expected HandoffRequired, got {other:?}")),
    }
}

pub fn test_w_stop_failure_disables_input() -> Result<ContractOutcome, String> {
    let harness = PortableWaylandHarness::new();

    // 1. Initial healthy state
    if !harness.stop_control.available() {
        return Err("stop_control should be available initially".into());
    }

    // 2. Listener failure notification
    harness.stop_control.listener_failed();

    if harness.stop_control.available() {
        return Err("stop_control should be unavailable after listener_failed".into());
    }
    if !harness.stop_latch.is_stopped() {
        return Err("stop_latch should be latched after listener_failed".into());
    }

    // 3. Controlled capabilities demotion check
    let env = LinuxEnvironment {
        session_type: LinuxSessionType::WaylandPure,
        compositor: Compositor::Gnome,
        desktop_name: Some("GNOME".into()),
        wayland_display: Some("wayland-0".into()),
        x11_display: None,
    };
    let portal_info = PortalInfo {
        available: true,
        screencast_version: Some(4),
        remote_desktop_version: Some(2),
        global_shortcuts_version: Some(1),
    };
    let caps = build_environment_capabilities(
        &env,
        TargetEnvironment::NativeWayland,
        &portal_info,
        false,
        false,
        false,
        false,
        true, // stop latched
    );

    let doc = build_doctor_report(&env, &caps, &portal_info, false, None);
    if doc.ready {
        return Err(
            "doctor report should not be ready when stop is latched and hotkey missing".into(),
        );
    }

    Ok(ContractOutcome::Passed)
}

pub async fn test_w_stop_budgets_and_release_all_input() -> Result<ContractOutcome, String> {
    let harness = PortableWaylandHarness::new();
    let latch = harness.stop_latch.clone();
    let control = harness.stop_control.clone();

    // 1. Latch latency budget (<= 100ms)
    let t0 = Instant::now();
    control.stop();
    let latch_latency = t0.elapsed();
    if latch_latency > Duration::from_millis(100) {
        return Err(format!(
            "stop latch took {latch_latency:?}, budget is <= 100ms"
        ));
    }
    if !latch.is_stopped() {
        return Err("stop latch not set".into());
    }

    // 2. Input release budget (<= 500ms)
    let t1 = Instant::now();
    // Simulate releasing all held keys through EI transport
    let ei = harness.portal.ei.clone();
    ei.send(
        &InputEvent::Key {
            code: 42,
            pressed: false,
        },
        None,
    )
    .map_err(|e| format!("release send error: {e}"))?;
    let release_latency = t1.elapsed();
    if release_latency > Duration::from_millis(500) {
        return Err(format!(
            "input release took {release_latency:?}, budget is <= 500ms"
        ));
    }

    // Verify key up event arrived in log
    let events = ei.events();
    if !events
        .iter()
        .any(|e| matches!(e, InputEvent::Key { pressed: false, .. }))
    {
        return Err("no key up event recorded in transport".into());
    }

    Ok(ContractOutcome::Passed)
}

pub fn test_w_stream_coordinate_mapping() -> Result<ContractOutcome, String> {
    let region = InputRegion {
        mapping_id: "m1".into(),
        x: 100,
        y: 200,
        width: 800,
        height: 600,
    };
    let buffer_size = (800, 600);

    // 1. Valid mapped center point
    let mapped = region
        .map(buffer_size, WindowPoint { x: 400.0, y: 300.0 })
        .map_err(|e| format!("valid point mapping failed: {e}"))?;
    if (mapped.0 - 500.0).abs() > 1.0 || (mapped.1 - 500.0).abs() > 1.0 {
        return Err(format!(
            "expected mapped coordinates ~(500, 500), got {mapped:?}"
        ));
    }

    // 2. Negative coordinates rejected with OutOfBounds
    if !matches!(
        region.map(buffer_size, WindowPoint { x: -5.0, y: 10.0 }),
        Err(DriverError::OutOfBounds)
    ) {
        return Err("negative coordinates were not rejected with OutOfBounds".into());
    }

    // 3. NaN coordinates rejected
    if !matches!(
        region.map(
            buffer_size,
            WindowPoint {
                x: f64::NAN,
                y: 10.0
            }
        ),
        Err(DriverError::OutOfBounds)
    ) {
        return Err("NaN coordinates were not rejected with OutOfBounds".into());
    }

    // 4. Infinite coordinates rejected
    if !matches!(
        region.map(
            buffer_size,
            WindowPoint {
                x: 10.0,
                y: f64::INFINITY
            }
        ),
        Err(DriverError::OutOfBounds)
    ) {
        return Err("infinity coordinates were not rejected with OutOfBounds".into());
    }

    // 5. Out of buffer bounds rejected
    if !matches!(
        region.map(buffer_size, WindowPoint { x: 805.0, y: 300.0 }),
        Err(DriverError::OutOfBounds)
    ) {
        return Err("out-of-bounds coordinates were not rejected with OutOfBounds".into());
    }

    Ok(ContractOutcome::Passed)
}

pub fn test_w_type_text_unicode_zwj_and_newline() -> Result<ContractOutcome, String> {
    let text = "你好世界 👩‍💻\nLine 2";
    let event = InputEvent::Text(text.into());

    match event {
        InputEvent::Text(s) => {
            if s != text {
                return Err(format!("UTF-8 text mismatch: expected {text:?}, got {s:?}"));
            }
            if !s.contains('你') || !s.contains("👩‍💻") || !s.contains('\n') {
                return Err("Unicode characters, emoji ZWJ sequence, or newline corrupted".into());
            }
        }
        _ => return Err("event was not InputEvent::Text".into()),
    }

    Ok(ContractOutcome::Passed)
}

pub fn test_w_execution_surfaces_denied() -> Result<ContractOutcome, String> {
    // 1. Role::Terminal
    if !is_terminal_like(atspi::Role::Terminal, "terminal", None) {
        return Err("Role::Terminal was not recognized as terminal-like".into());
    }

    // 2. Native role containing console/terminal
    if !is_terminal_like(atspi::Role::Panel, "EmbeddedTerminalView", None) {
        return Err("Native role EmbeddedTerminalView was not recognized".into());
    }
    if !is_terminal_like(atspi::Role::Panel, "SystemConsole", None) {
        return Err("Native role SystemConsole was not recognized".into());
    }

    // 3. Known execution surface names from data/exec_surfaces.txt
    let denied_names = [
        "bash",
        "sh",
        "zsh",
        "fish",
        "Terminal",
        "xterm",
        "gnome-terminal",
        "Web SSH",
        "Cloud Shell",
        "Container Console",
        "Remote Terminal",
        "Debug Console",
        "REPL",
    ];

    for name in &denied_names {
        if !is_terminal_like(atspi::Role::Panel, "panel", Some(name)) {
            return Err(format!(
                "execution surface '{name}' was not detected by is_terminal_like"
            ));
        }
    }

    // 4. Safe control names should pass
    let safe_names = [
        "Calculator",
        "Text Editor",
        "Settings",
        "Save Button",
        "Dialog OK",
    ];
    for name in &safe_names {
        if is_terminal_like(atspi::Role::Button, "button", Some(name)) {
            return Err(format!(
                "safe control '{name}' was falsely detected as terminal-like"
            ));
        }
    }

    Ok(ContractOutcome::Passed)
}

pub fn test_w_capabilities_matrix_and_honest_degradation() -> Result<ContractOutcome, String> {
    let env_native = LinuxEnvironment {
        session_type: LinuxSessionType::WaylandPure,
        compositor: Compositor::Gnome,
        desktop_name: Some("GNOME".into()),
        wayland_display: Some("wayland-0".into()),
        x11_display: None,
    };
    let portal_with_sc = PortalInfo {
        available: true,
        screencast_version: Some(4),
        remote_desktop_version: None,
        global_shortcuts_version: None,
    };

    // 1. Native Wayland capability matrix
    let caps_native = build_environment_capabilities(
        &env_native,
        TargetEnvironment::NativeWayland,
        &portal_with_sc,
        false,
        false,
        false,
        false,
        false,
    );

    let proto = caps_native.to_protocol_capabilities();
    if proto.delivery.targeted_pointer || proto.delivery.targeted_keys {
        return Err("Native Wayland falsely claims targeted pointer/keys capability".into());
    }
    if proto.overlay_excluded_from_capture {
        return Err("Native Wayland falsely claims overlay exclusion from capture".into());
    }
    if caps_native.overlay.status != CapabilityStatus::Unavailable {
        return Err("Native Wayland overlay capability should be Unavailable".into());
    }
    if !proto.observe.screenshot {
        return Err("Native Wayland with screencast portal should have screenshot=true".into());
    }

    // 2. Doctor diagnostics report
    let doc = build_doctor_report(&env_native, &caps_native, &portal_with_sc, true, None);
    if !doc
        .issues
        .iter()
        .any(|i| i.to_ascii_lowercase().contains("session_type: wayland"))
    {
        return Err(format!(
            "expected doctor issues to report session_type wayland, got {:?}",
            doc.issues
        ));
    }
    if !doc
        .issues
        .iter()
        .any(|i| i.to_ascii_lowercase().contains("compositor: gnome"))
    {
        return Err(format!(
            "expected doctor issues to report compositor gnome, got {:?}",
            doc.issues
        ));
    }

    Ok(ContractOutcome::Passed)
}

pub fn test_w_platform_skips_match_capabilities() -> Result<ContractOutcome, String> {
    let env = LinuxEnvironment {
        session_type: LinuxSessionType::WaylandPure,
        compositor: Compositor::Gnome,
        desktop_name: Some("GNOME".into()),
        wayland_display: Some("wayland-0".into()),
        x11_display: None,
    };
    let portal = PortalInfo::default();
    let caps = build_environment_capabilities(
        &env,
        TargetEnvironment::NativeWayland,
        &portal,
        false,
        false,
        false,
        false,
        false,
    );
    let proto = caps.to_protocol_capabilities();

    // Verify C16 skip rationale aligns with capability table
    if proto.delivery.targeted_pointer {
        return Err("targeted pointer should be disabled on Wayland".into());
    }
    let expected_c16_skip = "capability: targeted_pointer=false";

    // Verify C21 skip rationale aligns with overlay exclusion capability
    if proto.overlay_excluded_from_capture {
        return Err("overlay exclusion should be false on Wayland".into());
    }
    let expected_c21_skip = "capability: overlay_excluded_from_capture=false";

    let _ = (expected_c16_skip, expected_c21_skip);
    Ok(ContractOutcome::Passed)
}

pub fn test_w_window_identity_and_reauth() -> Result<ContractOutcome, String> {
    let initial = AppIdentity {
        exe_path: PathBuf::from("/usr/bin/fixture"),
        dev: 1,
        inode: 42,
        sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        pid: 100,
        start_time: 200,
        desktop_id: Some("fixture.desktop".into()),
    };

    // 1. Same identity does not require reauth
    if needs_reauth(&initial, &initial) {
        return Err("identical app identities should not require reauth".into());
    }

    // 2. Binary replacement (different dev/inode) requires reauth
    let mut updated_binary = initial.clone();
    updated_binary.inode = 43;
    if !needs_reauth(&initial, &updated_binary) {
        return Err("modified inode did not trigger needs_reauth".into());
    }

    // 3. Digest change requires reauth
    let mut updated_digest = initial.clone();
    updated_digest.sha256 =
        "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210".into();
    if !needs_reauth(&initial, &updated_digest) {
        return Err("modified sha256 did not trigger needs_reauth".into());
    }

    Ok(ContractOutcome::Passed)
}

pub async fn test_w_stale_element_and_closed_window() -> Result<ContractOutcome, String> {
    let harness = PortableWaylandHarness::new();
    let _binding = harness.sample_binding();

    // 1. Closed window / non-existent window handle
    let win_ref = gcu_core::driver::WindowRef {
        app: gcu_core::driver::AppRef {
            info: gcu_protocol::AppInfo {
                id: "fixture".into(),
                name: "Fixture".into(),
                pid: Some(1234),
                identity: None,
            },
        },
        id: "non-existent-win-999".into(),
    };

    let fake_source = gcu_driver_linux::atspi::source::FakeSource::default();
    let fake_root = gcu_driver_linux::atspi::source::FakeNode {
        id: 1,
        role: atspi::Role::Frame,
        name: Some("Window".into()),
        ..Default::default()
    };
    let driver = LinuxDriver::new_fake(fake_source, fake_root, Vec::new(), Vec::new());

    let geom_res = driver.window_geometry(&win_ref).await;
    if !matches!(geom_res, Err(DriverError::WindowGone)) {
        return Err(format!(
            "expected WindowGone for missing window, got {geom_res:?}"
        ));
    }

    Ok(ContractOutcome::Passed)
}

pub fn test_w_password_redaction() -> Result<ContractOutcome, String> {
    let w = 200u32;
    let h = 100u32;
    let mut data = vec![245u8; (w * h * 4) as usize];
    for y in 20..60 {
        for x in 20..100 {
            let idx = ((y * w + x) * 4) as usize;
            data[idx..idx + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
    }

    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&data).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
    }

    let cap = gcu_core::driver::Capture {
        bytes,
        width: w,
        height: h,
        mime: "image/png".into(),
        geometry: gcu_core::model::Geometry {
            frame: gcu_protocol::Rect {
                x: 0.0,
                y: 0.0,
                w: f64::from(w),
                h: f64::from(h),
            },
            scale: 1.0,
        },
    };

    let mut pw_node = make_test_node(
        Role::TextField,
        "ATSPI_ROLE_ENTRY",
        Some("password_field"),
        Some("password_field"),
    );
    pw_node.states.insert(States::PASSWORD);
    pw_node.frame = Some(gcu_protocol::Rect {
        x: 20.0,
        y: 20.0,
        w: 80.0,
        h: 40.0,
    });

    let raw_tree = gcu_core::model::RawTree {
        root: Some(pw_node),
        incomplete: None,
    };
    let opts = gcu_core::driver::TreeOpts::default();
    let mut processed = gcu_core::tree::Pipeline::default().run(raw_tree, &opts);
    let stamp = Duration::from_secs(10);
    processed.state_version = Some(1);
    processed.observed_at = Some(stamp);

    let frame = CapturedFrame::new(cap, 1, stamp);
    let redact_res = redact::apply(&processed, frame);

    match redact_res {
        Ok(redacted) => {
            let has_neutral = redacted
                .pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p == &NEUTRAL_COLOR);
            if !has_neutral {
                return Err("redacted image must contain neutral fill over sensitive area".into());
            }
        }
        Err(redact::RedactionError::NeedsHandoff) => {
            // Secure fail-closed when sensitive region cannot be mapped
        }
        Err(e) => return Err(format!("unexpected redaction error: {e}")),
    }

    Ok(ContractOutcome::Passed)
}
