//! Real NDJSON authorization and lifecycle tests with a trusted scripted Host.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support;
use gcu_host_api::{ApprovalDecision, PolicyStore};
use serde_json::json;
use support::{text, Harness};
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn authorization_scopes_follow_turn_session_and_trusted_host_persistence() {
    for decision in [
        ApprovalDecision::Turn,
        ApprovalDecision::Session,
        ApprovalDecision::Always,
    ] {
        // An Unknown fixture has no content hash or Linux dev/inode, so Always
        // must not stick. The file id plus the harness digest is a complete key.
        let mut h = if decision == ApprovalDecision::Always {
            Harness::driver(
                gcu_driver_fake::FakeDriver::from_scene("form")
                    .unwrap()
                    .with_linux_file_id(1, 2)
                    .unwrap(),
                &[decision, ApprovalDecision::Deny],
            )
        } else {
            Harness::new("form", &[decision, ApprovalDecision::Deny])
        };
        h.initialize(false).await;
        let first = h.observe().await;
        assert_ne!(first["isError"], true, "{decision:?}: {first}");
        assert_ne!(h.observe().await["isError"], true);
        assert_eq!(h.approvals.app_requests().len(), 1);
        h.send(json!({"jsonrpc":"2.0","method":"notifications/turn-ended"}))
            .await;
        // Notifications are processed before the following request.
        h.request("ping", json!({})).await;
        let r = h.observe().await;
        if decision == ApprovalDecision::Turn {
            assert_eq!(r["structuredContent"]["error"]["code"], "approval_denied");
            assert_eq!(h.approvals.app_requests().len(), 2);
        } else {
            assert_ne!(r["isError"], true, "{r}");
            assert!(text(&r).contains("App="), "new turn must force full state");
            assert_eq!(h.approvals.app_requests().len(), 1);
        }
        assert_eq!(
            h.policy.load().always.len(),
            usize::from(decision == ApprovalDecision::Always)
        );
        let driver = h.driver.clone();
        h.close().await;
        assert!(driver.release_calls() >= 2);
    }
}

#[tokio::test]
async fn elicitation_cancel_error_or_invalid_answer_falls_back_to_host() {
    for response in [
        json!({"result":{"action":"cancel"}}),
        json!({"error":{"code":-32603,"message":"no UI"}}),
        json!({"result":{"action":"accept","content":{"decision":"invalid"}}}),
    ] {
        let mut h = Harness::new("form", &[ApprovalDecision::Session]);
        h.initialize(true).await;
        h.send(json!({"jsonrpc":"2.0","id":200,"method":"tools/call","params":{"name":"get_app_state","arguments":{"app":"fake.form","include_screenshot":false}}})).await;
        let request = h.read().await;
        assert_eq!(request["method"], "elicitation/create");
        assert_eq!(
            request["params"]["requestedSchema"],
            json!({"type":"object","properties":{"decision":{"type":"string","enum":["turn","session","always","deny"]}},"required":["decision"]})
        );
        let meta = &request["params"]["_meta"];
        assert_eq!(meta["gcu/app"]["key"], "fake.form");
        assert_eq!(meta["gcu/app"]["image_path"], "/fixtures/form");
        assert_eq!(meta["gcu/app"]["signature"], "unknown");
        assert_eq!(meta["gcu/app"]["platform"], "linux");
        assert!(meta["gcu/app"].get("icon").is_none());
        assert_eq!(meta["gcu/risk"], "low");
        assert!(meta["gcu/reason"].is_null());
        let message = request["params"]["message"].as_str().unwrap();
        assert!(message.contains("Fixture form"), "{message}");
        assert!(!message.contains("App identity:"), "{message}");
        let mut response = response;
        response["jsonrpc"] = json!("2.0");
        response["id"] = request["id"].clone();
        h.send(response).await;
        let result = h.read().await;
        assert_eq!(result["id"], 200);
        assert_ne!(result["result"]["isError"], true, "{result}");
        assert_eq!(h.approvals.app_requests().len(), 1);
        h.close().await;
    }
}

