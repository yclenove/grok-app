//! Portable text policy tests; native desktop cases are opt-in.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "support/text.rs"]
mod support;

use gcu_core::driver::{Cancel, Delivery, DriverError, KeyRoute};
use gcu_driver_macos::input::text::{grapheme_chunks, type_text, TextOptions};
use support::Rig;

#[test]
fn unicode_zwj_combining_and_newlines_preserve_graphemes() {
    let text = format!("{}中文👩🏽‍💻e\u{301}\r\n末尾", "a".repeat(19));
    let chunks = grapheme_chunks(&text).unwrap();
    assert_eq!(chunks.concat(), text);
    assert!(chunks.iter().all(|s| s.encode_utf16().count() <= 20));
    assert!(chunks.iter().any(|s| s.contains("👩🏽‍💻")));
    assert!(chunks.iter().any(|s| s.contains("e\u{301}")));
    assert!(chunks.iter().any(|s| s.contains("\r\n")));
    assert_eq!(
        grapheme_chunks(&format!("a{}", "\u{301}".repeat(20))),
        Err(DriverError::InvalidRequest)
    );
}

#[tokio::test]
async fn type_text_plain_settable_appends_without_events() {
    let rig = Rig::plain("prefix");
    assert_eq!(
        rig.run("中文👩🏽‍💻\ne\u{301}", TextOptions::default())
            .await
            .unwrap(),
        Delivery::Accessibility
    );
    assert_eq!(rig.text.value(), "prefix中文👩🏽‍💻\ne\u{301}");
    assert!(rig.events.calls().is_empty());
}

#[tokio::test]
async fn type_text_rich_uses_unicode_and_noneditable_refuses() {
    let rig = Rig::rich();
    assert_eq!(
        rig.run("中文👩🏽‍💻\ne\u{301}", TextOptions::default())
            .await
            .unwrap(),
        Delivery::TargetedEvent
    );
    assert_eq!(rig.unicode(), "中文👩🏽‍💻\ne\u{301}");
    rig.text.editable(false);
    assert_eq!(
        rig.run("x", TextOptions::default()).await.unwrap_err(),
        DriverError::ElementNotActionable
    );
}

#[tokio::test]
async fn type_text_cancel_timeout_and_uncertain_never_replay() {
    let rig = std::sync::Arc::new(Rig::rich());
    let cancel = Cancel::default();
    cancel.cancel();
    assert!(!rig
        .run_cancel("x", TextOptions::default(), &cancel)
        .await
        .unwrap_err()
        .maybe_executed());
    rig.events.hold_posts();
    let running = rig.clone();
    let task = tokio::spawn(async move { running.run("x", TextOptions::default()).await });
    while rig.events.calls().is_empty() {
        tokio::task::yield_now().await;
    }
    rig.clock.advance(support::BUDGET);
    assert!(matches!(
        task.await.unwrap(),
        Err(DriverError::Timeout {
            maybe_executed: true
        })
    ));
    assert!(rig.ledger.is_empty());
    assert_eq!(rig.unicode(), "x");
}

#[tokio::test]
async fn type_text_explicit_stale_focus_refuses_before_mutation() {
    let rig = Rig::plain("");
    let result = type_text(
        &rig.context(),
        &rig.target,
        Some(&gcu_core::model::NativeHandle::new(42u64)),
        "x",
        KeyRoute::TargetedOnly,
        TextOptions::default(),
        &Cancel::default(),
        support::BUDGET,
    )
    .await;
    assert_eq!(result.unwrap_err(), DriverError::StaleElement);
    assert!(rig.events.calls().is_empty());
}

#[tokio::test]
async fn type_text_uncertain_ax_write_never_falls_back() {
    let rig = Rig::plain("original");
    rig.text.fail_append(DriverError::Native {
        message: "unknown".into(),
        maybe_executed: true,
    });
    assert!(rig
        .run("x", TextOptions::default())
        .await
        .unwrap_err()
        .maybe_executed());
    assert!(rig.events.calls().is_empty());
    assert_eq!(rig.board.writes(), 0);
}

#[tokio::test]
async fn type_text_focus_changed_after_first_chunk_is_unknown() {
    let rig = Rig::rich();
    rig.text.change_on_read(3);
    assert!(rig
        .run(&"x".repeat(21), TextOptions::default())
        .await
        .unwrap_err()
        .maybe_executed());
    assert_eq!(rig.unicode(), "x".repeat(20));
    assert!(rig.ledger.is_empty());
}

