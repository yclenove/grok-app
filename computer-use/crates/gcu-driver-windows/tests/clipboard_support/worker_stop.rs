//! A hotkey must use the parent Stop channel throughout a clipboard transaction.
use super::*;
use gcu_driver_windows::input::hotkey::{FakeSafetyApi, StopLatch};
use gcu_driver_windows::WindowsDriver;

#[tokio::test]
async fn hotkey_during_parent_staging_with_empty_ledger_kills_child_and_restores_backup() {
    let fixture = Fixture::new();
    let board = Arc::new(FakeClipboard::new(Arc::new(Desktop::default())));
    let release = Arc::new(Parent {
        clipboard: board.clone(),
        input: FakeInput::default(),
    });
    let api = Arc::new(FakeSafetyApi::default());
    let guard = api.start(
        Default::default(),
        Arc::new(crate::common::TestStop::default()),
    );
    let driver = WindowsDriver::isolated_with_releaser(
        fixture.config("input_clipboard_hang"),
        release.clone(),
    )
    .await
    .unwrap()
    .with_safety(guard.clone());
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let info = driver.key_window(&app).await.unwrap();
    let win = WindowRef { app, id: info.id };
    board.stop_after_stage(api);
    assert_eq!(
        driver.perform(&request(&win), &Cancel::default()).await,
        Err(DriverError::Cancelled {
            maybe_executed: true
        })
    );
    assert!(guard.is_stopped());
    assert_eq!(board.contents(), original());
    assert!(!board.pending());
    assert!(release.input.state().sent.is_empty());
    assert!(!fixture.0.join("held").exists(), "no stage ack after Stop");
    assert_eq!(board.writes(), 1);
    driver.release_all_input().await;
}

#[tokio::test]
async fn hotkey_aborts_worker_paste_restores_conditionally_and_releases_parent_ups() {
    for fault in ["input_clipboard_hang", "input_clipboard_key_hang"] {
        for external_copy in [false, true] {
            let fixture = Fixture::new();
            let board = Arc::new(FakeClipboard::new(Arc::new(Desktop::default())));
            let release = Arc::new(Parent {
                clipboard: board.clone(),
                input: FakeInput::default(),
            });
            let api = Arc::new(FakeSafetyApi::default());
            let latch = Arc::new(crate::common::TestStop::default());
            let guard = api.start(Default::default(), latch.clone());
            let driver = Arc::new(
                WindowsDriver::isolated_with_releaser(fixture.config(fault), release.clone())
                    .await
                    .unwrap()
                    .with_safety(guard.clone()),
            );
            let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
            let info = driver.key_window(&app).await.unwrap();
            let win = WindowRef { app, id: info.id };
            let d = driver.clone();
            let target = win.clone();
            let task =
                tokio::spawn(async move { d.perform(&request(&target), &Cancel::default()).await });
            fixture.held().await;
            if external_copy {
                board.copy_external();
            }
            let started = std::time::Instant::now();
            api.trigger();
            assert!(guard.is_stopped());
            assert!(latch.is_stopped());
            assert!(started.elapsed() <= Duration::from_millis(100));
            tokio::time::timeout(Duration::from_millis(500), async {
                while board.pending()
                    || (fault == "input_clipboard_key_hang"
                        && release.input.state().sent.len() != 2)
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(started.elapsed() <= Duration::from_millis(500));
            assert_eq!(
                task.await.unwrap(),
                Err(DriverError::Cancelled {
                    maybe_executed: true
                })
            );
            assert_eq!(
                board.contents(),
                if external_copy {
                    external()
                } else {
                    original()
                }
            );
            assert_eq!(board.restores(), usize::from(!external_copy));
            let sent = release.input.state().sent;
            assert_eq!(
                sent.len(),
                if fault == "input_clipboard_key_hang" {
                    2
                } else {
                    0
                }
            );
            assert!(sent
                .iter()
                .all(|s| matches!(s.event, Event::Key(_, false, ..))));
            assert_eq!(
                driver.perform(&request(&win), &Cancel::default()).await,
                Err(DriverError::Cancelled {
                    maybe_executed: false
                })
            );
            assert_eq!(board.writes(), 1);
            assert_eq!(
                std::fs::read_to_string(fixture.0.join("calls"))
                    .unwrap()
                    .lines()
                    .count(),
                1
            );
            driver.release_all_input().await;
        }
    }
}
