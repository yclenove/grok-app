use super::support::*;

#[tokio::test]
async fn sensitive_terms_zh_en_ja_confirm_and_near_misses_allow() {
    let app = notes();
    let config = config();
    let grants = grants_for(&app);
    let activity = FakeUserActivity::new();
    for name in [
        "发送",
        "支付",
        "删除",
        "send",
        "pay",
        "delete",
        "送信",
        "削除",
        "署名",
        "Ｓｅｎｄ",
    ] {
        let (window, tree) = labeled_button(name).await;
        let index = node(&tree, "btn").element_index;
        let decision = eval(
            &app,
            &config,
            &grants,
            &activity,
            Some(&window),
            Some(&tree),
            &click(index),
            false,
        );
        assert!(
            matches!(
                decision,
                PolicyDecision::NeedConfirm(ref request) if request.kind == ConfirmKind::Sensitive
            ),
            "{name} should confirm"
        );
    }
    for name in ["sending", "payload", "Save draft"] {
        let (window, tree) = labeled_button(name).await;
        let index = node(&tree, "btn").element_index;
        let decision = eval(
            &app,
            &config,
            &grants,
            &activity,
            Some(&window),
            Some(&tree),
            &click(index),
            false,
        );
        assert!(
            matches!(decision, PolicyDecision::Allow { .. }),
            "{name} should allow"
        );
    }
}

#[tokio::test]
async fn password_field_and_sensitive_chord_need_confirm() {
    let (window, tree) = scene("login", "Fixture login").await;
    let app = notes();
    let grants = grants_for(&app);
    let activity = FakeUserActivity::new();
    let mut intent = ActionIntent::new(Tool::TypeText);
    intent.element_index = Some(node(&tree, "password").element_index);
    intent.text = Some("secret".into());
    let decision = eval(
        &app,
        &config(),
        &grants,
        &activity,
        Some(&window),
        Some(&tree),
        &intent,
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedConfirm(_)));
    assert!(!reason(&decision).contains("secret"));
    let mut chord = ActionIntent::new(Tool::PressKey);
    chord.element_index = Some(node(&tree, "login").element_index);
    chord.key = Some("cmd+delete".into());
    let decision = eval(
        &app,
        &config(),
        &grants,
        &activity,
        Some(&window),
        Some(&tree),
        &chord,
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedConfirm(_)));
}

#[tokio::test]
async fn web_area_unknown_controls_confirm_unless_off() {
    let (window, tree) = scene("submit_unknown", "Fixture submit").await;
    let app = notes();
    let grants = grants_for(&app);
    let activity = FakeUserActivity::new();
    let icon = node(&tree, "icon-submit").element_index;
    let ok = node(&tree, "ok").element_index;
    let comment = node(&tree, "comment").element_index;
    let standard = config();
    for (label, intent) in [
        ("icon", click(icon)),
        ("ok", click(ok)),
        ("enter", {
            let mut intent = ActionIntent::new(Tool::PressKey);
            intent.element_index = Some(comment);
            intent.key = Some("Return".into());
            intent
        }),
        ("space", {
            let mut intent = ActionIntent::new(Tool::PressKey);
            intent.element_index = Some(icon);
            intent.key = Some("space".into());
            intent
        }),
        ("shortcut", {
            let mut intent = ActionIntent::new(Tool::PressKey);
            intent.element_index = Some(comment);
            intent.key = Some("ctrl+k".into());
            intent
        }),
        ("newline", {
            let mut intent = ActionIntent::new(Tool::TypeText);
            intent.element_index = Some(comment);
            intent.text = Some("hello\nworld".into());
            intent
        }),
    ] {
        let decision = eval(
            &app,
            &standard,
            &grants,
            &activity,
            Some(&window),
            Some(&tree),
            &intent,
            false,
        );
        assert!(
            matches!(decision, PolicyDecision::NeedConfirm(_)),
            "{label} in a web area should confirm"
        );
        if let PolicyDecision::NeedConfirm(request) = &decision {
            assert!(!request.summary.text.contains("hello"));
        }
    }
    let mut off = standard.clone();
    off.sensitive_action_confirmation = ConfirmationLevel::Off;
    let decision = eval(
        &app,
        &off,
        &grants,
        &activity,
        Some(&window),
        Some(&tree),
        &click(ok),
        false,
    );
    assert!(matches!(decision, PolicyDecision::Allow { .. }));
}

