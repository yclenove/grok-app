use crate::router_support::*;
use gcu_core::{action::ToolCall, model::NodeValue};
use gcu_driver_fake::faults::Fault;
use gcu_host_api::{ApprovalDecision, ConfirmationLevel, ForegroundPolicy, PolicyStore};
use gcu_protocol::{result::Outcome, ErrorCode};
use std::{sync::Arc, time::Duration};

async fn wait_confirm(h: &Harness, count: usize) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while h.approvals.confirm_requests().len() < count {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
async fn wait_app(h: &Harness, count: usize) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while h.approvals.app_requests().len() < count {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
fn strict(h: &Harness) {
    let mut config = h.policy.load();
    config.sensitive_action_confirmation = ConfirmationLevel::Strict;
    h.policy.stage(config);
    h.router.policy_changed();
}
fn spawn(
    h: &Harness,
    step: gcu_protocol::tools::BatchStep,
) -> tokio::task::JoinHandle<gcu_core::action::ToolOutcome> {
    let router = Arc::clone(&h.router);
    let session = Arc::clone(&h.session);
    tokio::spawn(async move {
        router
            .execute(&session, ToolCall::new("pending", "fake.form", step))
            .await
    })
}
#[tokio::test]
async fn approval_wait_session_close_denies_without_delivery() {
    let h = Harness::new(true, true);
    h.observe().await;
    h.session.session_closed().await;
    // A new session has no grants and waits on the same ScriptedApprovals.
    let session = Arc::new(gcu_core::session::Session::open(
        "second",
        h.activity.clone(),
        h.router.input_release(),
    ));
    session.begin_turn().unwrap();
    let router = h.router.clone();
    let pending = session.clone();
    let task = tokio::spawn(async move {
        router
            .execute(
                &pending,
                ToolCall::new("waiting", "fake.form", text("hello")),
            )
            .await
    });
    wait_app(&h, 2).await;
    session.session_closed().await;
    let out = tokio::time::timeout(Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(out.error.unwrap().code, ErrorCode::ApprovalDenied);
    assert_eq!(h.driver.perform_calls(), 0);
    assert!(session.action_ledger().entries().is_empty());
}
#[tokio::test]
async fn confirmation_wait_recipient_or_content_change_invalidates() {
    for (identifier, name, value) in [
        ("submit", "Other recipient", None),
        (
            "text",
            "Name",
            Some(NodeValue::Text("Changed outgoing content".into())),
        ),
    ] {
        let h = Harness::new(true, true);
        h.observe().await;
        strict(&h);
        let task = spawn(&h, click(3));
        wait_confirm(&h, 1).await;
        h.driver
            .inject(Fault::NodeChanged {
                window: "form-window".into(),
                identifier: identifier.into(),
                name: name.into(),
                value,
            })
            .await
            .unwrap();
        h.approvals.push_confirm(true);
        let out = task.await.unwrap();
        assert_eq!(out.error.as_ref().unwrap().code, ErrorCode::StaleElement);
        assert!(out.error.unwrap().state.is_some());
        assert_eq!(h.driver.perform_calls(), 0);
    }
}
#[tokio::test]
async fn confirmation_wait_process_or_policy_change_invalidates() {
    for process in [false, true] {
        let h = Harness::new(true, true);
        h.observe().await;
        strict(&h);
        let task = spawn(&h, click(3));
        wait_confirm(&h, 1).await;
        if process {
            h.driver
                .inject(Fault::ProcessChanged {
                    app: "fake.form".into(),
                    pid: 12345,
                })
                .await
                .unwrap();
        } else {
            let mut c = h.policy.load();
            c.user_idle_ms += 1;
            h.policy.stage(c);
            h.router.policy_changed();
        }
        h.approvals.push_confirm(true);
        let out = task.await.unwrap();
        assert!(matches!(
            out.error.as_ref().unwrap().code,
            ErrorCode::StaleElement | ErrorCode::WindowGone
        ));
        assert!(out.error.unwrap().state.is_some());
        assert_eq!(h.driver.perform_calls(), 0);
    }
}
#[tokio::test]
async fn coordinate_confirmation_wait_target_change_invalidates() {
    let h = Harness::new(true, true);
    h.observe().await;
    strict(&h);
    let task = spawn(&h, point(80., 145.));
    wait_confirm(&h, 1).await;
    h.driver
        .inject(Fault::NodeChanged {
            window: "form-window".into(),
            identifier: "submit".into(),
            name: "Other button".into(),
            value: None,
        })
        .await
        .unwrap();
    h.approvals.push_confirm(true);
    let out = task.await.unwrap();
    assert_eq!(out.error.unwrap().code, ErrorCode::StaleScreenshot);
    assert_eq!(h.driver.perform_calls(), 0);
}
#[tokio::test]
async fn confirmation_wait_stop_cancels_ui_and_releases_within_budget() {
    let h = Harness::new(true, true);
    h.observe().await;
    strict(&h);
    let task = spawn(&h, click(3));
    wait_confirm(&h, 1).await;
    let start = std::time::Instant::now();
    h.activity.request_stop();
    let out = tokio::time::timeout(Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(out.error.unwrap().code, ErrorCode::StoppedByUser);
    assert!(start.elapsed() < Duration::from_millis(500));
    assert!(h.driver.release_calls() > 0);
    assert_eq!(h.driver.perform_calls(), 0);
    h.session.turn_ended().await;
    h.session.begin_turn().unwrap();
    assert_eq!(
        h.execute(click(3)).await.error.unwrap().code,
        ErrorCode::StoppedByUser
    );
}
#[tokio::test]
async fn confirmation_accepted_once_delivers_and_crop_is_safe() {
    let h = Harness::new(true, true);
    h.observe().await;
    strict(&h);
    h.approvals.push_confirm(true);
    let out = h.execute(click(3)).await;
    assert_eq!(out.action.outcome, Outcome::Done);
    assert_eq!(h.approvals.confirm_requests().len(), 1);
    let crop = h.approvals.confirm_requests()[0].crop.clone().unwrap();
    assert!(crop.width.min(crop.height) >= 256);
    assert_eq!(h.driver.perform_calls(), 1);
}
#[tokio::test]
async fn foreground_ask_uses_confirmation_and_never_cannot_be_overridden() {
    for foreground in [ForegroundPolicy::Ask, ForegroundPolicy::Never] {
        let h = Harness::new(false, false);
        h.observe().await;
        let mut c = h.policy.load();
        c.allow_foreground = foreground;
        c.sensitive_action_confirmation = ConfirmationLevel::Strict;
        h.policy.stage(c);
        h.router.policy_changed();
        h.approvals.push_confirm(true);
        h.approvals.push_confirm(true);
        let out = h.execute(key("ctrl+a")).await;
        if foreground == ForegroundPolicy::Never {
            assert_eq!(out.error.unwrap().code, ErrorCode::CapabilityUnavailable);
            assert_eq!(h.driver.perform_calls(), 0);
        } else {
            assert_eq!(out.action.outcome, Outcome::Done);
            assert_eq!(h.approvals.confirm_requests().len(), 2);
        }
    }
}
#[tokio::test]
async fn denied_confirmation_and_app_timeout_do_not_deliver() {
    let h = Harness::new(true, true);
    h.observe().await;
    strict(&h);
    h.approvals.push_confirm(false);
    assert_eq!(
        h.execute(click(3)).await.error.unwrap().code,
        ErrorCode::ApprovalDenied
    );
    let mut c = h.policy.load();
    c.timeout_secs = 0;
    h.policy.stage(c);
    h.router.policy_changed();
    let session =
        gcu_core::session::Session::open("timeout", h.activity.clone(), h.router.input_release());
    let out = h
        .router
        .execute(
            &session,
            ToolCall::new("timeout", "fake.form", text("hello")),
        )
        .await;
    assert_eq!(out.error.unwrap().code, ErrorCode::ApprovalTimeout);
    assert_eq!(h.driver.perform_calls(), 0);
}
#[tokio::test]
async fn background_action_during_physical_input_runs_but_foreground_refuses() {
    for targeted in [true, false] {
        let h = Harness::new(targeted, targeted);
        h.observe().await;
        h.activity
            .set_last_physical_input(std::time::Instant::now());
        let out = h.execute(key("ctrl+a")).await;
        if targeted {
            assert_eq!(out.action.outcome, Outcome::Done);
        } else {
            assert_eq!(out.error.unwrap().code, ErrorCode::UserActive);
            assert_eq!(h.driver.perform_calls(), 0);
        }
    }
}
#[tokio::test]
async fn user_becomes_active_during_foreground_confirmation_refuses() {
    let h = Harness::new(false, false);
    h.observe().await;
    strict(&h);
    let task = spawn(&h, key("ctrl+a"));
    wait_confirm(&h, 1).await;
    h.activity
        .set_last_physical_input(std::time::Instant::now());
    h.approvals.push_confirm(true);
    let out = task.await.unwrap();
    assert_eq!(out.error.unwrap().code, ErrorCode::UserActive);
    assert_eq!(h.driver.perform_calls(), 0);
}
#[tokio::test]
async fn app_deny_and_turn_grant_require_correct_scope() {
    let h = Harness::new(true, true);
    h.observe().await;
    let s = gcu_core::session::Session::open("turn", h.activity.clone(), h.router.input_release());
    s.begin_turn().unwrap();
    h.approvals.push_decision(ApprovalDecision::Deny);
    assert_eq!(
        h.router
            .execute(&s, ToolCall::new("denied", "fake.form", text("hi")))
            .await
            .error
            .unwrap()
            .code,
        ErrorCode::ApprovalDenied
    );
    h.approvals.push_decision(ApprovalDecision::Turn);
    assert_eq!(
        h.router
            .execute(&s, ToolCall::new("turn", "fake.form", text("hi")))
            .await
            .action
            .outcome,
        Outcome::Done
    );
    s.turn_ended().await;
    s.begin_turn().unwrap();
    h.approvals.push_decision(ApprovalDecision::Deny);
    assert_eq!(
        h.router
            .execute(&s, ToolCall::new("expired", "fake.form", text("hi")))
            .await
            .error
            .unwrap()
            .code,
        ErrorCode::ApprovalDenied
    );
    assert_eq!(h.driver.perform_calls(), 1);
}

#[tokio::test]
async fn turn_ended_while_confirmation_pending_denies_without_delivery() {
    let h = Harness::new(true, true);
    h.observe().await;
    strict(&h);
    let task = spawn(&h, click(3));
    wait_confirm(&h, 1).await;
    h.session.turn_ended().await;
    let out = tokio::time::timeout(Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(out.error.unwrap().code, ErrorCode::ApprovalDenied);
    assert_eq!(h.driver.perform_calls(), 0);
}
#[tokio::test]
async fn confirmation_timeout_is_distinct_from_user_denial() {
    let h = Harness::new(true, true);
    h.observe().await;
    strict(&h);
    let mut c = h.policy.load();
    c.timeout_secs = 0;
    h.policy.stage(c);
    h.router.policy_changed();
    assert_eq!(
        h.execute(click(3)).await.error.unwrap().code,
        ErrorCode::ApprovalTimeout
    );
    assert_eq!(h.driver.perform_calls(), 0);
}

#[tokio::test]
async fn web_terminal_click_hands_off_without_perform_or_clearing_stop() {
    let h = Harness::driver(
        gcu_driver_fake::FakeDriver::from_scene("web_terminal")
            .unwrap()
            .with_targeted_input(true, true),
    );
    let params = serde_json::from_value(serde_json::json!({
        "app": "fake.web_terminal",
        "window": "web-terminal-window",
        "include_screenshot": false
    }))
    .unwrap();
    let state = h.router.observe(&h.session, params).await.unwrap();
    let index = state
        .text
        .lines()
        .find(|line| line.contains("button") && line.contains("Run"))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|token| token.parse().ok())
        .unwrap();
    let out = h
        .router
        .execute(
            &h.session,
            ToolCall::new("handoff", "fake.web_terminal", click(index)),
        )
        .await;
    assert_eq!(out.error.unwrap().code, ErrorCode::HandoffRequired);
    assert_eq!(h.driver.perform_calls(), 0);
    assert_eq!(h.approvals.handoffs().len(), 1);
    h.activity.request_stop();
    let stopped = h
        .router
        .execute(
            &h.session,
            ToolCall::new("stopped", "fake.web_terminal", click(index)),
        )
        .await;
    assert_eq!(stopped.error.unwrap().code, ErrorCode::StoppedByUser);
    assert_eq!(h.approvals.handoffs().len(), 1);
    assert_eq!(h.driver.perform_calls(), 0);
}
