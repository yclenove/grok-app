//! Deterministic `wait_for` contracts. The clock never sleeps on the wall.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "router_support/mod.rs"]
mod router_support;
mod wait_support;

use gcu_core::{
    action::{ActionRouter, ToolCall},
    driver::{UiEvent, NATIVE_TIMEOUT},
    model::{NodeValue, RawNode, RawTree, States, TextExtras},
    session::{Session, WindowLocks},
    settle::{SettleConfig, SettleEngine},
    wait::{transition, WaitPhase, WaitReport, WaitSignal},
};
use gcu_driver_fake::{faults::Fault, FakeDriver};
use gcu_host_api::{
    testing::{FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals},
    ApprovalDecision, ConfirmationLevel, ForegroundPolicy, PolicyConfig, UserActivity,
};
use gcu_protocol::{
    result::{Delivery, Effect, Outcome},
    tools::*,
    ErrorCode,
};
use std::{sync::Arc, time::Duration};
use wait_support::*;

fn present(text: &str) -> Condition {
    Condition::TextPresent(text.into())
}

fn assert_ready(report: &WaitReport, elapsed_ms: u64) {
    assert_eq!(report.phase, WaitPhase::Ready);
    assert_eq!(report.phase.as_str(), "ready");
    assert!(report.code.is_none());
    assert!(report.raw.is_some());
    assert_eq!(report.elapsed, Duration::from_millis(elapsed_ms));
}

fn conceal_password(raw: &mut RawTree, secret: &str) {
    fn walk(node: &mut RawNode, secret: &str) {
        if node.identifier.as_deref() == Some("text") {
            node.states.insert(States::PASSWORD);
            node.value = Some(NodeValue::Text(secret.into()));
            node.text_extras = Some(TextExtras {
                text: Some(secret.into()),
                ..TextExtras::default()
            });
        }
        for child in &mut node.children {
            walk(child, secret);
        }
    }
    if let Some(root) = &mut raw.root {
        walk(root, secret);
    }
}

async fn finished(task: tokio::task::JoinHandle<Result<WaitReport, ErrorCode>>) -> WaitReport {
    assert!(task.is_finished(), "wait did not finish");
    task.await.unwrap().unwrap()
}

#[tokio::test]
async fn text_present_already_visible_returns_ready() {
    let rig = Rig::new(Subscription::Quiet).await;
    let before = rig.driver.oracle().await;
    let task = rig.start(present("Submit"), 5_000);
    pump().await;
    let report = finished(task).await;
    assert_ready(&report, 0);
    assert_eq!(rig.driver.read_times(), vec![0]);
    assert_eq!(rig.clock.now_ms(), 0);
    assert_eq!(rig.driver.perform_calls(), 0);
    assert_eq!(rig.driver.oracle().await.text, before.text);
    assert!(within_driver_deadline(&rig.driver.timeouts()));
    assert_eq!(NATIVE_TIMEOUT, Duration::from_secs(5));
    assert_eq!(
        gcu_core::driver::DRIVER_DEADLINE,
        Duration::from_millis(4_750)
    );
}

#[tokio::test]
async fn text_absent_missing_string_returns_ready() {
    let rig = Rig::new(Subscription::None).await;
    let task = rig.start(Condition::TextAbsent("missing-text-xyz".into()), 5_000);
    pump().await;
    let report = finished(task).await;
    assert_ready(&report, 0);
    assert_eq!(rig.driver.read_times(), vec![0]);
    assert_eq!(rig.driver.perform_calls(), 0);
}

#[tokio::test]
async fn element_role_and_name_already_present_returns_ready() {
    let rig = Rig::new(Subscription::None).await;
    let task = rig.start(
        Condition::Element(ElementCondition {
            role: "button".into(),
            name: Some("Submit".into()),
        }),
        5_000,
    );
    pump().await;
    let report = finished(task).await;
    assert_ready(&report, 0);
    assert_eq!(rig.driver.read_times(), vec![0]);
}

