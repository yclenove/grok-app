use crate::router_support::*;
use gcu_core::{
    action::{ActionLedger, ToolCall},
    driver::{DriverError, PointerRoute},
};
use gcu_driver_fake::{faults::Fault, FakeDriver};
use gcu_protocol::{
    result::{Delivery, Effect, Outcome, SettleReason},
    tools::*,
    ErrorCode,
};
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn targeted_only_failure_never_escalates_to_foreground() {
    let h = Harness::new(false, false);
    h.observe().await;
    let mut call = ToolCall::new("targeted", "fake.form", point(80., 145.));
    call.pointer_route = PointerRoute::TargetedOnly;
    let out = h.router.execute(&h.session, call).await;
    assert_eq!(out.error.unwrap().code, ErrorCode::CapabilityUnavailable);
    assert_eq!(out.action.outcome, Outcome::Failed);
    assert_eq!(out.action.delivery, Delivery::None);
    assert_eq!(h.driver.perform_calls(), 1);
    assert!(h.driver.oracle().await.unwrap().foreground.is_none());
}
#[tokio::test]
async fn crash_uncertainty_records_once_releases_input_and_returns_state() {
    let h = Harness::new(true, true);
    h.observe().await;
    h.driver
        .inject_action(Fault::Crash {
            maybe_executed: true,
        })
        .await
        .unwrap();
    let out = h.execute(key("ctrl+a")).await;
    assert_eq!(out.action.outcome, Outcome::Unknown);
    assert!(!out.error.unwrap().retryable);
    assert!(out.state.is_some());
    assert_eq!(h.driver.perform_calls(), 1);
    assert_eq!(h.driver.oracle().await.unwrap().held_input, 0);
    assert!(h.driver.release_calls() > 0);
    assert!(h.session.action_ledger().unknown("call").is_some());
    assert_eq!(
        h.events.events().last().unwrap().1.outcome,
        Outcome::Unknown
    );
}
#[tokio::test]
async fn definite_timeout_is_failed_without_retry_or_unknown_marker() {
    let h = Harness::new(true, true);
    h.observe().await;
    h.driver
        .inject_action(Fault::Timeout {
            maybe_executed: false,
        })
        .await
        .unwrap();
    let out = h.execute(click(3)).await;
    assert_eq!(out.action.outcome, Outcome::Failed);
    assert_eq!(out.error.unwrap().code, ErrorCode::Timeout);
    assert_eq!(h.driver.perform_calls(), 1);
    assert!(!h.session.action_ledger().has_unknown());
    assert!(h.driver.oracle().await.unwrap().presses.is_empty());
}
#[tokio::test]
async fn stop_during_native_delivery_releases_within_500ms() {
    let h = Harness::new(true, true);
    h.observe().await;
    h.driver
        .inject_action(Fault::Delay(Duration::from_secs(4)))
        .await
        .unwrap();
    let r = h.router.clone();
    let s = h.session.clone();
    let task = tokio::spawn(async move {
        r.execute(&s, ToolCall::new("slow", "fake.form", click(3)))
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        while h.driver.perform_calls() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let start = std::time::Instant::now();
    h.activity.request_stop();
    let out = tokio::time::timeout(Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(out.error.unwrap().code, ErrorCode::StoppedByUser);
    assert!(h.driver.release_calls() > 0);
    assert!(start.elapsed() < Duration::from_millis(500));
    assert!(h.driver.oracle().await.unwrap().presses.is_empty());
}
#[tokio::test]
async fn concurrent_session_calls_are_serial_and_return_ordered_diffs() {
    let h = Harness::new(true, true);
    h.observe().await;
    h.driver
        .inject_action(Fault::Delay(Duration::from_millis(60)))
        .await
        .unwrap();
    let r = h.router.clone();
    let s = h.session.clone();
    let first = tokio::spawn(async move {
        r.execute(&s, ToolCall::new("first", "fake.form", text("A")))
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        while h.driver.perform_calls() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let second = h
        .router
        .execute(&h.session, ToolCall::new("second", "fake.form", text("B")))
        .await;
    assert_eq!(first.await.unwrap().action.outcome, Outcome::Done);
    assert_eq!(second.action.outcome, Outcome::Done);
    assert_eq!(h.driver.oracle().await.unwrap().text["text"], "AB");
    let ledger = h.session.action_ledger();
    assert_eq!(ledger.entries()[0].call_id, "first");
    assert_eq!(ledger.entries()[1].call_id, "second");
    let state = second.state.unwrap();
    assert!(
        state.text.contains("Changes since last state"),
        "{}",
        state.text
    );
}
/// Settle time the fake injects; the budgeted call must end long before it.
const SLOW_SETTLE: Duration = Duration::from_millis(5000);
#[tokio::test]
async fn no_events_uses_tree_polling_and_budget_is_hard() {
    let h = Harness::driver(FakeDriver::from_scene("form").unwrap().with_events(false));
    h.observe().await;
    let out = h.execute(click(3)).await;
    assert_eq!(out.action.settle.reason, SettleReason::TreeStable);
    assert!(out.action.settle.ms >= 150);
    h.driver
        .inject_action(Fault::SlowSettle(SLOW_SETTLE))
        .await
        .unwrap();
    let mut call = ToolCall::new("budget", "fake.form", click(3));
    call.settle_budget_ms = Some(40);
    let out = h.router.execute(&h.session, call).await;
    assert_eq!(out.action.settle.reason, SettleReason::Budget);
    // The budget (40ms) cut a settle that would take SLOW_SETTLE; the bound is half of that,
    // far above timer jitter on a slow runner and far below a settle that ignored the budget.
    assert!(
        out.action.settle.ms < SLOW_SETTLE.as_millis() as u64 / 2,
        "settle took {}ms",
        out.action.settle.ms
    );
}
#[tokio::test]
async fn quiet_events_settle_and_semantic_output_omits_image() {
    let h = Harness::new(true, true);
    h.observe().await;
    let out = h.execute(click(3)).await;
    assert_eq!(out.action.settle.reason, SettleReason::EventQuiet);
    assert!(out.action.settle.ms >= 120);
    assert_eq!(out.action.delivery, Delivery::Accessibility);
    assert_eq!(out.action.effect, Effect::Changed);
    assert!(out.state.unwrap().image.is_none());
    let out = h.execute(point(80., 145.)).await;
    // The earlier semantic press changed this screenshot's target value; require
    // a new screenshot before the model may reuse its pixels.
    assert_eq!(out.error.unwrap().code, ErrorCode::StaleScreenshot);
}
#[tokio::test]
async fn explicit_image_and_coordinate_success_only_return_safe_images() {
    let h = Harness::new(true, true);
    h.observe().await;
    let out = h.execute(point(80., 145.)).await;
    assert_eq!(out.action.delivery, Delivery::TargetedEvent);
    let image: Option<gcu_core::redact::RedactedImage> = out.state.unwrap().image;
    assert!(image.is_some());
    let mut call = ToolCall::new("image", "fake.form", click(3));
    call.attach_image = true;
    assert!(h
        .router
        .execute(&h.session, call)
        .await
        .state
        .unwrap()
        .image
        .is_some());
}
#[tokio::test]
async fn unchanged_effect_is_reported_without_claiming_business_success() {
    let h = Harness::new(true, true);
    h.observe().await;
    let out = h.execute(set(1, "")).await;
    assert_eq!(out.action.outcome, Outcome::Done);
    assert_eq!(out.action.effect, Effect::NoVisibleChange);
    assert!(out
        .action
        .notes
        .iter()
        .any(|n| n.contains("No visible change")));
}
#[tokio::test]
async fn secondary_selection_scroll_and_drag_use_resolved_routes() {
    let h = Harness::new(true, true);
    h.observe().await;
    assert_eq!(
        h.execute(BatchStep::PerformSecondaryAction(
            PerformSecondaryActionArgs {
                element_index: 2,
                action: "toggle".into()
            }
        ))
        .await
        .action
        .outcome,
        Outcome::Done
    );
    h.execute(set(1, "abcde")).await;
    assert_eq!(
        h.execute(BatchStep::SelectText(SelectTextArgs {
            element_index: 1,
            text: Some("bcd".into()),
            start: None,
            length: None
        }))
        .await
        .action
        .outcome,
        Outcome::Done
    );
    assert_eq!(h.execute(text("X")).await.action.outcome, Outcome::Done);
    assert_eq!(h.driver.oracle().await.unwrap().text["text"], "aXe");
    h.observe().await;
    let out = h
        .execute(BatchStep::Drag(DragArgs {
            from_x: 25.,
            from_y: 205.,
            to_x: 180.,
            to_y: 205.,
            screenshot_id: None,
        }))
        .await;
    assert_eq!(out.action.outcome, Outcome::Done);
    assert!(h.driver.oracle().await.unwrap().numbers["slider"] > 0.);
}
#[tokio::test]
async fn non_actionable_returns_fresh_state_and_preserves_oracle() {
    let h = Harness::new(true, true);
    h.observe().await;
    let out = h
        .execute(BatchStep::PerformSecondaryAction(
            PerformSecondaryActionArgs {
                element_index: 3,
                action: "not advertised".into(),
            },
        ))
        .await;
    assert_eq!(
        out.error.as_ref().unwrap().code,
        ErrorCode::ElementNotActionable
    );
    assert!(out.error.unwrap().state.is_some());
    assert_eq!(h.driver.perform_calls(), 0);
}
#[tokio::test]
async fn driver_refusals_after_perform_still_prove_no_effects() {
    for e in [
        DriverError::ElevationRequired,
        DriverError::PermissionMissing {
            permission: "input".into(),
        },
        DriverError::SecureDesktop,
        DriverError::WindowMinimized,
    ] {
        let h = Harness::new(true, true);
        h.observe().await;
        h.driver
            .inject_action(Fault::Error(e.clone()))
            .await
            .unwrap();
        let out = h.execute(click(3)).await;
        assert_eq!(out.error.unwrap().code, e.error_code());
        assert_eq!(out.action.outcome, Outcome::Failed);
        assert!(h.driver.oracle().await.unwrap().presses.is_empty());
    }
}
#[tokio::test]
async fn turn_boundary_forces_full_and_closed_session_never_delivers() {
    let h = Harness::new(true, true);
    h.observe().await;
    h.session.turn_ended().await;
    h.session.begin_turn().unwrap();
    let out = h.execute(click(3)).await;
    assert!(out.state.unwrap().text.starts_with("App="));
    h.session.session_closed().await;
    assert_eq!(
        h.execute(click(3)).await.error.unwrap().code,
        ErrorCode::ApprovalDenied
    );
    assert_eq!(h.driver.perform_calls(), 1);
}
#[tokio::test]
async fn ledger_is_bounded_and_unknown_query_tracks_eviction() {
    let h = Harness::new(true, true);
    h.observe().await;
    h.driver
        .inject_action(Fault::Crash {
            maybe_executed: true,
        })
        .await
        .unwrap();
    let unknown = h.execute(click(3)).await.action;
    let done = h.execute(click(3)).await.action;
    let mut ledger = ActionLedger::with_limit(1);
    ledger.record("u", &unknown, Duration::ZERO);
    assert!(ledger.has_unknown());
    assert!(ledger.unknown("u").is_some());
    ledger.record("d", &done, Duration::from_millis(1));
    assert!(!ledger.has_unknown());
    assert!(ledger.unknown("u").is_none());
    assert_eq!(ledger.entries().len(), 1);
    let mut empty = ActionLedger::with_limit(0);
    empty.record("u", &unknown, Duration::ZERO);
    assert!(empty.entries().is_empty());
    let _shared: Arc<_> = h.router.clone();
}
