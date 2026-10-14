//! Input recovery lives in the parent, independently of the disposable child.
use super::*;
use gcu_core::{
    driver::{Button, Delivery, KeyRoute, PointerRoute},
    model::WindowPoint,
};
use gcu_driver_windows::input::{api::Event, fake::FakeInput};
use std::sync::Arc;

async fn input_driver(f: &Fixture, fault: &str, release: Arc<FakeInput>) -> WindowsDriver {
    WindowsDriver::isolated_with_releaser(f.config(fault), release)
        .await
        .unwrap()
}
fn drag(win: &gcu_core::driver::WindowRef, route: PointerRoute) -> DriverAction {
    DriverAction::PointerDrag {
        win: win.clone(),
        path: vec![
            WindowPoint { x: 10., y: 20. },
            WindowPoint { x: 70., y: 80. },
        ],
        route,
    }
}
async fn held(f: &Fixture) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !f.dir.join("held").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn input_worker_routes_all_variants_without_parent_injection() {
    let f = Fixture::new();
    let release = Arc::new(FakeInput::default());
    let driver = input_driver(&f, "input_normal", release.clone()).await;
    let win = semantic::window(&driver).await;
    for action in [
        DriverAction::PointerClick {
            win: win.clone(),
            at: WindowPoint { x: 20., y: 30. },
            button: Button::Left,
            count: 1,
            route: PointerRoute::TargetedOnly,
        },
        DriverAction::PointerScroll {
            win: win.clone(),
            at: WindowPoint { x: 20., y: 30. },
            dx: 0,
            dy: 120,
            route: PointerRoute::Auto,
        },
        drag(&win, PointerRoute::TargetedOnly),
        DriverAction::PressKeys {
            win: win.clone(),
            chord: gcu_protocol::keys::KeyChord::parse("ctrl+a").unwrap(),
            route: KeyRoute::TargetedOnly,
        },
        DriverAction::TypeText {
            win: win.clone(),
            focus: None,
            text: "中文 😀".into(),
            route: KeyRoute::Auto,
        },
    ] {
        assert_eq!(
            driver.perform(&action, &Cancel::default()).await,
            Ok(Delivery::TargetedEvent)
        );
    }
    assert_eq!(
        driver
            .perform(
                &DriverAction::Activate { win: win.clone() },
                &Cancel::default()
            )
            .await,
        Ok(Delivery::Foreground)
    );
    assert_eq!(
        driver
            .perform(
                &drag(&win, PointerRoute::AllowForeground),
                &Cancel::default()
            )
            .await,
        Ok(Delivery::Foreground)
    );
    assert_eq!(f.calls(), vec!["mutation"; 7]);
    assert!(release.state().sent.is_empty());
    driver.release_all_input().await;
}
#[tokio::test]
async fn worker_killed_after_down_parent_releases_up_without_replay() {
    for route in [PointerRoute::TargetedOnly, PointerRoute::AllowForeground] {
        let f = Fixture::new();
        let release = Arc::new(FakeInput::default());
        let driver = Arc::new(input_driver(&f, "input_hang", release.clone()).await);
        let win = semantic::window(&driver).await;
        let task = {
            let driver = driver.clone();
            tokio::spawn(
                async move { driver.perform(&drag(&win, route), &Cancel::default()).await },
            )
        };
        held(&f).await;
        let started = std::time::Instant::now();
        driver.release_all_input().await;
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(matches!(
            task.await.unwrap(),
            Err(DriverError::Cancelled {
                maybe_executed: true
            })
        ));
        let sent = release.state().sent;
        assert_eq!(sent.len(), 1);
        assert!(matches!(
            sent[0].event,
            Event::Button(Button::Left, false, ..)
        ));
        assert_eq!(
            sent[0].delivery,
            if route == PointerRoute::AllowForeground {
                Delivery::Foreground
            } else {
                Delivery::TargetedEvent
            }
        );
        assert_eq!(f.calls(), ["mutation"]);
        #[cfg(target_os = "linux")]
        reaped(&f.pids()).await;
    }
}
#[tokio::test]
async fn input_transport_crash_parent_releases_unknown_without_retry() {
    let f = Fixture::new();
    let release = Arc::new(FakeInput::default());
    let driver = input_driver(&f, "input_crash", release.clone()).await;
    let win = semantic::window(&driver).await;
    assert!(matches!(
        driver
            .perform(&drag(&win, PointerRoute::TargetedOnly), &Cancel::default())
            .await,
        Err(DriverError::Crashed {
            maybe_executed: true
        })
    ));
    assert_eq!(f.calls(), ["mutation"]);
    assert_eq!(release.state().sent.len(), 1);
    assert!(matches!(
        release.state().sent[0].event,
        Event::Button(_, false, ..)
    ));
    driver.release_all_input().await;
}