#[tokio::test]
async fn element_unknown_role_is_refused_without_a_read() {
    let rig = Rig::new(Subscription::None).await;
    let task = rig.start(
        Condition::Element(ElementCondition {
            role: "not-a-role".into(),
            name: None,
        }),
        5_000,
    );
    pump().await;
    assert!(task.is_finished());
    let error = task.await.unwrap().unwrap_err();
    assert_eq!(error, ErrorCode::CapabilityUnavailable);
    assert!(rig.driver.read_times().is_empty());
}

#[tokio::test]
async fn stable_quiet_first_sample_returns_ready() {
    let rig = Rig::new(Subscription::Quiet).await;
    let task = rig.start(Condition::Stable(true), 5_000);
    pump().await;
    let report = finished(task).await;
    assert_ready(&report, 0);
    assert_eq!(rig.driver.read_times(), vec![0]);
}

#[tokio::test]
async fn window_title_matches_fixture_returns_ready() {
    let rig = Rig::new(Subscription::None).await;
    let task = rig.start(Condition::WindowTitle("Fixture form".into()), 5_000);
    pump().await;
    let report = finished(task).await;
    assert_ready(&report, 0);
    assert_eq!(report.info.unwrap().title, "Fixture form");
    assert_eq!(rig.driver.read_times(), vec![0]);
}

#[tokio::test]
async fn text_present_change_event_rereads_without_poll_interval() {
    let rig = Rig::new(Subscription::Quiet).await;
    rig.driver.arm_hold();
    let task = rig.start(present("fixture changed"), 5_000);
    pump().await;
    assert!(!task.is_finished());
    assert_eq!(rig.driver.read_times(), vec![0]);
    rig.driver.replace(changed(&rig.before));
    rig.driver.emit(UiEvent::Changed);
    pump().await;
    assert!(!task.is_finished(), "held read must not count as a match");
    assert_eq!(rig.driver.read_times(), vec![0]);
    rig.driver.release_hold();
    pump().await;
    let report = finished(task).await;
    assert_ready(&report, 0);
    assert_eq!(rig.driver.read_times(), vec![0, 0]);
    assert_eq!(rig.clock.now_ms(), 0);
    assert_eq!(rig.driver.perform_calls(), 0);
}

#[tokio::test]
async fn stable_change_event_requires_second_equal_sample() {
    let rig = Rig::new(Subscription::Quiet).await;
    rig.driver.prime(UiEvent::Changed);
    let task = rig.start(Condition::Stable(true), 5_000);
    pump().await;
    assert!(!task.is_finished());
    assert_eq!(rig.driver.read_times(), vec![0]);
    rig.advance(149).await;
    assert!(!task.is_finished());
    assert_eq!(rig.driver.read_times(), vec![0]);
    rig.advance(1).await;
    let report = finished(task).await;
    assert_ready(&report, 150);
    assert_eq!(rig.driver.read_times(), vec![0, 150]);
}

#[tokio::test]
async fn text_present_without_subscription_polls_at_150ms() {
    let rig = Rig::new(Subscription::None).await;
    let task = rig.start(present("fixture changed"), 5_000);
    pump().await;
    assert!(!task.is_finished());
    assert_eq!(rig.driver.read_times(), vec![0]);
    rig.driver.replace(changed(&rig.before));
    rig.advance(149).await;
    assert!(!task.is_finished());
    assert_eq!(rig.driver.read_times(), vec![0]);
    rig.advance(1).await;
    let report = finished(task).await;
    assert_ready(&report, 150);
    assert_eq!(rig.driver.read_times(), vec![0, 150]);
    assert_eq!(rig.driver.subscriptions(), 0);
}

#[tokio::test]
async fn text_present_never_satisfied_returns_timeout_and_last_tree() {
    let rig = Rig::new(Subscription::None).await;
    let task = rig.start(present("never-appears"), 200);
    pump().await;
    rig.advance(199).await;
    assert!(!task.is_finished());
    rig.advance(1).await;
    assert!(task.is_finished());
    let report = task.await.unwrap().unwrap();
    assert_eq!(report.phase, WaitPhase::TimedOut);
    assert_eq!(report.phase.as_str(), "timed_out");
    assert_eq!(report.code, Some(ErrorCode::Timeout));
    assert!(report.raw.is_some());
    assert_eq!(report.elapsed, Duration::from_millis(200));
    assert_eq!(rig.driver.perform_calls(), 0);
}

