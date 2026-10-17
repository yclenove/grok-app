//! Deterministic settle contracts; no wall-clock sleeps or native desktop.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod settle_support;

use gcu_core::{
    driver::{DRIVER_DEADLINE, NATIVE_TIMEOUT},
    model::{Incomplete, IncompleteReason, NodeValue, States},
    settle::{SettleConfig, SettleEngine},
};
use gcu_protocol::{result::SettleReason, ErrorCode};
use settle_support::*;
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn event_quiet_changed_tree_returns_after_quiet_interval() {
    let rig = Rig::new(Subscription::Quiet).await;
    rig.driver.replace(changed(&rig.before));
    let task = rig.start(SettleConfig::default(), 800);
    pump().await;
    rig.advance(119).await;
    assert!(!task.is_finished());
    rig.advance(1).await;
    let report = task.await.unwrap().unwrap();
    assert_eq!(report.settle.reason, SettleReason::EventQuiet);
    assert_eq!(report.settle.ms, 120);
    assert!(!report.timed_out());
    assert_eq!(rig.driver.read_times(), vec![0, 120]);
    assert_eq!(rig.driver.subscriptions(), 0);
}

#[tokio::test]
async fn tree_hash_consistent_without_subscription_polls_at_150ms() {
    let rig = Rig::new(Subscription::None).await;
    rig.driver.replace(changed(&rig.before));
    let task = rig.start(SettleConfig::default(), 800);
    pump().await;
    rig.advance(149).await;
    assert!(!task.is_finished());
    rig.advance(1).await;
    let report = task.await.unwrap().unwrap();
    assert_eq!(report.settle.reason, SettleReason::TreeStable);
    assert_eq!(report.settle.ms, 150);
    assert_eq!(rig.driver.read_times(), vec![0, 150]);
}

#[tokio::test]
async fn subscription_closed_falls_back_to_polling_and_releases_stream() {
    let rig = Rig::new(Subscription::Closed).await;
    rig.driver.replace(changed(&rig.before));
    let task = rig.start(SettleConfig::default(), 800);
    pump().await;
    rig.advance(150).await;
    assert_eq!(
        task.await.unwrap().unwrap().settle.reason,
        SettleReason::TreeStable
    );
    assert_eq!(rig.driver.subscriptions(), 0);
}

#[tokio::test]
async fn budget_deadline_cancels_pending_read_before_dropping_future() {
    let rig = Rig::new(Subscription::Quiet).await;
    rig.driver.stall();
    let task = rig.start(SettleConfig::default(), 40);
    pump().await;
    assert_eq!(rig.driver.read_times(), vec![0]);
    rig.advance(40).await;
    let report = task.await.unwrap().unwrap();
    assert_eq!(report.settle.reason, SettleReason::Budget);
    assert_eq!(report.settle.ms, 40);
    assert!(report.timed_out());
    assert!(rig.driver.cancelled_before_drop());
    assert_eq!(rig.driver.timeouts(), vec![Duration::from_millis(40)]);
    assert_eq!(rig.driver.subscriptions(), 0);
}

#[tokio::test]
async fn stop_during_read_cancels_releases_and_preserves_latch() {
    let rig = Rig::new(Subscription::Quiet).await;
    rig.driver.stall();
    let task = rig.start(SettleConfig::default(), 800);
    pump().await;
    rig.activity.request_stop();
    rig.advance(10).await;
    assert_eq!(task.await.unwrap().unwrap_err(), ErrorCode::StoppedByUser);
    assert!(rig.driver.cancelled_before_drop());
    assert_eq!(rig.driver.release_calls(), 1);
    assert!(rig.session.is_stopped());
    assert_eq!(rig.driver.subscriptions(), 0);
}

#[tokio::test]
async fn session_close_during_read_cancels_without_advancing_clock() {
    let rig = Rig::new(Subscription::Quiet).await;
    rig.driver.stall();
    let task = rig.start(SettleConfig::default(), 800);
    pump().await;
    rig.session.session_closed().await;
    pump().await;
    assert_eq!(task.await.unwrap().unwrap_err(), ErrorCode::ApprovalDenied);
    assert!(rig.driver.cancelled_before_drop());
    assert_eq!(rig.driver.subscriptions(), 0);
}

#[tokio::test]
async fn turn_end_between_polls_cancels_wait() {
    let rig = Rig::new(Subscription::None).await;
    let task = rig.start(SettleConfig::default(), 800);
    pump().await;
    rig.session.turn_ended().await;
    rig.advance(10).await;
    assert_eq!(task.await.unwrap().unwrap_err(), ErrorCode::ApprovalDenied);
}

