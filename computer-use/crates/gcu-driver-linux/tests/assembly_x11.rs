//! Linux X11 assembly: Host latch rebind, missing AT-SPI, and XTEST gates.
//!
//! FakeX and an in-memory tree prove the virtual path only. Physical Stop
//! timing, XGrabKey conflicts against another client, and XI2 versus injected
//! input need a real X11 desktop and stay `not_run`.

#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_core::driver::{
    Cancel, Capture, CaptureOpts, Driver, DriverAction, DriverError, PointerRoute, TreeOpts,
    WindowRef,
};
use gcu_core::model::{Geometry, Rect, WindowPoint};
use gcu_core::redact::{self, CapturedFrame};
use gcu_core::tree::Pipeline;
use gcu_driver_linux::{
    atspi::source::{FakeNode, FakeSource},
    x11::{
        fake::FakeWindow,
        hotkey::{SharedStopLatch, StopLatch},
        server::XEventKind,
        FakeX, XRootGeometry, XWindowGeometry,
    },
    LinuxDriver,
};
use gcu_protocol::{
    capabilities::{ForegroundCapability, PrivateApiStatus},
    tools::MouseButton as Button,
};
use std::{sync::Arc, time::Duration};

fn root() -> XRootGeometry {
    XRootGeometry {
        root_id: 1,
        width: 1920,
        height: 1080,
        scale: 1.0,
    }
}

fn window(id: u32, x: i32, y: i32) -> FakeWindow {
    FakeWindow {
        id,
        name: Some(format!("win-{id}")),
        pid: Some(100),
        geometry: XWindowGeometry {
            x,
            y,
            width: 200,
            height: 100,
            border_width: 0,
            depth: 24,
        },
        is_mapped: true,
        image: None,
    }
}

fn window_ref(id: u32) -> WindowRef {
    WindowRef {
        app: gcu_core::driver::AppRef {
            info: gcu_protocol::AppInfo {
                id: "test-app".into(),
                name: "test-app".into(),
                pid: Some(100),
                identity: None,
            },
        },
        id: id.to_string(),
    }
}

fn click(id: u32, route: PointerRoute) -> DriverAction {
    DriverAction::PointerClick {
        win: window_ref(id),
        at: WindowPoint { x: 10.0, y: 10.0 },
        button: Button::Left,
        count: 1,
        route,
    }
}

#[tokio::test]
async fn host_latch_rebind_stops_the_new_latch_not_the_original() {
    let server = Arc::new(FakeX::new(root(), true));
    server.add_window(window(100, 0, 0));
    server.set_active_window(Some(100));
    let driver = LinuxDriver::new_fake(FakeSource::default(), FakeNode::default(), vec![], vec![])
        .with_x_server(server.clone());
    let original = driver.stop_latch().clone();
    let host: Arc<dyn StopLatch> = Arc::new(SharedStopLatch::default());
    let driver = driver.with_stop_latch(host.clone());
    tokio::time::sleep(Duration::from_millis(40)).await;

    let keycode = driver.hotkey_manager().resolved_keycode().expect("grab");
    let modifiers = driver.hotkey_manager().config().modifiers;
    server.push_event(XEventKind::KeyPress(keycode, modifiers));

    let mut stopped = false;
    for _ in 0..50 {
        if host.is_stopped() {
            stopped = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(stopped, "listener must stop the latch it was rebound to");
    assert!(!original.is_stopped());
    assert!(driver.is_stopped());
    assert!(Arc::ptr_eq(driver.stop_latch(), &host));
    assert!(Arc::ptr_eq(driver.hotkey_manager().stop_latch(), &host));
}

#[tokio::test]
async fn missing_atspi_is_not_ready_and_refuses_coordinate_input() {
    let server = Arc::new(FakeX::new(root(), true));
    server.add_window(window(100, 0, 0));
    server.set_active_window(Some(100));
    let driver = LinuxDriver::offline_x11(server.clone())
        .with_stop_latch(Arc::new(SharedStopLatch::default()));
    let report = driver.doctor().await;
    assert!(!report.ready);
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.contains("AT-SPI bus not connected")));
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.contains("not a fallback")));
    let capabilities = driver.capabilities();
    assert!(!capabilities.observe.tree);
    assert!(!capabilities.observe.events);
    assert!(!capabilities.observe.screenshot);
    assert!(!capabilities.delivery.accessibility);
    assert_eq!(
        capabilities.delivery.foreground,
        ForegroundCapability::Unavailable
    );
    assert_eq!(
        capabilities.private_apis.get("xtest"),
        Some(&PrivateApiStatus::Available)
    );

    let cancel = Cancel::default();
    let foreground = driver
        .perform(&click(100, PointerRoute::AllowForeground), &cancel)
        .await;
    let targeted = driver
        .perform(&click(100, PointerRoute::TargetedOnly), &cancel)
        .await;
    assert_eq!(foreground.unwrap_err(), DriverError::CapabilityUnavailable);
    assert_eq!(targeted.unwrap_err(), DriverError::CapabilityUnavailable);
    assert!(server.get_events().is_empty());
}

