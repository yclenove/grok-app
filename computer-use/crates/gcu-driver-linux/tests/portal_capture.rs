//! Comprehensive tests for Wayland portal ScreenCast capture and restore token lifecycle.
//!
//! Covers:
//! - Restore token file (0600) and directory (0700) permissions.
//! - Origin binding (compositor, source kind, window identity, process instance, auth version).
//! - Any binding item change -> rejection and token cleanup.
//! - Validity lifetime: <= 24h and not exceeding parent authorization duration.
//! - Token cleanup and portal session closure on expiry, feature disable, revocation, session close.
//! - Unknown capture bounds -> no frame output (ADR 0004).
//! - Tree traversal incomplete -> sensitive localization handoff required (ADR 0004).
//! - Stale frames, cross-origin stream nodes, and expired credentials rejection.
//! - Successful single frame capture and PNG output.
//! - Capability table declarations: honest degradation, no false claim of full Wayland support.
//! - Live machine Wayland test marked with `#[ignore]`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_core::model::{Incomplete, IncompleteReason, RawNode, RawTree, Role};
use gcu_driver_linux::{
    wayland::{
        build_environment_capabilities, CapabilityStatus, Compositor, FakeFrameReceiver,
        FakePortalClient, LinuxEnvironment, LinuxSessionType, OriginBinding, PixelRegion,
        PortalCursorMode, PortalError, PortalInfo, PortalScreenCastClient, PortalSourceKind,
        PortalStream, RawCapturedFrame, RestoreError, RestoreRecord, RestoreTokenStore,
        TargetEnvironment, VideoTransform, WaylandCapturePipeline, MAX_TOKEN_LIFETIME_SECS,
    },
    CaptureError,
};
use std::sync::Arc;
use tempfile::tempdir;

fn sample_origin() -> OriginBinding {
    OriginBinding {
        compositor: "gnome".into(),
        source_kind: "window".into(),
        window_identity: "org.gnome.TextEditor-win-1".into(),
        process_instance: "pid:1234:start:5678".into(),
        auth_version: "v1.0".into(),
    }
}

fn make_test_node(role: Role) -> RawNode {
    RawNode {
        role,
        native_role: "window".into(),
        subrole: None,
        name: Some("test window".into()),
        value: None,
        description: None,
        placeholder: None,
        help: None,
        identifier: None,
        states: gcu_core::model::States::empty(),
        frame: None,
        actions: Default::default(),
        text_extras: None,
        url: None,
        children: Vec::new(),
        handle: gcu_core::model::NativeHandle::new(()),
        fingerprint: gcu_core::model::Fingerprint::default(),
    }
}

#[test]
fn test_restore_token_file_and_dir_permissions() {
    let tmp = tempdir().unwrap();
    let store_dir = tmp.path().join("secure-tokens");
    let store = RestoreTokenStore::new(&store_dir);

    let origin = sample_origin();
    let record = RestoreRecord::new("token-xyz-123", origin.clone(), 1000, 2000);

    store.save("test_key", &record).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let dir_meta = std::fs::metadata(&store_dir).unwrap();
        let dir_mode = dir_meta.permissions().mode() & 0o777;
        assert_eq!(
            dir_mode, 0o700,
            "token store directory must have 0700 permissions"
        );

        let file_path = store_dir.join("test_key.json");
        let file_meta = std::fs::metadata(&file_path).unwrap();
        let file_mode = file_meta.permissions().mode() & 0o777;
        assert_eq!(
            file_mode, 0o600,
            "token store file must have 0600 permissions"
        );
    }

    let loaded = store.load_and_validate("test_key", &origin, 1500).unwrap();
    assert_eq!(loaded, "token-xyz-123");
}