#[tokio::test]
async fn caller_drops_wait_cancels_read_and_subscription() {
    let rig = Rig::new(Subscription::Quiet).await;
    rig.driver.stall();
    let task = rig.start(SettleConfig::default(), 800);
    pump().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(rig.driver.cancelled_before_drop());
    assert_eq!(rig.driver.subscriptions(), 0);
}

#[tokio::test]
async fn event_flood_yields_to_clock_budget_and_stop() {
    let rig = Rig::new(Subscription::Flood).await;
    let task = rig.start(SettleConfig::default(), 40);
    pump().await;
    let polls = rig.driver.event_polls();
    pump().await;
    assert_eq!(
        rig.driver.event_polls(),
        polls,
        "must await a future deadline"
    );
    rig.advance(40).await;
    assert_eq!(
        task.await.unwrap().unwrap().settle.reason,
        SettleReason::Budget
    );
    assert!(rig.driver.event_polls() <= 128);

    let stopped = Rig::new(Subscription::Flood).await;
    let task = stopped.start(SettleConfig::default(), 800);
    pump().await;
    stopped.activity.request_stop();
    stopped.advance(10).await;
    assert_eq!(task.await.unwrap().unwrap_err(), ErrorCode::StoppedByUser);
}

#[tokio::test]
async fn incomplete_lazy_tree_never_confirms_quiet_until_complete() {
    let rig = Rig::new(Subscription::Quiet).await;
    let mut partial = rig.before.clone();
    partial.incomplete = Some(Incomplete {
        reason: IncompleteReason::PartialError,
        read_nodes: 7,
        unread_nodes: None,
    });
    rig.driver.replace(partial);
    let task = rig.start(SettleConfig::default(), 800);
    pump().await;
    for _ in 0..4 {
        rig.advance(150).await;
        assert!(!task.is_finished());
    }
    rig.driver.replace(changed(&rig.before));
    rig.advance(150).await;
    rig.advance(50).await;
    let report = task.await.unwrap().unwrap();
    assert!(report.observed_change);
    assert!(
        report.timed_out(),
        "one complete sample cannot establish stability"
    );
}

#[tokio::test]
async fn busy_tree_at_budget_cannot_claim_no_visible_change() {
    let rig = Rig::new(Subscription::Quiet).await;
    let mut busy = rig.before.clone();
    busy.root.as_mut().unwrap().states.insert(States::BUSY);
    rig.driver.replace(busy.clone());
    let task = rig.start(SettleConfig::default(), 200);
    pump().await;
    rig.advance(200).await;
    let report = task.await.unwrap().unwrap();
    assert!(report.timed_out());
    assert_eq!(
        report.effect(&busy, &busy, Some(false)).unwrap(),
        gcu_protocol::result::Effect::NotChecked
    );
}

#[tokio::test]
async fn silent_delayed_change_is_observed_before_unchanged_confirmation() {
    let rig = Rig::new(Subscription::Quiet).await;
    let task = rig.start(SettleConfig::default(), 800);
    pump().await;
    rig.advance(120).await;
    assert!(
        !task.is_finished(),
        "unchanged early samples need the full horizon"
    );
    rig.advance(450).await;
    rig.driver.replace(changed(&rig.before));
    rig.advance(150).await;
    rig.advance(80).await;
    let report = task.await.unwrap().unwrap();
    assert!(report.observed_change);
    assert_eq!(
        report
            .effect(&rig.before, &rig.before, Some(false))
            .unwrap(),
        gcu_protocol::result::Effect::Changed
    );
}

#[tokio::test]
async fn unchanged_complete_samples_and_pixels_confirm_no_visible_change() {
    let rig = Rig::new(Subscription::Quiet).await;
    let task = rig.start(SettleConfig::default(), 800);
    pump().await;
    rig.advance(120).await;
    for _ in 0..4 {
        rig.advance(150).await;
    }
    rig.advance(80).await;
    let report = task.await.unwrap().unwrap();
    assert!(!report.timed_out());
    assert_eq!(report.settle.ms, 800);
    assert_eq!(
        report
            .effect(&rig.before, &rig.before, Some(false))
            .unwrap(),
        gcu_protocol::result::Effect::NoVisibleChange
    );
    assert_eq!(
        report.effect(&rig.before, &rig.before, None).unwrap(),
        gcu_protocol::result::Effect::NotChecked
    );
}