#[tokio::test]
async fn timeout_above_30s_is_refused_without_a_read() {
    let rig = Rig::new(Subscription::None).await;
    let task = rig.start(present("Submit"), 30_001);
    pump().await;
    assert!(task.is_finished());
    assert_eq!(
        task.await.unwrap().unwrap_err(),
        ErrorCode::CapabilityUnavailable
    );
    assert!(rig.driver.read_times().is_empty());
}

#[tokio::test]
async fn deadline_at_30s_returns_timeout() {
    let rig = Rig::new(Subscription::None).await;
    let task = rig.start(present("never-appears"), 30_000);
    pump().await;
    rig.advance(29_999).await;
    assert!(!task.is_finished());
    assert!(
        rig.driver.read_times().len() <= 3,
        "{:?}",
        rig.driver.read_times()
    );
    rig.advance(1).await;
    let report = finished(task).await;
    assert_eq!(report.phase, WaitPhase::TimedOut);
    assert_eq!(report.elapsed, Duration::from_millis(30_000));
    assert!(report.raw.is_some());
    assert!(within_driver_deadline(&rig.driver.timeouts()));
}

#[tokio::test]
async fn clock_jump_past_native_ceiling_does_not_busy_poll() {
    let rig = Rig::new(Subscription::None).await;
    let task = rig.start(present("never-appears"), 10_000);
    pump().await;
    rig.advance(6_000).await;
    assert!(!task.is_finished());
    let times = rig.driver.read_times();
    assert!(times.len() <= 3, "{times:?}");
    assert!(within_driver_deadline(&rig.driver.timeouts()));
    assert_eq!(NATIVE_TIMEOUT, Duration::from_secs(5));
    rig.advance(4_000).await;
    let report = finished(task).await;
    assert_eq!(report.phase, WaitPhase::TimedOut);
    assert_eq!(report.elapsed, Duration::from_millis(10_000));
}

#[tokio::test]
async fn empty_text_present_is_not_a_match_all() {
    let rig = Rig::new(Subscription::None).await;
    let task = rig.start(present(""), 1);
    pump().await;
    rig.advance(1).await;
    let report = finished(task).await;
    assert_eq!(report.phase, WaitPhase::TimedOut);
    assert_eq!(rig.driver.read_times(), vec![0]);
}

#[tokio::test]
async fn window_gone_event_returns_window_gone() {
    let rig = Rig::new(Subscription::Quiet).await;
    let task = rig.start(present("never-appears"), 5_000);
    pump().await;
    assert_eq!(rig.driver.read_times(), vec![0]);
    assert!(!task.is_finished());
    rig.driver.emit(UiEvent::WindowGone);
    pump().await;
    assert!(task.is_finished());
    assert_eq!(task.await.unwrap().unwrap_err(), ErrorCode::WindowGone);
    assert_eq!(rig.driver.subscriptions(), 0);
    assert_eq!(rig.driver.perform_calls(), 0);
}

#[tokio::test]
async fn session_close_during_stalled_read_cancels() {
    let rig = Rig::new(Subscription::Quiet).await;
    rig.driver.stall();
    let task = rig.start(present("never-appears"), 5_000);
    pump().await;
    assert_eq!(rig.driver.read_times(), vec![0]);
    rig.session.session_closed().await;
    pump().await;
    assert!(task.is_finished());
    let report = task.await.unwrap().unwrap();
    assert_eq!(report.phase, WaitPhase::Cancelled);
    assert_eq!(report.phase.as_str(), "cancelled");
    assert_eq!(report.code, Some(ErrorCode::ApprovalDenied));
    assert!(report.raw.is_none());
    assert!(rig.driver.cancelled_before_drop());
    assert_eq!(rig.clock.now_ms(), 0);
    assert_eq!(rig.driver.subscriptions(), 0);
}

