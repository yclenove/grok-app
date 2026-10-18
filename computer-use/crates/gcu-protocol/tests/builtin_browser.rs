//! Pure L1 gates. `proven: true` here is a state-machine input, not a native WebView pass.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_protocol::builtin_browser::*;
use gcu_protocol::{browser_navigate_tool_json, tools_list_json};

const INSTANCE: &str = "b_0123456789abcdef0123456789abcdef";
const OTHER: &str = "b_fedcba9876543210fedcba9876543210";

fn window(session: &str, instance: &str) -> BrowserWindow {
    BrowserWindow {
        epoch: 1,
        document_revision: 3,
        started: 9,
        pid: 42,
        platform: OsPlatform::Linux,
        kind: BrowserKind::BuiltinBrowser,
        instance_id: instance.into(),
        session_id: session.into(),
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

fn sync(revision: u64, browser: Option<BrowserWindow>) -> BrowserSync {
    BrowserSync { revision, browser }
}

fn ready_book() -> Book {
    let mut book = Book::empty();
    book.note_policy(7);
    book.apply(&sync(1, Some(window("chat", INSTANCE))), true)
        .unwrap();
    book
}

fn nav<'a>(url: &'a str, stopped: bool) -> NavRequest<'a> {
    NavRequest {
        url,
        session_id: "chat",
        instance_id: INSTANCE,
        revision: 1,
        document_revision: 3,
        policy_version: 7,
        stopped,
    }
}

fn dispatches(plan: &Plan) -> bool {
    matches!(plan, Plan::Ready(_))
}

#[test]
fn platform_proof_is_not_run_and_l1_stays_closed() {
    for platform in [OsPlatform::Macos, OsPlatform::Windows, OsPlatform::Linux] {
        assert_eq!(platform_proof(platform), PlatformProof::NotRun);
        assert!(!l1_open(platform));
    }
    assert!(!l1_open(current_platform()));
}

#[test]
fn public_urls_canonicalize_and_private_forms_are_denied() {
    let allow = [
        ("https://example.com", "https://example.com/"),
        ("https://example.com:443/a", "https://example.com/a"),
        ("http://example.com:80", "http://example.com/"),
        ("https://example.com:8443/a", "https://example.com:8443/a"),
        ("https://example.com./a", "https://example.com/a"),
        (
            "https://example.com/javascript:alert(1)",
            "https://example.com/javascript:alert(1)",
        ),
        ("HTTPS://Example.COM/x", "https://example.com/x"),
        (
            "https://xn--e1afmkfd.example/",
            "https://xn--e1afmkfd.example/",
        ),
    ];
    for (raw, canonical) in allow {
        assert_eq!(
            classify(raw, UrlRole::Navigation),
            UrlClass::Public(canonical.into()),
            "{raw}"
        );
    }
    let deny = [
        "http://127.0.0.1/",
        "http://127.1/",
        "http://127.0.1/",
        "http://0x7f000001/",
        "http://0x7f.0.0.1/",
        "http://0177.0.0.1/",
        "http://2130706433/",
        "http://08/",
        "http://10.1.2.3/",
        "http://192.168.0.1/",
        "http://172.16.0.1/",
        "http://169.254.169.254/latest",
        "http://169.254.170.2/",
        "http://100.100.100.200/",
        "http://100.64.1.1/",
        "http://0.0.0.0/",
        "http://224.0.0.1/",
        "http://255.255.255.255/",
        "http://192.0.2.1/",
        "http://192.0.0.1/",
        "http://198.51.100.1/",
        "http://203.0.113.1/",
        "http://198.18.0.1/",
        "http://[::1]/",
        "http://[::ffff:8.8.8.8]/",
        "http://[2001:db8::1]/",
        "https://metadata.google.internal/",
        "https://METADATA.GOOGLE.INTERNAL./",
        "https://localhost/",
        "https://foo.localhost/",
        "https://printer.local/",
        "https://printer/",
        "https://example.com../",
        "javascript:alert(1)",
        "file:///etc/passwd",
        "data:text/html,hi",
        "vbscript:msgbox",
        "blob:https://example.com/id",
        "mailto:a@example.com",
        "tel:1",
        "ftp://example.com/",
        "https://tauri.localhost/",
        "https://trusted.example@evil.example/",
        "https://example.com\\@evil.example/",
        "http://127.0.0.1%2e1/",
        "https://ex ample.com/",
        "https://bücher.example/",
        "https://example.com/\u{202e}x",
        "ws://example.com/",
        "about:blank",
        "tauri://localhost",
        "https://user:pass@example.com/",
    ];
    for raw in deny {
        assert_eq!(
            classify(raw, UrlRole::Navigation),
            UrlClass::Denied,
            "{raw}"
        );
    }
    assert!(matches!(
        classify("wss://example.com/socket", UrlRole::Subresource),
        UrlClass::Public(_)
    ));
    assert_eq!(
        classify("ws://127.0.0.1/socket", UrlRole::Subresource),
        UrlClass::Denied
    );
    assert_eq!(
        classify(
            &format!("https://example.com/{}", "a".repeat(URL_MAX_BYTES)),
            UrlRole::Navigation
        ),
        UrlClass::Denied
    );
    assert!(matches!(
        classify("https://[2606:4700:4700::1111]/", UrlRole::Navigation),
        UrlClass::Public(_)
    ));
}