#[test]
fn test_restore_token_origin_binding_variations_rejected_and_evicted() {
    let tmp = tempdir().unwrap();
    let store = RestoreTokenStore::new(tmp.path().join("tokens"));
    let origin = sample_origin();

    // Helper to store fresh record and assert rejection on modification
    let check_mismatch = |modified: OriginBinding, field_name: &str| {
        let key = format!("key_{field_name}");
        let record = RestoreRecord::new("token-mismatch", origin.clone(), 1000, 5000);
        store.save(&key, &record).unwrap();

        let err = store
            .load_and_validate(&key, &modified, 1100)
            .expect_err("should reject origin mismatch");
        match err {
            RestoreError::OriginMismatch(msg) => {
                assert!(
                    msg.contains(field_name),
                    "error message '{msg}' should identify '{field_name}'"
                );
            }
            other => panic!("expected OriginMismatch, got: {other:?}"),
        }

        // Token file must be deleted upon mismatch
        let file_path = store.base_dir().join(format!("{key}.json"));
        assert!(
            !file_path.exists(),
            "mismatched token file must be evicted from disk"
        );
    };

    // 1. Compositor changed
    check_mismatch(
        OriginBinding {
            compositor: "kde".into(),
            ..origin.clone()
        },
        "compositor",
    );

    // 2. Source kind changed (e.g. window restored as monitor)
    check_mismatch(
        OriginBinding {
            source_kind: "monitor".into(),
            ..origin.clone()
        },
        "source kind",
    );

    // 3. Window identity changed
    check_mismatch(
        OriginBinding {
            window_identity: "different-window-id".into(),
            ..origin.clone()
        },
        "window identity",
    );

    // 4. Process instance changed (e.g. replaced executable or new PID)
    check_mismatch(
        OriginBinding {
            process_instance: "pid:9999:start:1111".into(),
            ..origin.clone()
        },
        "process instance",
    );

    // 5. Authorization / policy version changed
    check_mismatch(
        OriginBinding {
            auth_version: "v2.0-modified".into(),
            ..origin.clone()
        },
        "authorization version",
    );
}

#[test]
fn test_restore_token_lifetime_bounds_24h_and_auth_duration() {
    let tmp = tempdir().unwrap();
    let store = RestoreTokenStore::new(tmp.path().join("tokens"));
    let origin = sample_origin();

    let issued = 10_000;
    // Auth expires in 48 hours, but token must be capped at 24 hours
    let auth_expires = issued + 48 * 3600;
    let record = RestoreRecord::new("token-24h", origin.clone(), issued, auth_expires);
    assert_eq!(
        record.expires_at_secs,
        issued + MAX_TOKEN_LIFETIME_SECS,
        "token expiration must be bounded to 24h"
    );

    store.save("bound_24h", &record).unwrap();

    // Valid at 12 hours
    let valid = store
        .load_and_validate("bound_24h", &origin, issued + 12 * 3600)
        .unwrap();
    assert_eq!(valid, "token-24h");

    // Expired at 24h + 1s -> rejected and file evicted
    let err = store
        .load_and_validate("bound_24h", &origin, issued + MAX_TOKEN_LIFETIME_SECS + 1)
        .unwrap_err();
    assert!(matches!(err, RestoreError::Expired { .. }));
    assert!(!store.base_dir().join("bound_24h.json").exists());

    // Parent authorization duration is shorter than 24h (e.g. 2 hours)
    let short_auth = issued + 2 * 3600;
    let record_short = RestoreRecord::new("token-short", origin.clone(), issued, short_auth);
    assert_eq!(record_short.expires_at_secs, short_auth);

    store.save("bound_short", &record_short).unwrap();

    // Checked after auth duration (e.g. at 3 hours) -> rejected with AuthExpired
    let err = store
        .load_and_validate("bound_short", &origin, issued + 3 * 3600)
        .unwrap_err();
    assert!(matches!(err, RestoreError::AuthExpired { .. }));
    assert!(!store.base_dir().join("bound_short.json").exists());
}

#[test]
fn test_token_revocation_and_clear_all_lifecycle() {
    let tmp = tempdir().unwrap();
    let store = RestoreTokenStore::new(tmp.path().join("tokens"));
    let origin = sample_origin();

    let rec1 = RestoreRecord::new("tok-1", origin.clone(), 100, 2000);
    let rec2 = RestoreRecord::new("tok-2", origin.clone(), 100, 2000);
    store.save("session_1", &rec1).unwrap();
    store.save("session_2", &rec2).unwrap();

    assert_eq!(store.list_keys().unwrap(), vec!["session_1", "session_2"]);

    // Revocation of single session token
    store.revoke("session_1").unwrap();
    assert_eq!(store.list_keys().unwrap(), vec!["session_2"]);

    // Feature disabled or session cleanup clears all tokens
    let cleared = store.clear_all().unwrap();
    assert_eq!(cleared, 1);
    assert_eq!(store.list_keys().unwrap().len(), 0);
}