#[tokio::test]
async fn turn_end_between_polls_returns_approval_denied() {
    let rig = Rig::new(Subscription::None).await;
    let task = rig.start(present("never-appears"), 5_000);
    pump().await;
    assert!(!task.is_finished());
    rig.session.turn_ended().await;
    rig.advance(10).await;
    assert!(task.is_finished());
    let report = task.await.unwrap().unwrap();
    assert_eq!(report.phase, WaitPhase::Cancelled);
    assert_eq!(report.code, Some(ErrorCode::ApprovalDenied));
    assert_eq!(rig.driver.perform_calls(), 0);
}

#[tokio::test]
async fn dropped_wait_cancels_stalled_read() {
    let rig = Rig::new(Subscription::Quiet).await;
    rig.driver.stall();
    let task = rig.start(present("never-appears"), 5_000);
    pump().await;
    assert_eq!(rig.driver.read_times(), vec![0]);
    task.abort();
    pump().await;
    assert!(rig.driver.cancelled_before_drop());
    assert_eq!(rig.driver.subscriptions(), 0);
    assert_eq!(rig.driver.perform_calls(), 0);
}

#[tokio::test]
async fn stop_during_stalled_read_releases_input() {
    let rig = Rig::new(Subscription::Quiet).await;
    rig.driver.stall();
    let task = rig.start(present("never-appears"), 5_000);
    pump().await;
    assert_eq!(rig.driver.read_times(), vec![0]);
    rig.activity.request_stop();
    rig.advance(10).await;
    assert!(task.is_finished());
    let report = task.await.unwrap().unwrap();
    assert_eq!(report.phase, WaitPhase::Cancelled);
    assert_eq!(report.code, Some(ErrorCode::StoppedByUser));
    assert!(report.raw.is_none());
    assert_eq!(rig.driver.release_calls(), 1);
    assert!(rig.activity.latched_stop());
    assert_eq!(rig.driver.subscriptions(), 0);
    assert!(rig.driver.cancelled_before_drop());
}

#[tokio::test]
async fn stalled_read_uses_remaining_horizon_not_native_timeout() {
    let rig = Rig::new(Subscription::None).await;
    rig.driver.stall();
    let task = rig.start(present("never-appears"), 40);
    pump().await;
    assert_eq!(rig.driver.timeouts(), vec![Duration::from_millis(40)]);
    rig.advance(40).await;
    let report = finished(task).await;
    assert_eq!(report.phase, WaitPhase::TimedOut);
    assert!(rig.driver.cancelled_before_drop());
    assert!(within_driver_deadline(&rig.driver.timeouts()));
}

#[tokio::test]
async fn stalled_read_at_driver_deadline_does_not_finish_a_30s_wait() {
    let rig = Rig::new(Subscription::None).await;
    rig.driver.stall();
    let task = rig.start(present("never-appears"), 30_000);
    pump().await;
    assert_eq!(
        rig.driver.timeouts(),
        vec![gcu_core::driver::DRIVER_DEADLINE]
    );
    rig.advance(4_750).await;
    assert!(!task.is_finished());
    assert!(rig.driver.cancelled_before_drop());
    task.abort();
}

#[tokio::test]
async fn event_flood_does_not_spin_or_mutate() {
    let rig = Rig::new(Subscription::Flood).await;
    let before = rig.driver.oracle().await;
    let task = rig.start(present("never-appears"), 40);
    pump().await;
    let reads = rig.driver.read_times().len();
    let polls = rig.driver.event_polls();
    assert!(polls <= 64, "{polls}");
    pump().await;
    assert_eq!(rig.driver.read_times().len(), reads);
    assert_eq!(rig.driver.event_polls(), polls);
    rig.advance(40).await;
    let report = finished(task).await;
    assert_eq!(report.phase, WaitPhase::TimedOut);
    assert_eq!(rig.driver.event_polls(), polls);
    assert!(
        rig.driver.read_times().len() <= 3,
        "{:?}",
        rig.driver.read_times()
    );
    assert_eq!(rig.driver.perform_calls(), 0);
    assert_eq!(rig.driver.oracle().await.text, before.text);
}