#[test]
fn navigation_dangerous_url_has_zero_dispatch() {
    let mut dispatched = 0u32;
    let book = ready_book();
    for url in [
        "javascript:alert(1)",
        "file:///tmp/x",
        "data:text/html,x",
        "http://127.0.0.1/",
    ] {
        let plan = book.plan(nav(url, false), true);
        assert!(matches!(plan, Plan::Refuse(NavFault::UrlBlocked)), "{url}");
        if dispatches(&plan) {
            dispatched += 1;
        }
        let response = navigate_response(serde_json::json!({
            "url": url,
            "instance_id": INSTANCE,
            "revision": 1,
            "document_revision": 3,
            "policy_version": 7,
            "session_id": "chat"
        }));
        assert_eq!(response, Err((-32014, "browser_url_blocked")));
    }
    let closed = book.plan(nav("https://example.com/", false), false);
    assert!(matches!(closed, Plan::Refuse(NavFault::Unavailable)));
    if dispatches(&closed) {
        dispatched += 1;
    }
    assert_eq!(dispatched, 0);
    assert_eq!(
        navigate_response(serde_json::json!({
            "url": "https://example.com/",
            "instance_id": INSTANCE,
            "revision": 1,
            "document_revision": 3,
            "policy_version": 7,
            "session_id": "chat"
        })),
        Err((-32010, "browser_unavailable"))
    );
    assert_eq!(
        navigate_response(serde_json::json!({
            "url": "javascript:alert(1)",
            "instance_id": INSTANCE,
            "revision": 1,
            "policy_version": 7,
            "session_id": "chat"
        })),
        Err((-32602, "invalid parameters"))
    );
}

#[test]
fn delivered_navigation_is_not_recorded_as_unknown() {
    let delivered = classify_navigated(&serde_json::json!({
        "instance_id": INSTANCE,
        "dispatched": true
    }));
    assert!(delivered.is_ok());
    let extra = classify_navigated(&serde_json::json!({
        "instance_id": INSTANCE,
        "dispatched": true,
        "document_revision": 3
    }));
    assert!(extra.is_ok());
    assert_eq!(
        classify_navigated(&serde_json::json!({
            "instance_id": INSTANCE,
            "dispatched": false
        })),
        Err(NavFault::Internal)
    );
    assert_eq!(
        classify_navigated(&serde_json::json!({"instance_id": INSTANCE})),
        Err(NavFault::Unknown)
    );
    assert_eq!(host_error(NavFault::Busy), (-32002, "busy"));
    assert_eq!(host_error(NavFault::Internal), (-32603, "internal error"));
    assert_eq!(
        model_error(NavFault::Busy),
        Some(gcu_protocol::ErrorCode::Busy)
    );
    assert_eq!(
        model_error(NavFault::Internal),
        Some(gcu_protocol::ErrorCode::Internal)
    );
    assert!(model_error(NavFault::Unknown).is_none());
}

