//! Integration and unit tests for Wayland detection, capability matrix, and doctor diagnostics.
//!
//! Covers:
//! - Compositor detection: GNOME (Mutter), KDE (KWin), wlroots family, Other, Unknown.
//! - Session type detection: Pure X11, WaylandWithXWayland, WaylandPure, Fallback.
//! - Capability matrix across Pure X11, XWayland, and Native Wayland target environments.
//! - Missing portal degradation: capabilities and doctor report actionable next steps.
//! - Unknown compositor diagnostics in doctor.
//! - XWayland window vs Native Wayland window distinction:
//!   - XWayland window allows capture and foreground input.
//!   - Native Wayland window returns `CapabilityUnavailable` for capture (prior to P7-02)
//!     and foreground input (prior to P7-03), but preserves AT-SPI semantic actions.
//! - Stop hotkey conflict / disabled stop disables foreground delivery and returns `CapabilityUnavailable`.
//! - Dynamic stop latch disabling foreground input and restoring upon clear.
//! - Single source of truth: `doctor.capabilities` exactly matches `*driver.capabilities()`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_core::{
    driver::{
        AppRef, Cancel, CaptureOpts, Driver, DriverAction, DriverError, PointerRoute, WindowRef,
    },
    model::{NativeHandle, NodeAction, Rect, WindowPoint},
};
use gcu_driver_linux::{
    atspi::source::{FakeNode, FakeSource},
    detect_environment,
    wayland::{
        build_environment_capabilities, CapabilityStatus, Compositor, FakePortalChecker,
        LinuxEnvironment, LinuxSessionType, MockEnv, PortalChecker, PortalInfo, TargetEnvironment,
    },
    x11::{
        activity::SharedActivityClock,
        fake::FakeX,
        hotkey::{HotkeyConfig, HotkeyManager, SharedStopLatch, StopLatch},
        FakeWindow, RawImage, XRootGeometry, XWindowGeometry,
    },
    LinuxDriver,
};
use gcu_protocol::{
    capabilities::ForegroundCapability, tools::MouseButton as Button, AppInfo, WindowInfo,
};
use std::sync::Arc;

fn make_fake_x(win_id: u32) -> FakeX {
    let x = FakeX::new(
        XRootGeometry {
            root_id: 1,
            width: 1920,
            height: 1080,
            scale: 1.0,
        },
        true,
    );
    x.add_window(FakeWindow {
        id: win_id,
        name: Some(format!("window-{}", win_id)),
        pid: Some(1000 + win_id),
        geometry: XWindowGeometry {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
            border_width: 0,
            depth: 24,
        },
        is_mapped: true,
        image: Some(RawImage {
            width: 800,
            height: 600,
            data: vec![128; 800 * 600 * 4],
        }),
    });
    x.set_active_window(Some(win_id));
    x
}

