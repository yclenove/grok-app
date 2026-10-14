//! Portable targeted-event tests. Live macOS cases are ignored (`not_run`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use gcu_core::driver::{
    AppQuery, Button, Cancel, Delivery, Driver, DriverAction, DriverError, KeyChord, KeyRoute,
    PointerRoute, WindowRef, DRIVER_DEADLINE,
};
use gcu_core::model::WindowPoint;
use gcu_driver_macos::api::MacApi;
use gcu_driver_macos::input::{
    classify_skylight, plan_click, utf16_chunks, Call, EventKind, FakeEvents, InputTarget,
    PostFault, PostPath, Probe, FIELD_WINDOW_NUMBER,
};
use gcu_driver_macos::testing::{
    ax_window, cg_window, rect, running_app, signed_facts, FakeMacApi,
};
use gcu_driver_macos::MacosDriver;
use gcu_protocol::capabilities::PrivateApiStatus;

fn target(bundle: &str, name: &str) -> InputTarget {
    InputTarget {
        pid: 4,
        window_id: 7,
        bundle_id: bundle.into(),
        app_name: name.into(),
        origin_x: 10.0,
        origin_y: 20.0,
        width_px: 160.0,
        height_px: 80.0,
        scale: 2.0,
    }
}

fn click(route: PointerRoute) -> DriverAction {
    DriverAction::PointerClick {
        win: WindowRef {
            app: gcu_core::driver::AppRef {
                info: gcu_protocol::AppInfo {
                    id: "com.example.Editor".into(),
                    name: "Editor".into(),
                    pid: Some(4),
                    identity: None,
                },
            },
            id: "cg:7:4".into(),
        },
        at: WindowPoint { x: 4.0, y: 6.0 },
        button: Button::Left,
        count: 1,
        route,
    }
}

async fn run(
    fake: &Arc<FakeEvents>,
    bundle: &str,
    name: &str,
    action: &DriverAction,
    cancel: &Cancel,
) -> Result<Delivery, DriverError> {
    gcu_driver_macos::input::deliver(
        Arc::clone(fake) as Arc<dyn gcu_driver_macos::input::EventApi>,
        Arc::new(gcu_driver_macos::input::InputLedger::new()),
        &target(bundle, name),
        action,
        cancel,
        DRIVER_DEADLINE,
    )
    .await
}

#[test]
fn click_plan_sets_field_51_and_window_local_point() {
    let events = plan_click(
        &target("com.example.Editor", "Editor"),
        WindowPoint { x: 4.0, y: 6.0 },
        Button::Left,
        1,
        PostPath::ToPid,
    )
    .unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].field_51, Some(i64::from(7)));
    assert_eq!(FIELD_WINDOW_NUMBER, 51);
    assert_eq!(events[0].window_local, Some((2.0, 3.0)));
    assert!(events[0].screen.is_none());
    assert_eq!(events[0].kind, EventKind::MouseDown);
    assert!(!events[1].down);
    let hid = plan_click(
        &target("com.example.Editor", "Editor"),
        WindowPoint { x: 4.0, y: 6.0 },
        Button::Right,
        1,
        PostPath::HidTap,
    )
    .unwrap();
    assert!(hid[0].field_51.is_none());
    assert!(hid[0].window_local.is_none());
    assert_eq!(hid[0].screen, Some((12.0, 23.0)));
}

#[test]
fn text_chunks_keep_surrogate_pairs_together() {
    let text = format!("{}😀", "a".repeat(19));
    let chunks = utf16_chunks(&text);
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].chars().count(), 19);
    assert_eq!(chunks[1], "😀");
}

#[test]
fn skylight_classify_degrades_when_symbols_or_the_feature_are_absent() {
    assert_eq!(
        classify_skylight(true, false, false),
        PrivateApiStatus::Missing
    );
    assert_eq!(
        classify_skylight(false, false, true),
        PrivateApiStatus::Disabled
    );
    assert_eq!(
        classify_skylight(true, true, true),
        PrivateApiStatus::Disabled
    );
    assert_eq!(
        classify_skylight(true, false, true),
        PrivateApiStatus::Available
    );
}