#[tokio::test]
async fn password_value_does_not_satisfy_text_present() {
    let rig = Rig::new(Subscription::None).await;
    let mut secret = rig.before.clone();
    conceal_password(&mut secret, "secret-value");
    rig.driver.replace(secret);
    let hidden = rig.start(present("secret-value"), 1);
    pump().await;
    rig.advance(1).await;
    let hidden = finished(hidden).await;
    assert_eq!(hidden.phase, WaitPhase::TimedOut);
    let rig = Rig::new(Subscription::None).await;
    let mut secret = rig.before.clone();
    conceal_password(&mut secret, "secret-value");
    rig.driver.replace(secret);
    let name = rig.start(present("Name"), 5_000);
    pump().await;
    let name = finished(name).await;
    assert_eq!(name.phase, WaitPhase::Ready);
    assert_eq!(rig.driver.perform_calls(), 0);
}

#[tokio::test]
async fn transition_terminal_phases_ignore_later_signals() {
    assert_eq!(
        transition(WaitPhase::Waiting, WaitSignal::StillWaiting),
        WaitPhase::Waiting
    );
    assert_eq!(
        transition(WaitPhase::Waiting, WaitSignal::ConditionMet),
        WaitPhase::Ready
    );
    assert_eq!(
        transition(WaitPhase::Waiting, WaitSignal::Deadline),
        WaitPhase::TimedOut
    );
    assert_eq!(
        transition(WaitPhase::Waiting, WaitSignal::Cancel),
        WaitPhase::Cancelled
    );
    for phase in [WaitPhase::Ready, WaitPhase::TimedOut, WaitPhase::Cancelled] {
        for signal in [
            WaitSignal::StillWaiting,
            WaitSignal::ConditionMet,
            WaitSignal::Deadline,
            WaitSignal::Cancel,
        ] {
            assert_eq!(transition(phase, signal), phase);
        }
    }
    assert_eq!(WaitPhase::Waiting.as_str(), "waiting");
}

fn router_with(
    driver: Arc<FakeDriver>,
    decision: Option<ApprovalDecision>,
) -> (
    Arc<ActionRouter>,
    Arc<Session>,
    Arc<ScriptedApprovals>,
    Arc<FakeUserActivity>,
) {
    let activity = Arc::new(FakeUserActivity::new());
    let approvals = Arc::new(ScriptedApprovals::new());
    if let Some(decision) = decision {
        approvals.push_decision(decision);
    }
    let policy = Arc::new(MemoryPolicyStore::new(PolicyConfig {
        sensitive_action_confirmation: ConfirmationLevel::Off,
        allow_foreground: ForegroundPolicy::Always,
        ..PolicyConfig::default()
    }));
    let router = Arc::new(ActionRouter::new(
        driver,
        approvals.clone(),
        policy,
        Arc::new(RecordingEventSink::new()),
        WindowLocks::new(),
    ));
    let session = Arc::new(Session::open(
        "wait-router",
        activity.clone(),
        router.input_release(),
    ));
    session.begin_turn().unwrap();
    (router, session, approvals, activity)
}

fn login_wait(until: Condition, timeout_ms: u32) -> BatchStep {
    BatchStep::WaitFor(WaitForArgs { until, timeout_ms })
}

#[tokio::test]
async fn router_wait_for_submit_delivers_no_input() {
    let h = router_support::Harness::new(true, true);
    let before = h.driver.oracle().await.unwrap();
    let out = h
        .router
        .wait_for(
            &h.session,
            ToolCall::new("call", "fake.form", login_wait(present("Submit"), 5_000)),
        )
        .await;
    assert!(out.error.is_none(), "{out:?}");
    assert_eq!(out.action.outcome, Outcome::Done);
    assert_eq!(out.action.delivery, Delivery::None);
    assert_eq!(out.action.effect, Effect::NotChecked);
    assert!(out
        .action
        .notes
        .iter()
        .any(|note| note.contains("No input")));
    let state = out.state.unwrap();
    assert!(state.text.contains("Submit"));
    assert!(state.state.action.is_some());
    assert_eq!(h.driver.perform_calls(), 0);
    assert_eq!(h.driver.oracle().await.unwrap().text, before.text);
}