#[tokio::test]
async fn test_portal_screencast_session_and_stream_validation() {
    let client = FakePortalClient::new();

    // 1. Happy path screencast negotiation
    let (session, res) = client
        .start_screencast(
            PortalSourceKind::Window,
            Some("prev-token"),
            PortalCursorMode::Hidden,
        )
        .await
        .unwrap();

    assert_eq!(client.last_requested_token(), Some("prev-token".into()));
    assert_eq!(res.stream.node_id, 42);
    assert_eq!(res.stream.source_type, PortalSourceKind::Window);
    assert!(session.is_active());

    // Closing the session marks it inactive
    session.close().await.unwrap();
    assert!(!session.is_active());

    // 2. Stream count validation (must be exactly 1)
    client.set_streams(vec![]);
    let err = client
        .start_screencast(PortalSourceKind::Window, None, PortalCursorMode::Hidden)
        .await
        .unwrap_err();
    assert_eq!(err, PortalError::InvalidStreamCount(0));

    let stream = PortalStream {
        node_id: 10,
        source_type: PortalSourceKind::Window,
        position: None,
        size: Some((800, 600)),
        mapping_id: None,
        pipewire_serial: Some(1),
    };
    client.set_streams(vec![stream.clone(), stream.clone()]);
    let err = client
        .start_screencast(PortalSourceKind::Window, None, PortalCursorMode::Hidden)
        .await
        .unwrap_err();
    assert_eq!(err, PortalError::InvalidStreamCount(2));

    // 3. Node ID validation (0 or u32::MAX is invalid)
    client.set_streams(vec![PortalStream {
        node_id: 0,
        ..stream.clone()
    }]);
    let err = client
        .start_screencast(PortalSourceKind::Window, None, PortalCursorMode::Hidden)
        .await
        .unwrap_err();
    assert_eq!(err, PortalError::InvalidNodeId(0));

    client.set_streams(vec![PortalStream {
        node_id: u32::MAX,
        ..stream.clone()
    }]);
    let err = client
        .start_screencast(PortalSourceKind::Window, None, PortalCursorMode::Hidden)
        .await
        .unwrap_err();
    assert_eq!(err, PortalError::InvalidNodeId(u32::MAX));

    // 4. Source kind mismatch (requested Window, got Monitor)
    client.set_streams(vec![PortalStream {
        node_id: 11,
        source_type: PortalSourceKind::Monitor,
        ..stream.clone()
    }]);
    let err = client
        .start_screencast(PortalSourceKind::Window, None, PortalCursorMode::Hidden)
        .await
        .unwrap_err();
    assert_eq!(
        err,
        PortalError::SourceMismatch {
            requested: PortalSourceKind::Window,
            granted: PortalSourceKind::Monitor
        }
    );

    // 5. PipeWire serial validation (0 is invalid)
    client.set_streams(vec![PortalStream {
        node_id: 12,
        pipewire_serial: Some(0),
        ..stream.clone()
    }]);
    let err = client
        .start_screencast(PortalSourceKind::Window, None, PortalCursorMode::Hidden)
        .await
        .unwrap_err();
    assert_eq!(err, PortalError::InvalidPipeWireSerial);

    // 6. Stream non-positive dimensions
    client.set_streams(vec![PortalStream {
        node_id: 13,
        size: Some((0, 600)),
        ..stream.clone()
    }]);
    let err = client
        .start_screencast(PortalSourceKind::Window, None, PortalCursorMode::Hidden)
        .await
        .unwrap_err();
    assert_eq!(err, PortalError::InvalidStreamDimensions(0, 600));

    // 7. User cancellation injection
    client.inject_error(PortalError::CancelledByUser);
    let err = client
        .start_screencast(PortalSourceKind::Window, None, PortalCursorMode::Hidden)
        .await
        .unwrap_err();
    assert_eq!(err, PortalError::CancelledByUser);
}

