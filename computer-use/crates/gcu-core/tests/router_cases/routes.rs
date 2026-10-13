use crate::router_support::*;
use gcu_core::{action::ToolCall, session::Session};
use gcu_driver_fake::{faults::Fault, FakeDriver};
use gcu_host_api::ApprovalDecision;
use gcu_protocol::{
    result::{Delivery, Outcome},
    tools::*,
    ErrorCode,
};
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn different_sessions_same_window_share_mutation_lock() {
    let h = Harness::new(true, true);
    h.observe().await;
    let other = Arc::new(Session::open(
        "other",
        h.activity.clone(),
        h.router.input_release(),
    ));
    other.begin_turn().unwrap();
    h.approvals.push_decision(ApprovalDecision::Session);
    h.router
        .observe(
            &other,
            serde_json::from_value(serde_json::json!({"app":"fake.form"})).unwrap(),
        )
        .await
        .unwrap();
    h.driver
        .inject_action(Fault::Delay(Duration::from_millis(100)))
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
        .execute(&other, ToolCall::new("second", "fake.form", text("B")))
        .await;
    assert_eq!(first.await.unwrap().action.outcome, Outcome::Done);
    assert_eq!(second.action.outcome, Outcome::Done);
    assert_eq!(h.driver.oracle().await.unwrap().text["text"], "AB");
    assert_eq!(h.session.action_ledger().entries().len(), 1);
    assert_eq!(other.action_ledger().entries().len(), 1);
}
#[tokio::test]
async fn native_deadline_respects_driver_zero_effect_proof_and_never_replays() {
    let h = Harness::new(true, true);
    h.observe().await;
    h.driver
        .inject_action(Fault::Delay(Duration::from_secs(6)))
        .await
        .unwrap();
    let start = std::time::Instant::now();
    let out = h.execute(click(3)).await;
    assert_eq!(out.error.unwrap().code, ErrorCode::Timeout);
    assert_eq!(out.action.outcome, Outcome::Failed);
    assert!(h.driver.oracle().await.unwrap().presses.is_empty());
    assert_eq!(h.driver.perform_calls(), 1);
    assert!(start.elapsed() < Duration::from_secs(6));
}
#[tokio::test]
async fn type_text_selects_targeted_and_foreground_without_semantic_capability() {
    for targeted in [true, false] {
        let h = Harness::driver(
            FakeDriver::from_scene("form")
                .unwrap()
                .with_accessibility(false)
                .with_targeted_input(targeted, targeted),
        );
        h.observe().await;
        let out = h.execute(text("你好👩‍💻")).await;
        assert_eq!(out.action.outcome, Outcome::Done);
        assert_eq!(
            out.action.delivery,
            if targeted {
                Delivery::TargetedEvent
            } else {
                Delivery::Foreground
            }
        );
        assert_eq!(h.driver.oracle().await.unwrap().text["text"], "你好👩‍💻");
        h.observe().await;
        let out = h.execute(click(3)).await;
        assert_eq!(out.action.outcome, Outcome::Done);
        assert_eq!(
            out.action.delivery,
            if targeted {
                Delivery::TargetedEvent
            } else {
                Delivery::Foreground
            }
        );
    }
}
#[tokio::test]
async fn select_range_and_invalid_literal_have_exact_effects() {
    let h = Harness::new(true, true);
    h.observe().await;
    h.execute(set(1, "abcde")).await;
    let range = BatchStep::SelectText(SelectTextArgs {
        element_index: 1,
        text: None,
        start: Some(1),
        length: Some(3),
    });
    assert_eq!(h.execute(range).await.action.outcome, Outcome::Done);
    assert_eq!(h.execute(text("X")).await.action.outcome, Outcome::Done);
    assert_eq!(h.driver.oracle().await.unwrap().text["text"], "aXe");
    let missing = BatchStep::SelectText(SelectTextArgs {
        element_index: 1,
        text: Some("absent".into()),
        start: None,
        length: None,
    });
    assert_eq!(
        h.execute(missing).await.error.unwrap().code,
        ErrorCode::ElementNotActionable
    );
}
#[tokio::test]
async fn scroll_element_and_coordinate_use_correct_routes() {
    let h = Harness::driver(FakeDriver::from_scene("list").unwrap());
    h.router
        .observe(
            &h.session,
            serde_json::from_value(serde_json::json!({"app":"fake.list"})).unwrap(),
        )
        .await
        .unwrap();
    let step = BatchStep::Scroll(ScrollArgs {
        element_index: Some(1),
        x: None,
        y: None,
        direction: Direction::Down,
        pages: 1.,
    });
    let out = h
        .router
        .execute(&h.session, ToolCall::new("scroll", "fake.list", step))
        .await;
    assert_eq!(out.action.outcome, Outcome::Done);
    assert_eq!(out.action.delivery, Delivery::Accessibility);
    h.router
        .observe(
            &h.session,
            serde_json::from_value(serde_json::json!({"app":"fake.list"})).unwrap(),
        )
        .await
        .unwrap();
    let step = BatchStep::Scroll(ScrollArgs {
        element_index: None,
        x: Some(40.),
        y: Some(100.),
        direction: Direction::Down,
        pages: 1.,
    });
    let out = h
        .router
        .execute(&h.session, ToolCall::new("scroll-pixel", "fake.list", step))
        .await;
    assert_eq!(out.action.outcome, Outcome::Done);
    assert_eq!(out.action.delivery, Delivery::TargetedEvent);
    assert!(out.state.unwrap().image.is_some());
}
#[tokio::test]
async fn unsupported_wait_does_not_start_batch_execution() {
    let h = Harness::new(true, true);
    h.observe().await;
    let out = h.execute(BatchStep::Expect(Condition::Stable(true))).await;
    assert_eq!(out.error.unwrap().code, ErrorCode::CapabilityUnavailable);
    assert_eq!(h.driver.perform_calls(), 0);
}
#[tokio::test]
async fn read_timeout_is_refused_and_does_not_mark_unknown() {
    let h = Harness::new(true, true);
    h.observe().await;
    h.driver
        .inject(Fault::Timeout {
            maybe_executed: true,
        })
        .await
        .unwrap();
    let out = h.execute(click(3)).await;
    assert_eq!(out.error.unwrap().code, ErrorCode::Timeout);
    assert_eq!(out.action.outcome, Outcome::Refused);
    assert_eq!(h.driver.perform_calls(), 0);
    assert!(!h.session.action_ledger().has_unknown());
}
#[tokio::test]
async fn screenshotless_observation_never_authorizes_coordinate_input() {
    let h = Harness::new(true, true);
    h.router
        .observe(
            &h.session,
            serde_json::from_value(
                serde_json::json!({"app":"fake.form","include_screenshot":false}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        h.execute(point(80., 145.)).await.error.unwrap().code,
        ErrorCode::StaleScreenshot
    );
    assert_eq!(h.driver.perform_calls(), 0);
}