#[tokio::test]
async fn native_editor_save_allows_until_strict() {
    let (window, tree) = scene("ide", "Fixture IDE — main.rs").await;
    let app = notes();
    let grants = grants_for(&app);
    let activity = FakeUserActivity::new();
    let save = node(&tree, "save").element_index;
    let decision = eval(
        &app,
        &config(),
        &grants,
        &activity,
        Some(&window),
        Some(&tree),
        &click(save),
        false,
    );
    assert!(matches!(
        decision,
        PolicyDecision::Allow {
            route_limit: RouteLimit::BackgroundOnly
        }
    ));
    let mut strict = config();
    strict.sensitive_action_confirmation = ConfirmationLevel::Strict;
    let decision = eval(
        &app,
        &strict,
        &grants,
        &activity,
        Some(&window),
        Some(&tree),
        &click(save),
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedConfirm(_)));
    let editor = node(&tree, "editor").element_index;
    let mut newline = ActionIntent::new(Tool::TypeText);
    newline.element_index = Some(editor);
    newline.text = Some("line\n".into());
    let decision = eval(
        &app,
        &config(),
        &grants,
        &activity,
        Some(&window),
        Some(&tree),
        &newline,
        false,
    );
    assert!(matches!(decision, PolicyDecision::Allow { .. }));
    let mut shortcut = ActionIntent::new(Tool::PressKey);
    shortcut.element_index = Some(editor);
    shortcut.key = Some("cmd+s".into());
    let decision = eval(
        &app,
        &config(),
        &grants,
        &activity,
        Some(&window),
        Some(&tree),
        &shortcut,
        false,
    );
    assert!(matches!(decision, PolicyDecision::Allow { .. }));
}

#[tokio::test]
async fn ide_execution_paths_refuse_and_a_miss_hands_off() {
    let (window, tree) = scene("ide", "Fixture IDE — main.rs").await;
    let app = notes();
    let grants = grants_for(&app);
    let activity = FakeUserActivity::new();
    let terminal = node(&tree, "terminal").element_index;
    let menu = node(&tree, "terminal-menu").element_index;
    let mut cases = vec![
        ("click", {
            let mut intent = click(terminal);
            intent.tool = Tool::Click;
            intent
        }),
        ("menu", {
            let mut intent = click(menu);
            intent.tool = Tool::Menu;
            intent
        }),
        ("secondary", {
            let mut intent = click(menu);
            intent.tool = Tool::Secondary;
            intent
        }),
        ("drag", {
            let mut intent = click(terminal);
            intent.tool = Tool::Drag;
            intent
        }),
        ("batch", {
            let mut intent = click(terminal);
            intent.tool = Tool::Batch;
            intent
        }),
        ("script", {
            let mut intent = click(terminal);
            intent.tool = Tool::Script;
            intent
        }),
        ("coordinate", {
            let mut intent = ActionIntent::new(Tool::Click);
            intent.at = Some(center(node(&tree, "terminal")));
            intent
        }),
        ("debug", click(node(&tree, "debug").element_index)),
        ("repl", click(node(&tree, "repl").element_index)),
        ("task", click(node(&tree, "task").element_index)),
    ];
    for (label, intent) in &cases {
        let decision = eval(
            &app,
            &config(),
            &grants,
            &activity,
            Some(&window),
            Some(&tree),
            intent,
            false,
        );
        assert_eq!(
            refused(&decision),
            Some(gcu_protocol::ErrorCode::ElementNotActionable),
            "{label}"
        );
    }
    cases.clear();
    let mut miss = ActionIntent::new(Tool::Click);
    miss.at = Some((-1.0, -1.0));
    let decision = eval(
        &app,
        &config(),
        &grants,
        &activity,
        Some(&window),
        Some(&tree),
        &miss,
        false,
    );
    assert!(matches!(
        decision,
        PolicyDecision::Handoff(HandoffReason::Other(_))
    ));
}