#[tokio::test]
async fn targeted_click_posts_field_51_and_does_not_activate() {
    let fake = Arc::new(FakeEvents::ready());
    let delivery = run(
        &fake,
        "com.example.Editor",
        "Editor",
        &click(PointerRoute::TargetedOnly),
        &Cancel::default(),
    )
    .await
    .unwrap();
    assert_eq!(delivery, Delivery::TargetedEvent);
    assert_eq!(fake.frontmost(), Some(1));
    let calls = fake.calls();
    assert!(calls.iter().all(|call| !matches!(call, Call::Activate(_))));
    match &calls[0] {
        Call::Post(event) => {
            assert_eq!(event.field_51, Some(7));
            assert_eq!(event.window_local, Some((2.0, 3.0)));
            assert_eq!(event.path, PostPath::ToPid);
        }
        other => panic!("expected post, got {other:?}"),
    }
    assert!(calls.iter().any(|call| matches!(call, Call::Verify { .. })));
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, Call::Post(event) if event.kind == EventKind::MouseDown))
            .count(),
        1
    );
}

#[tokio::test]
async fn targeted_only_does_not_upgrade_when_the_probe_fails() {
    let mut probe = Probe::verified_os();
    probe.field_51 = false;
    let fake = Arc::new(FakeEvents::new(probe));
    let error = run(
        &fake,
        "com.example.Editor",
        "Editor",
        &click(PointerRoute::TargetedOnly),
        &Cancel::default(),
    )
    .await
    .unwrap_err();
    assert_eq!(error, DriverError::CapabilityUnavailable);
    assert!(!error.maybe_executed());
    assert!(fake.calls().is_empty());
    assert_eq!(fake.frontmost(), Some(1));
}

#[tokio::test]
async fn auto_matches_targeted_only_and_does_not_upgrade() {
    let mut probe = Probe::verified_os();
    probe.os_major = 15;
    probe.arrival_verified = false;
    let fake = Arc::new(FakeEvents::new(probe));
    let error = run(
        &fake,
        "com.example.Editor",
        "Editor",
        &click(PointerRoute::Auto),
        &Cancel::default(),
    )
    .await
    .unwrap_err();
    assert_eq!(error, DriverError::CapabilityUnavailable);
    assert!(fake.calls().is_empty());
}

#[tokio::test]
async fn older_os_background_click_runs_only_after_arrival_was_verified() {
    let mut probe = Probe::verified_os();
    probe.os_major = 14;
    probe.arrival_verified = true;
    let fake = Arc::new(FakeEvents::new(probe));
    let delivery = run(
        &fake,
        "com.example.Editor",
        "Editor",
        &click(PointerRoute::TargetedOnly),
        &Cancel::default(),
    )
    .await
    .unwrap();
    assert_eq!(delivery, Delivery::TargetedEvent);
}

#[tokio::test]
async fn chromium_targeted_click_is_unavailable_and_does_not_upgrade() {
    let fake = Arc::new(FakeEvents::ready());
    let error = run(
        &fake,
        "com.google.Chrome",
        "Google Chrome",
        &click(PointerRoute::Auto),
        &Cancel::default(),
    )
    .await
    .unwrap_err();
    assert_eq!(error, DriverError::CapabilityUnavailable);
    assert!(fake.calls().is_empty());
    assert_eq!(fake.frontmost(), Some(1));
}

