//! Grok denylist exception only for a verified native window. Titles do not grant it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_protocol::builtin_browser::VerifiedWindow;
use gcu_protocol::ErrorCode;

use super::support::*;

fn grok() -> AppInfo {
    app(
        "Grok",
        identity(
            Platform::Macos,
            "com.grokapp.desktop",
            "/Applications/Grok.app/Contents/MacOS/Grok",
            None,
            SignatureState::Unsigned,
            None,
            None,
        ),
    )
}

fn verified(session: &str, window: &str) -> VerifiedWindow {
    VerifiedWindow {
        session_id: session.into(),
        window_id: window.into(),
        pid: 42,
        started: 9,
        generation: "gen-verified".into(),
    }
}

fn window(id: &str, title: &str) -> PolicyWindow {
    PolicyWindow {
        id: id.into(),
        title: title.into(),
        url: None,
    }
}

fn decide(
    target: &AppInfo,
    window: &PolicyWindow,
    grant: Option<&VerifiedWindow>,
    intent: &ActionIntent,
    extra: &[&str],
) -> PolicyDecision {
    let activity = FakeUserActivity::new();
    let mut config = config();
    config.extra_denied_apps = extra.iter().map(|item| (*item).to_owned()).collect();
    let grants = grants_for(target);
    engine().evaluate(
        &PolicyCtx {
            session_id: "chat",
            app: target,
            window: Some(window),
            tree: None,
            activity: &activity,
            config: &config,
            grants: &grants,
            policy_version: 7,
            closed: false,
            browser: grant,
            scope_epoch: None,
        },
        intent,
    )
}

fn code(decision: &PolicyDecision) -> Option<ErrorCode> {
    match decision {
        PolicyDecision::Refuse(code, _) => Some(*code),
        _ => None,
    }
}

#[test]
fn page_claimed_identity_never_grants_access() {
    let target = grok();
    let titled = window("other-window", "cu-browser");
    let mut effects = 0u32;
    for tool in [Tool::Click, Tool::TypeText, Tool::PressKey, Tool::SetValue] {
        let mut intent = ActionIntent::new(tool);
        intent.element_index = Some(1);
        intent.snapshot_id = "snap".into();
        intent.action_digest = [1; 32];
        intent.parameters = "x".into();
        if tool == Tool::TypeText {
            intent.text = Some("hello".into());
        }
        if tool == Tool::PressKey {
            intent.key = Some("a".into());
        }
        let decision = decide(&target, &titled, None, &intent, &[]);
        assert_eq!(code(&decision), Some(ErrorCode::AppDenied), "{tool:?}");
        if matches!(decision, PolicyDecision::Allow { .. }) {
            effects += 1;
        }
    }
    assert_eq!(effects, 0);
}

#[test]
fn browser_injection_cannot_bypass_policy() {
    let target = grok();
    let page = window("native-1", "cu-browser");
    let grant = verified("chat", "native-1");
    let mut effects = 0u32;
    let bare = decide(&target, &page, None, &click(1), &[]);
    assert_eq!(code(&bare), Some(ErrorCode::AppDenied));
    if matches!(bare, PolicyDecision::Allow { .. }) {
        effects += 1;
    }
    let mut typed = ActionIntent::new(Tool::TypeText);
    typed.text = Some("javascript:alert(1)".into());
    typed.element_index = Some(1);
    typed.snapshot_id = "snap".into();
    typed.action_digest = [2; 32];
    typed.parameters = "type".into();
    let script = decide(&target, &page, Some(&grant), &typed, &[]);
    assert_eq!(code(&script), Some(ErrorCode::KeyBlocked));
    if matches!(script, PolicyDecision::Allow { .. }) {
        effects += 1;
    }
    let mut spoofed = grant.clone();
    spoofed.window_id = "spoofed".into();
    let mismatch = decide(&target, &page, Some(&spoofed), &click(1), &[]);
    assert_eq!(code(&mismatch), Some(ErrorCode::AppDenied));
    let mut empty = grant.clone();
    empty.generation.clear();
    assert_eq!(
        code(&decide(&target, &page, Some(&empty), &click(1), &[])),
        Some(ErrorCode::AppDenied)
    );
    let mut dead = grant.clone();
    dead.pid = 0;
    dead.started = 0;
    assert_eq!(
        code(&decide(&target, &page, Some(&dead), &click(1), &[])),
        Some(ErrorCode::AppDenied)
    );
    let extra = decide(
        &target,
        &page,
        Some(&grant),
        &click(1),
        &["com.grokapp.desktop"],
    );
    assert_eq!(code(&extra), Some(ErrorCode::AppDenied));
    if matches!(extra, PolicyDecision::Allow { .. }) {
        effects += 1;
    }
    assert_eq!(effects, 0);
}

#[tokio::test]
async fn verified_browser_still_blocks_address_bar_keys() {
    let source = r#"(apps: [(id: "fake.labels", name: "Grok", executable: "/Applications/Grok.app/Contents/MacOS/Grok", pid: 42,
windows: [(id: "native-1", title: "Not the address", frame: (x: 0.0, y: 0.0, w: 800.0, h: 600.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("Page"),
frame: Some((x: 0.0, y: 0.0, w: 800.0, h: 600.0)),
children: [(id: "addr", role: text_field, states: "ENABLED", name: Some("Address and search"),
frame: Some((x: 10.0, y: 10.0, w: 400.0, h: 24.0)), actions: [press])])))])])"#;
    let driver = FakeDriver::from_ron(source).unwrap();
    let app_ref = driver
        .resolve_app(&AppQuery("fake.labels".into()))
        .await
        .unwrap();
    let info = driver.key_window(&app_ref).await.unwrap();
    let raw = driver
        .snapshot_tree(
            &WindowRef {
                app: app_ref,
                id: info.id.clone(),
            },
            &TreeOpts::default(),
            &Cancel::default(),
        )
        .await
        .unwrap();
    let tree = Pipeline::default().run(raw, &TreeOpts::default());
    let (index, _) = tree
        .nodes()
        .into_iter()
        .find(|(_, node)| node.node.identifier.as_deref() == Some("addr"))
        .unwrap();
    let page = PolicyWindow {
        id: info.id,
        title: info.title,
        url: None,
    };
    let grant = verified("chat", &page.id);
    let mut intent = ActionIntent::new(Tool::PressKey);
    intent.key = Some("a".into());
    intent.element_index = Some(u32::try_from(index).unwrap());
    intent.snapshot_id = "snap".into();
    intent.action_digest = [4; 32];
    intent.parameters = "key".into();
    let activity = FakeUserActivity::new();
    let config = config();
    let target = grok();
    let grants = grants_for(&target);
    let decision = engine().evaluate(
        &PolicyCtx {
            session_id: "chat",
            app: &target,
            window: Some(&page),
            tree: Some(&tree),
            activity: &activity,
            config: &config,
            grants: &grants,
            policy_version: 7,
            closed: false,
            browser: Some(&grant),
            scope_epoch: None,
        },
        &intent,
    );
    assert_eq!(code(&decision), Some(ErrorCode::KeyBlocked));
}