#[tokio::test]
async fn effect_raw_unrendered_value_and_pixel_only_change_are_retained() {
    let rig = Rig::new(Subscription::None).await;
    rig.driver.stall();
    let task = rig.start(SettleConfig::default(), 20);
    pump().await;
    rig.advance(20).await;
    let report = task.await.unwrap().unwrap();
    let mut raw = rig.before.clone();
    raw.root.as_mut().unwrap().value = Some(NodeValue::Text("beyond text limit".repeat(100)));
    use gcu_protocol::result::Effect;
    assert_eq!(
        report.effect(&rig.before, &raw, None).unwrap(),
        Effect::Changed
    );
    assert_eq!(
        report.effect(&rig.before, &rig.before, Some(true)).unwrap(),
        Effect::Changed
    );
    assert_eq!(
        report
            .effect(&rig.before, &rig.before, Some(false))
            .unwrap(),
        Effect::NotChecked
    );
}

#[tokio::test]
async fn configurable_parameters_and_zero_intervals_remain_bounded() {
    let rig = Rig::new(Subscription::None).await;
    rig.driver.replace(changed(&rig.before));
    let config = SettleConfig {
        poll_interval: Duration::from_millis(25),
        ..SettleConfig::default()
    };
    let task = rig.start(config, 100);
    pump().await;
    rig.advance(25).await;
    assert_eq!(task.await.unwrap().unwrap().settle.ms, 25);
    let config = SettleConfig {
        event_quiet: Duration::ZERO,
        poll_interval: Duration::ZERO,
        ..SettleConfig::default()
    };
    let engine = SettleEngine::new(config, Arc::new(gcu_core::settle::RuntimeClock::default()));
    assert!(!engine.config().poll_interval.is_zero());
    assert!(!engine.config().event_quiet.is_zero());
    assert_eq!(DRIVER_DEADLINE, Duration::from_millis(4750));
    assert_eq!(NATIVE_TIMEOUT, Duration::from_secs(5));
}

#[tokio::test]
async fn event_at_quiet_boundary_invalidates_old_tree_evidence() {
    let rig = Rig::new(Subscription::Quiet).await;
    rig.driver.replace(changed(&rig.before));
    let task = rig.start(SettleConfig::default(), 800);
    pump().await;
    rig.advance(119).await;
    rig.driver.emit(gcu_core::driver::UiEvent::Changed);
    pump().await;
    rig.advance(1).await;
    assert!(!task.is_finished());
    rig.advance(30).await;
    assert!(
        !task.is_finished(),
        "the first post-event read is not a stable pair"
    );
    rig.advance(89).await;
    let report = task.await.unwrap().unwrap();
    assert_eq!(report.settle.reason, SettleReason::EventQuiet);
    assert_eq!(report.settle.ms, 239);
}

#[tokio::test]
async fn tree_changes_between_polls_reset_hash_stability_until_budget() {
    let rig = Rig::new(Subscription::None).await;
    let task = rig.start(SettleConfig::default(), 400);
    pump().await;
    rig.driver.replace(changed(&rig.before));
    rig.advance(150).await;
    assert!(!task.is_finished());
    rig.driver.replace(rig.before.clone());
    rig.advance(150).await;
    assert!(!task.is_finished());
    rig.advance(100).await;
    let report = task.await.unwrap().unwrap();
    assert!(report.timed_out());
    assert!(
        report.observed_change,
        "a reverted change must not disappear"
    );
}

#[tokio::test]
async fn window_gone_event_cancels_exact_window_read() {
    let rig = Rig::new(Subscription::Quiet).await;
    rig.driver.stall();
    let task = rig.start(SettleConfig::default(), 800);
    pump().await;
    rig.driver.emit(gcu_core::driver::UiEvent::WindowGone);
    pump().await;
    assert_eq!(task.await.unwrap().unwrap_err(), ErrorCode::WindowGone);
    assert!(rig.driver.cancelled_before_drop());
    assert_eq!(rig.driver.subscriptions(), 0);
}

#[tokio::test]
async fn zero_budget_performs_no_reads_and_large_budget_retains_cap() {
    let rig = Rig::new(Subscription::None).await;
    let report = rig
        .start(SettleConfig::default(), 0)
        .await
        .unwrap()
        .unwrap();
    assert!(report.timed_out());
    assert!(rig.driver.read_times().is_empty());
    rig.driver.stall();
    let task = rig.start(SettleConfig::default(), u64::MAX);
    pump().await;
    assert_eq!(rig.driver.timeouts(), vec![Duration::from_millis(1500)]);
    rig.advance(1500).await;
    assert_eq!(task.await.unwrap().unwrap().settle.ms, 1500);
}