#[test]
fn test_compositor_detection() {
    // 1. GNOME (Mutter)
    let env = MockEnv::default()
        .with("XDG_CURRENT_DESKTOP", "GNOME")
        .with("WAYLAND_DISPLAY", "wayland-0");
    assert_eq!(detect_environment(&env).compositor, Compositor::Gnome);

    let env = MockEnv::default()
        .with("XDG_CURRENT_DESKTOP", "ubuntu:GNOME")
        .with("WAYLAND_DISPLAY", "wayland-0");
    assert_eq!(detect_environment(&env).compositor, Compositor::Gnome);

    let env = MockEnv::default()
        .with("DESKTOP_SESSION", "gnome-wayland")
        .with("WAYLAND_DISPLAY", "wayland-0");
    assert_eq!(detect_environment(&env).compositor, Compositor::Gnome);

    // 2. KDE (KWin)
    let env = MockEnv::default()
        .with("XDG_CURRENT_DESKTOP", "KDE")
        .with("WAYLAND_DISPLAY", "wayland-0");
    assert_eq!(detect_environment(&env).compositor, Compositor::Kde);

    let env = MockEnv::default()
        .with("XDG_CURRENT_DESKTOP", "plasma:KDE")
        .with("WAYLAND_DISPLAY", "wayland-0");
    assert_eq!(detect_environment(&env).compositor, Compositor::Kde);

    let env = MockEnv::default()
        .with("KDE_FULL_SESSION", "true")
        .with("WAYLAND_DISPLAY", "wayland-0");
    assert_eq!(detect_environment(&env).compositor, Compositor::Kde);

    // 3. wlroots family
    let env = MockEnv::default()
        .with("SWAYSOCK", "/run/user/1000/sway-ipc.sock")
        .with("WAYLAND_DISPLAY", "wayland-0");
    assert_eq!(detect_environment(&env).compositor, Compositor::Wlroots);

    let env = MockEnv::default()
        .with("HYPRLAND_INSTANCE_SIGNATURE", "sig123")
        .with("WAYLAND_DISPLAY", "wayland-0");
    assert_eq!(detect_environment(&env).compositor, Compositor::Wlroots);

    let env = MockEnv::default()
        .with("XDG_CURRENT_DESKTOP", "river")
        .with("WAYLAND_DISPLAY", "wayland-0");
    assert_eq!(detect_environment(&env).compositor, Compositor::Wlroots);

    let env = MockEnv::default()
        .with("XDG_CURRENT_DESKTOP", "Wayfire")
        .with("WAYLAND_DISPLAY", "wayland-0");
    assert_eq!(detect_environment(&env).compositor, Compositor::Wlroots);

    // 4. Other known compositor
    let env = MockEnv::default()
        .with("XDG_CURRENT_DESKTOP", "COSMIC")
        .with("WAYLAND_DISPLAY", "wayland-0");
    assert_eq!(detect_environment(&env).compositor, Compositor::Other);

    let env = MockEnv::default()
        .with("XDG_CURRENT_DESKTOP", "XFCE")
        .with("DISPLAY", ":0");
    assert_eq!(detect_environment(&env).compositor, Compositor::Other);

    // 5. Unknown
    let env = MockEnv::default();
    assert_eq!(detect_environment(&env).compositor, Compositor::Unknown);
}

#[test]
fn test_session_type_detection() {
    // Pure X11
    let env = MockEnv::default()
        .with("XDG_SESSION_TYPE", "x11")
        .with("DISPLAY", ":0");
    let detected = detect_environment(&env);
    assert_eq!(detected.session_type, LinuxSessionType::PureX11);
    assert!(!detected.session_type.is_wayland());

    // Wayland with XWayland
    let env = MockEnv::default()
        .with("XDG_SESSION_TYPE", "wayland")
        .with("WAYLAND_DISPLAY", "wayland-0")
        .with("DISPLAY", ":0");
    let detected = detect_environment(&env);
    assert_eq!(detected.session_type, LinuxSessionType::WaylandWithXWayland);
    assert!(detected.session_type.is_wayland());
    assert!(detected.session_type.has_xwayland());

    // Pure Wayland (no XWayland / no DISPLAY)
    let env = MockEnv::default()
        .with("XDG_SESSION_TYPE", "wayland")
        .with("WAYLAND_DISPLAY", "wayland-0");
    let detected = detect_environment(&env);
    assert_eq!(detected.session_type, LinuxSessionType::WaylandPure);
    assert!(detected.session_type.is_wayland());
    assert!(!detected.session_type.has_xwayland());

    // Fallbacks when XDG_SESSION_TYPE is unset
    let env = MockEnv::default()
        .with("WAYLAND_DISPLAY", "wayland-1")
        .with("DISPLAY", ":1");
    assert_eq!(
        detect_environment(&env).session_type,
        LinuxSessionType::WaylandWithXWayland
    );

    let env = MockEnv::default().with("WAYLAND_DISPLAY", "wayland-1");
    assert_eq!(
        detect_environment(&env).session_type,
        LinuxSessionType::WaylandPure
    );

    let env = MockEnv::default().with("DISPLAY", ":0");
    assert_eq!(
        detect_environment(&env).session_type,
        LinuxSessionType::PureX11
    );

    let env = MockEnv::default();
    assert_eq!(
        detect_environment(&env).session_type,
        LinuxSessionType::Unknown
    );
}

