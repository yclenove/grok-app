//! Linux Wayland assembly. Fake portal frames prove the virtual path only.
//!
//! Real GNOME, KDE, and wlroots portal consent, re-authorization, revoke,
//! expiry, PipeWire frame source and mapping, Stop loss, and session close
//! need a physical desktop and stay `not_run`.

#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_core::driver::{
    AppRef, Cancel, CaptureOpts, Driver, DriverAction, DriverError, PointerRoute, WindowRef,
};
use gcu_core::model::WindowPoint;
use gcu_driver_linux::atspi::source::{FakeNode, FakeSource};
use gcu_driver_linux::wayland::assembly::{
    compositor_allows_native, pipewire_frames_compiled, FrameSource, WindowConsent,
};
use gcu_driver_linux::wayland::input_control::StopControl;
use gcu_driver_linux::x11::fake::FakeWindow;
use gcu_driver_linux::x11::hotkey::{SharedStopLatch, StopLatch};
use gcu_driver_linux::x11::{FakeX, XRootGeometry, XWindowGeometry};
use gcu_driver_linux::{
    detect_environment, Compositor, FakeFrameReceiver, FakePortalClient, LinuxDriver,
    LinuxEnvironment, LinuxSessionType, MockEnv, OriginBinding, PortalInfo, PortalSourceKind,
    PortalStream, RestoreRecord, RestoreTokenStore, MAX_TOKEN_LIFETIME_SECS,
};
use gcu_protocol::{AppInfo, Rect, WindowInfo};
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::Ordering;
use std::sync::Arc;

const NOW: u64 = 1_700_000_000;

fn gnome_pure() -> LinuxEnvironment {
    LinuxEnvironment {
        session_type: LinuxSessionType::WaylandPure,
        compositor: Compositor::Gnome,
        desktop_name: Some("GNOME".into()),
        wayland_display: Some("wayland-0".into()),
        x11_display: None,
    }
}

fn origin() -> OriginBinding {
    OriginBinding {
        compositor: "gnome:epoch".into(),
        source_kind: "window".into(),
        window_identity: "wayland:1:7".into(),
        process_instance: "/usr/bin/app:10".into(),
        auth_version: "auth-1".into(),
    }
}

fn consent() -> WindowConsent {
    WindowConsent {
        window_id: "wayland:1".into(),
        compositor: Compositor::Gnome,
        origin: origin(),
        permit_issued_at_secs: NOW,
        permit_expires_at_secs: NOW + 3_600,
        now_secs: NOW,
        expected_mapping_id: "mapping-42".into(),
        expected_serial: 1001,
        token_key: "win-1".into(),
    }
}

fn stream() -> PortalStream {
    PortalStream {
        node_id: 42,
        source_type: PortalSourceKind::Window,
        position: Some((0, 0)),
        size: Some((2, 2)),
        mapping_id: Some("mapping-42".into()),
        pipewire_serial: Some(1001),
    }
}

fn frames() -> FrameSource {
    FrameSource::virtual_frames(Arc::new(FakeFrameReceiver::new(42, NOW)))
}

fn app() -> AppRef {
    AppRef {
        info: AppInfo {
            id: "app".into(),
            name: "App".into(),
            pid: Some(10),
            identity: None,
        },
    }
}

fn window_info() -> WindowInfo {
    WindowInfo {
        id: "wayland:1".into(),
        title: "native".into(),
        frame: Rect {
            x: 10.0,
            y: 20.0,
            w: 800.0,
            h: 600.0,
        },
        scale: 2.0,
        on_current_desktop: Some(true),
    }
}

fn win(id: &str) -> WindowRef {
    WindowRef {
        app: app(),
        id: id.into(),
    }
}

fn pure_driver() -> LinuxDriver {
    LinuxDriver::new_fake(
        FakeSource::default(),
        FakeNode::default(),
        vec![app().info],
        vec![("app".into(), window_info())],
    )
    .with_environment(gnome_pure())
}

fn portal() -> FakePortalClient {
    let client = FakePortalClient::new();
    client.set_streams(vec![stream()]);
    client.set_restore_token(Some("restore-1".into()));
    client
}