#[tokio::test]
async fn xtest_foreground_gate_does_not_upgrade_targeted_input() {
    let server = Arc::new(FakeX::new(root(), true));
    server.add_window(window(100, 0, 0));
    server.set_active_window(Some(100));
    let driver = LinuxDriver::new_fake(FakeSource::default(), FakeNode::default(), vec![], vec![])
        .with_x_server(server.clone())
        .with_stop_latch(Arc::new(SharedStopLatch::default()));
    assert!(driver.capabilities().observe.tree);
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Policy
    );
    assert_eq!(
        driver.capabilities().private_apis.get("x11_composite"),
        Some(&PrivateApiStatus::Available)
    );

    let cancel = Cancel::default();
    let targeted = driver
        .perform(&click(100, PointerRoute::TargetedOnly), &cancel)
        .await;
    assert_eq!(targeted.unwrap_err(), DriverError::CapabilityUnavailable);
    assert!(server.get_events().is_empty());

    let foreground = driver
        .perform(&click(100, PointerRoute::AllowForeground), &cancel)
        .await;
    assert!(foreground.is_ok(), "{foreground:?}");
    assert!(!server.get_events().is_empty());
}

#[tokio::test]
async fn partial_tree_rejects_image_delivery() {
    let driver = LinuxDriver::new_fake(FakeSource::default(), FakeNode::default(), vec![], vec![]);
    let opts = TreeOpts {
        max_tree_nodes: 0,
        ..TreeOpts::default()
    };
    let raw = driver
        .snapshot_tree(&window_ref(1), &opts, &Cancel::default())
        .await
        .expect("zero budget is an incomplete tree, not an error");
    assert!(raw.incomplete.is_some());
    let mut tree = Pipeline::default().run(raw, &opts);
    tree.state_version = Some(1);
    tree.observed_at = Some(Duration::ZERO);
    let capture = Capture {
        bytes: vec![0, 0, 0, 255],
        width: 1,
        height: 1,
        mime: "image/raw".into(),
        geometry: Geometry {
            frame: Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            scale: 1.0,
        },
    };
    let error = redact::apply(&tree, CapturedFrame::new(capture, 1, Duration::ZERO))
        .expect_err("incomplete tree must not return pixels");
    assert!(matches!(error, redact::RedactionError::NeedsHandoff));
}

#[tokio::test]
async fn occluded_window_without_composite_rejects_capture() {
    let server = Arc::new(FakeX::new(root(), false));
    server.add_window(window(100, 0, 0));
    server.add_window(window(200, 20, 20));
    server.set_stacking_order(vec![100, 200]);
    let driver = LinuxDriver::offline_x11(server);
    assert_eq!(
        driver.capabilities().private_apis.get("x11_composite"),
        Some(&PrivateApiStatus::Missing)
    );
    let error = driver
        .capture(
            &window_ref(100),
            &CaptureOpts::default(),
            &Cancel::default(),
        )
        .await
        .expect_err("occlusion without composite is not a root crop");
    assert_eq!(error, DriverError::CapabilityUnavailable);
}

#[tokio::test]
#[ignore = "not_run: needs a physical X11 desktop, not Xvfb or FakeX"]
async fn physical_stop_timing_xgrabkey_conflict_and_xi2_need_a_real_desktop() {
    // A second X client must already own the grab, a physical keyboard must
    // press Ctrl+Alt+Shift+BackSpace, and XI2 must distinguish that device
    // from XTEST. FakeX and Xvfb do not provide those conditions.
    panic!("not_run: physical X11 desktop was not available");
}