#[test]
fn test_environment_capabilities_matrix_per_target() {
    let portal_full = PortalInfo {
        available: true,
        screencast_version: Some(4),
        remote_desktop_version: Some(2),
        global_shortcuts_version: Some(1),
    };

    let env_x11 = LinuxEnvironment {
        session_type: LinuxSessionType::PureX11,
        compositor: Compositor::Other,
        desktop_name: Some("XFCE".into()),
        wayland_display: None,
        x11_display: Some(":0".into()),
    };

    // 1. Pure X11
    let caps_x11 = build_environment_capabilities(
        &env_x11,
        TargetEnvironment::PureX11,
        &portal_full,
        true,  // composite ok
        true,  // xtest ok
        true,  // hotkey ok
        true,  // guard ok
        false, // not stopped
    );
    assert_eq!(
        caps_x11.tree_observation.status,
        CapabilityStatus::Available
    );
    assert_eq!(caps_x11.screenshot.status, CapabilityStatus::Available);
    assert_eq!(
        caps_x11.semantic_actions.status,
        CapabilityStatus::Available
    );
    assert_eq!(
        caps_x11.targeted_input.status,
        CapabilityStatus::Unavailable
    );
    assert_eq!(
        caps_x11.foreground_input.status,
        CapabilityStatus::Available
    );
    assert_eq!(
        caps_x11.global_stop_hotkey.status,
        CapabilityStatus::Available
    );
    assert_eq!(
        caps_x11.user_activity_guard.status,
        CapabilityStatus::Available
    );

    // 2. XWayland window
    let env_wayland = LinuxEnvironment {
        session_type: LinuxSessionType::WaylandWithXWayland,
        compositor: Compositor::Gnome,
        desktop_name: Some("GNOME".into()),
        wayland_display: Some("wayland-0".into()),
        x11_display: Some(":0".into()),
    };
    let caps_xwayland = build_environment_capabilities(
        &env_wayland,
        TargetEnvironment::XWaylandWindow,
        &portal_full,
        true,
        true,
        true,
        true,
        false,
    );
    assert_eq!(
        caps_xwayland.tree_observation.status,
        CapabilityStatus::Available
    );
    assert_eq!(caps_xwayland.screenshot.status, CapabilityStatus::Available);
    assert_eq!(
        caps_xwayland.semantic_actions.status,
        CapabilityStatus::Available
    );
    assert_eq!(
        caps_xwayland.targeted_input.status,
        CapabilityStatus::Unavailable
    );
    assert_eq!(
        caps_xwayland.foreground_input.status,
        CapabilityStatus::Available
    );

    // 3. Native Wayland: screenshot and foreground/targeted input unavailable prior to P7-02/03
    let caps_native = build_environment_capabilities(
        &env_wayland,
        TargetEnvironment::NativeWayland,
        &portal_full,
        true,
        true,
        true,
        false, // guard unavailable on Wayland
        false,
    );
    assert_eq!(
        caps_native.tree_observation.status,
        CapabilityStatus::Available
    );
    assert_eq!(
        caps_native.semantic_actions.status,
        CapabilityStatus::Available
    );
    assert_eq!(caps_native.screenshot.status, CapabilityStatus::Available);

    // Verify screenshot is Unavailable when portal lacks screencast
    let portal_no_screencast = PortalInfo {
        available: true,
        screencast_version: None,
        ..portal_full.clone()
    };
    let caps_native_no_screencast = build_environment_capabilities(
        &env_wayland,
        TargetEnvironment::NativeWayland,
        &portal_no_screencast,
        true,
        true,
        true,
        false,
        false,
    );
    assert_eq!(
        caps_native_no_screencast.screenshot.status,
        CapabilityStatus::Unavailable
    );
    assert_eq!(
        caps_native.targeted_input.status,
        CapabilityStatus::Unavailable
    );
    assert_eq!(
        caps_native.foreground_input.status,
        CapabilityStatus::Unavailable
    );
    assert_eq!(
        caps_native.user_activity_guard.status,
        CapabilityStatus::Unavailable
    );
    assert_eq!(
        caps_native.global_stop_hotkey.status,
        CapabilityStatus::Unavailable
    );
}