#[tokio::test]
async fn chromium_foreground_is_labeled_foreground_and_uses_the_hid_tap() {
    let fake = Arc::new(FakeEvents::ready());
    let delivery = run(
        &fake,
        "com.google.Chrome",
        "Google Chrome",
        &click(PointerRoute::AllowForeground),
        &Cancel::default(),
    )
    .await
    .unwrap();
    assert_eq!(delivery, Delivery::Foreground);
    assert_eq!(fake.frontmost(), Some(4));
    let calls = fake.calls();
    assert!(matches!(calls[0], Call::Activate(4)));
    match &calls[1] {
        Call::Post(event) => {
            assert_eq!(event.path, PostPath::HidTap);
            assert!(event.field_51.is_none());
            assert_eq!(event.screen, Some((12.0, 23.0)));
        }
        other => panic!("expected hid post, got {other:?}"),
    }
    assert!(calls
        .iter()
        .all(|call| !matches!(call, Call::Verify { .. })));
}

#[tokio::test]
async fn allow_foreground_uses_the_hid_tap_when_the_probe_fails() {
    let mut probe = Probe::verified_os();
    probe.window_location_symbol = false;
    probe.field_51 = false;
    let fake = Arc::new(FakeEvents::new(probe));
    let delivery = run(
        &fake,
        "com.example.Editor",
        "Editor",
        &click(PointerRoute::AllowForeground),
        &Cancel::default(),
    )
    .await
    .unwrap();
    assert_eq!(delivery, Delivery::Foreground);
    assert!(fake
        .calls()
        .iter()
        .any(|call| matches!(call, Call::Activate(_))));
}

#[tokio::test]
async fn arrival_failure_is_unknown_and_is_not_retried_or_upgraded() {
    let fake = Arc::new(FakeEvents::ready());
    fake.fail_arrival();
    let error = run(
        &fake,
        "com.example.Editor",
        "Editor",
        &click(PointerRoute::TargetedOnly),
        &Cancel::default(),
    )
    .await
    .unwrap_err();
    assert!(error.maybe_executed());
    assert!(matches!(error, DriverError::Native { .. }));
    let downs = fake
        .calls()
        .iter()
        .filter(|call| matches!(call, Call::Post(event) if event.kind == EventKind::MouseDown))
        .count();
    assert_eq!(downs, 1);
    assert!(fake
        .calls()
        .iter()
        .all(|call| !matches!(call, Call::Activate(_))));
    let again = run(
        &fake,
        "com.example.Editor",
        "Editor",
        &click(PointerRoute::TargetedOnly),
        &Cancel::default(),
    )
    .await
    .unwrap_err();
    assert_eq!(again, DriverError::CapabilityUnavailable);
    assert!(!again.maybe_executed());
    let downs = fake
        .calls()
        .iter()
        .filter(|call| matches!(call, Call::Post(event) if event.kind == EventKind::MouseDown))
        .count();
    assert_eq!(downs, 1);
}

#[tokio::test]
async fn post_fault_releases_the_button_and_reports_unknown() {
    let fake = Arc::new(FakeEvents::ready());
    fake.fail_next(PostFault::Uncertain);
    let error = run(
        &fake,
        "com.example.Editor",
        "Editor",
        &click(PointerRoute::TargetedOnly),
        &Cancel::default(),
    )
    .await
    .unwrap_err();
    assert!(error.maybe_executed());
    let kinds: Vec<_> = fake
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Post(event) => Some(event.kind),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, vec![EventKind::MouseDown, EventKind::MouseUp]);
}

#[tokio::test]
async fn not_sent_before_delivery_is_not_executed_and_does_not_retry() {
    let fake = Arc::new(FakeEvents::ready());
    fake.fail_next(PostFault::NotSent);
    let error = run(
        &fake,
        "com.example.Editor",
        "Editor",
        &click(PointerRoute::TargetedOnly),
        &Cancel::default(),
    )
    .await
    .unwrap_err();
    assert!(!error.maybe_executed());
    let kinds: Vec<_> = fake
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Post(event) => Some(event.kind),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, vec![EventKind::MouseDown]);
}