#[tokio::test]
async fn web_terminal_input_hands_off_and_observation_is_allowed() {
    let (window, tree) = scene("web_terminal", "Web SSH — bastion").await;
    let app = notes();
    let activity = FakeUserActivity::new();
    let mut typed = ActionIntent::new(Tool::TypeText);
    typed.element_index = Some(node(&tree, "command").element_index);
    typed.text = Some("ls".into());
    let decision = eval(
        &app,
        &config(),
        &GrantTable::default(),
        &activity,
        Some(&window),
        Some(&tree),
        &typed,
        false,
    );
    assert!(matches!(
        decision,
        PolicyDecision::Handoff(HandoffReason::Other(_))
    ));
    let decision = eval(
        &app,
        &config(),
        &grants_for(&app),
        &activity,
        Some(&window),
        Some(&tree),
        &click(node(&tree, "run").element_index),
        false,
    );
    assert!(matches!(
        decision,
        PolicyDecision::Handoff(HandoffReason::Other(_))
    ));
    let observe = ActionIntent::new(Tool::Observe);
    let decision = eval(
        &app,
        &config(),
        &GrantTable::default(),
        &activity,
        Some(&window),
        Some(&tree),
        &observe,
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedApproval(_)));
    let decision = eval(
        &app,
        &config(),
        &grants_for(&app),
        &activity,
        Some(&window),
        Some(&tree),
        &observe,
        false,
    );
    assert!(matches!(
        decision,
        PolicyDecision::Allow {
            route_limit: RouteLimit::ObserveOnly
        }
    ));
}

#[tokio::test]
async fn address_bar_schemes_and_metadata_hosts_block_without_echoing_text() {
    let (page, page_tree) = scene("submit_unknown", "Fixture submit").await;
    let (address, address_tree) = scene("submit_unknown", "Fixture address").await;
    let app = notes();
    let grants = grants_for(&app);
    let activity = FakeUserActivity::new();
    let comment = node(&page_tree, "comment").element_index;
    let bar = node(&address_tree, "address").element_index;
    let mut hello = ActionIntent::new(Tool::TypeText);
    hello.element_index = Some(bar);
    hello.text = Some("hello".into());
    let decision = eval(
        &app,
        &config(),
        &grants,
        &activity,
        Some(&address),
        Some(&address_tree),
        &hello,
        false,
    );
    assert_eq!(
        refused(&decision),
        Some(gcu_protocol::ErrorCode::KeyBlocked)
    );
    assert!(!reason(&decision).contains("hello"));
    let click_bar = click(bar);
    let decision = eval(
        &app,
        &config(),
        &grants,
        &activity,
        Some(&address),
        Some(&address_tree),
        &click_bar,
        false,
    );
    assert!(matches!(decision, PolicyDecision::Allow { .. }));
    for text in [
        "javascript:alert(1)",
        "  file:///etc/passwd",
        "http://169.254.169.254/latest",
        "http://metadata.google.internal/computeMetadata",
        "http://0xa9.0xfe.0xa9.0xfe/",
        "http://2852039166/",
    ] {
        let mut intent = ActionIntent::new(Tool::TypeText);
        intent.element_index = Some(comment);
        intent.text = Some(text.into());
        let decision = eval(
            &app,
            &config(),
            &grants,
            &activity,
            Some(&page),
            Some(&page_tree),
            &intent,
            false,
        );
        assert_eq!(
            refused(&decision),
            Some(gcu_protocol::ErrorCode::KeyBlocked),
            "{text}"
        );
        let reason = reason(&decision);
        assert!(!reason.contains("javascript") && !reason.contains("169.254"));
        assert!(!reason.contains("metadata.google.internal"));
    }
    let mut benign = ActionIntent::new(Tool::TypeText);
    benign.element_index = Some(comment);
    benign.text = Some("hello".to_string());
    let decision = eval(
        &app,
        &config(),
        &grants,
        &activity,
        Some(&page),
        Some(&page_tree),
        &benign,
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedConfirm(_)));
}