#[tokio::test]
async fn test_missing_portal_degrades_capabilities_and_doctor() {
    let checker = FakePortalChecker::unavailable();
    let portal_info = checker.probe().await;
    assert!(!portal_info.available);

    let env = LinuxEnvironment {
        session_type: LinuxSessionType::WaylandWithXWayland,
        compositor: Compositor::Gnome,
        desktop_name: Some("GNOME".into()),
        wayland_display: Some("wayland-0".into()),
        x11_display: Some(":0".into()),
    };

    let stop_latch = Arc::new(SharedStopLatch::new(false));
    let hotkey_manager = Arc::new(HotkeyManager::new(
        HotkeyConfig::default(),
        stop_latch.clone(),
    ));
    hotkey_manager.set_available(true);
    let activity_clock = Arc::new(SharedActivityClock::new(true));

    let driver = LinuxDriver::new_fake(FakeSource::default(), FakeNode::default(), vec![], vec![])
        .with_environment(env)
        .with_portal_info(portal_info)
        .with_hotkey_manager(hotkey_manager)
        .with_activity_clock(activity_clock);

    // Doctor issues contain portal failure and next steps
    let doc = driver.doctor().await;
    assert!(doc
        .issues
        .iter()
        .any(|i| i.contains("portal is unavailable") || i.contains("portal: unavailable")));
    assert!(doc
        .issues
        .iter()
        .any(|n| n.contains("xdg-desktop-portal") || n.contains("Install xdg-desktop-portal")));

    // When portal exists but Screencast/RemoteDesktop are missing:
    let partial_portal = PortalInfo {
        available: true,
        screencast_version: None,
        remote_desktop_version: None,
        global_shortcuts_version: Some(1),
    };
    let driver2 = driver.with_portal_info(partial_portal);
    let doc2 = driver2.doctor().await;
    assert!(doc2
        .issues
        .iter()
        .any(|i| i.contains("screencast_unavailable")
            || i.contains("ScreenCast")
            || i.contains("screencast")));
    assert!(doc2
        .issues
        .iter()
        .any(|i| i.contains("remote_desktop_unavailable")
            || i.contains("RemoteDesktop")
            || i.contains("remote_desktop")));
}

#[tokio::test]
async fn test_unknown_compositor_doctor_issues() {
    let env = LinuxEnvironment {
        session_type: LinuxSessionType::WaylandWithXWayland,
        compositor: Compositor::Unknown,
        desktop_name: None,
        wayland_display: Some("wayland-0".into()),
        x11_display: Some(":0".into()),
    };

    let driver = LinuxDriver::new_fake(FakeSource::default(), FakeNode::default(), vec![], vec![])
        .with_environment(env);

    let doc = driver.doctor().await;
    assert!(doc
        .issues
        .iter()
        .any(|i| i.contains("compositor: unknown") || i.contains("compositor is unknown")));
    assert!(doc
        .issues
        .iter()
        .any(|n| n.contains("GNOME, KDE, or wlroots")));
}