#[tokio::test]
async fn test_unknown_capture_bounds_rejected_adr_0004() {
    let stream = PortalStream {
        node_id: 50,
        source_type: PortalSourceKind::Window,
        position: None,
        size: Some((100, 100)),
        mapping_id: None,
        pipewire_serial: Some(1),
    };
    let complete_tree = RawTree {
        root: Some(make_test_node(Role::Window)),
        incomplete: None,
    };

    // Frame with zero width
    let receiver = FakeFrameReceiver::new(50, 1000);
    receiver.set_frame(Some(RawCapturedFrame {
        stream_node_id: 50,
        sequence: 1,
        width: 0,
        height: 100,
        crop_region: None,
        transform: VideoTransform::Normal,
        captured_at_secs: 1000,
        rgba: Arc::new([]),
    }));

    let err =
        WaylandCapturePipeline::capture_frame(&receiver, &stream, 1000, 2000, &complete_tree, 1001)
            .await
            .unwrap_err();
    assert_eq!(err, CaptureError::UnknownCaptureBounds);

    // Frame with crop region exceeding buffer bounds
    receiver.set_frame(Some(RawCapturedFrame {
        stream_node_id: 50,
        sequence: 2,
        width: 100,
        height: 100,
        crop_region: Some(PixelRegion {
            x: 90,
            y: 90,
            width: 20, // 90 + 20 > 100
            height: 20,
        }),
        transform: VideoTransform::Normal,
        captured_at_secs: 1000,
        rgba: Arc::from(vec![0u8; 100 * 100 * 4].into_boxed_slice()),
    }));

    let err =
        WaylandCapturePipeline::capture_frame(&receiver, &stream, 1000, 2000, &complete_tree, 1001)
            .await
            .unwrap_err();
    assert_eq!(err, CaptureError::UnknownCaptureBounds);
}

