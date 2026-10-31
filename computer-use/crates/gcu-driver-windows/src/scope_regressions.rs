//! Production-path regressions for scoped Windows admission.
//!
//! Each test calls a driver, input, or UIA function. The claims come from
//! discovery or from the scope value that function already carries.
use crate::{
    capture::{CaptureBackend, CapturePath, FakeCapture, Paint},
    input::{
        api::Event,
        fake::FakeInput,
        hotkey::{FakeSafetyApi, StopLatch},
        InputBackend,
    },
    scope_identity::{JobScope, ProcessClaim, WindowClaim},
    testing::{medium_security, signed_image, window, FakeWinApi},
    uia::{
        actions,
        cache::{Patterns, Properties},
        events::FakeUiaEvents,
        source::{FakeNode, FakeUia},
        UiaElement,
    },
    WindowsDriver,
};
use gcu_core::{
    driver::{
        AppQuery, AppRef, Button, Cancel, CaptureOpts, Driver, DriverAction, DriverError,
        PointerRoute, TreeOpts, WindowRef,
    },
    model::{NativeHandle, RawNode, WindowPoint},
    scope::{
        ExecutableProof, FixtureScope, FixtureTarget, InstanceEvidence, RegisteredWindow,
        ScopeLease, ScopedDriver,
    },
};
use gcu_host_api::{testing::FakeUserActivity, UserActivity};
use gcu_protocol::AppInfo;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

struct TestStop(AtomicBool);
impl StopLatch for TestStop {
    fn is_stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    fn request_stop(&self) {
        self.0.store(true, Ordering::SeqCst)
    }
}

fn guard() -> Arc<crate::input::hotkey::SafetyGuard> {
    FakeSafetyApi::default().start(
        Default::default(),
        Arc::new(TestStop(AtomicBool::new(false))),
    )
}

async fn editor() -> (Arc<FakeWinApi>, WindowsDriver, AppRef, WindowRef) {
    let api = Arc::new(FakeWinApi::default());
    api.update(|state| {
        state.paths.insert(7, "C:\\Editor.exe".into());
        state
            .images
            .insert("C:\\Editor.exe".into(), signed_image("C:\\Editor.exe"));
        state.processes.insert(7, medium_security());
        state.windows.push(window(8, 7));
        state.desktops.insert(8, Some(true));
    });
    let driver = WindowsDriver::with_api(Arc::clone(&api)).await.unwrap();
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let info = driver.key_window(&app).await.unwrap();
    let win = WindowRef {
        app: app.clone(),
        id: info.id,
    };
    (api, driver, app, win)
}

fn canary_source() -> FakeUia {
    let source = FakeUia::new(FakeNode {
        properties: Properties {
            name: Some("Editor".into()),
            ..Properties::default()
        },
        children: vec![FakeNode {
            properties: Properties {
                name: Some("CANARY-FOREIGN".into()),
                ..Properties::default()
            },
            ..FakeNode::default()
        }],
    });
    source.set_fragment(1, 7, 8);
    source.set_fragment(2, 99, 8);
    source
}

fn contains(node: &RawNode, name: &str) -> bool {
    node.name.as_deref() == Some(name) || node.children.iter().any(|child| contains(child, name))
}

fn claim() -> WindowClaim {
    WindowClaim {
        hwnd: 8,
        incarnation: 99,
        owner: ProcessClaim {
            pid: 7,
            created: 42,
            boot: "boot-1".into(),
        },
    }
}

async fn press_handle(hwnd: u64) -> (FakeUia, NativeHandle) {
    let source = FakeUia::new(FakeNode {
        properties: Properties {
            patterns: Patterns {
                invoke: true,
                ..Patterns::default()
            },
            ..Properties::default()
        },
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
            id: "8".into(),
        },
        node,
        hwnd,
        runtime_id: props.runtime_id,
        fingerprint: 0,
    });
    (source, handle)
}

fn click(win: WindowRef, route: PointerRoute) -> DriverAction {
    DriverAction::PointerClick {
        win,
        at: WindowPoint { x: 10., y: 10. },
        button: Button::Left,
        count: 1,
        route,
    }
}

fn button_downs(input: &FakeInput) -> usize {
    input
        .state()
        .sent
        .iter()
        .filter(|sent| matches!(sent.event, Event::Button(_, true, _, _)))
        .count()
}

fn wheels(input: &FakeInput) -> usize {
    input
        .state()
        .sent
        .iter()
        .filter(|sent| matches!(sent.event, Event::Wheel(..)))
        .count()
}

fn replace_owner(api: &FakeWinApi) {
    api.update(|state| {
        state.processes.get_mut(&7).unwrap().created += 1;
        state.windows[0].process_created += 1;
    });
}