fn mode_of(path: &std::path::Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn wayland_display_is_not_classified_as_pure_x11() {
    let both = detect_environment(
        &MockEnv::new()
            .set("WAYLAND_DISPLAY", "wayland-0")
            .set("DISPLAY", ":0")
            .set("XDG_CURRENT_DESKTOP", "GNOME"),
    );
    assert_eq!(both.session_type, LinuxSessionType::WaylandWithXWayland);
    assert_eq!(both.compositor, Compositor::Gnome);
    assert!(both.session_type.is_wayland());

    let x11 = detect_environment(&MockEnv::new().set("DISPLAY", ":0"));
    assert_eq!(x11.session_type, LinuxSessionType::PureX11);
    assert!(!x11.session_type.is_wayland());
    assert!(!compositor_allows_native(Compositor::Other));
    assert!(!compositor_allows_native(Compositor::Unknown));
    assert!(compositor_allows_native(Compositor::Wlroots));
}

#[tokio::test]
async fn portal_info_alone_does_not_capture() {
    let portal_info = PortalInfo {
        available: true,
        screencast_version: Some(4),
        remote_desktop_version: Some(2),
        global_shortcuts_version: Some(1),
    };
    let driver = pure_driver().with_portal_info(portal_info);
    assert!(!driver.capabilities().observe.screenshot);
    assert!(!driver.wayland_stop_registered());
    let client = portal();
    let error = driver
        .capture(
            &win("wayland:1"),
            &CaptureOpts::default(),
            &Cancel::default(),
        )
        .await
        .expect_err("probe is not a source");
    assert_eq!(error, DriverError::CapabilityUnavailable);
    assert!(client.session_flags().is_empty());
    let doc = driver.doctor().await;
    assert_eq!(&doc.capabilities, driver.capabilities());
    assert!(doc
        .issues
        .iter()
        .any(|issue| issue.contains("status capsule is unavailable")));
    assert!(doc.issues.iter().any(
        |issue| issue.contains("GlobalShortcuts interface detection is not Stop registration")
    ));
    assert!(doc
        .issues
        .iter()
        .any(|issue| issue.contains("screencast source is not proven")));
    let mentions_pipewire = doc
        .issues
        .iter()
        .any(|issue| issue.contains("pipewire feature is not compiled"));
    assert_eq!(pipewire_frames_compiled(), cfg!(feature = "pipewire"));
    assert_eq!(mentions_pipewire, !pipewire_frames_compiled());
}

#[tokio::test]
async fn consented_window_returns_png_and_stores_a_private_binding() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = dir.path().join("tokens");
    let tokens = Arc::new(RestoreTokenStore::new(&store));
    let client = portal();
    let latch = Arc::new(SharedStopLatch::default());
    let mut driver = pure_driver().with_stop_latch(latch.clone());
    driver
        .consent_screencast(&client, &frames(), &consent(), tokens.clone())
        .await
        .unwrap();
    assert!(!latch.is_stopped());
    assert!(driver.capabilities().observe.screenshot);
    assert_eq!(mode_of(&store), 0o700);
    let file = store.join("win-1.json");
    assert_eq!(mode_of(&file), 0o600);
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(saved["token"], "restore-1");
    assert_eq!(saved["binding"]["compositor"], "gnome:epoch");
    assert_eq!(saved["binding"]["source_kind"], "window");
    assert_eq!(saved["binding"]["window_identity"], "wayland:1:7");
    assert_eq!(saved["binding"]["process_instance"], "/usr/bin/app:10");
    assert_eq!(saved["binding"]["auth_version"], "auth-1");
    assert!(saved["expires_at_secs"].as_u64().unwrap() <= NOW + MAX_TOKEN_LIFETIME_SECS);
    assert!(
        saved["expires_at_secs"].as_u64().unwrap()
            <= saved["auth_expires_at_secs"].as_u64().unwrap()
    );
    assert!(client.session_flags()[0].load(Ordering::SeqCst));

    let shot = driver
        .capture(
            &win("wayland:1"),
            &CaptureOpts::default(),
            &Cancel::default(),
        )
        .await
        .unwrap();
    assert_eq!(&shot.bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    let geometry = driver.window_geometry(&win("wayland:1")).await.unwrap();
    assert_eq!(shot.geometry, geometry);
    assert_eq!(geometry.scale, 1.0);
    assert_eq!(geometry.frame.w, 2.0);
    assert_eq!(geometry.frame.h, 2.0);
    assert_eq!(geometry.frame.x, 0.0);
    let listed = driver.list_windows(Some(&app())).await.unwrap();
    assert_eq!(listed[0].frame, geometry.frame);
    assert!(driver.ledger().is_empty());
    assert!(driver.portal_input().is_none());
}

#[tokio::test]
async fn expired_token_does_not_open_a_session() {
    let dir = tempfile::TempDir::new().unwrap();
    let tokens = Arc::new(RestoreTokenStore::new(dir.path().join("tokens")));
    let stale = origin();
    tokens
        .save(
            "win-1",
            &RestoreRecord::new("old", stale, NOW - 100, NOW - 1),
        )
        .unwrap();
    let client = portal();
    let latch = Arc::new(SharedStopLatch::default());
    let mut driver = pure_driver().with_stop_latch(latch.clone());
    let error = driver
        .consent_screencast(&client, &frames(), &consent(), tokens.clone())
        .await
        .unwrap_err();
    assert!(matches!(error, DriverError::PermissionMissing { .. }));
    assert!(client.session_flags().is_empty());
    assert!(!latch.is_stopped());
    assert!(tokens.list_keys().unwrap().is_empty());
}

#[tokio::test]
async fn origin_mismatch_drops_the_old_token_and_does_not_replay_it() {
    let dir = tempfile::TempDir::new().unwrap();
    let tokens = Arc::new(RestoreTokenStore::new(dir.path().join("tokens")));
    let mut previous = origin();
    previous.window_identity = "wayland:1:other".into();
    tokens
        .save(
            "win-1",
            &RestoreRecord::new("old-token", previous, NOW, NOW + 60),
        )
        .unwrap();
    let client = portal();
    let mut driver = pure_driver();
    driver
        .consent_screencast(&client, &frames(), &consent(), tokens.clone())
        .await
        .unwrap();
    assert_eq!(client.last_requested_token(), None);
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(tokens.base_dir().join("win-1.json")).unwrap())
            .unwrap();
    assert_eq!(saved["binding"]["window_identity"], "wayland:1:7");
    assert_ne!(saved["token"], "old-token");
    assert!(client.session_flags()[0].load(Ordering::SeqCst));
}

