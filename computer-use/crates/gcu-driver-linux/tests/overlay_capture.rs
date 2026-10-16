//! Screenshot exclusion, cleanup and Wayland capability regression tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_driver_linux::{
    wayland::{
        build_doctor_report, build_environment_capabilities,
        capabilities::{CapabilityStatus, TargetEnvironment},
        detect_environment, MockEnv, PortalInfo,
    },
    x11::{capture_window, FakeX, XRootGeometry, XServer},
};
use gcu_protocol::capabilities::PrivateApiStatus;

#[test]
fn failed_capture_restores_overlays_and_own_overlay_is_never_a_target() {
    let x = FakeX::new(
        XRootGeometry {
            root_id: 1,
            width: 1000,
            height: 800,
            scale: 1.,
        },
        true,
    );
    x.overlay.register(99);
    assert!(capture_window(&x, 42).is_err());
    assert_eq!(x.overlay.transitions(), [true, false]);
    assert!(capture_window(&x, 99).is_err());
    assert_eq!(x.overlay.transitions(), [true, false]);
}
#[test]
fn overlapping_capture_leases_keep_overlay_hidden_until_last_drop() {
    let x = FakeX::new(
        XRootGeometry {
            root_id: 1,
            width: 1000,
            height: 800,
            scale: 1.,
        },
        true,
    );
    x.overlay.register(99);
    let lease = x.overlay_capture_scope(42).unwrap();
    assert!(!lease.allows_root_capture());
    let second = x.overlay_capture_scope(43).unwrap();
    drop(lease);
    assert!(x.overlay.is_hidden());
    drop(second);
    assert!(!x.overlay.is_hidden());
    assert!(x.overlay_capture_scope(43).is_ok());
}
#[test]
fn overlay_visible_without_composite_refuses_unverifiable_root_pixels() {
    use gcu_driver_linux::x11::{FakeWindow, RawImage, XWindowGeometry};
    let x = FakeX::new(
        XRootGeometry {
            root_id: 1,
            width: 2,
            height: 2,
            scale: 1.,
        },
        false,
    );
    x.add_window(FakeWindow {
        id: 42,
        name: Some("fixture".into()),
        pid: Some(10),
        geometry: XWindowGeometry {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
            border_width: 0,
            depth: 24,
        },
        is_mapped: true,
        image: None,
    });
    x.set_root_image(RawImage {
        width: 2,
        height: 2,
        data: vec![255; 16],
    });
    assert!(capture_window(&x, 42).is_ok());
    x.overlay.register(99);
    assert!(capture_window(&x, 42).is_err());
    assert_eq!(x.overlay.transitions(), [true, false, true, false]);
}
#[test]
fn wayland_even_with_xwayland_reports_overlay_unavailable_and_reason() {
    for display in [false, true] {
        let mut env = MockEnv::new()
            .set("WAYLAND_DISPLAY", "wayland-0")
            .set("XDG_SESSION_TYPE", "wayland");
        if display {
            env = env.set("DISPLAY", ":0");
        }
        let env = detect_environment(&env);
        let portal = PortalInfo::default();
        let target = if display {
            TargetEnvironment::XWaylandWindow
        } else {
            TargetEnvironment::NativeWayland
        };
        let caps =
            build_environment_capabilities(&env, target, &portal, true, true, true, true, false);
        assert_eq!(caps.overlay.status, CapabilityStatus::Unavailable);
        assert!(caps
            .overlay
            .reason
            .contains("physical input provenance and independent Stop"));
        assert_eq!(
            caps.to_protocol_capabilities().private_apis.get("overlay"),
            Some(&PrivateApiStatus::Disabled)
        );
        let doctor = build_doctor_report(&env, &caps, &portal, true, None);
        assert!(doctor.issues.contains(&caps.overlay.reason));
        assert!(!doctor.capabilities.overlay_excluded_from_capture);
    }
}

#[test]
fn headless_overlay_is_unavailable_and_pure_x11_requires_native_probe() {
    for (env, expected) in [
        (MockEnv::new(), CapabilityStatus::Unavailable),
        (
            MockEnv::new().set("DISPLAY", ":0"),
            CapabilityStatus::Partial,
        ),
    ] {
        let env = detect_environment(&env);
        let caps = build_environment_capabilities(
            &env,
            TargetEnvironment::PureX11,
            &PortalInfo::default(),
            true,
            true,
            true,
            true,
            false,
        );
        assert_eq!(caps.overlay.status, expected);
    }
}
