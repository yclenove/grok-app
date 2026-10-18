//! Conditional `browser_navigate` registration. `proven: true` is a book input.
//! It is not evidence that a native WebView accepted a network destination.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use gcu_driver_fake::FakeDriver;
use gcu_mcp::{browser_tool, Profile, ServerConfig};
use gcu_protocol::builtin_browser::{
    reset_shared_for_tests, BrowserKind, BrowserSync, BrowserWindow, Geometry, OsPlatform,
};
use serde_json::{json, Value};
use support::Harness;

const SESSION: &str = "chat-browser-e2e";
const INSTANCE: &str = "b_0123456789abcdef0123456789abcdef";

static LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

struct BookGuard;
impl BookGuard {
    fn new() -> Self {
        reset_shared_for_tests();
        Self
    }
}
impl Drop for BookGuard {
    fn drop(&mut self) {
        reset_shared_for_tests();
    }
}

fn window() -> BrowserWindow {
    BrowserWindow {
        epoch: 1,
        document_revision: 3,
        started: 9,
        pid: 42,
        platform: OsPlatform::Linux,
        kind: BrowserKind::BuiltinBrowser,
        instance_id: INSTANCE.into(),
        session_id: SESSION.into(),
        window_id: "native-window-1".into(),
        generation: "gen-1".into(),
        geometry: Geometry {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
            scale: 2.0,
            window_width: 1200.0,
            window_height: 800.0,
        },
    }
}

fn names(listed: &Value) -> Vec<String> {
    listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect()
}

fn code(result: &Value) -> String {
    result["structuredContent"]["error"]["code"]
        .as_str()
        .unwrap_or("")
        .to_owned()
}

fn harness(profile: Profile, session: Option<&str>) -> Harness {
    Harness::configured(
        FakeDriver::from_scene("form").unwrap(),
        &[],
        ServerConfig {
            profile,
            browser_session: session.map(str::to_owned),
        },
    )
}

#[tokio::test]
async fn browser_navigate_registers_and_drops_cached_calls_when_the_owner_clears() {
    let _lock = LOCK.lock().await;
    let _book = BookGuard::new();
    let mut h = harness(Profile::Default, Some(SESSION));
    h.initialize(false).await;
    let before = h.request("tools/list", json!({})).await;
    assert_eq!(names(&before).len(), 15);
    assert!(!names(&before).contains(&"browser_navigate".to_owned()));

    browser_tool::apply_sync(
        BrowserSync {
            revision: 1,
            browser: Some(window()),
        },
        true,
    )
    .await
    .unwrap();
    let note = h.read().await;
    assert_eq!(note["method"], "notifications/tools/list_changed");
    assert!(note.get("id").is_none() || note["id"].is_null());

    let listed = h.request("tools/list", json!({})).await;
    assert!(names(&listed).contains(&"browser_navigate".to_owned()));
    assert_eq!(names(&listed).len(), 16);
    assert!(serde_json::to_vec(&listed["result"]).unwrap().len() <= 14_000);

    let blocked = h
        .call("browser_navigate", json!({"url": "http://127.0.0.1/"}))
        .await;
    assert_eq!(code(&blocked), "key_blocked");
    let closed = h
        .call("browser_navigate", json!({"url": "https://example.com/"}))
        .await;
    assert_eq!(code(&closed), "capability_unavailable");
    assert!(!closed.to_string().contains("dispatched"));

    browser_tool::clear_owner(SESSION).await.unwrap();
    let dropped = h.read().await;
    assert_eq!(dropped["method"], "notifications/tools/list_changed");
    let after = h.request("tools/list", json!({})).await;
    assert!(!names(&after).contains(&"browser_navigate".to_owned()));
    let cached = h
        .call("browser_navigate", json!({"url": "https://example.com/"}))
        .await;
    assert_eq!(code(&cached), "capability_unavailable");
    h.close().await;
}

#[tokio::test]
async fn minimal_profile_and_other_sessions_never_list_browser_navigate() {
    let _lock = LOCK.lock().await;
    let _book = BookGuard::new();
    browser_tool::apply_sync(
        BrowserSync {
            revision: 1,
            browser: Some(window()),
        },
        true,
    )
    .await
    .unwrap();
    for (profile, session, count) in [
        (Profile::Minimal, Some(SESSION), 9),
        (Profile::Full, None, 15),
        (Profile::Default, Some("other-session"), 15),
    ] {
        let mut h = harness(profile, session);
        h.initialize(false).await;
        let listed = h.request("tools/list", json!({})).await;
        let found = names(&listed);
        assert_eq!(found.len(), count);
        assert!(!found.contains(&"browser_navigate".to_owned()));
        let called = h
            .call("browser_navigate", json!({"url": "https://example.com/"}))
            .await;
        assert_eq!(code(&called), "capability_unavailable");
        h.close().await;
    }
}