#[tokio::test]
async fn test_xwayland_vs_native_window_distinction() {
    let env = LinuxEnvironment {
        session_type: LinuxSessionType::WaylandWithXWayland,
        compositor: Compositor::Gnome,
        desktop_name: Some("GNOME".into()),
        wayland_display: Some("wayland-0".into()),
        x11_display: Some(":0".into()),
    };

    let fake_x = Arc::new(make_fake_x(100));
    let stop_latch = Arc::new(SharedStopLatch::new(false));
    let hotkey_mgr = Arc::new(HotkeyManager::new(
        HotkeyConfig::default(),
        stop_latch.clone(),
    ));
    hotkey_mgr.set_available(true);
    let activity_clock = Arc::new(SharedActivityClock::new(true));

    let x11_app_info = AppInfo {
        id: "app-100".into(),
        name: "XWayland App".into(),
        pid: Some(1100),
        identity: None,
    };
    let native_app_info = AppInfo {
        id: "app-native".into(),
        name: "Native Wayland App".into(),
        pid: Some(1200),
        identity: None,
    };

    let x11_win = WindowRef {
        app: AppRef {
            info: x11_app_info.clone(),
        },
        id: "100".into(),
    };
    let native_win = WindowRef {
        app: AppRef {
            info: native_app_info.clone(),
        },
        id: "/org/a11y/atspi/accessible/window200".into(),
    };

    let windows = vec![
        (
            "app-100".to_string(),
            WindowInfo {
                id: "100".into(),
                title: "XWayland Window".into(),
                frame: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 800.0,
                    h: 600.0,
                },
                scale: 1.0,
                on_current_desktop: Some(true),
            },
        ),
        (
            "app-native".to_string(),
            WindowInfo {
                id: "/org/a11y/atspi/accessible/window200".into(),
                title: "Native Wayland Window".into(),
                frame: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 800.0,
                    h: 600.0,
                },
                scale: 1.0,
                on_current_desktop: Some(true),
            },
        ),
    ];

    let native_btn = FakeNode {
        id: 20,
        name: Some("Submit".into()),
        actions: vec![NodeAction::Press],
        ..Default::default()
    };
    let root = FakeNode {
        id: 1,
        name: Some("Root".into()),
        children: vec![native_btn],
        ..Default::default()
    };

    let driver = LinuxDriver::new_fake(
        FakeSource::default(),
        root,
        vec![x11_app_info, native_app_info],
        windows,
    )
    .with_environment(env)
    .with_x_server(fake_x)
    .with_hotkey_manager(hotkey_mgr)
    .with_activity_clock(activity_clock);

    // Verify window classification
    assert!(!driver.is_native_wayland_window(&x11_win));
    assert!(driver.is_native_wayland_window(&native_win));

    let cancel = Cancel::default();

    // 1. Capture:
    // XWayland window -> capture works via X11 composite/crop
    let x11_cap = driver
        .capture(&x11_win, &CaptureOpts::default(), &cancel)
        .await;
    assert!(x11_cap.is_ok(), "XWayland capture should succeed");

    // Native Wayland window -> rejected with CapabilityUnavailable prior to P7-02
    let native_cap = driver
        .capture(&native_win, &CaptureOpts::default(), &cancel)
        .await;
    assert_eq!(native_cap.unwrap_err(), DriverError::CapabilityUnavailable);

    // 2. Foreground Input:
    // XWayland window -> foreground pointer input works via XTEST
    let click_x11 = driver
        .perform(
            &DriverAction::PointerClick {
                win: x11_win.clone(),
                at: WindowPoint { x: 50.0, y: 50.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::Auto,
            },
            &cancel,
        )
        .await;
    assert!(click_x11.is_ok(), "XWayland click should succeed");

    // Native Wayland window -> rejected with CapabilityUnavailable prior to P7-03
    let click_native = driver
        .perform(
            &DriverAction::PointerClick {
                win: native_win.clone(),
                at: WindowPoint { x: 50.0, y: 50.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::Auto,
            },
            &cancel,
        )
        .await;
    assert_eq!(
        click_native.unwrap_err(),
        DriverError::CapabilityUnavailable
    );

    // 3. AT-SPI Semantic Actions:
    // Native Wayland window STILL allows semantic actions (AT-SPI Press)
    let semantic_press = driver
        .perform(
            &DriverAction::Press {
                el: NativeHandle::new(20u64),
            },
            &cancel,
        )
        .await;
    assert!(
        semantic_press.is_ok(),
        "Native Wayland window retains AT-SPI semantic action capability"
    );
}

#[tokio::test]
async fn test_hotkey_conflict_disables_foreground_and_returns_capability_unavailable() {
    let fake_x = Arc::new(make_fake_x(100));
    let stop_latch = Arc::new(SharedStopLatch::new(false));
    let hotkey_mgr = Arc::new(HotkeyManager::new(
        HotkeyConfig::default(),
        stop_latch.clone(),
    ));
    // Hotkey registration failed or occupied by another app
    hotkey_mgr.set_available(false);

    let app_info = AppInfo {
        id: "app-100".into(),
        name: "Test App".into(),
        pid: Some(1100),
        identity: None,
    };
    let win = WindowRef {
        app: AppRef {
            info: app_info.clone(),
        },
        id: "100".into(),
    };
    let windows = vec![(
        "app-100".to_string(),
        WindowInfo {
            id: "100".into(),
            title: "Test Window".into(),
            frame: Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
            scale: 1.0,
            on_current_desktop: Some(true),
        },
    )];

    let driver = LinuxDriver::new_fake(
        FakeSource::default(),
        FakeNode::default(),
        vec![app_info],
        windows,
    )
    .with_x_server(fake_x)
    .with_hotkey_manager(hotkey_mgr);

    // Foreground input capability disabled
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Unavailable
    );

    // Doctor reflects hotkey unavailability
    let doc = driver.doctor().await;
    assert!(doc
        .issues
        .iter()
        .any(|i| i.contains("stop_hotkey: unavailable")));

    // Perform returns CapabilityUnavailable
    let cancel = Cancel::default();
    let res = driver
        .perform(
            &DriverAction::PointerClick {
                win,
                at: WindowPoint { x: 10.0, y: 10.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::Auto,
            },
            &cancel,
        )
        .await;
    assert_eq!(res.unwrap_err(), DriverError::CapabilityUnavailable);
}

#[tokio::test]
async fn test_stop_latch_tripped_disables_foreground_and_perform() {
    let fake_x = Arc::new(make_fake_x(100));
    let stop_latch = Arc::new(SharedStopLatch::new(false));
    let hotkey_mgr = Arc::new(HotkeyManager::new(
        HotkeyConfig::default(),
        stop_latch.clone(),
    ));
    hotkey_mgr.set_available(true);
    let activity_clock = Arc::new(SharedActivityClock::new(true));

    let app_info = AppInfo {
        id: "app-100".into(),
        name: "Test App".into(),
        pid: Some(1100),
        identity: None,
    };
    let win = WindowRef {
        app: AppRef {
            info: app_info.clone(),
        },
        id: "100".into(),
    };
    let windows = vec![(
        "app-100".to_string(),
        WindowInfo {
            id: "100".into(),
            title: "Test Window".into(),
            frame: Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
            scale: 1.0,
            on_current_desktop: Some(true),
        },
    )];

    let driver = LinuxDriver::new_fake(
        FakeSource::default(),
        FakeNode::default(),
        vec![app_info],
        windows,
    )
    .with_x_server(fake_x)
    .with_hotkey_manager(hotkey_mgr)
    .with_activity_clock(activity_clock)
    .with_stop_latch(stop_latch.clone());

    // Initially available
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Policy
    );

    // Stop latch tripped (e.g. user pressed emergency stop or Stop hotkey fired)
    stop_latch.request_stop();
    assert!(driver.is_stopped());

    // Capabilities dynamically reflect ForegroundCapability::Unavailable
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Unavailable
    );

    // Perform rejected
    let cancel = Cancel::default();
    let res = driver
        .perform(
            &DriverAction::PointerClick {
                win: win.clone(),
                at: WindowPoint { x: 10.0, y: 10.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::Auto,
            },
            &cancel,
        )
        .await;
    assert_eq!(res.unwrap_err(), DriverError::CapabilityUnavailable);

    // Clearing stop latch restores capabilities
    stop_latch.clear_for_test();
    assert!(!driver.is_stopped());
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Policy
    );
}

