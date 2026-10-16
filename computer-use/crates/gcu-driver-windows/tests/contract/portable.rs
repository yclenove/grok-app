//! Deterministic supplements; these never count as native desktop passes.
use crate::common;
use gcu_core::{
    action::ToolCall, driver::*, policy::PolicyDecision, testing::host, tree::Pipeline,
};
use gcu_driver_windows::{
    gate::IntegrityLevel,
    input::{fake::FakeInput, hotkey::FakeSafetyApi, watchdog::HEARTBEAT_TIMEOUT, InputBackend},
    uia::{
        cache::{Patterns, Properties},
        source::{FakeNode, FakeUia},
    },
    WindowsDriver,
};
use gcu_protocol::{capabilities::ForegroundCapability, result::Outcome, ErrorCode};
use std::{sync::Arc, time::Duration};

async fn uia_fixture(name: &str) -> (Arc<FakeUia>, WindowsDriver, WindowRef) {
    let (api, _, _, win) = common::fixture().await;
    let uia = Arc::new(FakeUia::new(FakeNode {
        properties: Properties {
            control_type: 50032,
            name: Some("Workspace".into()),
            ..Default::default()
        },
        children: vec![
            FakeNode {
                properties: Properties {
                    control_type: 50000,
                    name: Some(name.into()),
                    patterns: Patterns {
                        invoke: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
            FakeNode {
                properties: Properties {
                    control_type: 50000,
                    name: Some("ordinary_control".into()),
                    patterns: Patterns {
                        invoke: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
        ],
    }));
    let driver = WindowsDriver::with_uia(
        api,
        uia.clone(),
        Arc::new(gcu_driver_windows::uia::events::FakeUiaEvents::default()),
    )
    .await
    .unwrap();
    let app = driver
        .resolve_app(&AppQuery(win.app.info.id))
        .await
        .unwrap();
    let win = WindowRef {
        id: driver.key_window(&app).await.unwrap().id,
        app,
    };
    (uia, driver, win)
}

#[tokio::test]
async fn uipi_high_target_refuses_observation_and_every_action_without_delivery() {
    let (api, driver, _, win) = common::fixture().await;
    let input = Arc::new(FakeInput::default());
    let driver = driver
        .with_input(Arc::new(InputBackend::new(input.clone())))
        .with_safety(common::fake_guard());
    api.update(|s| s.processes.get_mut(&7).unwrap().integrity = IntegrityLevel::High);
    assert!(matches!(
        driver
            .snapshot_tree(&win, &Default::default(), &Cancel::default())
            .await,
        Err(DriverError::ElevationRequired)
    ));
    for action in common::actions(&win) {
        assert_eq!(
            driver.perform(&action, &Cancel::default()).await,
            Err(DriverError::ElevationRequired)
        );
    }
    assert!(input.state().sent.is_empty());
}

#[tokio::test]
async fn hotkey_conflict_and_listener_loss_disable_foreground_with_zero_sends() {
    for conflict in [true, false] {
        let (_, driver, _, win) = common::fixture().await;
        let api = FakeSafetyApi::default();
        if conflict {
            api.fail_open("RegisterHotKey conflict");
        }
        let guard = api.start(Default::default(), Arc::new(common::TestStop::default()));
        if !conflict {
            api.remove_hooks();
            api.advance(HEARTBEAT_TIMEOUT + Duration::from_millis(1));
        }
        let input = Arc::new(FakeInput::default());
        let driver = driver
            .with_input(Arc::new(InputBackend::new(input.clone())))
            .with_safety(guard);
        assert_eq!(
            driver.capabilities().delivery.foreground,
            ForegroundCapability::Unavailable
        );
        for action in common::actions(&win).into_iter().filter(|a| {
            matches!(
                a,
                DriverAction::TypeText {
                    route: KeyRoute::AllowForeground,
                    ..
                } | DriverAction::PointerClick {
                    route: PointerRoute::AllowForeground,
                    ..
                }
            )
        }) {
            assert!(driver.perform(&action, &Cancel::default()).await.is_err());
        }
        assert!(input.state().sent.is_empty());
    }
}

#[tokio::test]
async fn uncertain_native_timeout_router_returns_unknown_and_never_replays() {
    let (_, driver, win) = uia_fixture("counter_button").await;
    let input = Arc::new(FakeInput::default());
    input.update(|s| {
        s.fail_at = Some(0);
        s.send_error = DriverError::Timeout {
            maybe_executed: true,
        };
    });
    let driver = driver
        .with_input(Arc::new(InputBackend::new(input.clone())))
        .with_safety(common::fake_guard());
    let (router, session) = host::router(Arc::new(driver));
    router
        .observe(
            &session,
            serde_json::from_value(
                serde_json::json!({"app":win.app.info.id,"include_screenshot":false}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let out = router
        .execute(
            &session,
            ToolCall::new(
                "timeout",
                win.app.info.id,
                serde_json::from_value(serde_json::json!({"press_key":{"key":"Return"}})).unwrap(),
            ),
        )
        .await;
    assert_eq!(out.action.outcome, Outcome::Unknown, "{out:?}");
    assert!(out.state.is_some());
    assert!(session.action_ledger().has_unknown());
    assert_eq!(
        input
            .state()
            .sent
            .iter()
            .filter(|s| matches!(
                s.event,
                gcu_driver_windows::input::api::Event::Key(_, true, ..)
            ))
            .count(),
        1,
        "external input trace has one key down; cleanup may send ups"
    );
}

#[tokio::test]
async fn uia_execution_hints_feed_policy_all_paths_refused_unknown_handoff() {
    for name in [
        "Integrated Terminal",
        "Debug Console",
        "Run Task",
        "Python REPL",
        "Web SSH",
        "Cloud Shell",
        "Container Console",
        "Remote Desktop",
    ] {
        let (uia, driver, win) = uia_fixture(name).await;
        let raw = driver
            .snapshot_tree(&win, &Default::default(), &Cancel::default())
            .await
            .unwrap();
        let tree = Pipeline::default().run(raw, &Default::default());
        let index = tree
            .nodes()
            .into_iter()
            .find(|(_, n)| n.node.name.as_deref() == Some(name))
            .unwrap()
            .1
            .element_index;
        for (tool, targeted, unresolved) in host::execution_decisions(&tree, index) {
            assert!(
                matches!(
                    targeted,
                    PolicyDecision::Refuse(ErrorCode::ElementNotActionable, _)
                        | PolicyDecision::Handoff(_)
                ),
                "{name}: {tool:?}"
            );
            // No known target is supplied: execution-only and unknown mixed UI must fail closed.
            assert!(
                matches!(unresolved, PolicyDecision::Handoff(_)),
                "{name}: {tool:?} mixed surface must hand off"
            );
        }
        assert!(
            uia.action_calls().is_empty(),
            "policy must not reach the native provider"
        );
    }
}

#[test]
fn stalled_listener_stop_uses_controlled_clock_and_retains_host_latch() {
    use gcu_driver_windows::input::hotkey::StopLatch;
    use std::sync::atomic::{AtomicBool, Ordering};
    let api = FakeSafetyApi::default();
    let latch = Arc::new(common::TestStop::default());
    let guard = api.start(Default::default(), latch.clone());
    let released = Arc::new(AtomicBool::new(false));
    let release = released.clone();
    guard.bind_stop(Arc::new(move || release.store(true, Ordering::SeqCst)));
    api.stall_poll();
    api.trigger();
    assert!(latch.is_stopped());
    assert!(released.load(Ordering::SeqCst));
    api.advance(HEARTBEAT_TIMEOUT + Duration::from_millis(1));
    assert!(!guard.available());
    assert!(guard.check(true).is_err());
    let reconnected = FakeSafetyApi::default().start(Default::default(), latch);
    assert!(reconnected.is_stopped());
}

#[cfg(not(windows))]
#[test]
fn desktop_absent_reports_native_cases_not_run() {
    use super::harness::{Outcome, Report, CASES};
    let mut report = Report::default();
    for (id, _) in CASES {
        report.record(
            id,
            Ok(Outcome::NotRun(
                "requires Windows interactive desktop".into(),
            )),
        );
    }
    report.finish();
    assert_eq!(
        (report.passed, report.skipped, report.not_run, report.failed),
        (0, 0, 29, 0)
    );
}

#[tokio::test]
async fn password_removed_by_budget_redacts_pixels_derivatives_and_missing_region_handoffs() {
    use gcu_core::{
        model::{NodeValue, Rect},
        redact::{self, CapturedFrame, RedactionError, NEUTRAL_COLOR},
    };
    use gcu_driver_windows::capture::{CaptureBackend, CapturePath, FakeCapture, Paint};
    let (api, _, _, _) = common::fixture().await;
    let region = Rect {
        x: 10.,
        y: 10.,
        w: 30.,
        h: 20.,
    };
    let uia = Arc::new(FakeUia::new(FakeNode {
        properties: Properties {
            control_type: 50032,
            name: Some("Fixture".into()),
            ..Default::default()
        },
        children: vec![FakeNode {
            properties: Properties {
                control_type: 50004,
                password: true,
                name: Some("Password".into()),
                value: Some(NodeValue::Text("synthetic-secret".into())),
                frame: Some(region),
                ..Default::default()
            },
            ..Default::default()
        }],
    }));
    let capture = Arc::new(FakeCapture::new(CapturePath::Wgc));
    capture.paint(
        8,
        Paint {
            own: [255, 0, 180, 255],
            cover: [0, 0, 0, 255],
            occluded: false,
            mark: None,
        },
    );
    let driver = WindowsDriver::with_uia(
        api,
        uia.clone(),
        Arc::new(gcu_driver_windows::uia::events::FakeUiaEvents::default()),
    )
    .await
    .unwrap()
    .with_capture(Arc::new(CaptureBackend::new(capture.clone(), capture)));
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let win = WindowRef {
        id: driver.key_window(&app).await.unwrap().id,
        app,
    };
    let raw = driver
        .snapshot_tree(&win, &Default::default(), &Cancel::default())
        .await
        .unwrap();
    assert!(!format!("{raw:?}").contains("synthetic-secret"));
    let mut tree = Pipeline::default().run(
        raw,
        &TreeOpts {
            max_tree_nodes: 1,
            ..Default::default()
        },
    );
    assert_eq!(tree.sensitive_regions.len(), 1);
    tree.state_version = Some(1);
    tree.observed_at = Some(Duration::ZERO);
    let raw_capture = driver
        .capture(&win, &Default::default(), &Cancel::default())
        .await
        .unwrap();
    let image = redact::apply(
        &tree,
        CapturedFrame::new(raw_capture.clone(), 1, Duration::ZERO),
    )
    .unwrap();
    // This fake HWND and UIA root both have origin (0,0).
    let crop = image
        .approval_crop(Rect {
            x: 15.,
            y: 15.,
            w: 20.,
            h: 10.,
        })
        .unwrap();
    for safe in [crop.clone(), crop.thumbnail(256).unwrap()] {
        assert!(
            safe.pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == NEUTRAL_COLOR),
            "regions={:?}, crop={safe:?}, first={:?}",
            tree.sensitive_regions,
            &safe.pixels()[..4]
        );
        assert!(!format!("{safe:?}").contains("synthetic-secret"));
        assert!(!safe.encode(Default::default()).unwrap().bytes().is_empty());
    }
    tree.sensitive_regions_reliable = false;
    assert_eq!(
        redact::apply(&tree, CapturedFrame::new(raw_capture, 1, Duration::ZERO)).unwrap_err(),
        RedactionError::NeedsHandoff
    );
}