#[tokio::test]
async fn test_incomplete_tree_forces_handoff_adr_0004() {
    let stream = PortalStream {
        node_id: 50,
        source_type: PortalSourceKind::Window,
        position: None,
        size: Some((2, 2)),
        mapping_id: None,
        pipewire_serial: Some(1),
    };
    let receiver = FakeFrameReceiver::new(50, 1000);

    // Tree marked incomplete due to deadline or node cap
    let incomplete_tree = RawTree {
        root: Some(make_test_node(Role::Window)),
        incomplete: Some(Incomplete {
            reason: IncompleteReason::Deadline,
            read_nodes: 120,
            unread_nodes: None,
        }),
    };

    let err = WaylandCapturePipeline::capture_frame(
        &receiver,
        &stream,
        1000,
        2000,
        &incomplete_tree,
        1001,
    )
    .await
    .unwrap_err();

    match err {
        CaptureError::HandoffRequired(msg) => {
            assert!(msg.contains("incomplete"));
            assert!(msg.contains("Deadline"));
        }
        other => panic!("expected HandoffRequired, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_cross_origin_stale_and_expired_credentials_rejection() {
    let stream = PortalStream {
        node_id: 50,
        source_type: PortalSourceKind::Window,
        position: None,
        size: Some((2, 2)),
        mapping_id: None,
        pipewire_serial: Some(1),
    };
    let complete_tree = RawTree {
        root: Some(make_test_node(Role::Window)),
        incomplete: None,
    };

    // 1. Expired credentials
    let receiver = FakeFrameReceiver::new(50, 1000);
    let err = WaylandCapturePipeline::capture_frame(
        &receiver,
        &stream,
        1000,
        1500, // expired before now (2000)
        &complete_tree,
        2000,
    )
    .await
    .unwrap_err();
    assert_eq!(err, CaptureError::CredentialsExpired);

    // 2. Cross-origin stream node ID
    let receiver_cross_origin = FakeFrameReceiver::new(99, 1000); // 99 != 50
    let err = WaylandCapturePipeline::capture_frame(
        &receiver_cross_origin,
        &stream,
        1000,
        3000,
        &complete_tree,
        1001,
    )
    .await
    .unwrap_err();
    assert_eq!(
        err,
        CaptureError::CrossOriginFrame {
            expected: 50,
            actual: 99
        }
    );

    // 3. Stale frame (captured 60 seconds before permit issue time)
    let receiver_stale = FakeFrameReceiver::new(50, 900); // captured at 900, permit issued at 1000
    let err = WaylandCapturePipeline::capture_frame(
        &receiver_stale,
        &stream,
        1000,
        3000,
        &complete_tree,
        1001,
    )
    .await
    .unwrap_err();
    assert_eq!(
        err,
        CaptureError::StaleFrame {
            captured_at: 900,
            issued_at: 1000
        }
    );
}

#[tokio::test]
async fn test_happy_path_frame_capture_and_crop() {
    let stream = PortalStream {
        node_id: 77,
        source_type: PortalSourceKind::Window,
        position: None,
        size: Some((4, 4)),
        mapping_id: None,
        pipewire_serial: Some(1),
    };
    let complete_tree = RawTree {
        root: Some(make_test_node(Role::Window)),
        incomplete: None,
    };

    // Create a 4x4 buffer with specific colors
    let mut pixels = vec![0u8; 4 * 4 * 4];
    // Fill row 1, col 1 with red
    let idx = (4 + 1) * 4;
    pixels[idx] = 255;
    pixels[idx + 3] = 255;

    let receiver = FakeFrameReceiver::new(77, 1000);
    receiver.set_frame(Some(RawCapturedFrame {
        stream_node_id: 77,
        sequence: 10,
        width: 4,
        height: 4,
        crop_region: Some(PixelRegion {
            x: 1,
            y: 1,
            width: 2,
            height: 2,
        }),
        transform: VideoTransform::Normal,
        captured_at_secs: 1000,
        rgba: Arc::from(pixels.into_boxed_slice()),
    }));

    let capture =
        WaylandCapturePipeline::capture_frame(&receiver, &stream, 1000, 2000, &complete_tree, 1001)
            .await
            .unwrap();

    assert_eq!(capture.width, 2);
    assert_eq!(capture.height, 2);
    assert_eq!(capture.mime, "image/png");
    assert_eq!(capture.geometry.scale, 1.0);
    assert_eq!(capture.geometry.frame.w, 2.0);
    assert_eq!(capture.geometry.frame.h, 2.0);
    assert!(!capture.bytes.is_empty());

    // Verify PNG header magic bytes
    assert_eq!(&capture.bytes[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
}

#[test]
fn test_capabilities_truthfulness_and_honest_degradation() {
    let env_wayland = LinuxEnvironment {
        session_type: LinuxSessionType::WaylandPure,
        compositor: Compositor::Gnome,
        desktop_name: Some("GNOME".into()),
        wayland_display: Some("wayland-0".into()),
        x11_display: None,
    };

    // 1. With active ScreenCast portal backend: screenshot is Available
    let portal_active = PortalInfo {
        available: true,
        screencast_version: Some(4),
        remote_desktop_version: None,
        global_shortcuts_version: None,
    };
    let caps_active = build_environment_capabilities(
        &env_wayland,
        TargetEnvironment::NativeWayland,
        &portal_active,
        false,
        false,
        false,
        false,
        false,
    );
    assert_eq!(
        caps_active.screenshot.status,
        CapabilityStatus::Available,
        "screenshot must be declared Available when ScreenCast portal is active"
    );
    // Honest degradation: targeted input and user activity guard remain Unavailable
    assert_eq!(
        caps_active.targeted_input.status,
        CapabilityStatus::Unavailable
    );
    assert_eq!(
        caps_active.user_activity_guard.status,
        CapabilityStatus::Unavailable
    );
    let proto_caps = caps_active.to_protocol_capabilities();
    assert!(proto_caps.observe.screenshot);
    assert!(proto_caps.observe.tree); // tree is available
    assert!(!proto_caps.delivery.targeted_pointer); // targeted is false

    // 2. With missing ScreenCast portal backend: screenshot is Unavailable
    let portal_missing = PortalInfo {
        available: true,
        screencast_version: None,
        remote_desktop_version: None,
        global_shortcuts_version: None,
    };
    let caps_missing = build_environment_capabilities(
        &env_wayland,
        TargetEnvironment::NativeWayland,
        &portal_missing,
        false,
        false,
        false,
        false,
        false,
    );
    assert_eq!(
        caps_missing.screenshot.status,
        CapabilityStatus::Unavailable
    );
    assert!(caps_missing
        .screenshot
        .reason
        .contains("ScreenCast backend interface missing"));
    assert!(caps_missing
        .screenshot
        .next_step
        .unwrap()
        .contains("xdg-desktop-portal"));
}

#[test]
#[ignore = "requires live Wayland session with active xdg-desktop-portal and pipewire"]
fn test_live_wayland_session_screencast() {
    // This test targets live hardware/CI with mutter --headless or real desktop.
    // Kept ignored in standard CI and platform test runs.
}
