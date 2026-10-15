//! Clipboard transactions use synthetic contents only on every platform.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod clipboard_support;
mod common;
use clipboard_support::*;
use gcu_core::driver::*;
use gcu_driver_windows::input::{api::Event, text, InputBackend};
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn paste_success_restores_all_saved_formats_and_reports_clipboard() {
    let (input, board, driver, win) = fixture().await;
    assert_eq!(
        driver.perform(&request(&win), &Cancel::default()).await,
        Ok(Delivery::Clipboard)
    );
    assert_eq!(board.contents(), original());
    assert_eq!(board.writes(), 1);
    assert_eq!(input.events().len(), 4);
    assert!(input.recorded());
}

#[tokio::test]
async fn two_sessions_separate_backends_serialize_clipboard_transactions() {
    let shared = Arc::new(Desktop::default());
    let (a, board_a, driver_a, win_a) = fixture_on(shared.clone()).await;
    let (_, board_b, driver_b, win_b) = fixture_on(shared).await;
    a.pause_before_paste();
    let task =
        tokio::spawn(async move { driver_a.perform(&request(&win_a), &Cancel::default()).await });
    board_a.wait_staged().await;
    let mut second =
        tokio::spawn(async move { driver_b.perform(&request(&win_b), &Cancel::default()).await });
    assert!(tokio::time::timeout(Duration::from_millis(30), &mut second)
        .await
        .is_err());
    assert_eq!(board_b.writes(), 1);
    a.resume();
    assert_eq!(task.await.unwrap(), Ok(Delivery::Clipboard));
    assert_eq!(second.await.unwrap(), Ok(Delivery::Clipboard));
    assert_eq!(board_b.writes(), 2);
    assert_eq!(board_b.contents(), original());
}

#[tokio::test]
async fn sequence_or_content_replaced_before_paste_sends_nothing_and_preserves_external() {
    for change in [Change::Sequence, Change::Content, Change::Both] {
        let (input, board, driver, win) = fixture().await;
        board.change_on_check(change);
        let error = driver
            .perform(&request(&win), &Cancel::default())
            .await
            .unwrap_err();
        assert!(!error.maybe_executed());
        assert!(input.events().is_empty());
        assert_eq!(board.restores(), 0);
    }
}

#[tokio::test]
async fn focus_changed_before_paste_refuses_and_restores_original() {
    let (input, board, driver, win) = fixture().await;
    input.change_focus_on_check();
    assert!(driver
        .perform(&request(&win), &Cancel::default())
        .await
        .is_err());
    assert!(input.events().is_empty());
    assert_eq!(board.contents(), original());
}

#[tokio::test]
async fn sensitive_or_high_risk_text_uses_chunks_without_clipboard_fallback() {
    for kind in [
        Risk::Password,
        Risk::Web,
        Risk::SensitiveLabel,
        Risk::Unknown,
        Risk::Secret,
    ] {
        let (input, board, driver, win) = fixture().await;
        input.risk(kind);
        assert_eq!(
            driver.perform(&request(&win), &Cancel::default()).await,
            Ok(Delivery::Foreground)
        );
        assert_eq!(board.writes(), 0);
        assert!(input
            .events()
            .iter()
            .all(|event| matches!(event, Event::Unicode(..))));
    }
}

#[tokio::test]
async fn external_copy_after_paste_is_never_overwritten_by_restore() {
    let (input, board, driver, win) = fixture().await;
    input.external_copy_after_paste();
    assert_eq!(
        driver.perform(&request(&win), &Cancel::default()).await,
        Ok(Delivery::Clipboard)
    );
    assert_eq!(board.contents(), external());
    assert_eq!(board.restores(), 0);
}

#[tokio::test]
async fn cancelled_or_fake_stop_before_paste_restores_without_sending() {
    for stop in [false, true] {
        let (input, board, driver, win) = fixture().await;
        let cancel = Cancel::default();
        if stop {
            input.stop_before_paste();
        } else {
            board.cancel_on_check(cancel.clone());
        }
        assert!(driver.perform(&request(&win), &cancel).await.is_err());
        assert!(input.events().is_empty());
        assert_eq!(board.contents(), original());
    }
}

