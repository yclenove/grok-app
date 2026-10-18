//! Browser profile scope, minor gate, and closed-L1 navigate answers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_host_protocol::messages::{inbound_permitted, Request, VERSION};
use gcu_host_protocol::{clear_profile, messages::RpcError};
use gcu_protocol::builtin_browser::{current_platform, l1_open, navigate_response};
use serde_json::json;

fn request(method: &str, id: Option<u64>) -> Request {
    Request {
        jsonrpc: "2.0".into(),
        id,
        method: method.into(),
        params: json!({}),
    }
}

#[test]
fn browser_methods_require_minor_1_and_an_id() {
    let sync = request("engine.browser.sync", Some(1));
    assert!(!inbound_permitted(false, false, 0, &sync));
    assert!(inbound_permitted(false, false, 1, &sync));
    assert!(!inbound_permitted(true, false, 1, &sync));
    assert!(!inbound_permitted(false, true, 1, &sync));
    let missing = request("engine.browser.sync", None);
    assert!(!inbound_permitted(false, false, 1, &missing));
    let navigate = request("host.browser.navigate", Some(2));
    assert!(inbound_permitted(true, false, 1, &navigate));
    assert!(!inbound_permitted(true, false, 0, &navigate));
    assert!(!inbound_permitted(false, false, 1, &navigate));
    assert!(inbound_permitted(
        false,
        false,
        VERSION.minor,
        &request("engine.session.open", Some(3))
    ));
    assert_eq!(VERSION.minor, 1);
}

#[test]
fn navigate_while_l1_is_closed_never_reports_dispatch() {
    assert!(!l1_open(current_platform()));
    let blocked = navigate_response(json!({
        "url": "javascript:alert(1)",
        "instance_id": "b_0123456789abcdef0123456789abcdef",
        "revision": 1,
        "document_revision": 1,
        "policy_version": 1,
        "session_id": "chat"
    }));
    assert_eq!(blocked, Err((-32014, "browser_url_blocked")));
    let closed = navigate_response(json!({
        "url": "https://example.com/",
        "instance_id": "b_0123456789abcdef0123456789abcdef",
        "revision": 1,
        "document_revision": 1,
        "policy_version": 1,
        "session_id": "chat"
    }));
    assert_eq!(closed, Err((-32010, "browser_unavailable")));
    assert!(closed
        .ok()
        .and_then(|value| value.get("dispatched").cloned())
        .is_none());
    let _ = RpcError::new(-32010, "browser_unavailable");
}

#[test]
fn profile_clear_is_scoped_and_complete() {
    let root = std::env::temp_dir().join(format!("cu-browser-clear-{:x}", rand::random::<u64>()));
    std::fs::create_dir_all(&root).unwrap();
    let sentinel = root.join("keep-me");
    std::fs::write(&sentinel, b"safe").unwrap();
    let profile = root.join("cu-browser-profile");
    std::fs::create_dir(&profile).unwrap();
    std::fs::write(profile.join("cookie"), b"secret").unwrap();
    clear_profile(&root).unwrap();
    assert!(!profile.exists());
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"safe");
    clear_profile(&root).unwrap();
    #[cfg(unix)]
    {
        let outside = root.join("outside-target");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("sentinel"), b"stay").unwrap();
        std::os::unix::fs::symlink(&outside, &profile).unwrap();
        assert!(clear_profile(&root).is_err());
        assert_eq!(std::fs::read(outside.join("sentinel")).unwrap(), b"stay");
        assert!(profile.symlink_metadata().unwrap().file_type().is_symlink());
        std::fs::remove_file(&profile).unwrap();
        std::fs::remove_dir_all(&outside).unwrap();
    }
    std::fs::remove_dir_all(&root).unwrap();
}