#[test]
fn blocked_chords_refuse_even_when_confirmation_is_off() {
    let app = notes();
    let mut config = config();
    config.sensitive_action_confirmation = ConfirmationLevel::Off;
    let activity = FakeUserActivity::new();
    for key in [
        "ctrl+alt+delete",
        "ctrl+alt+shift+backspace",
        "super+l",
        "cmd+shift+q",
    ] {
        let mut intent = ActionIntent::new(Tool::PressKey);
        intent.key = Some(key.into());
        let decision = eval(
            &app,
            &config,
            &grants_for(&app),
            &activity,
            None,
            None,
            &intent,
            false,
        );
        assert_eq!(
            refused(&decision),
            Some(gcu_protocol::ErrorCode::KeyBlocked),
            "{key}"
        );
    }
    let mut intent = ActionIntent::new(Tool::PressKey);
    intent.key = Some("not a key".into());
    let decision = eval(
        &app,
        &config,
        &grants_for(&app),
        &activity,
        None,
        None,
        &intent,
        false,
    );
    assert_eq!(
        refused(&decision),
        Some(gcu_protocol::ErrorCode::InvalidKey)
    );
}

#[tokio::test]
async fn user_activity_blocks_only_the_foreground_path() {
    let (window, tree) = scene("ide", "Fixture IDE — main.rs").await;
    let app = notes();
    let grants = grants_for(&app);
    let save = click(node(&tree, "save").element_index);
    let activity = FakeUserActivity::new();
    let now = Instant::now();
    activity.set_now(now);
    activity.set_last_physical_input(now);
    let mut foreground = save.clone();
    foreground.foreground = true;
    let decision = eval(
        &app,
        &config(),
        &grants,
        &activity,
        Some(&window),
        Some(&tree),
        &foreground,
        false,
    );
    assert_eq!(
        refused(&decision),
        Some(gcu_protocol::ErrorCode::UserActive)
    );
    let decision = eval(
        &app,
        &config(),
        &grants,
        &activity,
        Some(&window),
        Some(&tree),
        &save,
        false,
    );
    assert!(matches!(decision, PolicyDecision::Allow { .. }));
    activity.set_last_physical_input(now - Duration::from_millis(3_000));
    let decision = eval(
        &app,
        &config(),
        &grants,
        &activity,
        Some(&window),
        Some(&tree),
        &foreground,
        false,
    );
    assert!(matches!(decision, PolicyDecision::Allow { .. }));
    activity.clear_physical_input();
    let decision = eval(
        &app,
        &config(),
        &grants,
        &activity,
        Some(&window),
        Some(&tree),
        &foreground,
        false,
    );
    assert!(matches!(decision, PolicyDecision::Allow { .. }));
}

#[tokio::test]
async fn foreground_policy_never_refuses_and_ask_confirms() {
    let (window, tree) = scene("ide", "Fixture IDE — main.rs").await;
    let app = notes();
    let grants = grants_for(&app);
    let activity = FakeUserActivity::new();
    let mut intent = click(node(&tree, "save").element_index);
    intent.foreground = true;
    let mut never = config();
    never.allow_foreground = ForegroundPolicy::Never;
    let decision = eval(
        &app,
        &never,
        &grants,
        &activity,
        Some(&window),
        Some(&tree),
        &intent,
        false,
    );
    assert_eq!(
        refused(&decision),
        Some(gcu_protocol::ErrorCode::CapabilityUnavailable)
    );
    let mut ask = config();
    ask.allow_foreground = ForegroundPolicy::Ask;
    let decision = eval(
        &app,
        &ask,
        &grants,
        &activity,
        Some(&window),
        Some(&tree),
        &intent,
        false,
    );
    match decision {
        PolicyDecision::NeedConfirm(request) => {
            assert_eq!(request.kind, ConfirmKind::Foreground);
            assert!(request.summary.text.contains("Save draft"));
        }
        other => panic!("expected foreground confirm, got {other:?}"),
    }
    let (web_window, web_tree) = scene("submit_unknown", "Fixture submit").await;
    let mut web = click(node(&web_tree, "ok").element_index);
    web.foreground = true;
    let decision = eval(
        &app,
        &ask,
        &grants,
        &activity,
        Some(&web_window),
        Some(&web_tree),
        &web,
        false,
    );
    match decision {
        PolicyDecision::NeedConfirm(request) => assert_eq!(request.kind, ConfirmKind::Sensitive),
        other => panic!("expected sensitive confirm, got {other:?}"),
    }
}