#[tokio::test]
async fn worker_crashes_mid_chord_parent_releases_every_possible_key_up() {
    let f = Fixture::new();
    let release = Arc::new(FakeInput::default());
    let driver = input_driver(&f, "input_key_crash", release.clone()).await;
    let win = semantic::window(&driver).await;
    let action = DriverAction::PressKeys {
        win,
        chord: gcu_protocol::keys::KeyChord::parse("ctrl+a").unwrap(),
        route: KeyRoute::AllowForeground,
    };
    assert!(matches!(
        driver.perform(&action, &Cancel::default()).await,
        Err(DriverError::Crashed {
            maybe_executed: true
        })
    ));
    let events: Vec<_> = release.state().sent.iter().map(|s| s.event).collect();
    assert_eq!(
        events,
        [
            Event::Key(65, false, false, false),
            Event::Key(17, false, false, false)
        ]
    );
    assert_eq!(f.calls(), ["mutation"]);
    driver.release_all_input().await;
}

#[tokio::test]
async fn worker_abandoned_or_cancelled_mid_chord_parent_releases_ups() {
    for abandon in [false, true] {
        let f = Fixture::new();
        let release = Arc::new(FakeInput::default());
        let driver = Arc::new(input_driver(&f, "input_key_hang", release.clone()).await);
        let win = semantic::window(&driver).await;
        let cancel = Cancel::default();
        let task = {
            let driver = driver.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move {
                driver
                    .perform(
                        &DriverAction::PressKeys {
                            win,
                            chord: gcu_protocol::keys::KeyChord::parse("ctrl+a").unwrap(),
                            route: KeyRoute::TargetedOnly,
                        },
                        &cancel,
                    )
                    .await
            })
        };
        held(&f).await;
        if abandon {
            task.abort();
            let _ = task.await;
        } else {
            cancel.cancel();
            assert!(matches!(
                task.await.unwrap(),
                Err(DriverError::Cancelled {
                    maybe_executed: true
                })
            ));
        }
        tokio::time::timeout(Duration::from_millis(500), async {
            while release.state().sent.len() != 2 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        assert!(release
            .state()
            .sent
            .iter()
            .all(|s| matches!(s.event, Event::Key(_, false, ..))));
        assert_eq!(f.calls(), ["mutation"]);
        driver.release_all_input().await;
    }
}

#[tokio::test]
async fn parent_failed_release_retains_up_and_blocks_next_mutation() {
    let f = Fixture::new();
    let release = Arc::new(FakeInput::default());
    release.update(|s| s.release_error = true);
    let driver = input_driver(&f, "input_crash", release.clone()).await;
    let win = semantic::window(&driver).await;
    let action = drag(&win, PointerRoute::TargetedOnly);
    assert!(driver
        .perform(&action, &Cancel::default())
        .await
        .unwrap_err()
        .maybe_executed());
    assert_eq!(release.state().sent.len(), 1);
    assert_eq!(
        driver.perform(&action, &Cancel::default()).await,
        Err(DriverError::CapabilityUnavailable)
    );
    assert_eq!(f.calls(), ["mutation"]);
    driver.release_all_input().await;
    assert_eq!(release.state().sent.len(), 2);
}

#[tokio::test]
async fn parent_failed_stop_release_waits_for_next_explicit_stop() {
    let f = Fixture::new();
    let release = Arc::new(FakeInput::default());
    release.update(|s| s.release_error = true);
    let driver = Arc::new(input_driver(&f, "input_hang", release.clone()).await);
    let win = semantic::window(&driver).await;
    let task = {
        let driver = driver.clone();
        tokio::spawn(async move {
            driver
                .perform(&drag(&win, PointerRoute::TargetedOnly), &Cancel::default())
                .await
        })
    };
    held(&f).await;
    driver.release_all_input().await;
    assert!(task.await.unwrap().unwrap_err().maybe_executed());
    assert_eq!(release.state().sent.len(), 1);
    // Give the idle watchdog a chance to observe the same Stop generation.
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(release.state().sent.len(), 1);
    driver.release_all_input().await;
    assert_eq!(release.state().sent.len(), 2);
    assert_eq!(f.calls(), ["mutation"]);
}

#[tokio::test]
async fn worker_proven_zero_packet_discards_parent_ups_without_retry() {
    let f = Fixture::new();
    let release = Arc::new(FakeInput::default());
    let driver = input_driver(&f, "input_zero", release.clone()).await;
    let win = semantic::window(&driver).await;
    let action = DriverAction::TypeText {
        win,
        focus: None,
        text: "中文".into(),
        route: KeyRoute::TargetedOnly,
    };
    assert!(matches!(
        driver.perform(&action, &Cancel::default()).await,
        Err(DriverError::Native {
            maybe_executed: false,
            ..
        })
    ));
    assert!(release.state().sent.is_empty());
    driver.release_all_input().await;
    assert!(release.state().sent.is_empty());
    assert_eq!(f.calls(), ["mutation"]);
}
