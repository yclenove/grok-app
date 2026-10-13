use super::support::*;

#[test]
fn denylist_builtin_identities_refuse_or_hand_off() {
    let activity = FakeUserActivity::new();
    let idle = GrantTable::default();
    let cases = [
        (
            "com.apple.Terminal",
            "My Notes",
            Platform::Macos,
            "com.apple.Terminal",
            "/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal",
            "renamed",
        ),
        (
            "com.apple.Terminal",
            "Terminal",
            Platform::Macos,
            "COM.APPLE.TERMINAL",
            "/tmp/Terminal",
            "case",
        ),
        (
            "com.grokapp.desktop",
            "Grok",
            Platform::Macos,
            "com.grokapp.desktop",
            "/Applications/Grok.app/Contents/MacOS/Grok",
            "grok",
        ),
        (
            "com.1password.1password",
            "1Password",
            Platform::Macos,
            "com.1password.1password",
            "/Applications/1Password.app/Contents/MacOS/1Password",
            "credentials",
        ),
        (
            "com.crowdstrike.falcon.Agent",
            "Falcon",
            Platform::Macos,
            "com.crowdstrike.falcon.Agent",
            "/Library/Falcon",
            "security",
        ),
        (
            "gnome-terminal",
            "Notes",
            Platform::Linux,
            "org.example.Wrapper",
            "/usr/bin/gnome-terminal",
            "symlink",
        ),
        (
            "windowsterminal.exe",
            "Notes",
            Platform::Windows,
            "C:\\Aliases\\launch-terminal.cmd",
            "C:\\Program Files\\WindowsApps\\WindowsTerminal.exe",
            "alias",
        ),
        (
            "xterm",
            "xterm",
            Platform::Linux,
            "org.example.XTerm",
            "/usr/bin/xterm",
            "xterm",
        ),
    ];
    for (rule, name, platform, key, image, label) in cases {
        let target = app(
            name,
            identity(
                platform,
                key,
                image,
                None,
                SignatureState::Unsigned,
                None,
                None,
            ),
        );
        let decision = eval(
            &target,
            &config(),
            &idle,
            &activity,
            None,
            None,
            &ActionIntent::new(Tool::GetAppState),
            false,
        );
        assert_eq!(
            refused(&decision),
            Some(gcu_protocol::ErrorCode::AppDenied),
            "{label} ({rule}) should be denied"
        );
    }
    let prompt = app(
        "SecurityAgent",
        identity(
            Platform::Macos,
            "com.apple.SecurityAgent",
            "/System/Library/CoreServices/SecurityAgent",
            None,
            SignatureState::Valid,
            Some("APPLE"),
            None,
        ),
    );
    let decision = eval(
        &prompt,
        &config(),
        &grants_for(&prompt),
        &activity,
        None,
        None,
        &ActionIntent::new(Tool::GetAppState),
        false,
    );
    assert_eq!(
        decision,
        PolicyDecision::Handoff(HandoffReason::SystemPrompt)
    );
}

#[test]
fn denylist_extra_append_refuses_and_bang_cannot_remove() {
    let activity = FakeUserActivity::new();
    let mut config = config();
    config.extra_denied_apps = vec!["com.example.Custom".into()];
    let custom = app(
        "Custom",
        identity(
            Platform::Linux,
            "com.example.Custom",
            "/usr/bin/custom",
            Some([1; 32]),
            SignatureState::Unsigned,
            None,
            None,
        ),
    );
    let decision = eval(
        &custom,
        &config,
        &grants_for(&custom),
        &activity,
        None,
        None,
        &ActionIntent::new(Tool::Click),
        false,
    );
    assert_eq!(refused(&decision), Some(gcu_protocol::ErrorCode::AppDenied));
    config.extra_denied_apps = vec!["!com.apple.Terminal".into()];
    let terminal = app(
        "My Notes",
        identity(
            Platform::Macos,
            "com.apple.Terminal",
            "/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal",
            None,
            SignatureState::Unsigned,
            None,
            None,
        ),
    );
    config.always.push(terminal.identity.clone());
    let decision = eval(
        &terminal,
        &config,
        &grants_for(&terminal),
        &activity,
        None,
        None,
        &ActionIntent::new(Tool::GetAppState),
        false,
    );
    assert_eq!(refused(&decision), Some(gcu_protocol::ErrorCode::AppDenied));
}

#[test]
fn denylist_foreign_platform_and_display_hint_do_not_overreach() {
    let activity = FakeUserActivity::new();
    let foreign = app(
        "Notes",
        identity(
            Platform::Windows,
            "com.apple.Terminal",
            "C:\\Notes\\Notes.exe",
            Some([4; 32]),
            SignatureState::Unsigned,
            None,
            None,
        ),
    );
    let decision = eval(
        &foreign,
        &config(),
        &GrantTable::default(),
        &activity,
        None,
        None,
        &ActionIntent::new(Tool::GetAppState),
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedApproval(_)));
    let hinted = app(
        "Terminal",
        identity(
            Platform::Linux,
            "org.example.Notes",
            "/usr/bin/notes",
            Some([5; 32]),
            SignatureState::Unsigned,
            None,
            None,
        ),
    );
    let decision = eval(
        &hinted,
        &config(),
        &grants_for(&hinted),
        &activity,
        None,
        None,
        &ActionIntent::new(Tool::GetAppState),
        false,
    );
    assert!(matches!(
        decision,
        PolicyDecision::Allow {
            route_limit: RouteLimit::ObserveOnly
        }
    ));
    let empty = app(
        "Terminal",
        identity(
            Platform::Linux,
            "",
            "",
            None,
            SignatureState::Unknown,
            None,
            None,
        ),
    );
    let decision = eval(
        &empty,
        &config(),
        &GrantTable::default(),
        &activity,
        None,
        None,
        &ActionIntent::new(Tool::GetAppState),
        false,
    );
    assert!(matches!(
        decision,
        PolicyDecision::Handoff(HandoffReason::Other(_))
    ));
    let bash = app(
        "Notes",
        identity(
            Platform::Linux,
            "bash",
            "/usr/bin/bash",
            Some([6; 32]),
            SignatureState::Unsigned,
            None,
            None,
        ),
    );
    let decision = eval(
        &bash,
        &config(),
        &grants_for(&bash),
        &activity,
        None,
        None,
        &ActionIntent::new(Tool::GetAppState),
        false,
    );
    assert!(matches!(
        decision,
        PolicyDecision::Handoff(HandoffReason::Other(_))
    ));
}

#[test]
fn discovery_skips_denylist_and_stop_still_refuses() {
    let activity = FakeUserActivity::new();
    let terminal = app(
        "Terminal",
        identity(
            Platform::Macos,
            "com.apple.Terminal",
            "/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal",
            None,
            SignatureState::Unsigned,
            None,
            None,
        ),
    );
    let decision = eval(
        &terminal,
        &config(),
        &GrantTable::default(),
        &activity,
        None,
        None,
        &ActionIntent::new(Tool::ListApps),
        false,
    );
    assert!(matches!(
        decision,
        PolicyDecision::Allow {
            route_limit: RouteLimit::ObserveOnly
        }
    ));
    activity.request_stop();
    let decision = eval(
        &notes(),
        &config(),
        &GrantTable::default(),
        &activity,
        None,
        None,
        &ActionIntent::new(Tool::ListWindows),
        false,
    );
    assert_eq!(
        refused(&decision),
        Some(gcu_protocol::ErrorCode::StoppedByUser)
    );
}