#[tokio::test(start_paused = true)]
async fn cancel_during_post_releases_up_and_is_unknown() {
    let fake = Arc::new(FakeEvents::ready());
    fake.hold_posts();
    let cancel = Cancel::default();
    let fake2 = Arc::clone(&fake);
    let cancel2 = cancel.clone();
    let task = tokio::spawn(async move {
        run(
            &fake2,
            "com.example.Editor",
            "Editor",
            &click(PointerRoute::TargetedOnly),
            &cancel2,
        )
        .await
    });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_millis(1)).await;
    cancel.cancel();
    tokio::time::advance(DRIVER_DEADLINE).await;
    let error = task.await.unwrap().unwrap_err();
    assert_eq!(
        error,
        DriverError::Cancelled {
            maybe_executed: true
        }
    );
    let kinds: Vec<_> = fake
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Post(event) => Some(event.kind),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, vec![EventKind::MouseDown, EventKind::MouseUp]);
}

#[tokio::test(start_paused = true)]
async fn timeout_during_post_is_unknown_and_releases_up() {
    let fake = Arc::new(FakeEvents::ready());
    fake.hold_posts();
    let fake2 = Arc::clone(&fake);
    let task = tokio::spawn(async move {
        gcu_driver_macos::input::deliver(
            Arc::clone(&fake2) as Arc<dyn gcu_driver_macos::input::EventApi>,
            Arc::new(gcu_driver_macos::input::InputLedger::new()),
            &target("com.example.Editor", "Editor"),
            &click(PointerRoute::TargetedOnly),
            &Cancel::default(),
            Duration::from_millis(250),
        )
        .await
    });
    tokio::time::advance(Duration::from_millis(400)).await;
    let error = task.await.unwrap().unwrap_err();
    assert_eq!(
        error,
        DriverError::Timeout {
            maybe_executed: true
        }
    );
    assert!(fake
        .calls()
        .iter()
        .any(|call| { matches!(call, Call::Post(event) if event.kind == EventKind::MouseUp) }));
}

#[tokio::test]
async fn permission_prompt_and_bad_window_refuse_before_post() {
    let mut probe = Probe::verified_os();
    probe.accessibility = false;
    let fake = Arc::new(FakeEvents::new(probe));
    assert_eq!(
        run(
            &fake,
            "com.example.Editor",
            "Editor",
            &click(PointerRoute::AllowForeground),
            &Cancel::default(),
        )
        .await
        .unwrap_err(),
        DriverError::PermissionMissing {
            permission: "accessibility".into(),
        }
    );
    assert!(fake.calls().is_empty());

    let mut probe = Probe::verified_os();
    probe.secure_prompt = true;
    let fake = Arc::new(FakeEvents::new(probe));
    assert_eq!(
        run(
            &fake,
            "com.example.Editor",
            "Editor",
            &click(PointerRoute::TargetedOnly),
            &Cancel::default(),
        )
        .await
        .unwrap_err(),
        DriverError::SecureDesktop
    );

    let mut probe = Probe::verified_os();
    probe.appkit_ready = false;
    let fake = Arc::new(FakeEvents::new(probe));
    let error = run(
        &fake,
        "com.example.Editor",
        "Editor",
        &click(PointerRoute::AllowForeground),
        &Cancel::default(),
    )
    .await
    .unwrap_err();
    assert!(!error.maybe_executed());
    assert!(matches!(error, DriverError::Native { .. }));
    assert!(fake.calls().is_empty());

    let fake = Arc::new(FakeEvents::ready());
    fake.close_window(4, 7);
    assert_eq!(
        run(
            &fake,
            "com.example.Editor",
            "Editor",
            &click(PointerRoute::TargetedOnly),
            &Cancel::default(),
        )
        .await
        .unwrap_err(),
        DriverError::WindowGone
    );
    assert!(fake.calls().is_empty());
}

