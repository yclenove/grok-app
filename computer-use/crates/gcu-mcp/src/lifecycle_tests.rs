#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use gcu_driver_fake::FakeDriver;
use gcu_host_api::{
    testing::{FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals},
    ApprovalDecision, PolicyConfig,
};
use serde_json::json;

async fn fixture() -> (
    crate::Connection,
    Arc<ScriptedApprovals>,
    Arc<FakeUserActivity>,
    Arc<FakeDriver>,
) {
    let approvals = Arc::new(ScriptedApprovals::new());
    let activity = Arc::new(FakeUserActivity::new());
    let driver = Arc::new(
        FakeDriver::from_scene("form")
            .unwrap()
            .with_image_digest([7; 32])
            .unwrap(),
    );
    let host = crate::Host {
        approvals: approvals.clone(),
        activity: activity.clone(),
        policy: Arc::new(MemoryPolicyStore::new(PolicyConfig::default())),
        events: Arc::new(RecordingEventSink::new()),
    };
    let connection = crate::Connection::open_host_only(driver.clone(), host, Default::default())
        .await
        .unwrap();
    (connection, approvals, activity, driver)
}

async fn observe(connection: &crate::Connection) -> rmcp::model::CallToolResult {
    connection
        .call(
            "get_app_state",
            json!({"app":"fake.form", "include_screenshot":false}),
        )
        .await
}

#[tokio::test]
async fn turn_ended_turn_grant_expires_session_grant_survives() {
    for decision in [ApprovalDecision::Turn, ApprovalDecision::Session] {
        let (mut connection, approvals, _, _) = fixture().await;
        approvals.push_decision(decision);
        approvals.push_decision(ApprovalDecision::Deny);
        assert_ne!(observe(&connection).await.is_error, Some(true));
        assert_ne!(observe(&connection).await.is_error, Some(true));
        connection.lifecycle().turn_ended(Some("turn-1")).await;
        assert_eq!(
            observe(&connection).await.is_error,
            Some(decision == ApprovalDecision::Turn)
        );
        assert_eq!(
            approvals.app_requests().len(),
            if decision == ApprovalDecision::Turn {
                2
            } else {
                1
            }
        );
        connection.close().await.unwrap();
    }
}

#[tokio::test]
async fn turn_ended_duplicate_does_not_retire_next_turn_or_clear_stop() {
    let (mut connection, approvals, activity, driver) = fixture().await;
    approvals.push_decision(ApprovalDecision::Turn);
    approvals.push_decision(ApprovalDecision::Turn);
    assert_ne!(observe(&connection).await.is_error, Some(true));
    connection.lifecycle().turn_ended(Some("first")).await;
    let released = driver.release_calls();
    connection.lifecycle().turn_ended(Some("first")).await;
    assert_eq!(driver.release_calls(), released);
    assert_ne!(observe(&connection).await.is_error, Some(true));
    connection.lifecycle().turn_ended(Some("first")).await;
    assert_ne!(observe(&connection).await.is_error, Some(true));
    assert_eq!(approvals.app_requests().len(), 2);
    activity.request_stop();
    connection.lifecycle().turn_ended(Some("second")).await;
    assert!(connection.session().is_stopped());
    assert_eq!(
        observe(&connection).await.structured_content.unwrap()["error"]["code"],
        "stopped_by_user"
    );
    connection.close().await.unwrap();
}

#[tokio::test]
async fn idle_default_45_seconds_expires_once_and_activity_resets_deadline() {
    let (mut connection, approvals, _, driver) = fixture().await;
    approvals.push_decision(ApprovalDecision::Turn);
    approvals.push_decision(ApprovalDecision::Deny);
    assert_ne!(observe(&connection).await.is_error, Some(true));
    let grants = connection.session().grants();
    // Pause after real filesystem/worker startup; auto-advancing a paused
    // runtime during spawn_blocking can otherwise fire its OS-work deadline.
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(44)).await;
    assert_eq!(connection.session().grants(), grants);
    drop(connection.lifecycle().active());
    tokio::time::advance(Duration::from_secs(44)).await;
    assert_eq!(connection.session().grants(), grants);
    assert_eq!(driver.release_calls(), 0);
    tokio::time::sleep(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    assert_eq!(driver.release_calls(), 1);
    tokio::time::advance(Duration::from_secs(90)).await;
    assert_eq!(driver.release_calls(), 1);
    assert_eq!(connection.session().grants(), Default::default());
    tokio::time::resume();
    assert_eq!(observe(&connection).await.is_error, Some(true));
    assert_eq!(approvals.app_requests().len(), 2);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn idle_configured_timeout_does_not_end_busy_approval() {
    let (mut connection, approvals, _, driver) = fixture().await;
    connection
        .lifecycle()
        .set_idle_timeout(Duration::from_secs(3));
    {
        let call = observe(&connection);
        tokio::pin!(call);
        while approvals.app_requests().is_empty() {
            tokio::select! { biased; _ = &mut call => panic!("approval must wait"), _ = tokio::task::yield_now() => () }
        }
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(10)).await;
        tokio::task::yield_now().await;
        assert_eq!(driver.release_calls(), 0);
        tokio::time::resume();
        approvals.push_decision(ApprovalDecision::Turn);
        assert_ne!(call.await.is_error, Some(true));
    }
    tokio::time::pause();
    tokio::time::sleep(Duration::from_secs(3)).await;
    tokio::task::yield_now().await;
    assert_eq!(driver.release_calls(), 1);
    tokio::time::resume();
    connection.close().await.unwrap();
}

#[tokio::test]
async fn turn_ended_pending_approval_is_denied_without_driver_action() {
    let (mut connection, approvals, _, driver) = fixture().await;
    {
        let call = observe(&connection);
        tokio::pin!(call);
        while approvals.app_requests().is_empty() {
            tokio::select! { biased; _ = &mut call => panic!("approval must wait"), _ = tokio::task::yield_now() => () }
        }
        connection.lifecycle().turn_ended(None).await;
        let result = tokio::time::timeout(Duration::from_secs(1), call)
            .await
            .unwrap();
        assert_eq!(
            result.structured_content.unwrap()["error"]["code"],
            "approval_denied"
        );
        assert_eq!(driver.perform_calls(), 0);
    }
    connection.close().await.unwrap();
}