#[tokio::test]
async fn serial_mismatch_closes_the_new_session_and_stays_stopped() {
    let dir = tempfile::TempDir::new().unwrap();
    let tokens = Arc::new(RestoreTokenStore::new(dir.path().join("tokens")));
    let client = portal();
    let latch = Arc::new(SharedStopLatch::default());
    let mut driver = pure_driver().with_stop_latch(latch.clone());
    driver
        .consent_screencast(&client, &frames(), &consent(), tokens.clone())
        .await
        .unwrap();
    let mut bad = stream();
    bad.pipewire_serial = Some(5);
    client.set_streams(vec![bad]);
    let error = driver
        .consent_screencast(&client, &frames(), &consent(), tokens.clone())
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::CapabilityUnavailable);
    assert_eq!(client.last_requested_token().as_deref(), Some("restore-1"));
    assert!(latch.is_stopped());
    assert!(!client.session_flags()[1].load(Ordering::SeqCst));
    assert!(tokens.list_keys().unwrap().is_empty());
    let again = driver
        .capture(
            &win("wayland:1"),
            &CaptureOpts::default(),
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(again, DriverError::CapabilityUnavailable);
    assert!(driver.is_stopped());
    assert!(!client.session_flags()[0].load(Ordering::SeqCst));
}

#[tokio::test]
async fn unproven_source_never_opens_or_is_closed() {
    let dir = tempfile::TempDir::new().unwrap();
    let tokens = Arc::new(RestoreTokenStore::new(dir.path().join("tokens")));
    let latch = Arc::new(SharedStopLatch::default());
    let mut driver = pure_driver().with_stop_latch(latch.clone());

    let monitor = portal();
    let mut full_desktop = stream();
    full_desktop.source_type = PortalSourceKind::Monitor;
    monitor.set_streams(vec![full_desktop]);
    let error = driver
        .consent_screencast(&monitor, &frames(), &consent(), tokens.clone())
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::CapabilityUnavailable);
    assert!(monitor.session_flags().is_empty());
    assert!(!latch.is_stopped());

    let missing = portal();
    let mut unmapped = stream();
    unmapped.mapping_id = None;
    missing.set_streams(vec![unmapped]);
    let error = driver
        .consent_screencast(&missing, &frames(), &consent(), tokens.clone())
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::CapabilityUnavailable);
    assert!(!missing.session_flags()[0].load(Ordering::SeqCst));
    assert!(latch.is_stopped());

    let wide = consent();
    let mut request = wide;
    request.permit_expires_at_secs = NOW + MAX_TOKEN_LIFETIME_SECS + 1;
    let fresh = Arc::new(SharedStopLatch::default());
    let mut second = pure_driver().with_stop_latch(fresh.clone());
    let refused = portal();
    let error = second
        .consent_screencast(&refused, &frames(), &request, tokens)
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::CapabilityUnavailable);
    assert!(refused.session_flags().is_empty());
    assert!(!fresh.is_stopped());
}