#[tokio::test]
async fn scroll_drag_keys_and_text_use_the_targeted_path() {
    let window = click(PointerRoute::TargetedOnly);
    let DriverAction::PointerClick { win, .. } = window else {
        panic!("click");
    };
    let scroll = DriverAction::PointerScroll {
        win: win.clone(),
        at: WindowPoint { x: 4.0, y: 6.0 },
        dx: -3,
        dy: 8,
        route: PointerRoute::TargetedOnly,
    };
    let drag = DriverAction::PointerDrag {
        win: win.clone(),
        path: vec![
            WindowPoint { x: 2.0, y: 2.0 },
            WindowPoint { x: 6.0, y: 4.0 },
            WindowPoint { x: 8.0, y: 8.0 },
        ],
        route: PointerRoute::TargetedOnly,
    };
    let keys = DriverAction::PressKeys {
        win: win.clone(),
        chord: KeyChord::parse("cmd+a").unwrap(),
        route: KeyRoute::TargetedOnly,
    };
    let text = DriverAction::TypeText {
        win,
        focus: None,
        text: "hi".into(),
        route: KeyRoute::TargetedOnly,
    };
    for action in [scroll, drag, keys, text] {
        let fake = Arc::new(FakeEvents::ready());
        let delivery = run(
            &fake,
            "com.example.Editor",
            "Editor",
            &action,
            &Cancel::default(),
        )
        .await
        .unwrap();
        assert_eq!(delivery, Delivery::TargetedEvent);
        assert!(fake
            .calls()
            .iter()
            .all(|call| !matches!(call, Call::Activate(_))));
        assert!(fake.calls().iter().any(|call| {
            matches!(call, Call::Post(event) if event.field_51 == Some(7) && event.path == PostPath::ToPid)
        }));
    }
}

#[tokio::test]
async fn point_outside_the_window_is_rejected_before_post() {
    let mut action = click(PointerRoute::TargetedOnly);
    if let DriverAction::PointerClick { at, .. } = &mut action {
        *at = WindowPoint {
            x: 10_000.0,
            y: 1.0,
        };
    }
    let fake = Arc::new(FakeEvents::ready());
    assert_eq!(
        run(
            &fake,
            "com.example.Editor",
            "Editor",
            &action,
            &Cancel::default(),
        )
        .await
        .unwrap_err(),
        DriverError::OutOfBounds
    );
    assert!(fake.calls().is_empty());
}

async fn session(bundle: &str) -> (Arc<FakeMacApi>, MacosDriver, WindowRef) {
    let api = Arc::new(FakeMacApi::default());
    let facts = signed_facts(bundle, "/Applications/Editor.app");
    api.add_running(4, &facts);
    api.update(|state| {
        state.frontmost = Some(11);
        state
            .cg_windows
            .push(cg_window(7, 4, "Editor", rect(0.0, 0.0, 80.0, 40.0)));
    });
    let mut driver = MacosDriver::with_api(Arc::clone(&api) as Arc<dyn MacApi>)
        .await
        .unwrap();
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let windows = driver.list_windows(Some(&app)).await.unwrap();
    let win = WindowRef {
        app,
        id: windows[0].id.clone(),
    };
    driver.inject_events(Arc::new(FakeEvents::ready()));
    (api, driver, win)
}

