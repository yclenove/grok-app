//! All clipboard tests use memory only, including on macOS.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "support/text.rs"]
mod support;

use gcu_core::driver::{Delivery, DriverError};
use gcu_driver_macos::input::text::TextOptions;
use support::Rig;

fn allowed() -> TextOptions {
    TextOptions {
        allow_clipboard: true,
        sensitive_text: false,
        high_risk_target: false,
    }
}

#[tokio::test]
async fn clipboard_owned_payload_restores_all_original_formats() {
    let rig = Rig::rich();
    let original = rig.board.snapshot();
    assert_eq!(
        rig.run(&"x".repeat(2001), allowed()).await.unwrap(),
        Delivery::Clipboard
    );
    assert!(rig.board.snapshot().content == original.content);
    assert!(rig.board.snapshot().change_count > original.change_count);
    assert!(rig.ledger.is_empty());
}

#[tokio::test]
async fn clipboard_sensitive_high_risk_and_newline_never_write() {
    for options in [
        TextOptions {
            sensitive_text: true,
            ..allowed()
        },
        TextOptions {
            high_risk_target: true,
            ..allowed()
        },
    ] {
        let rig = Rig::rich();
        assert_eq!(
            rig.run(&"x".repeat(2001), options).await.unwrap_err(),
            DriverError::CapabilityUnavailable
        );
        assert_eq!(rig.board.writes(), 0);
    }
    let rig = Rig::rich();
    assert!(rig
        .run(&format!("{}\n", "x".repeat(2001)), allowed())
        .await
        .is_err());
    assert_eq!(rig.board.writes(), 0);
}

#[tokio::test]
async fn clipboard_foreign_content_before_paste_cancels_without_overwrite() {
    let rig = Rig::rich();
    rig.board.replace_after_write("foreign");
    assert!(!rig
        .run(&"x".repeat(2001), allowed())
        .await
        .unwrap_err()
        .maybe_executed());
    assert!(rig.events.calls().is_empty());
    assert!(rig.board.snapshot().content == support::content("foreign"));
}

#[tokio::test]
async fn clipboard_focus_change_cancels_and_restores_owned_payload() {
    let rig = Rig::rich();
    let original = rig.board.snapshot().content;
    rig.text.change_on_read(3);
    assert!(rig.run(&"x".repeat(2001), allowed()).await.is_err());
    assert!(rig.events.calls().is_empty());
    assert!(rig.board.snapshot().content == original);
}

#[tokio::test]
async fn clipboard_foreign_replacement_after_paste_is_preserved() {
    let rig = Rig::rich();
    rig.text
        .foreign_after_paste(rig.board.clone(), "external-after-paste");
    assert_eq!(
        rig.run(&"x".repeat(2001), allowed()).await.unwrap(),
        Delivery::Clipboard
    );
    assert!(rig.board.snapshot().content == support::content("external-after-paste"));
    assert_eq!(rig.board.writes(), 1);
}

#[tokio::test]
async fn clipboard_sequence_and_content_are_independently_checked() {
    for (payload, increment) in [("x".repeat(2001), true), ("foreign".into(), false)] {
        let rig = Rig::rich();
        rig.board.replace_on_read(2, &payload, increment);
        assert!(rig.run(&"x".repeat(2001), allowed()).await.is_err());
        assert!(rig.events.calls().is_empty());
        assert!(rig.board.snapshot().content == support::content(&payload));
        assert_eq!(rig.board.writes(), 1);
    }
}

#[tokio::test]
async fn clipboard_focus_change_between_modifier_and_v_never_pastes() {
    let rig = Rig::rich();
    rig.text.change_on_read(4);
    assert!(rig
        .run(&"x".repeat(2001), allowed())
        .await
        .unwrap_err()
        .maybe_executed());
    assert!(!rig.events.calls().iter().any(|call| matches!(call, gcu_driver_macos::input::Call::Post(e) if e.down && e.keycode == Some(0x09))));
    assert!(rig.ledger.is_empty());
    assert_eq!(rig.board.writes(), 2);
}

#[tokio::test]
async fn clipboard_background_missing_foreground_sensitive_field_refuses() {
    let rig = Rig::rich();
    rig.text.frontmost(false);
    assert!(rig.run(&"x".repeat(2001), allowed()).await.is_err());
    assert_eq!(rig.board.writes(), 0);
    for sensitive in [true, false] {
        let rig = Rig::rich();
        rig.text.sensitive(sensitive);
        rig.text.high_risk(!sensitive);
        assert!(rig.run(&"x".repeat(2001), allowed()).await.is_err());
        assert_eq!(rig.board.writes(), 0);
    }
}

#[tokio::test]
async fn clipboard_concurrent_sessions_keep_lock_through_restore() {
    use gcu_core::driver::{Cancel, KeyRoute};
    use gcu_driver_macos::input::text::type_text;
    use std::sync::Arc;
    let first = Arc::new(Rig::rich());
    first.text.hold_observed();
    let running = first.clone();
    let task = tokio::spawn(async move { running.run(&"a".repeat(2001), allowed()).await });
    first.text.observed.notified().await;
    let mut second = Rig::rich();
    second.board = first.board.clone();
    let cancelled = Cancel::default();
    let context = second.context();
    let payload = "b".repeat(2001);
    let pending = type_text(
        &context,
        &second.target,
        None,
        &payload,
        KeyRoute::AllowForeground,
        allowed(),
        &cancelled,
        support::BUDGET,
    );
    tokio::pin!(pending);
    // Poll the second call while the first holds the process-global lock.
    tokio::select! { biased; result = &mut pending => panic!("second session escaped lock: {}", result.is_ok()), _ = tokio::task::yield_now() => {} }
    assert_eq!(first.board.writes(), 1);
    cancelled.cancel();
    assert!(matches!(
        pending.await,
        Err(DriverError::Cancelled {
            maybe_executed: false
        })
    ));
    first.text.resume.notify_one();
    assert_eq!(task.await.unwrap().unwrap(), Delivery::Clipboard);
    assert_eq!(
        second.run(&"b".repeat(2001), allowed()).await.unwrap(),
        Delivery::Clipboard
    );
    assert_eq!(first.board.writes(), 4);
}