#[test]
fn private_network_requests_never_connect() {
    assert_eq!(platform_proof(OsPlatform::Linux), PlatformProof::NotRun);
    assert_eq!(platform_proof(OsPlatform::Macos), PlatformProof::NotRun);
    assert_eq!(platform_proof(OsPlatform::Windows), PlatformProof::NotRun);
    let mut connections = 0u32;
    let book = Book::empty();
    for url in [
        "http://10.0.0.2/",
        "http://192.168.1.1/",
        "https://metadata.google.internal/computeMetadata/v1/",
        "http://[fc00::1]/",
        "http://[fe80::1]/",
    ] {
        let plan = book.plan(nav(url, false), false);
        assert!(!dispatches(&plan), "{url}");
        if dispatches(&plan) {
            connections += 1;
        }
    }
    assert_eq!(connections, 0);
}

#[test]
fn frames_popups_redirects_keep_zero_capabilities() {
    let mut dispatched = 0u32;
    let book = ready_book();
    for url in [
        "javascript:top.location='http://127.0.0.1/'",
        "file:///etc/passwd",
        "https://tauri.localhost/asset",
        "http://169.254.169.254/latest/meta-data",
        "intent://scan/#Intent;scheme=http;end",
        "grok://open",
        "asset://localhost/index.html",
        "media://token",
    ] {
        let plan = book.plan(nav(url, false), true);
        if dispatches(&plan) {
            dispatched += 1;
        }
        assert!(rejects_command(CONTENT_LABEL, "cu_browser_navigate"));
        assert!(rejects_command(WINDOW_LABEL, "cu_browser_state"));
    }
    assert_eq!(dispatched, 0);
    assert!(!rejects_command("main", "cu_browser_navigate"));
}

#[test]
fn remote_content_cannot_reach_app_surfaces() {
    for command in [
        "cu_approval_decide",
        "media_server_endpoint",
        "session_send",
        "plugin:event|emit",
        "cu_browser_navigate",
        "cu_browser_clear",
    ] {
        assert!(rejects_command(CONTENT_LABEL, command), "{command}");
        assert!(rejects_command(WINDOW_LABEL, command), "{command}");
    }
    assert_eq!(
        classify("https://tauri.localhost/", UrlRole::Navigation),
        UrlClass::Denied
    );
    assert_eq!(
        classify("http://127.0.0.1:9/mcp", UrlRole::Subresource),
        UrlClass::Denied
    );
}

#[test]
fn chrome_origin_cannot_be_spoofed() {
    assert_eq!(
        display_origin("https://example.com/a/b?q=1"),
        Some("https://example.com".into())
    );
    assert_eq!(
        display_origin("https://example.com:8443/a"),
        Some("https://example.com:8443".into())
    );
    assert_eq!(display_origin("https://user@evil.example/"), None);
    assert_eq!(display_origin("https://bücher.example/"), None);
    assert_eq!(display_origin("https://example.com/\u{202e}hidden"), None);
    assert_eq!(
        display_origin("https://xn--e1afmkfd.example/path"),
        Some("https://xn--e1afmkfd.example".into())
    );
    let long = format!("https://example.com/{}", "p".repeat(80));
    let shown = abbreviate_address(&long, 40).unwrap();
    assert!(shown.starts_with("https://example.com/"));
    assert!(shown.ends_with('…'));
    assert!(shown.contains("example.com"));
    assert!(escape_untrusted("a\u{202e}b").contains("\\u{202e}"));
}