#[tokio::test]
async fn test_doctor_capabilities_single_source_of_truth() {
    let cases = vec![
        // 1. Pure X11, all healthy
        (
            LinuxEnvironment {
                session_type: LinuxSessionType::PureX11,
                compositor: Compositor::Other,
                desktop_name: Some("XFCE".into()),
                wayland_display: None,
                x11_display: Some(":0".into()),
            },
            PortalInfo::default(),
            true, // hotkey
            true, // guard
        ),
        // 2. Pure X11, hotkey conflict
        (
            LinuxEnvironment {
                session_type: LinuxSessionType::PureX11,
                compositor: Compositor::Unknown,
                desktop_name: None,
                wayland_display: None,
                x11_display: Some(":0".into()),
            },
            PortalInfo::default(),
            false,
            true,
        ),
        // 3. Wayland GNOME, portals healthy
        (
            LinuxEnvironment {
                session_type: LinuxSessionType::WaylandWithXWayland,
                compositor: Compositor::Gnome,
                desktop_name: Some("GNOME".into()),
                wayland_display: Some("wayland-0".into()),
                x11_display: Some(":0".into()),
            },
            PortalInfo {
                available: true,
                screencast_version: Some(4),
                remote_desktop_version: Some(2),
                global_shortcuts_version: Some(1),
            },
            true,
            true,
        ),
        // 4. Wayland KDE, portal missing, hotkey unavailable
        (
            LinuxEnvironment {
                session_type: LinuxSessionType::WaylandWithXWayland,
                compositor: Compositor::Kde,
                desktop_name: Some("KDE".into()),
                wayland_display: Some("wayland-0".into()),
                x11_display: Some(":0".into()),
            },
            PortalInfo {
                available: false,
                screencast_version: None,
                remote_desktop_version: None,
                global_shortcuts_version: None,
            },
            false,
            false,
        ),
        // 5. Pure Wayland wlroots
        (
            LinuxEnvironment {
                session_type: LinuxSessionType::WaylandPure,
                compositor: Compositor::Wlroots,
                desktop_name: Some("sway".into()),
                wayland_display: Some("wayland-0".into()),
                x11_display: None,
            },
            PortalInfo {
                available: true,
                screencast_version: Some(4),
                remote_desktop_version: None,
                global_shortcuts_version: None,
            },
            false,
            false,
        ),
    ];

    for (i, (env, portal, hotkey_ok, guard_ok)) in cases.into_iter().enumerate() {
        let stop_latch = Arc::new(SharedStopLatch::new(false));
        let hotkey_mgr = Arc::new(HotkeyManager::new(
            HotkeyConfig::default(),
            stop_latch.clone(),
        ));
        hotkey_mgr.set_available(hotkey_ok);
        let activity_clock = Arc::new(SharedActivityClock::new(guard_ok));

        let driver =
            LinuxDriver::new_fake(FakeSource::default(), FakeNode::default(), vec![], vec![])
                .with_environment(env)
                .with_portal_info(portal)
                .with_hotkey_manager(hotkey_mgr)
                .with_activity_clock(activity_clock);

        let doc = driver.doctor().await;
        let caps = driver.capabilities();

        assert_eq!(
            doc.capabilities, *caps,
            "Case {}: doctor.capabilities must match driver.capabilities()",
            i
        );
    }
}