#[tokio::test]
async fn revoke_and_close_release_the_portal_and_stay_stopped() {
    let dir = tempfile::TempDir::new().unwrap();
    let tokens = Arc::new(RestoreTokenStore::new(dir.path().join("tokens")));
    let client = portal();
    let mut driver = pure_driver();
    driver
        .consent_screencast(&client, &frames(), &consent(), tokens.clone())
        .await
        .unwrap();
    driver.revoke_wayland_grant().await.unwrap();
    assert!(driver.is_stopped());
    assert!(!client.session_flags()[0].load(Ordering::SeqCst));
    assert!(tokens.list_keys().unwrap().is_empty());
    assert!(!driver.capabilities().observe.screenshot);
    let error = driver
        .capture(
            &win("wayland:1"),
            &CaptureOpts::default(),
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::CapabilityUnavailable);

    let tokens = Arc::new(RestoreTokenStore::new(dir.path().join("close")));
    let client = portal();
    let mut driver = pure_driver();
    driver
        .consent_screencast(&client, &frames(), &consent(), tokens.clone())
        .await
        .unwrap();
    driver.close_wayland_feature().await.unwrap();
    assert!(driver.is_stopped());
    assert!(!client.session_flags()[0].load(Ordering::SeqCst));
    assert!(tokens.base_dir().is_dir());
    assert!(tokens.list_keys().unwrap().is_empty());
}

#[tokio::test]
async fn expiry_closes_the_portal_and_deletes_the_token() {
    let dir = tempfile::TempDir::new().unwrap();
    let tokens = Arc::new(RestoreTokenStore::new(dir.path().join("tokens")));
    let client = portal();
    let mut driver = pure_driver();
    driver
        .consent_screencast(&client, &frames(), &consent(), tokens.clone())
        .await
        .unwrap();
    driver.advance_capture_clock(NOW + 3_600);
    assert!(!driver.capabilities().observe.screenshot);
    let error = driver
        .capture(
            &win("wayland:1"),
            &CaptureOpts::default(),
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        DriverError::PermissionMissing { ref permission } if permission == "screencast"
    ));
    assert!(driver.is_stopped());
    assert!(!client.session_flags()[0].load(Ordering::SeqCst));
    assert!(tokens.list_keys().unwrap().is_empty());
}

#[tokio::test]
async fn reis_stays_inside_the_granted_region_and_sends_nothing() {
    let dir = tempfile::TempDir::new().unwrap();
    let tokens = Arc::new(RestoreTokenStore::new(dir.path().join("tokens")));
    let client = portal();
    let latch = Arc::new(SharedStopLatch::default());
    let mut driver = pure_driver().with_stop_latch(latch.clone());
    driver
        .consent_screencast(&client, &frames(), &consent(), tokens)
        .await
        .unwrap();
    assert_eq!(
        driver
            .proven_reis_point(&win("wayland:1"), WindowPoint { x: 0.0, y: 0.0 })
            .unwrap_err(),
        DriverError::CapabilityUnavailable
    );
    let stop = StopControl::registered(latch.clone());
    stop.heartbeat();
    let driver = driver.bind_registered_stop(stop.clone());
    assert!(driver.wayland_stop_registered());
    driver
        .proven_reis_point(&win("wayland:1"), WindowPoint { x: 0.0, y: 0.0 })
        .unwrap();
    assert_eq!(
        driver
            .proven_reis_point(&win("wayland:1"), WindowPoint { x: 2.0, y: 0.0 })
            .unwrap_err(),
        DriverError::OutOfBounds
    );
    assert_eq!(
        driver
            .proven_reis_point(&win("other"), WindowPoint { x: 0.0, y: 0.0 })
            .unwrap_err(),
        DriverError::CapabilityUnavailable
    );
    assert!(driver.ledger().is_empty());
    assert!(driver.portal_input().is_none());
    assert!(client.session_flags()[0].load(Ordering::SeqCst));
}

#[tokio::test]
async fn stop_loss_closes_capture_and_stays_stopped() {
    let dir = tempfile::TempDir::new().unwrap();
    let tokens = Arc::new(RestoreTokenStore::new(dir.path().join("tokens")));
    let client = portal();
    let latch = Arc::new(SharedStopLatch::default());
    let mut driver = pure_driver().with_stop_latch(latch.clone());
    driver
        .consent_screencast(&client, &frames(), &consent(), tokens)
        .await
        .unwrap();
    let stop = StopControl::registered(latch.clone());
    stop.heartbeat();
    let driver = driver.bind_registered_stop(stop.clone());
    stop.listener_failed();
    assert!(!driver.wayland_stop_registered());
    assert!(latch.is_stopped());
    let error = driver
        .capture(
            &win("wayland:1"),
            &CaptureOpts::default(),
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::CapabilityUnavailable);
    assert!(!client.session_flags()[0].load(Ordering::SeqCst));
    assert!(driver.is_stopped());
}