#[tokio::test]
async fn scoped_snapshot_omits_a_foreign_child_without_reading_it() {
    let (api, _, _, _) = editor().await;
    let source = canary_source();
    let driver = WindowsDriver::with_uia(
        api,
        Arc::new(source.clone()),
        Arc::new(FakeUiaEvents::default()),
    )
    .await
    .unwrap();
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let info = driver.key_window(&app).await.unwrap();
    let win = WindowRef { app, id: info.id };
    let binding = driver.window_binding(&win).unwrap();
    let target = FixtureTarget::new(
        binding.owner().clone(),
        "editor",
        ExecutableProof::new("C:\\Editor.exe").unwrap(),
        vec![RegisteredWindow::new(binding)],
    )
    .unwrap();
    let scope =
        FixtureScope::try_new("run", 1, "engine", "desktop", "lease", vec![target]).unwrap();
    let scoped = ScopedDriver::new(
        driver,
        ScopeLease::install(scope),
        Arc::new(FakeUserActivity::new()) as Arc<dyn UserActivity>,
    );
    let tree = scoped
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let root = tree.root.unwrap();
    assert!(contains(&root, "Editor"));
    assert!(!contains(&root, "CANARY-FOREIGN"));
    assert!(root.children.is_empty());
    assert_eq!(source.content_reads(2), 0);
    assert!(source.content_reads(1) >= 1);
}

#[tokio::test]
async fn scoped_click_does_not_sample_focus_or_post_a_foreign_child() {
    let (_, driver, _, win) = editor().await;
    let input = Arc::new(FakeInput::default());
    input.update(|state| {
        state.child_pid = 99;
        state.child_created = 5;
    });
    let driver = driver
        .with_input(Arc::new(InputBackend::new(Arc::clone(&input))))
        .with_safety(guard());
    driver.note_scoped_session();
    let binding = driver.window_binding(&win).unwrap();
    driver.pin_admitted(Some(&binding));
    let error = driver
        .perform(
            &click(win.clone(), PointerRoute::TargetedOnly),
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::ScopeDenied);
    assert!(!error.maybe_executed());
    assert_eq!(input.state().samples, 0);
    assert_eq!(input.state().focuses, 0);
    assert!(input.state().sent.is_empty());
    input.update(|state| {
        state.child_pid = 0;
        state.child_created = 0;
    });
    driver
        .perform(&click(win, PointerRoute::TargetedOnly), &Cancel::default())
        .await
        .unwrap();
    assert!(!input.state().sent.is_empty());
}

#[tokio::test]
async fn unscoped_click_still_posts_a_foreign_child() {
    let (_, driver, _, win) = editor().await;
    let input = Arc::new(FakeInput::default());
    input.update(|state| {
        state.child_pid = 99;
        state.child_created = 5;
    });
    let driver = driver
        .with_input(Arc::new(InputBackend::new(Arc::clone(&input))))
        .with_safety(guard());
    driver
        .perform(&click(win, PointerRoute::TargetedOnly), &Cancel::default())
        .await
        .unwrap();
    assert!(!input.state().sent.is_empty());
}

#[tokio::test]
async fn scoped_foreground_refuses_an_overlay_and_a_drag_that_crosses_another_window() {
    let (_, driver, _, win) = editor().await;
    let input = Arc::new(FakeInput::default());
    input.update(|state| state.hit_receiver = Some(77));
    let driver = driver
        .with_input(Arc::new(InputBackend::new(Arc::clone(&input))))
        .with_safety(guard());
    driver.note_scoped_session();
    let binding = driver.window_binding(&win).unwrap();
    driver.pin_admitted(Some(&binding));
    let error = driver
        .perform(
            &click(win.clone(), PointerRoute::AllowForeground),
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::ScopeDenied);
    assert_eq!(button_downs(&input), 0);
    assert_eq!(wheels(&input), 0);

    input.update(|state| {
        state.hit_receiver = Some(8);
        state.hit_at = vec![(20.0, 10.0, 77)];
        state.sent.clear();
    });
    let error = driver
        .perform(
            &DriverAction::PointerDrag {
                win: win.clone(),
                path: vec![
                    WindowPoint { x: 10., y: 10. },
                    WindowPoint { x: 30., y: 10. },
                ],
                route: PointerRoute::AllowForeground,
            },
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::ScopeDenied);
    assert_eq!(button_downs(&input), 0);
    assert_eq!(wheels(&input), 0);

    input.update(|state| {
        state.hit_at.clear();
        state.hit_receiver = None;
        state.capture_hwnd = Some(77);
        state.sent.clear();
    });
    let error = driver
        .perform(
            &click(win.clone(), PointerRoute::AllowForeground),
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::ScopeDenied);
    assert_eq!(button_downs(&input), 0);

    input.update(|state| {
        state.capture_hwnd = None;
        state.hit_receiver = Some(8);
        state.sent.clear();
    });
    driver
        .perform(
            &click(win, PointerRoute::AllowForeground),
            &Cancel::default(),
        )
        .await
        .unwrap();
    assert!(button_downs(&input) >= 1);
}

