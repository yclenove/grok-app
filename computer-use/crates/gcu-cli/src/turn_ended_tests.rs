use super::*;
use gcu_host_api::{testing::ScriptedApprovals, ApprovalDecision};
use std::sync::Arc;

#[tokio::test]
async fn hook_control_turn_grant_expires_once_without_affecting_other_session_or_resume() {
    let fixture = tempfile::TempDir::new().unwrap();
    let mut host = crate::host::LocalHost::fixture(fixture.path());
    let approvals = Arc::new(ScriptedApprovals::new());
    host.host.approvals = approvals.clone();
    let driver = Arc::new(
        gcu_driver_fake::FakeDriver::from_scene("form")
            .unwrap()
            .with_image_digest([9; 32])
            .unwrap(),
    );
    let mut connection =
        gcu_mcp::Connection::open_host_only(driver.clone(), host.host.clone(), Default::default())
            .await
            .unwrap();
    let mut other =
        gcu_mcp::Connection::open_host_only(driver.clone(), host.host.clone(), Default::default())
            .await
            .unwrap();
    let control = resume::Control::start_with_lifecycle(
        connection.session(),
        host.activity.clone(),
        connection.lifecycle(),
    )
    .await
    .unwrap_or_else(|error| panic!("{}", error.message));
    let args = serde_json::json!({"app":"fake.form","include_screenshot":false});
    approvals.push_decision(ApprovalDecision::Turn);
    approvals.push_decision(ApprovalDecision::Turn);
    assert_ne!(
        connection
            .call("get_app_state", args.clone())
            .await
            .is_error,
        Some(true)
    );
    assert_ne!(
        other.call("get_app_state", args.clone()).await.is_error,
        Some(true)
    );
    assert_eq!(approvals.app_requests().len(), 2);
    let token = std::fs::read(control.directory().join("token")).unwrap();
    let payload = br#"{"hookEventName":"stop","reason":"end_turn","future":"ignored"}"#;
    for _ in 0..2 {
        notify(Some(control.directory().into()), payload)
            .await
            .unwrap_or_else(|error| panic!("{}", error.message));
    }
    assert_eq!(driver.release_calls(), 1);
    assert_eq!(
        std::fs::read(control.directory().join("token")).unwrap(),
        token
    );
    assert_ne!(
        other.call("get_app_state", args.clone()).await.is_error,
        Some(true)
    );
    approvals.push_decision(ApprovalDecision::Deny);
    assert_eq!(
        connection
            .call("get_app_state", args)
            .await
            .structured_content
            .unwrap()["error"]["code"],
        "approval_denied"
    );
    assert_eq!(approvals.app_requests().len(), 3);
    connection.close().await.unwrap();
    other.close().await.unwrap();
}

#[tokio::test]
async fn control_invalid_token_cannot_end_authorized_turn_or_clear_stop() {
    let fixture = tempfile::TempDir::new().unwrap();
    let mut host = crate::host::LocalHost::fixture(fixture.path());
    let approvals = Arc::new(ScriptedApprovals::new());
    host.host.approvals = approvals.clone();
    let driver = Arc::new(
        gcu_driver_fake::FakeDriver::from_scene("form")
            .unwrap()
            .with_image_digest([9; 32])
            .unwrap(),
    );
    let mut connection =
        gcu_mcp::Connection::open_host_only(driver.clone(), host.host.clone(), Default::default())
            .await
            .unwrap();
    let control = resume::Control::start_with_lifecycle(
        connection.session(),
        host.activity.clone(),
        connection.lifecycle(),
    )
    .await
    .unwrap_or_else(|error| panic!("{}", error.message));
    approvals.push_decision(ApprovalDecision::Turn);
    let args = serde_json::json!({"app":"fake.form","include_screenshot":false});
    assert_ne!(
        connection
            .call("get_app_state", args.clone())
            .await
            .is_error,
        Some(true)
    );
    let grants = connection.session().grants();
    assert!(resume::send(control.directory(), "turn-ended wrong id")
        .await
        .is_err());
    assert_eq!(connection.session().grants(), grants);
    assert_ne!(
        connection
            .call("get_app_state", args.clone())
            .await
            .is_error,
        Some(true)
    );
    assert_eq!(approvals.app_requests().len(), 1);
    resume::send(control.directory(), "stop")
        .await
        .unwrap_or_else(|error| panic!("{}", error.message));
    notify(
        Some(control.directory().into()),
        br#"{"type":"agent-turn-complete"}"#,
    )
    .await
    .unwrap_or_else(|error| panic!("{}", error.message));
    assert!(connection.session().is_stopped());
    connection.close().await.unwrap();
}