#[tokio::test]
async fn unsupported_compositors_do_not_call_the_portal() {
    for compositor in [Compositor::Other, Compositor::Unknown] {
        let mut env = gnome_pure();
        env.compositor = compositor;
        let client = portal();
        let mut driver = pure_driver().with_environment(env);
        let error = driver
            .consent_screencast(
                &client,
                &frames(),
                &consent(),
                Arc::new(RestoreTokenStore::new(
                    tempfile::TempDir::new().unwrap().path().join("tokens"),
                )),
            )
            .await
            .unwrap_err();
        assert_eq!(error, DriverError::CapabilityUnavailable);
        assert!(client.session_flags().is_empty());
        let doc = driver.doctor().await;
        assert!(doc.ready);
        assert!(doc
            .issues
            .iter()
            .any(|issue| issue.contains("disabled") && issue.contains(compositor.slug())));
    }
}

#[tokio::test]
async fn xwayland_uses_only_the_verified_x_window() {
    let server = Arc::new(FakeX::new(
        XRootGeometry {
            root_id: 1,
            width: 800,
            height: 600,
            scale: 1.0,
        },
        true,
    ));
    server.add_window(FakeWindow {
        id: 42,
        name: Some("xwayland".into()),
        pid: Some(10),
        geometry: XWindowGeometry {
            x: 0,
            y: 0,
            width: 40,
            height: 20,
            border_width: 0,
            depth: 24,
        },
        is_mapped: true,
        image: None,
    });
    let mut env = gnome_pure();
    env.session_type = LinuxSessionType::WaylandWithXWayland;
    env.x11_display = Some(":0".into());
    let driver = pure_driver()
        .with_environment(env)
        .with_x_server(server.clone());
    assert!(!driver.is_native_wayland_window(&win("42")));
    assert!(driver.is_native_wayland_window(&win("wayland:1")));
    let shot = driver
        .capture(&win("42"), &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(&shot.bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    let clicked = driver
        .perform(
            &DriverAction::PointerClick {
                win: win("wayland:1"),
                at: WindowPoint { x: 1.0, y: 1.0 },
                button: gcu_protocol::tools::MouseButton::Left,
                count: 1,
                route: PointerRoute::AllowForeground,
            },
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(clicked, DriverError::CapabilityUnavailable);
    assert!(server.get_events().is_empty());
}

#[tokio::test]
async fn cross_origin_frame_closes_the_session() {
    let dir = tempfile::TempDir::new().unwrap();
    let tokens = Arc::new(RestoreTokenStore::new(dir.path().join("tokens")));
    let client = portal();
    let mut foreign = stream();
    foreign.node_id = 7;
    client.set_streams(vec![foreign]);
    let mut driver = pure_driver();
    driver
        .consent_screencast(&client, &frames(), &consent(), tokens.clone())
        .await
        .unwrap();
    let error = driver
        .capture(
            &win("wayland:1"),
            &CaptureOpts::default(),
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::CapabilityUnavailable);
    assert!(driver.is_stopped());
    assert!(!client.session_flags()[0].load(Ordering::SeqCst));
    assert!(tokens.list_keys().unwrap().is_empty());
}

#[test]
#[ignore = "not_run: real GNOME portal, reauthorization, revoke, expiry, frame source and mapping, Stop loss, and session close"]
fn real_gnome_portal_lifecycle() {
    panic!("not_run: real GNOME desktop was not available; FakePortalClient and Xvfb do not substitute");
}

#[test]
#[ignore = "not_run: real KDE portal, reauthorization, revoke, expiry, frame source and mapping, Stop loss, and session close"]
fn real_kde_portal_lifecycle() {
    panic!(
        "not_run: real KDE desktop was not available; FakePortalClient and Xvfb do not substitute"
    );
}

#[test]
#[ignore = "not_run: real wlroots portal, reauthorization, revoke, expiry, frame source and mapping, Stop loss, and session close"]
fn real_wlroots_portal_lifecycle() {
    panic!("not_run: real wlroots desktop was not available; FakePortalClient and Xvfb do not substitute");
}