#[tokio::test]
async fn driver_targeted_click_does_not_change_frontmost_or_raise() {
    let (api, driver, win) = session("com.example.Editor").await;
    assert!(driver.capabilities().delivery.targeted_pointer);
    assert!(driver.capabilities().version.contains("P1-06"));
    assert_eq!(
        driver.capabilities().private_apis.get("skylight").copied(),
        Some(PrivateApiStatus::Available)
    );
    let delivery = driver
        .perform(
            &DriverAction::PointerClick {
                win,
                at: WindowPoint { x: 4.0, y: 6.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await
        .unwrap();
    assert_eq!(delivery, Delivery::TargetedEvent);
    let state = api.state().unwrap();
    assert_eq!(state.frontmost, Some(11));
    assert!(state.raises.is_empty());
}

#[tokio::test]
async fn driver_without_events_stays_unavailable() {
    let (api, driver, win) = session("com.example.Editor").await;
    drop(driver);
    let driver = MacosDriver::with_api(Arc::clone(&api) as Arc<dyn MacApi>)
        .await
        .unwrap();
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let windows = driver.list_windows(Some(&app)).await.unwrap();
    assert!(!driver.capabilities().delivery.targeted_pointer);
    assert_eq!(
        driver
            .perform(
                &DriverAction::PointerClick {
                    win: WindowRef {
                        app,
                        id: windows[0].id.clone(),
                    },
                    at: WindowPoint { x: 1.0, y: 1.0 },
                    button: Button::Left,
                    count: 1,
                    route: PointerRoute::TargetedOnly,
                },
                &Cancel::default(),
            )
            .await
            .unwrap_err(),
        DriverError::CapabilityUnavailable
    );
    let _ = win;
}

#[tokio::test]
async fn driver_refuses_revoked_permission_wrong_pid_prompt_and_minimized() {
    let (api, mut driver, win) = session("com.example.Editor").await;
    let fake = Arc::new(FakeEvents::ready());
    driver.inject_events(Arc::clone(&fake) as Arc<dyn gcu_driver_macos::input::EventApi>);
    api.update(|state| state.accessibility = false);
    assert_eq!(
        driver
            .perform(
                &DriverAction::PointerClick {
                    win: win.clone(),
                    at: WindowPoint { x: 4.0, y: 6.0 },
                    button: Button::Left,
                    count: 1,
                    route: PointerRoute::AllowForeground,
                },
                &Cancel::default(),
            )
            .await
            .unwrap_err(),
        DriverError::PermissionMissing {
            permission: "accessibility".into(),
        }
    );
    api.update(|state| state.accessibility = true);
    api.add_running(
        8,
        &signed_facts("com.example.Other", "/Applications/Other.app"),
    );
    api.update(|state| {
        state
            .cg_windows
            .push(cg_window(7, 8, "Other", rect(0.0, 0.0, 80.0, 40.0)));
    });
    driver.list_windows(Some(&win.app)).await.unwrap();
    assert_eq!(
        driver
            .perform(
                &DriverAction::PointerClick {
                    win: WindowRef {
                        app: win.app.clone(),
                        id: "cg:7:8".into(),
                    },
                    at: WindowPoint { x: 4.0, y: 6.0 },
                    button: Button::Left,
                    count: 1,
                    route: PointerRoute::TargetedOnly,
                },
                &Cancel::default(),
            )
            .await
            .unwrap_err(),
        DriverError::AppNotFound
    );
    api.update(|state| {
        state.running.push(running_app(
            9,
            &signed_facts(
                "com.apple.SecurityAgent",
                "/System/Library/CoreServices/SecurityAgent.app",
            ),
        ));
        state.cg_windows.push(cg_window(
            2,
            9,
            "Authenticate",
            rect(20.0, 20.0, 300.0, 180.0),
        ));
    });
    assert_eq!(
        driver
            .perform(
                &DriverAction::PointerClick {
                    win: win.clone(),
                    at: WindowPoint { x: 4.0, y: 6.0 },
                    button: Button::Left,
                    count: 1,
                    route: PointerRoute::TargetedOnly,
                },
                &Cancel::default(),
            )
            .await
            .unwrap_err(),
        DriverError::SecureDesktop
    );
    api.update(|state| {
        state.running.retain(|app| app.pid != 9);
        state.cg_windows.retain(|window| window.pid != 9);
        state
            .ax_windows
            .insert(4, vec![ax_window(0, "Editor", rect(0.0, 0.0, 80.0, 40.0))]);
        state.ax_windows.get_mut(&4).unwrap()[0].minimized = true;
    });
    assert_eq!(
        driver
            .perform(
                &DriverAction::PointerDrag {
                    win,
                    path: vec![
                        WindowPoint { x: 2.0, y: 2.0 },
                        WindowPoint { x: 4.0, y: 4.0 },
                    ],
                    route: PointerRoute::TargetedOnly,
                },
                &Cancel::default(),
            )
            .await
            .unwrap_err(),
        DriverError::WindowMinimized
    );
    assert!(fake.calls().is_empty());
}

/// Background click, type, and drag on a real Mac. CI compiles this and does
/// not run it (`not_run`). See the crate README and ADR 0006.
#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "needs an authorized Mac and AppKit on the main thread (not_run)"]
async fn live_fixture_background_click_type_and_drag() {
    assert!(
        gcu_driver_macos::input::ensure_appkit(),
        "call this from the main thread so NSApplication exists"
    );
    let driver = MacosDriver::new().await.expect("live driver");
    let mut app = None;
    for _ in 0..40 {
        if let Ok(found) = driver.resolve_app(&AppQuery::from("fixture")).await {
            app = Some(found);
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let app = app.expect("gcu-fixture must be running and another app must be frontmost");
    let info = driver.key_window(&app).await.expect("window");
    let win = WindowRef {
        app,
        id: info.id.clone(),
    };
    let tree = driver
        .snapshot_tree(
            &win,
            &gcu_core::driver::TreeOpts::default(),
            &Cancel::default(),
        )
        .await
        .expect("tree");
    let button = tree
        .root
        .as_ref()
        .and_then(|root| find_increment(root))
        .expect("increment button");
    let frame = button.frame.expect("frame");
    let at = WindowPoint {
        x: frame.x + frame.w / 2.0,
        y: frame.y + frame.h / 2.0,
    };
    let state_path = std::env::var("GCU_FIXTURE_STATE").expect("GCU_FIXTURE_STATE");
    let before = std::fs::read_to_string(&state_path).expect("state");
    let before: serde_json::Value = serde_json::from_str(&before).expect("json");
    let before_count = before["counter"].as_u64().unwrap_or(0);
    let delivery = driver
        .perform(
            &DriverAction::PointerClick {
                win: win.clone(),
                at,
                button: Button::Left,
                count: 1,
                route: PointerRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await
        .expect("background click");
    assert_eq!(delivery, Delivery::TargetedEvent);
    let after = std::fs::read_to_string(&state_path).expect("state");
    let after: serde_json::Value = serde_json::from_str(&after).expect("json");
    assert_eq!(after["counter"].as_u64().unwrap_or(0), before_count + 1);
    let typed = driver
        .perform(
            &DriverAction::TypeText {
                win: win.clone(),
                focus: None,
                text: "Z".into(),
                route: KeyRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await
        .expect("background type");
    assert_eq!(typed, Delivery::TargetedEvent);
    let dragged = driver
        .perform(
            &DriverAction::PointerDrag {
                win,
                path: vec![
                    WindowPoint { x: 20.0, y: 20.0 },
                    WindowPoint { x: 40.0, y: 24.0 },
                ],
                route: PointerRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await
        .expect("background drag");
    assert_eq!(dragged, Delivery::TargetedEvent);
}

/// Chrome behind another window must not receive a background click.
#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "needs an authorized Mac; Chrome occluded; ADR 0006 (not_run)"]
async fn live_chrome_occluded_targeted_click_is_unavailable() {
    assert!(
        gcu_driver_macos::input::ensure_appkit(),
        "call this from the main thread so NSApplication exists"
    );
    let driver = MacosDriver::new().await.expect("live driver");
    let app = driver
        .resolve_app(&AppQuery::from("Google Chrome"))
        .await
        .expect("Chrome");
    let info = driver.key_window(&app).await.expect("chrome window");
    let error = driver
        .perform(
            &DriverAction::PointerClick {
                win: WindowRef { app, id: info.id },
                at: WindowPoint { x: 40.0, y: 40.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await
        .expect_err("chromium background click");
    assert_eq!(error, DriverError::CapabilityUnavailable);
    assert!(!error.maybe_executed());
}

#[cfg(target_os = "macos")]
fn find_increment(node: &gcu_core::model::RawNode) -> Option<&gcu_core::model::RawNode> {
    let hit = node.name.as_deref().is_some_and(|name| {
        name.contains("Increment") || name.contains("计数") || name.contains("counter")
    });
    if hit && node.frame.is_some() {
        return Some(node);
    }
    node.children.iter().find_map(find_increment)
}