#[tokio::test]
async fn cancelled_partial_paste_is_unknown_no_retry_and_releases_keys() {
    let (input, board, driver, win) = fixture().await;
    let cancel = Cancel::default();
    input.cancel_during_paste(cancel.clone());
    assert!(driver
        .perform(&request(&win), &cancel)
        .await
        .unwrap_err()
        .maybe_executed());
    assert_eq!(board.writes(), 1);
    assert_eq!(board.contents(), original());
    assert_eq!(
        input
            .events()
            .iter()
            .filter(|e| matches!(e, Event::Key(_, true, ..)))
            .count(),
        1
    );
    assert!(input
        .events()
        .iter()
        .any(|e| matches!(e, Event::Key(17, false, ..))));
}

#[tokio::test]
async fn text_over_8192_units_chunks_preserve_surrogates_crlf_and_ceiling() {
    let (_, _, driver, win) = fixture().await;
    let value = format!("{}😀\r\n{}", "中".repeat(8191), "x".repeat(9000));
    let packets = text::chunks(&value).unwrap();
    assert!(packets.iter().all(|p| p.len() <= 512));
    let mut units = Vec::new();
    for packet in packets {
        let chunk: Vec<_> = packet
            .iter()
            .filter_map(|e| match e {
                Event::Unicode(u, true) => Some(*u),
                _ => None,
            })
            .collect();
        assert!(String::from_utf16(&chunk).is_ok());
        assert!(!chunk.ends_with(&[13]));
        units.extend(chunk);
    }
    assert_eq!(String::from_utf16(&units).unwrap(), value);
    assert_eq!(
        driver.capabilities().limits.type_text_max,
        text::TEXT_MAX_UNITS as u32
    );
    assert_eq!(
        text::chunks(&"a".repeat(text::TEXT_MAX_UNITS + 1)),
        Err(DriverError::InvalidRequest)
    );
    let board = Arc::new(FakeClipboard::new(Arc::new(Desktop::default())));
    let input = Arc::new(TestInput::new(board.clone()));
    let backend = InputBackend::new(input.clone());
    let req = DriverAction::TypeText {
        win,
        focus: None,
        text: value,
        route: KeyRoute::TargetedOnly,
    };
    assert_eq!(
        backend
            .perform(
                gcu_driver_windows::testing::window(8, 7),
                &req,
                &Cancel::default(),
                tokio::time::Instant::now() + Duration::from_secs(4)
            )
            .await,
        Ok(Delivery::TargetedEvent)
    );
    assert_eq!(board.writes(), 0);
}

#[cfg(windows)]
#[path = "clipboard_support/native.rs"]
mod native;

#[cfg(all(feature = "test-worker-faults", debug_assertions))]
#[path = "clipboard_support/worker.rs"]
mod worker;

#[tokio::test]
async fn cancelled_in_process_stage_restores_before_next_transaction() {
    let (input, board, driver, win) = fixture().await;
    input.pause_before_paste();
    let driver = Arc::new(driver);
    let d = driver.clone();
    let first = win.clone();
    let task = tokio::spawn(async move { d.perform(&request(&first), &Cancel::default()).await });
    board.wait_staged().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    input.resume();
    assert_eq!(
        driver.perform(&request(&win), &Cancel::default()).await,
        Ok(Delivery::Clipboard)
    );
    assert_eq!(board.contents(), original());
    assert_eq!(board.writes(), 2);
    assert_eq!(
        input.events().len(),
        4,
        "abandoned stage never sends paste keys"
    );
}

#[tokio::test]
async fn failed_restore_after_cancel_is_unknown_and_retains_backup_for_stop() {
    let (input, board, driver, win) = fixture().await;
    let cancel = Cancel::default();
    board.cancel_on_check(cancel.clone());
    board.set_restore_failure(true);
    assert_eq!(
        driver.perform(&request(&win), &cancel).await,
        Err(DriverError::Cancelled {
            maybe_executed: true
        })
    );
    assert!(input.events().is_empty());
    assert!(gcu_driver_windows::input::clipboard::ClipboardApi::pending(
        board.as_ref()
    ));
    assert!(driver
        .perform(&request(&win), &Cancel::default())
        .await
        .is_err());
    assert_eq!(
        board.writes(),
        1,
        "pending recovery cannot start another transaction"
    );
    board.set_restore_failure(false);
    driver.release_all_input().await;
    assert_eq!(board.contents(), original());
}
