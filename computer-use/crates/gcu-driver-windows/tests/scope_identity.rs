//! Desktop-free scope identity. Real HWND reuse and capture occlusion stay not_run.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;

use gcu_core::{
    driver::{
        AppRef, Button, Cancel, CaptureOpts, Driver, DriverAction, DriverError, PointerRoute,
        WindowRef,
    },
    model::{NativeHandle, WindowPoint},
    scope::{InstanceEvidence, SCOPE_DENIED_MESSAGE},
};
use gcu_driver_windows::{
    capture::{CaptureBackend, CapturePath, FakeCapture, Paint},
    desktop::HwndStamp,
    input::{fake::FakeInput, InputBackend},
    testing::{window, FakeWinApi},
    uia::{
        actions,
        cache::{Patterns, Properties},
        source::{FakeNode, FakeUia, NodeId, UiaSource},
        UiaElement,
    },
    WindowsDriver,
};
use gcu_protocol::AppInfo;
use std::{sync::Arc, time::Duration};

fn denied(error: DriverError) {
    assert_eq!(error, DriverError::ScopeDenied);
    assert_eq!(error.to_string(), SCOPE_DENIED_MESSAGE);
    assert!(!error.to_string().to_lowercase().contains("editor"));
    assert!(!error.maybe_executed());
}

#[tokio::test]
async fn proved_window_still_returns_geometry() {
    let (_, driver, _, win) = common::fixture().await;
    assert!(driver.window_geometry(&win).await.is_ok());
}

#[tokio::test]
async fn evidence_ignores_path_and_rejects_pid_reuse_hwnd_reuse_and_a_forged_id() {
    let (api, driver, app, win) = common::fixture().await;
    let first = driver.app_instance(&app).expect("proved process");
    let handle = NativeHandle::new(UiaElement {
        window: win.clone(),
        node: NodeId {
            generation: 1,
            slot: 1,
        },
        hwnd: 8,
        runtime_id: vec![1],
        fingerprint: 0,
    });
    assert!(driver.element_binding(&handle).is_none());
    let crossed = NativeHandle::new(UiaElement {
        window: win.clone(),
        node: NodeId {
            generation: 1,
            slot: 1,
        },
        hwnd: 9,
        runtime_id: vec![1],
        fingerprint: 0,
    });
    assert!(driver.element_binding(&crossed).is_none());
    assert!(driver.element_binding(&NativeHandle::new(())).is_none());
    let mut forged = win.clone();
    forged.id = "8".into();
    assert!(driver.window_binding(&forged).is_none());

    api.update(|state| {
        state.paths.insert(7, "C:\\Impostor.exe".into());
    });
    assert_eq!(driver.app_instance(&app), Some(first.clone()));
    assert!(driver.window_binding(&win).is_some());

    api.update(|state| state.processes.get_mut(&7).unwrap().created += 1);
    let reused = driver.app_instance(&app).expect("live pid");
    assert_eq!(reused.pid(), first.pid());
    assert_ne!(reused.start().ticks(), first.start().ticks());
    assert!(driver.window_binding(&win).is_none());

    api.update(|state| {
        state.processes.get_mut(&7).unwrap().created -= 1;
        state.windows[0].created += 1;
    });
    assert!(driver.window_binding(&win).is_none());
    assert_eq!(
        driver
            .window_geometry(&win)
            .await
            .expect_err("product gate"),
        DriverError::WindowGone
    );
}

fn clear_creation_time(api: &FakeWinApi) {
    api.update(|state| {
        state.processes.get_mut(&7).unwrap().created = 0;
        state.windows[0].process_created = 0;
    });
}

fn painted(driver: WindowsDriver) -> WindowsDriver {
    let wgc = Arc::new(FakeCapture::new(CapturePath::Wgc));
    let print = Arc::new(FakeCapture::new(CapturePath::PrintWindow));
    wgc.paint(
        8,
        Paint {
            own: [1, 2, 3, 255],
            cover: [9, 9, 9, 255],
            occluded: true,
            mark: None,
        },
    );
    driver.with_capture(Arc::new(CaptureBackend::new(wgc, print)))
}