#[tokio::test]
async fn native_timeout_shorter_than_budget_cancels_read() {
    let rig = Rig::new(Subscription::None).await;
    rig.driver.stall();
    let engine = SettleEngine::new(SettleConfig::default(), rig.clock.clone());
    let opts = gcu_core::driver::TreeOpts {
        timeout: Duration::from_millis(30),
        ..Default::default()
    };
    let request = gcu_core::settle::SettleRequest {
        driver: rig.driver.as_ref(),
        session: &rig.session,
        window: &rig.window,
        tree_opts: &opts,
        events: None,
        before: &rig.before,
        budget: Duration::from_millis(800),
    };
    let wait = engine.wait(request);
    tokio::pin!(wait);
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(wait.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    rig.clock.advance(30);
    assert_eq!(wait.await.unwrap_err(), ErrorCode::Timeout);
    assert!(rig.driver.cancelled_before_drop());
}

mod router_support;

#[tokio::test]
async fn tree_stable_with_subscription_can_finish_before_long_quiet_interval() {
    let rig = Rig::new(Subscription::Quiet).await;
    rig.driver.replace(changed(&rig.before));
    let config = SettleConfig {
        event_quiet: Duration::from_millis(500),
        poll_interval: Duration::from_millis(25),
        ..Default::default()
    };
    let task = rig.start(config, 800);
    pump().await;
    rig.advance(25).await;
    let report = task.await.unwrap().unwrap();
    assert_eq!(report.settle.reason, SettleReason::TreeStable);
    assert_eq!(report.settle.ms, 25);
}

#[tokio::test]
async fn queued_event_at_budget_prevents_unchanged_confirmation() {
    let rig = Rig::new(Subscription::Quiet).await;
    let task = rig.start(SettleConfig::default(), 800);
    pump().await;
    rig.advance(120).await;
    for _ in 0..4 {
        rig.advance(150).await;
    }
    rig.driver.emit(gcu_core::driver::UiEvent::GeometryChanged);
    rig.advance(80).await;
    let report = task.await.unwrap().unwrap();
    assert!(report.timed_out());
    assert_eq!(
        report
            .effect(&rig.before, &rig.before, Some(false))
            .unwrap(),
        gcu_protocol::result::Effect::NotChecked
    );
}

#[tokio::test]
async fn router_emits_existing_settle_shape_and_unknown_is_never_replayed() {
    use gcu_core::{
        action::ActionRouter,
        session::{Session, WindowLocks},
    };
    use gcu_driver_fake::faults::Fault;
    use gcu_protocol::result::{Effect, Outcome};
    let h = router_support::Harness::new(true, true);
    let clock = Arc::new(ManualClock::default());
    let router = Arc::new(
        ActionRouter::new(
            h.driver.clone(),
            h.approvals.clone(),
            h.policy.clone(),
            h.events.clone(),
            WindowLocks::new(),
        )
        .with_settle_engine(SettleEngine::new(SettleConfig::default(), clock.clone())),
    );
    let session = Arc::new(Session::open(
        "router-settle",
        h.activity.clone(),
        router.input_release(),
    ));
    session.begin_turn().unwrap();
    router
        .observe(
            &session,
            serde_json::from_value(serde_json::json!({"app":"fake.form"})).unwrap(),
        )
        .await
        .unwrap();
    let task = tokio::spawn({
        let router = router.clone();
        let session = session.clone();
        async move {
            router
                .execute(
                    &session,
                    gcu_core::action::ToolCall::new("first", "fake.form", router_support::click(3)),
                )
                .await
        }
    });
    clock.wait_started.notified().await;
    clock.advance(120);
    let out = task.await.unwrap();
    assert!(out.error.is_none());
    assert_eq!(out.action.effect, Effect::Changed);
    let wire = serde_json::to_value(&out.action.settle).unwrap();
    assert_eq!(wire.as_object().unwrap().len(), 2);
    assert_eq!(wire["reason"], "event_quiet");
    assert!(wire["ms"].as_u64().unwrap() >= 120);
    assert_eq!(
        out.state.unwrap().state.action.unwrap().settle,
        out.action.settle
    );
    h.driver
        .inject_action(Fault::Timeout {
            maybe_executed: true,
        })
        .await
        .unwrap();
    let unknown = router
        .execute(
            &session,
            gcu_core::action::ToolCall::new("unknown", "fake.form", router_support::click(3)),
        )
        .await;
    assert_eq!(unknown.action.outcome, Outcome::Unknown);
    assert_eq!(h.driver.perform_calls(), 2);
    assert_eq!(h.driver.oracle().await.unwrap().presses["submit"], 2);
}