#[test]
fn chrome_cannot_navigate_or_escalate() {
    for command in CHROME_COMMANDS {
        assert!(!rejects_command(CHROME_LABEL, command), "{command}");
        assert!(rejects_command(CONTENT_LABEL, command), "{command}");
        assert!(rejects_command(WINDOW_LABEL, command), "{command}");
    }
    assert!(rejects_command(CHROME_LABEL, "cu_approval_decide"));
    assert!(rejects_command(CHROME_LABEL, "cu_browser_clear"));
    assert!(rejects_command(CHROME_LABEL, "cu_browser_open"));
    let book = ready_book();
    assert!(matches!(
        book.plan(nav("javascript:void(0)", false), true),
        Plan::Refuse(NavFault::UrlBlocked)
    ));
}

#[test]
fn browser_binding_reuse_fails_closed() {
    let mut book = Book::empty();
    let short = {
        let mut window = window("chat", INSTANCE);
        window.instance_id = "b_01".into();
        window
    };
    assert_eq!(
        book.apply(&sync(1, Some(short)), true),
        Err(ApplyFault::Invalid)
    );
    assert_eq!(book.revision(), 0);
    assert_eq!(
        book.apply(&sync(1, Some(window("chat", INSTANCE))), false),
        Err(ApplyFault::Unavailable)
    );
    assert!(book.owner().is_none());
    book.apply(&sync(1, Some(window("chat", INSTANCE))), true)
        .unwrap();
    assert_eq!(book.apply(&sync(1, None), true), Err(ApplyFault::Stale));
    book.apply(&sync(2, None), true).unwrap();
    assert!(book.owner().is_none());
    assert_eq!(
        book.apply(&sync(3, Some(window("chat", INSTANCE))), true),
        Err(ApplyFault::Stale)
    );
    book.apply(&sync(3, Some(window("chat", OTHER))), true)
        .unwrap();
    assert_eq!(book.owner(), Some("chat"));
    book.apply(&sync(4, None), false).unwrap();
    let mut again = window("chat", OTHER);
    again.epoch = 2;
    assert_eq!(
        book.apply(&sync(5, Some(again)), true),
        Err(ApplyFault::Stale)
    );
}

#[test]
fn browser_images_are_owned_and_revocable() {
    let mut book = ready_book();
    book.bind_image("chat", "img-1", INSTANCE, 3).unwrap();
    assert!(book.lookup_image("chat", "img-1").is_ok());
    assert_eq!(book.lookup_image("other", "img-1"), Err(ImageFault::Owner));
    assert!(book.image_blocked("other", "img-1"));
    assert!(!book.image_blocked("chat", "missing"));
    let mut next = window("chat", INSTANCE);
    next.document_revision = 4;
    book.apply(&sync(2, Some(next)), true).unwrap();
    assert_eq!(book.lookup_image("chat", "img-1"), Err(ImageFault::Stale));
    assert_eq!(
        book.bind_image("chat", "img-1", INSTANCE, 3),
        Err(ImageFault::Stale)
    );
    book.bind_image("chat", "img-2", INSTANCE, 4).unwrap();
    book.apply(&sync(3, None), true).unwrap();
    assert!(book.revoked_image_ids().iter().any(|id| id == "img-2"));
    assert_eq!(book.lookup_image("chat", "img-2"), Err(ImageFault::Gone));
    assert_eq!(
        book.bind_image("chat", "img-3", INSTANCE, 4),
        Err(ImageFault::Late)
    );
    assert!(book.image_blocked("chat", "img-2"));
}

