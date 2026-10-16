//! P2-07's real guard boundary with P2-08's synthetic clipboard transaction.
use super::*;
use gcu_driver_windows::input::clipboard::ClipboardApi;
use gcu_driver_windows::input::hotkey::{FakeSafetyApi, StopLatch};

#[tokio::test]
async fn hotkey_during_final_clipboard_check_aborts_paste_and_preserves_external_copy() {
    for external_copy in [false, true] {
        let (input, board, driver, win) = fixture().await;
        let api = Arc::new(FakeSafetyApi::default());
        let latch = Arc::new(crate::common::TestStop::default());
        let guard = api.start(Default::default(), latch.clone());
        let driver = driver.with_safety(guard.clone());
        board.stop_on_check(api);
        if external_copy {
            board.change_on_check(Change::Both);
        }
        assert!(driver
            .perform(&request(&win), &Cancel::default())
            .await
            .is_err());
        assert!(guard.is_stopped());
        assert!(latch.is_stopped());
        assert!(input.events().is_empty(), "Stop prevents every paste down");
        assert!(!board.pending());
        assert_eq!(
            board.contents(),
            if external_copy {
                external()
            } else {
                original()
            }
        );
        assert_eq!(board.restores(), usize::from(!external_copy));
        assert_eq!(
            driver.perform(&request(&win), &Cancel::default()).await,
            Err(DriverError::Cancelled {
                maybe_executed: false
            })
        );
        assert_eq!(board.writes(), 1, "Stop never replays the transaction");
    }
}

#[tokio::test]
async fn hotkey_during_blocked_paste_check_restores_without_waiting_for_action_queue() {
    let (input, board, driver, win) = fixture().await;
    let api = Arc::new(FakeSafetyApi::default());
    let guard = api.start(
        Default::default(),
        Arc::new(crate::common::TestStop::default()),
    );
    let driver = Arc::new(driver.with_safety(guard.clone()));
    input.pause_before_paste();
    let d = driver.clone();
    let task = tokio::spawn(async move { d.perform(&request(&win), &Cancel::default()).await });
    board.wait_staged().await;
    api.trigger();
    assert!(guard.is_stopped());
    tokio::time::timeout(std::time::Duration::from_millis(500), async {
        while board.pending() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(board.contents(), original());
    input.resume();
    assert!(task.await.unwrap().is_err());
    assert!(input.events().is_empty());
}