#[tokio::test]
async fn clipboard_cancel_after_post_releases_and_conditionally_restores() {
    use gcu_core::driver::Cancel;
    use std::sync::Arc;
    let rig = Arc::new(Rig::rich());
    let original = rig.board.snapshot().content;
    rig.text.hold_observed();
    let cancel = Cancel::default();
    let signal = cancel.clone();
    let running = rig.clone();
    let task = tokio::spawn(async move {
        running
            .run_cancel(&"x".repeat(2001), allowed(), &signal)
            .await
    });
    rig.text.observed.notified().await;
    cancel.cancel();
    assert!(matches!(
        task.await.unwrap(),
        Err(DriverError::Cancelled {
            maybe_executed: true
        })
    ));
    // A next transaction can only acquire the lock after native cleanup ends.
    let next = Rig::rich();
    next.run(&"y".repeat(2001), allowed()).await.unwrap();
    assert!(rig.board.snapshot().content == original);
    assert!(rig.ledger.is_empty());
}

#[tokio::test]
async fn clipboard_consumption_deadline_uses_injected_clock_and_restores() {
    let rig = std::sync::Arc::new(Rig::rich());
    let original = rig.board.snapshot().content;
    rig.text.hold_observed();
    let running = rig.clone();
    let task = tokio::spawn(async move { running.run(&"x".repeat(2001), allowed()).await });
    rig.text.observed.notified().await;
    rig.clock.advance(support::BUDGET);
    assert_eq!(
        task.await.unwrap(),
        Err(DriverError::Timeout {
            maybe_executed: true
        })
    );
    Rig::rich().run(&"y".repeat(2001), allowed()).await.unwrap();
    assert!(rig.board.snapshot().content == original);
    assert!(rig.ledger.is_empty());
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires GUI AppKit; isolated named NSPasteboard; not_run on Linux"]
fn live_named_pasteboard_formats_and_change_count() {
    let binary = std::env::var("GCU_TEXT_SMOKE_BIN")
        .expect("build --example text_smoke and set GCU_TEXT_SMOKE_BIN");
    assert!(std::process::Command::new(binary)
        .arg("clipboard")
        .status()
        .unwrap()
        .success());
}

struct ExpireOnWrite {
    board: std::sync::Arc<support::FakePasteboard>,
    clock: std::sync::Arc<gcu_driver_macos::input::clock::FakeClock>,
}
impl gcu_driver_macos::input::clipboard::Pasteboard for ExpireOnWrite {
    fn read(
        &self,
    ) -> gcu_core::driver::Result<gcu_driver_macos::input::clipboard::PasteboardSnapshot> {
        self.board.read()
    }
    fn replace_if(
        &self,
        expected: &gcu_driver_macos::input::clipboard::PasteboardSnapshot,
        content: &gcu_driver_macos::input::clipboard::PasteboardContent,
    ) -> gcu_core::driver::Result<Option<gcu_driver_macos::input::clipboard::PasteboardSnapshot>>
    {
        let result = self.board.replace_if(expected, content)?;
        self.clock.advance(support::BUDGET);
        Ok(result)
    }
}

#[tokio::test]
async fn clipboard_deadline_after_write_restores_without_paste() {
    use gcu_core::driver::{Cancel, KeyRoute};
    use gcu_driver_macos::input::text::type_text;
    use std::sync::Arc;
    let rig = Rig::rich();
    let original = rig.board.snapshot().content;
    let clock = Arc::new(gcu_driver_macos::input::clock::FakeClock::new());
    let mut context = rig.context();
    context.clock = clock.clone();
    context.pasteboard = Arc::new(ExpireOnWrite {
        board: rig.board.clone(),
        clock,
    });
    let result = type_text(
        &context,
        &rig.target,
        None,
        &"x".repeat(2001),
        KeyRoute::AllowForeground,
        allowed(),
        &Cancel::default(),
        support::BUDGET,
    )
    .await;
    assert!(matches!(
        result,
        Err(DriverError::Timeout {
            maybe_executed: false
        })
    ));
    // The caller's deadline can win before the blocking restore completes.
    Rig::rich().run(&"y".repeat(2001), allowed()).await.unwrap();
    assert!(rig.events.calls().is_empty());
    assert!(rig.board.snapshot().content == original);
    assert_eq!(rig.board.writes(), 2);
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires disposable GUI desktop, TextEdit scratch, and fixture-only general clipboard; not_run on Linux"]
fn live_general_paste_consumed_before_fixture_clipboard_restores() {
    let binary = std::env::var("GCU_TEXT_SMOKE_BIN")
        .expect("build --example text_smoke and set GCU_TEXT_SMOKE_BIN");
    assert!(std::process::Command::new(binary)
        .arg("paste")
        .status()
        .unwrap()
        .success());
}