#[tokio::test]
async fn replacement_after_admission_does_not_enter_uia_or_capture() {
    let (api, _, _, _) = editor().await;
    let source = canary_source();
    let driver = WindowsDriver::with_uia(
        Arc::clone(&api),
        Arc::new(source.clone()),
        Arc::new(FakeUiaEvents::default()),
    )
    .await
    .unwrap();
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let info = driver.key_window(&app).await.unwrap();
    let win = WindowRef { app, id: info.id };
    let wgc = Arc::new(FakeCapture::new(CapturePath::Wgc));
    let print = Arc::new(FakeCapture::new(CapturePath::PrintWindow));
    wgc.paint(
        8,
        Paint {
            own: [1, 2, 3, 255],
            cover: [9, 9, 9, 255],
            occluded: false,
            mark: None,
        },
    );
    let driver = driver.with_capture(Arc::new(CaptureBackend::new(wgc, print)));
    driver.note_scoped_session();
    let barrier_api = Arc::clone(&api);
    driver.set_native_barrier(Some(Arc::new(move || replace_owner(&barrier_api))));
    assert!(driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .is_err());
    assert_eq!(source.content_reads(1), 0);
    assert_eq!(source.content_reads(2), 0);

    let (api, driver, _, win) = editor().await;
    let wgc = Arc::new(FakeCapture::new(CapturePath::Wgc));
    let print = Arc::new(FakeCapture::new(CapturePath::PrintWindow));
    wgc.paint(
        8,
        Paint {
            own: [1, 2, 3, 255],
            cover: [9, 9, 9, 255],
            occluded: false,
            mark: None,
        },
    );
    let driver = driver.with_capture(Arc::new(CaptureBackend::new(
        Arc::clone(&wgc) as Arc<dyn crate::capture::CaptureSource>,
        print,
    )));
    driver.note_scoped_session();
    let barrier_api = Arc::clone(&api);
    driver.set_native_barrier(Some(Arc::new(move || replace_owner(&barrier_api))));
    assert!(driver
        .capture(&win, &CaptureOpts::default(), &Cancel::default())
        .await
        .is_err());
    let stats = driver.capture_stats().unwrap();
    assert_eq!(stats.wgc_attempts, 0);
    assert_eq!(stats.print_window_attempts, 0);
    assert_eq!(stats.published, 0);
}

#[tokio::test]
async fn unscoped_uia_pattern_dispatch_is_unchanged() {
    let (source, handle) = press_handle(8).await;
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

    let (source, handle) = press_handle(8).await;
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

#[tokio::test]
async fn scoped_uia_pattern_dispatch_requires_the_element_owner() {
    let scope = JobScope::Scoped(claim());
    let (source, handle) = press_handle(8).await;
    source.set_owner_pid(Some(99));
    let error = actions::perform_scoped(
        &source,
        8,
        &DriverAction::Press { el: handle },
        &Cancel::default(),
        &scope,
    )
    .await
    .unwrap_err();
    assert_eq!(error, DriverError::ScopeDenied);
    assert!(source.action_calls().is_empty());

    let (source, handle) = press_handle(8).await;
    let error = actions::perform_scoped(
        &source,
        8,
        &DriverAction::Press { el: handle },
        &Cancel::default(),
        &scope,
    )
    .await
    .unwrap_err();
    assert_eq!(error, DriverError::ScopeDenied);
    assert!(source.action_calls().is_empty());

    let (source, handle) = press_handle(9).await;
    source.set_owner_pid(Some(7));
    let error = actions::perform_scoped(
        &source,
        8,
        &DriverAction::Press { el: handle },
        &Cancel::default(),
        &scope,
    )
    .await
    .unwrap_err();
    assert_eq!(error, DriverError::ScopeDenied);
    assert!(source.action_calls().is_empty());

    let (source, handle) = press_handle(8).await;
    source.set_owner_pid(Some(7));
    let error = actions::perform_scoped(
        &source,
        8,
        &DriverAction::Press { el: handle.clone() },
        &Cancel::default(),
        &JobScope::Missing,
    )
    .await
    .unwrap_err();
    assert_eq!(error, DriverError::ScopeDenied);
    assert!(source.action_calls().is_empty());
    actions::perform_scoped(
        &source,
        8,
        &DriverAction::Press { el: handle },
        &Cancel::default(),
        &scope,
    )
    .await
    .unwrap();
    assert_eq!(source.action_calls().len(), 1);
}