#[tokio::test]
async fn router_timeout_above_30s_and_bad_role_do_not_observe() {
    let h = router_support::Harness::new(true, true);
    for step in [
        login_wait(present("Submit"), 30_001),
        login_wait(
            Condition::Element(ElementCondition {
                role: "not-a-role".into(),
                name: None,
            }),
            5_000,
        ),
        login_wait(Condition::Stable(false), 5_000),
    ] {
        let out = h
            .router
            .execute(&h.session, ToolCall::new("call", "fake.form", step))
            .await;
        assert_eq!(out.error.unwrap().code, ErrorCode::CapabilityUnavailable);
        assert!(out.state.is_none());
    }
    assert!(h.approvals.app_requests().is_empty());
    assert_eq!(h.driver.perform_calls(), 0);
}

#[tokio::test]
async fn router_stop_before_wait_returns_stopped_by_user() {
    let h = router_support::Harness::new(true, true);
    h.activity.request_stop();
    let out = h.execute(login_wait(present("Submit"), 5_000)).await;
    assert_eq!(out.error.unwrap().code, ErrorCode::StoppedByUser);
    assert!(out.state.is_none());
    assert_eq!(h.driver.perform_calls(), 0);
    assert!(h.activity.latched_stop());
}

#[tokio::test]
async fn router_window_gone_returns_window_gone_without_perform() {
    let h = router_support::Harness::new(true, true);
    h.driver
        .inject(Fault::WindowGone {
            window: "form-window".into(),
        })
        .await
        .unwrap();
    let out = h.execute(login_wait(present("Submit"), 5_000)).await;
    assert_eq!(out.error.unwrap().code, ErrorCode::WindowGone);
    assert_eq!(h.driver.perform_calls(), 0);
}

#[tokio::test]
async fn router_denied_observation_does_not_publish_secret() {
    let driver = Arc::new(FakeDriver::from_scene("login").unwrap());
    driver
        .inject(Fault::NodeChanged {
            window: "login-window".into(),
            identifier: "password".into(),
            name: "Password".into(),
            value: Some(NodeValue::Text("secret-value".into())),
        })
        .await
        .unwrap();
    let (router, session, approvals, _) = router_with(driver.clone(), Some(ApprovalDecision::Deny));
    let out = router
        .execute(
            &session,
            ToolCall::new("call", "fake.login", login_wait(present("Log in"), 5_000)),
        )
        .await;
    let error = out.error.unwrap();
    assert_eq!(error.code, ErrorCode::ApprovalDenied);
    assert!(error.state.is_none());
    assert!(out.state.is_none());
    assert!(!error.message.contains("secret-value"));
    assert!(!format!("{error:?}").contains("secret-value"));
    assert_eq!(approvals.app_requests().len(), 1);
    assert!(approvals.confirm_requests().is_empty());
    assert_eq!(driver.perform_calls(), 0);
}

