use super::{clear_app_data, cu_browser_open, decide_history, decide_navigate};
use gcu_protocol::builtin_browser::{current_platform, l1_open, CHROME_LABEL};

#[test]
fn open_refuses_without_a_content_webview() {
    let source = include_str!("../browser.rs");
    assert!(!source.contains("WebviewWindowBuilder"));
    assert!(!l1_open(current_platform()));
    assert_eq!(cu_browser_open(), Err("browser_unavailable".into()));
}

#[test]
fn chrome_cannot_navigate_or_escalate() {
    assert_eq!(
        decide_navigate(CHROME_LABEL, "javascript:alert(1)"),
        Err("browser_url_blocked")
    );
    assert_eq!(
        decide_navigate(CHROME_LABEL, "https://example.com/"),
        Err("browser_unavailable")
    );
    assert_eq!(
        decide_navigate("cu-browser-content", "https://example.com/"),
        Err("untrusted")
    );
    assert_eq!(
        decide_navigate("main", "https://example.com/"),
        Err("untrusted")
    );
    assert_eq!(decide_history("cu-browser"), Err("untrusted"));
    assert_eq!(decide_history(CHROME_LABEL), Err("browser_unavailable"));
}

#[test]
fn profile_clear_error_is_not_ok() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(1);
    let root = std::env::temp_dir().join(format!("cu-browser-ui-{nanos}"));
    std::fs::create_dir_all(&root).expect("temp");
    let profile = root.join("cu-browser-profile");
    std::fs::create_dir(&profile).expect("profile");
    std::fs::write(profile.join("cookie"), b"secret").expect("cookie");
    clear_app_data(&root).expect("clear");
    assert!(!profile.exists());
    clear_app_data(&root).expect("missing profile");
    let failed = clear_app_data(std::path::Path::new("relative"));
    assert_eq!(failed, Err("browser_clear_failed".to_owned()));
    let _ = std::fs::remove_dir_all(&root);
}