#[tokio::test]
async fn driver_type_text_runs_window_gate_and_real_text_dispatch() {
    use gcu_core::driver::{AppQuery, Driver, DriverAction, WindowRef};
    use gcu_driver_macos::{
        testing::{cg_window, rect, signed_facts, FakeMacApi},
        MacosDriver,
    };
    use std::sync::Arc;
    let rig = Rig::plain("prefix");
    let api = Arc::new(FakeMacApi::default());
    api.add_running(
        4,
        &signed_facts("com.example.Editor", "/Applications/Editor.app"),
    );
    api.update(|state| {
        state
            .cg_windows
            .push(cg_window(7, 4, "Editor", rect(0.0, 0.0, 100.0, 100.0)));
    });
    let mut driver = MacosDriver::with_api(api.clone()).await.unwrap();
    driver.inject_events(rig.events.clone());
    driver.inject_text(
        rig.text.clone(),
        rig.board.clone(),
        driver.input_guards().unwrap().clock.clone(),
    );
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let windows = driver.list_windows(Some(&app)).await.unwrap();
    let win = WindowRef {
        app,
        id: windows[0].id.clone(),
    };
    let action = DriverAction::TypeText {
        win,
        focus: None,
        text: "中文".into(),
        route: KeyRoute::TargetedOnly,
    };
    assert_eq!(
        driver.perform(&action, &Cancel::default()).await.unwrap(),
        Delivery::Accessibility
    );
    assert_eq!(rig.text.value(), "prefix中文");
    api.update(|state| state.accessibility = false);
    assert!(matches!(
        driver.perform(&action, &Cancel::default()).await,
        Err(DriverError::PermissionMissing { .. })
    ));
    assert_eq!(rig.text.value(), "prefix中文");
    api.update(|state| state.accessibility = true);
    driver.input_guards().unwrap().request_stop();
    for result in [
        driver.perform(&action, &Cancel::default()).await,
        driver
            .perform_text(&action, TextOptions::default(), &Cancel::default())
            .await,
    ] {
        assert_eq!(
            result,
            Err(DriverError::Cancelled {
                maybe_executed: false
            })
        );
    }
    assert_eq!(rig.text.value(), "prefix中文");
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires GUI/TCC and a blank TextEdit fixture; not_run on Linux"]
fn live_textedit_unicode_reaches_fixture() {
    native_smoke("textedit");
}
#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires GUI/TCC and a blank VS Code editor fixture; not_run on Linux"]
fn live_electron_rich_unicode_reaches_fixture() {
    native_smoke("electron");
}
#[cfg(target_os = "macos")]
fn native_smoke(mode: &str) {
    let binary = std::env::var("GCU_TEXT_SMOKE_BIN")
        .expect("build --example text_smoke and set GCU_TEXT_SMOKE_BIN");
    assert!(std::process::Command::new(binary)
        .arg(mode)
        .status()
        .unwrap()
        .success());
}

#[tokio::test]
async fn type_text_limits_and_route_ceiling_prevent_side_effects() {
    let rig = Rig::rich();
    for value in [
        String::new(),
        "x".repeat(8193),
        format!("a{}", "\u{301}".repeat(20)),
    ] {
        assert_eq!(
            rig.run(&value, TextOptions::default()).await.unwrap_err(),
            DriverError::InvalidRequest
        );
    }
    assert!(rig.events.calls().is_empty());
    let mut probe = gcu_driver_macos::input::Probe::verified_os();
    probe.field_51 = false;
    rig.events.set_probe(probe);
    let result = type_text(
        &rig.context(),
        &rig.target,
        None,
        "x",
        KeyRoute::TargetedOnly,
        TextOptions::default(),
        &Cancel::default(),
        support::BUDGET,
    )
    .await;
    assert_eq!(result.unwrap_err(), DriverError::CapabilityUnavailable);
    assert!(rig.events.calls().is_empty());
}

#[tokio::test]
async fn type_text_not_sent_and_uncertain_posts_do_not_retry() {
    for (fault, uncertain) in [
        (gcu_driver_macos::input::PostFault::NotSent, false),
        (gcu_driver_macos::input::PostFault::Uncertain, true),
    ] {
        let rig = Rig::rich();
        rig.events.fail_next(fault);
        assert_eq!(
            rig.run("中文", TextOptions::default())
                .await
                .unwrap_err()
                .maybe_executed(),
            uncertain
        );
        assert_eq!(rig.unicode(), "中文");
        assert!(rig.ledger.is_empty());
        assert_eq!(rig.board.writes(), 0);
    }
}