#[tokio::test]
async fn router_granted_observation_masks_password_value() {
    let driver = Arc::new(FakeDriver::from_scene("login").unwrap());
    driver
        .inject(Fault::NodeChanged {
            window: "login-window".into(),
            identifier: "password".into(),
            name: "Password".into(),
            value: Some(NodeValue::Text("secret-value".into())),
        })
        .await
        .unwrap();
    let (router, session, approvals, _) =
        router_with(driver.clone(), Some(ApprovalDecision::Session));
    let out = router
        .wait_for(
            &session,
            ToolCall::new(
                "call",
                "fake.login",
                login_wait(
                    Condition::Element(ElementCondition {
                        role: "button".into(),
                        name: Some("Log in".into()),
                    }),
                    5_000,
                ),
            ),
        )
        .await;
    assert!(out.error.is_none(), "{out:?}");
    assert_eq!(out.action.delivery, Delivery::None);
    let text = out.state.unwrap().text;
    // The snapshot store strips password names and identifiers before delivery.
    assert!(text.contains("•••"), "{text}");
    assert!(text.contains("Log in"), "{text}");
    assert!(!text.contains("secret-value"), "{text}");
    assert!(approvals.confirm_requests().is_empty());
    assert_eq!(driver.perform_calls(), 0);

    let (router, session, _, _) = router_with(driver.clone(), Some(ApprovalDecision::Session));
    let hidden = router
        .execute(
            &session,
            ToolCall::new(
                "call",
                "fake.login",
                login_wait(present("secret-value"), 80),
            ),
        )
        .await;
    assert_eq!(hidden.action.outcome, Outcome::Failed);
    let error = hidden.error.unwrap();
    assert_eq!(error.code, ErrorCode::Timeout);
    let text = hidden.state.unwrap().text;
    assert!(text.contains("•••"), "{text}");
    assert!(!text.contains("secret-value"), "{text}");
    assert!(error.state.as_ref().unwrap().snapshot_id.starts_with("s_"));
    assert!(!format!("{error:?}").contains("secret-value"));
    assert_eq!(driver.perform_calls(), 0);
}

#[tokio::test]
async fn router_manual_clock_waits_past_the_native_ceiling() {
    let rig = Rig::new(Subscription::None).await;
    let approvals = Arc::new(ScriptedApprovals::new());
    approvals.push_decision(ApprovalDecision::Session);
    let policy = Arc::new(MemoryPolicyStore::new(PolicyConfig {
        sensitive_action_confirmation: ConfirmationLevel::Off,
        allow_foreground: ForegroundPolicy::Always,
        ..PolicyConfig::default()
    }));
    let router = Arc::new(
        ActionRouter::new(
            rig.driver.clone(),
            approvals,
            policy,
            Arc::new(RecordingEventSink::new()),
            WindowLocks::new(),
        )
        .with_settle_engine(SettleEngine::new(
            SettleConfig::default(),
            rig.clock.clone(),
        )),
    );
    let session = rig.session.clone();
    let task = tokio::spawn(async move {
        router
            .execute(
                &session,
                ToolCall::new(
                    "call",
                    "fake.form",
                    login_wait(present("never-appears"), 10_000),
                ),
            )
            .await
    });
    pump().await;
    rig.advance(6_000).await;
    assert!(
        !task.is_finished(),
        "a 10s wait must survive the 5s native cap"
    );
    assert!(
        rig.driver.read_times().len() <= 4,
        "{:?}",
        rig.driver.read_times()
    );
    let timeouts = rig.driver.timeouts();
    let above_deadline = timeouts
        .iter()
        .filter(|timeout| **timeout > gcu_core::driver::DRIVER_DEADLINE)
        .count();
    assert!(
        above_deadline <= 1,
        "only the authorization read may use the 5s native ceiling: {timeouts:?}"
    );
    assert!(
        timeouts.iter().all(|timeout| *timeout <= NATIVE_TIMEOUT),
        "{timeouts:?}"
    );
    rig.advance(4_000).await;
    assert!(task.is_finished());
    let out = task.await.unwrap();
    assert_eq!(out.error.as_ref().unwrap().code, ErrorCode::Timeout);
    assert!(out.state.is_some());
    assert!(out.error.unwrap().state.is_some());
    assert_eq!(out.action.delivery, Delivery::None);
    assert_eq!(out.action.outcome, Outcome::Failed);
    assert_eq!(rig.driver.perform_calls(), 0);
    assert_eq!(rig.clock.now_ms(), 10_000);
}

#[test]
fn native_timeout_constant_is_unchanged() {
    assert_eq!(NATIVE_TIMEOUT, Duration::from_secs(5));
    assert_eq!(
        gcu_core::driver::DRIVER_DEADLINE,
        Duration::from_millis(4_750)
    );
}
