//! Driver integration: exact identities, live gates, cancellation and honest capabilities.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_core::{
    driver::*,
    model::{NodeAction, States, TextRange},
};
use gcu_driver_windows::{
    gate::IntegrityLevel,
    testing::{medium_security, signed_image, window, FakeWinApi},
    uia::{
        cache::{Patterns, Properties},
        events::{EventKind, FakeUiaEvents},
        source::{FakeNode, FakeUia},
        UiaElement,
    },
    WindowsDriver,
};
use std::{
    future::poll_fn,
    sync::Arc,
    time::{Duration, Instant},
};

async fn fixture() -> (
    Arc<FakeWinApi>,
    Arc<FakeUia>,
    Arc<FakeUiaEvents>,
    WindowsDriver,
    WindowRef,
) {
    let api = Arc::new(FakeWinApi::default());
    api.update(|s| {
        s.paths.insert(7, "C:\\Editor.exe".into());
        s.images
            .insert("C:\\Editor.exe".into(), signed_image("C:\\Editor.exe"));
        s.processes.insert(7, medium_security());
        let mut w = window(8, 7);
        w.foreground = true;
        s.windows.push(w);
        s.desktops.insert(8, Some(true));
    });
    let source = Arc::new(FakeUia::new(FakeNode {
        properties: Properties {
            control_type: 50032,
            name: Some("Editor".into()),
            ..Default::default()
        },
        children: vec![FakeNode {
            properties: Properties {
                control_type: 50004,
                name: Some("Document".into()),
                focused: true,
                read_only: Some(false),
                patterns: Patterns {
                    value: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }],
    }));
    let events = Arc::new(FakeUiaEvents::default());
    let driver = WindowsDriver::with_uia(api.clone(), source.clone(), events.clone())
        .await
        .unwrap();
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let info = driver.key_window(&app).await.unwrap();
    let win = WindowRef { app, id: info.id };
    (api, source, events, driver, win)
}

#[tokio::test]
async fn driver_observes_tree_geometry_focus_and_events_without_advertising_input() {
    let (_, _, events, driver, win) = fixture().await;
    let caps = driver.capabilities();
    assert!(caps.observe.tree && caps.observe.events);
    assert!(
        !caps.observe.screenshot
            && !caps.delivery.accessibility
            && !caps.delivery.targeted_keys
            && !caps.delivery.targeted_pointer
    );
    assert_eq!(
        caps.delivery.foreground,
        gcu_protocol::capabilities::ForegroundCapability::Unavailable
    );
    assert_eq!(caps.limits.max_tree_nodes, 5000);
    assert!(driver.subscribe(&win).is_none()); // only after an exact gated observation
    let raw = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let node = &raw.root.unwrap().children[0];
    let handle = node.handle.downcast_ref::<UiaElement>().unwrap();
    assert_eq!(handle.window, win);
    assert!(node.states.contains(States::FOCUSED | States::SETTABLE));
    assert!(driver.window_geometry(&win).await.unwrap().frame.w > 0.);
    assert!(driver.focused_element(&win.app).await.unwrap().is_some());
    let mut stream = driver.subscribe(&win).unwrap();
    tokio::time::timeout(Duration::from_millis(500), async {
        while events.registrations() == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    events.emit(8, EventKind::Structure);
    assert_eq!(
        poll_fn(|cx| stream.as_mut().poll_next(cx)).await,
        Some(UiEvent::Changed)
    );
    drop(stream);
    assert_eq!(events.registrations(), 0);
    let mut forged = win.clone();
    forged.app.info.id = "forged".into();
    assert!(driver.subscribe(&forged).is_none());
    assert_eq!(
        driver
            .capture(&win, &CaptureOpts::default(), &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::CapabilityUnavailable
    );
    driver.release_all_input().await;
    assert!(driver.doctor().await.ready);
}

#[tokio::test]
async fn driver_every_semantic_action_on_uia_handle_remains_unavailable() {
    let (_, _, _, driver, win) = fixture().await;
    let raw = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let el = raw.root.unwrap().children[0].handle.clone();
    for action in [
        DriverAction::Press { el: el.clone() },
        DriverAction::SecondaryAction {
            el: el.clone(),
            action: NodeAction::Press,
        },
        DriverAction::SetValue {
            el: el.clone(),
            value: "text".into(),
        },
        DriverAction::SelectText {
            el: el.clone(),
            range: TextRange::default(),
        },
        DriverAction::ScrollElement {
            el,
            dir: Dir::Down,
            pages: 1.,
        },
    ] {
        assert_eq!(
            driver
                .perform(&action, &Cancel::default())
                .await
                .unwrap_err(),
            DriverError::CapabilityUnavailable
        );
    }
}

#[tokio::test]
async fn driver_live_admin_secure_desktop_and_reused_hwnd_gate_before_uia_read() {
    let (api, source, _, driver, win) = fixture().await;
    for (level, secure, expected) in [
        (IntegrityLevel::High, false, DriverError::ElevationRequired),
        (IntegrityLevel::Medium, true, DriverError::SecureDesktop),
    ] {
        api.update(|s| {
            let process = s.processes.get_mut(&7).unwrap();
            process.integrity = level;
            process.secure_desktop = secure;
        });
        let before = source.stats().0;
        assert_eq!(
            driver
                .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
                .await
                .unwrap_err(),
            expected
        );
        assert_eq!(source.stats().0, before);
    }
    api.update(|s| {
        s.processes.insert(7, medium_security());
        s.windows[0].created += 1;
    });
    assert_eq!(
        driver
            .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::WindowGone
    );
}

#[tokio::test]
async fn driver_security_change_during_read_rejects_returned_payload() {
    let (api, source, _, driver, win) = fixture().await;
    source.set_delay(Duration::from_millis(10));
    let changed = api.clone();
    let trigger = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(25)).await;
        changed.update(|s| s.processes.get_mut(&7).unwrap().integrity = IntegrityLevel::High);
    });
    let error = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    trigger.await.unwrap();
    assert_eq!(error, DriverError::ElevationRequired);
    assert!(driver.subscribe(&win).is_none());
}

#[tokio::test]
async fn driver_cancelled_slow_uia_does_not_queue_stop_or_release() {
    let (_, source, _, driver, win) = fixture().await;
    source.set_delay(Duration::from_secs(10));
    let cancel = Cancel::default();
    let stop = cancel.clone();
    let trigger = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        stop.cancel();
    });
    let start = Instant::now();
    let error = driver
        .snapshot_tree(&win, &TreeOpts::default(), &cancel)
        .await
        .unwrap_err();
    trigger.await.unwrap();
    assert_eq!(
        error,
        DriverError::Cancelled {
            maybe_executed: false
        }
    );
    assert!(start.elapsed() < Duration::from_millis(500));
    tokio::time::timeout(Duration::from_millis(100), driver.release_all_input())
        .await
        .unwrap();
}

#[tokio::test]
async fn driver_partial_native_tree_retains_completeness() {
    let (_, source, _, driver, win) = fixture().await;
    source.fail_children(
        1,
        DriverError::Native {
            message: "fixture failure".into(),
            maybe_executed: false,
        },
    );
    let tree = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(
        tree.incomplete.unwrap().reason,
        gcu_core::model::IncompleteReason::PartialError
    );
}

#[tokio::test]
async fn driver_subscription_rechecks_security_after_previous_observation() {
    let (api, _, events, driver, win) = fixture().await;
    driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    api.update(|s| s.processes.get_mut(&7).unwrap().integrity = IntegrityLevel::High);
    let mut stream = driver.subscribe(&win).unwrap();
    assert_eq!(
        tokio::time::timeout(
            Duration::from_millis(500),
            poll_fn(|cx| stream.as_mut().poll_next(cx))
        )
        .await
        .unwrap(),
        None
    );
    assert_eq!(events.registrations(), 0);
}