#[test]
fn navigation_stop_and_approval_races_are_safe() {
    let book = ready_book();
    let mut dispatched = 0u32;
    let stopped = book.plan(nav("https://example.com/go", true), true);
    assert!(matches!(stopped, Plan::Refuse(NavFault::Stopped)));
    if dispatches(&stopped) {
        dispatched += 1;
    }
    let mut stale = nav("https://example.com/go", false);
    stale.revision = 0;
    let stale = book.plan(stale, true);
    assert!(matches!(stale, Plan::Refuse(NavFault::Stale)));
    let mut document = nav("https://example.com/go", false);
    document.document_revision = 2;
    let document = book.plan(document, true);
    assert!(matches!(document, Plan::Refuse(NavFault::Stale)));
    if dispatches(&document) {
        dispatched += 1;
    }
    if dispatches(&stale) {
        dispatched += 1;
    }
    let mut unset = Book::empty();
    unset
        .apply(&sync(1, Some(window("chat", INSTANCE))), true)
        .unwrap();
    let pending = unset.plan(nav("https://example.com/go", false), true);
    assert!(matches!(pending, Plan::Refuse(NavFault::Stale)));
    if dispatches(&pending) {
        dispatched += 1;
    }
    assert!(matches!(
        book.plan(nav("https://example.com/go", false), true),
        Plan::Ready(_)
    ));
    assert_eq!(dispatched, 0);
    assert_eq!(host_error(NavFault::Stopped), (-32015, "browser_stopped"));
    assert_eq!(
        host_error(NavFault::Unknown),
        (-32016, "browser_navigation_unknown")
    );
    assert!(model_error(NavFault::Unknown).is_none());
    assert_eq!(
        model_error(NavFault::UrlBlocked),
        Some(gcu_protocol::ErrorCode::KeyBlocked)
    );
}

#[test]
fn conditional_tool_stays_out_of_the_default_catalog() {
    let catalog = tools_list_json();
    let names: Vec<_> = catalog
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(!names.contains(&"browser_navigate"));
    assert_eq!(names.len(), 16);
    let tool = browser_navigate_tool_json();
    assert_eq!(tool["name"], "browser_navigate");
    assert!(tool["inputSchema"]["properties"]["url"].is_object());
    assert!(tool["inputSchema"].get("session_id").is_none());
    let bytes = serde_json::to_vec(&tool).unwrap().len();
    assert!(bytes < 492, "{bytes}");
    let conditional = gcu_protocol::browser_conditional_catalog_json();
    let conditional_names: Vec<_> = conditional
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(conditional_names.contains(&"browser_navigate"));
    assert!(!conditional_names.contains(&"computer_script"));
    assert_eq!(conditional_names.len(), 16);
    assert!(serde_json::to_vec(&conditional).unwrap().len() <= 14_000);
}

#[test]
fn profile_path_must_be_the_fixed_directory() {
    // An absolute path on every platform; "/tmp/..." has no drive on Windows.
    let root = std::env::temp_dir();
    assert!(root.is_absolute());
    let app = root.join("app-data");
    let profile = profile_dir(&app);
    assert!(is_exact_profile(&app, &profile));
    assert!(!is_exact_profile(&app, &app.join("other")));
    assert!(!is_exact_profile(
        &app,
        &root.join("elsewhere").join("cu-browser-profile")
    ));
    assert!(!is_exact_profile(
        std::path::Path::new("relative"),
        std::path::Path::new("relative/cu-browser-profile")
    ));
}

#[test]
fn null_sync_is_accepted_when_unproven_and_short_ids_are_not() {
    let mut book = Book::empty();
    book.apply(&sync(1, None), false).unwrap();
    assert_eq!(book.revision(), 1);
    assert!(book.owner().is_none());
    let mut bad = window("chat", INSTANCE);
    bad.pid = 0;
    assert_eq!(
        book.apply(&sync(2, Some(bad)), true),
        Err(ApplyFault::Invalid)
    );
    let mut geometry = window("chat", INSTANCE);
    geometry.geometry.width = 10_000.0;
    assert_eq!(
        book.apply(&sync(2, Some(geometry)), true),
        Err(ApplyFault::Invalid)
    );
}