#[tokio::test]
async fn elicitation_accept_turn_and_deny_are_honored_without_automatic_always() {
    for decision in ["turn", "session", "always", "deny"] {
        let mut h = Harness::new("form", &[ApprovalDecision::Deny]);
        h.initialize(true).await;
        h.send(json!({"jsonrpc":"2.0","id":200,"method":"tools/call","params":{"name":"get_app_state","arguments":{"app":"fake.form","include_screenshot":false}}})).await;
        let request = h.read().await;
        h.send(json!({"jsonrpc":"2.0","id":request["id"],"result":{"action":"accept","content":{"decision":decision}}})).await;
        let r = h.read().await;
        assert_eq!(
            r["result"]["isError"],
            decision == "always" || decision == "deny",
            "{r}"
        );
        assert_eq!(
            h.approvals.app_requests().len(),
            usize::from(decision == "always")
        );
        assert!(h.policy.load().always.is_empty());
        h.close().await;
    }
}

#[tokio::test]
async fn stdin_eof_cancels_pending_approval_and_releases_input() {
    let mut h = Harness::new("form", &[]);
    h.initialize(false).await;
    h.send(json!({"jsonrpc":"2.0","id":200,"method":"tools/call","params":{"name":"get_app_state","arguments":{"app":"fake.form","include_screenshot":false}}})).await;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while h.approvals.app_requests().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let driver = h.driver.clone();
    h.writer.shutdown().await.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), h.task)
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
    assert!(driver.release_calls() > 0);
}

#[tokio::test]
async fn launch_requires_authorization_and_respects_denylist_stop_and_foreground_policy() {
    let mut h = Harness::new("form", &[ApprovalDecision::Deny]);
    h.initialize(false).await;
    let r = h.call("launch_app", json!({"app":"fake.form"})).await;
    assert_eq!(r["structuredContent"]["error"]["code"], "approval_denied");
    assert_eq!(r["structuredContent"]["action"]["outcome"], "refused");
    h.activity.request_stop();
    let r = h.call("launch_app", json!({"app":"fake.form"})).await;
    assert_eq!(r["structuredContent"]["error"]["code"], "stopped_by_user");
    h.close().await;
    let fixture = gcu_driver_fake::scene::builtin("terminal")
        .unwrap()
        .replace("/fixtures/terminal", "/usr/bin/gnome-terminal");
    let mut h = Harness::driver(
        gcu_driver_fake::FakeDriver::from_ron(&fixture).unwrap(),
        &[],
    );
    h.initialize(false).await;
    let apps = h.call("list_apps", json!({})).await;
    let apps: serde_json::Value = serde_json::from_str(&text(&apps)).unwrap();
    let r = h
        .call("launch_app", json!({"app":apps["apps"][0]["id"]}))
        .await;
    assert_eq!(r["structuredContent"]["error"]["code"], "app_denied");
    assert!(h.approvals.app_requests().is_empty());
    h.close().await;
}

#[test]
fn elicitation_meta_strips_bidirectional_controls_from_model_reason() {
    use gcu_host_api::{AppIdentity, AppInfo, ModelReason, Platform, Risk, SignatureState};
    let app = AppInfo {
        name: "Fixture form".into(),
        pid: Some(1),
        icon: None,
        identity: AppIdentity {
            platform: Platform::Linux,
            key: "fake.form".into(),
            team_id: None,
            publisher: None,
            sha256: None,
            signature: SignatureState::Unknown,
            image_path: "/fixtures/form".into(),
            proof: gcu_host_api::GrantProof::default(),
        },
    };
    let reason = ModelReason::new("\u{202E}<pay>\nnow");
    let meta = gcu_mcp::approval_meta(&app, Some(&reason), Risk::Low);
    let text = meta["gcu/reason"].as_str().unwrap();
    assert!(text.contains('\u{FFFD}'), "{text}");
    assert!(!text.contains('\u{202E}'), "{text}");
    assert!(!text.contains('\n'), "{text}");
    assert!(text.contains("<pay>"), "{text}");
}