async fn unproved_window(api: &FakeWinApi, driver: &WindowsDriver, app: AppRef) -> WindowRef {
    clear_creation_time(api);
    let listed = driver.list_windows(Some(&app)).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(
        listed[0].id.starts_with("8:7:0:63:"),
        "hwnd 8, pid 7, creation time 0, generation 99"
    );
    WindowRef {
        app,
        id: listed[0].id.clone(),
    }
}

#[tokio::test]
async fn unproved_creation_time_does_not_capture_or_return_geometry() {
    let (api, driver, app, _) = common::fixture().await;
    driver.note_scoped_session();
    let win = unproved_window(&api, &driver, app).await;
    let driver = painted(driver);
    denied(
        driver
            .capture(&win, &CaptureOpts::default(), &Cancel::default())
            .await
            .unwrap_err(),
    );
    let stats = driver.capture_stats().unwrap();
    assert_eq!(stats.wgc_attempts, 0);
    assert_eq!(stats.print_window_attempts, 0);
    assert_eq!(stats.published, 0);
    denied(driver.window_geometry(&win).await.unwrap_err());
}

#[tokio::test]
async fn unscoped_unproved_creation_time_returns_geometry_and_capture() {
    let (api, driver, app, _) = common::fixture().await;
    let win = unproved_window(&api, &driver, app).await;
    let driver = painted(driver);
    let capture = driver
        .capture(&win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(capture.width, 800);
    assert_eq!(capture.height, 600);
    assert_eq!(capture.mime, "image/png");
    assert!(!capture.bytes.is_empty());
    assert_eq!(capture.geometry.frame.w, 800.0);
    assert_eq!(capture.geometry.frame.h, 600.0);
    assert_eq!(capture.geometry.scale, 1.0);
    let stats = driver.capture_stats().unwrap();
    assert_eq!(stats.wgc_attempts, 1);
    assert_eq!(stats.print_window_attempts, 0);
    assert_eq!(stats.published, 1);
    let geometry = driver.window_geometry(&win).await.unwrap();
    assert_eq!(geometry.frame, capture.geometry.frame);
    assert_eq!(geometry.scale, capture.geometry.scale);
}

#[tokio::test]
async fn unscoped_empty_boot_keeps_geometry_and_a_targeted_click() {
    let (api, driver, _, win) = common::fixture().await;
    let binding = driver.window_binding(&win).expect("proved window");
    driver.pin_admitted(Some(&binding));
    api.update(|state| state.boot.clear());
    assert!(driver.window_geometry(&win).await.is_ok());
    assert!(driver.app_instance(&win.app).is_none());
    let input = Arc::new(FakeInput::default());
    let driver = driver
        .with_input(Arc::new(InputBackend::new(input.clone())))
        .with_safety(common::fake_guard());
    driver
        .perform(
            &DriverAction::PointerClick {
                win,
                at: WindowPoint { x: 10., y: 10. },
                button: Button::Left,
                count: 1,
                route: PointerRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await
        .unwrap();
    assert!(!input.state().sent.is_empty());
}

#[tokio::test]
async fn scoped_empty_boot_refuses_geometry_and_capture() {
    let (api, driver, _, win) = common::fixture().await;
    let binding = driver.window_binding(&win).expect("proved window");
    driver.note_scoped_session();
    driver.pin_admitted(Some(&binding));
    api.update(|state| state.boot.clear());
    denied(driver.window_geometry(&win).await.unwrap_err());
    let wgc = Arc::new(FakeCapture::new(CapturePath::Wgc));
    let print = Arc::new(FakeCapture::new(CapturePath::PrintWindow));
    let driver = driver.with_capture(Arc::new(CaptureBackend::new(wgc, print)));
    denied(
        driver
            .capture(&win, &CaptureOpts::default(), &Cancel::default())
            .await
            .unwrap_err(),
    );
    let stats = driver.capture_stats().unwrap();
    assert_eq!(stats.wgc_attempts, 0);
    assert_eq!(stats.print_window_attempts, 0);
    assert_eq!(stats.published, 0);
}

#[tokio::test]
async fn zero_creation_time_is_not_sent() {
    let (api, driver, _, win) = common::fixture().await;
    let input = Arc::new(FakeInput::default());
    let driver = driver
        .with_input(Arc::new(InputBackend::new(input.clone())))
        .with_safety(common::fake_guard());
    api.update(|state| {
        state.processes.get_mut(&7).unwrap().created = 0;
        state.windows[0].process_created = 0;
    });
    let listed = driver.list_windows(Some(&win.app)).await.unwrap();
    let target = WindowRef {
        app: win.app.clone(),
        id: listed[0].id.clone(),
    };
    denied(
        driver
            .perform(
                &DriverAction::PointerClick {
                    win: target,
                    at: WindowPoint { x: 10., y: 10. },
                    button: Button::Left,
                    count: 1,
                    route: PointerRoute::TargetedOnly,
                },
                &Cancel::default(),
            )
            .await
            .unwrap_err(),
    );
    assert!(input.state().sent.is_empty());
    assert_eq!(input.state().activations, 0);
}

#[tokio::test]
async fn direct_input_refuses_unproved_scalars_before_send() {
    let input = Arc::new(FakeInput::default());
    let backend = InputBackend::new(input.clone());
    let mut native = window(8, 7);
    native.process_created = 0;
    let win = WindowRef {
        app: AppRef {
            info: AppInfo {
                id: "fixture".into(),
                name: "Fixture".into(),
                pid: Some(7),
                identity: None,
            },
        },
        id: "8".into(),
    };
    denied(
        backend
            .perform(
                native,
                &DriverAction::PointerClick {
                    win,
                    at: WindowPoint { x: 10., y: 10. },
                    button: Button::Left,
                    count: 1,
                    route: PointerRoute::TargetedOnly,
                },
                &Cancel::default(),
                tokio::time::Instant::now() + Duration::from_secs(2),
            )
            .await
            .unwrap_err(),
    );
    assert!(input.state().sent.is_empty());
}

fn pressable() -> Properties {
    Properties {
        patterns: Patterns {
            invoke: true,
            ..Patterns::default()
        },
        ..Properties::default()
    }
}

async fn element(hwnd: u64, id: &str) -> (FakeUia, NativeHandle) {
    let source = FakeUia::new(FakeNode {
        properties: pressable(),
        ..FakeNode::default()
    });
    let node = source.root(8).await.unwrap().unwrap();
    let props = source.properties(node).await.unwrap();
    let handle = NativeHandle::new(UiaElement {
        window: WindowRef {
            app: AppRef {
                info: AppInfo {
                    id: "fixture".into(),
                    name: "Fixture".into(),
                    pid: Some(7),
                    identity: None,
                },
            },
            id: id.into(),
        },
        node,
        hwnd,
        runtime_id: props.runtime_id,
        fingerprint: 0,
    });
    (source, handle)
}

#[tokio::test]
async fn unscoped_uia_invoke_allows_a_foreign_owner_and_a_missing_probe() {
    let (source, handle) = element(8, "8").await;
    source.set_owner_pid(Some(7));
    assert!(actions::perform(
        &source,
        8,
        &DriverAction::Press { el: handle },
        &Cancel::default(),
    )
    .await
    .is_ok());
    assert_eq!(source.action_calls().len(), 1);

    let (source, handle) = element(8, "8").await;
    source.set_owner_pid(Some(99));
    actions::perform(
        &source,
        8,
        &DriverAction::Press { el: handle },
        &Cancel::default(),
    )
    .await
    .unwrap();
    assert_eq!(source.action_calls().len(), 1);

    let (source, handle) = element(8, "8").await;
    actions::perform(
        &source,
        8,
        &DriverAction::Press { el: handle },
        &Cancel::default(),
    )
    .await
    .unwrap();
    assert_eq!(source.action_calls().len(), 1);

    let (source, handle) = element(9, "8").await;
    source.set_owner_pid(Some(7));
    actions::perform(
        &source,
        8,
        &DriverAction::Press { el: handle },
        &Cancel::default(),
    )
    .await
    .unwrap();
    assert_eq!(source.action_calls().len(), 1);

    let foreign = HwndStamp::from(&window(9, 7)).id();
    let (source, handle) = element(8, &foreign).await;
    source.set_owner_pid(Some(7));
    actions::perform(
        &source,
        8,
        &DriverAction::Press { el: handle },
        &Cancel::default(),
    )
    .await
    .unwrap();
    assert_eq!(source.action_calls().len(), 1);
}
