use super::*;

fn settings() -> AppSettings {
    AppSettings {
        subagents_enabled: false,
        experimental_memory: false,
        todo_gate_enabled: true,
        todo_gate_max_fires_per_prompt: 99,
        subagent_worktree_snapshot_enabled: true,
        two_pass_compaction_enabled: true,
        auto_wake_enabled: true,
        ..AppSettings::default()
    }
}

#[test]
fn runtime_setters_do_not_interpret_or_change_multiline_config_literals() {
    let note = "[ui]\npermission_mode = 'literal'\nyolo = false\n[subagents]\nenabled = true\n[features]\nauto_wake = false\n[mcp_servers.fake]\nenabled = true\n";
    let text = format!(
        "[model.literal-carrier]\nnote = '''\n{note}'''\n[ui]\npermission_mode = 'default'\n"
    );
    let next = runtime_text(
        &text,
        &settings(),
        "always_approve",
        true,
        &ExtensionsPrefs::default(),
    )
    .unwrap();
    let snapshot = ConfigSnapshot::parse(&next).unwrap();
    assert_eq!(
        snapshot.model_fields("literal-carrier").unwrap().unwrap()["note"],
        note,
        "literal data was rewritten by runtime preparation"
    );
    let (binding, native) =
        ProcessModelBinding::from_text(&next, false, false, "grok-4.6").unwrap();
    assert_eq!(binding.resolve("grok-4.6"), "grok-4.6");
    assert!(native.is_none());
}

#[test]
fn inline_and_quoted_mcp_config_reaches_the_actual_structural_reader() {
    let text = "ui = { permission_mode = 'dontAsk', yolo_mode = false }\nmcp_servers = { 'chat.cut' = { command = 'synthetic-unexecuted', args = ['[ui]'], env = { TOKEN = 'synthetic-only' }, headers = { 'X-Label' = '中文' }, enabled = true }, 'official-aux' = { command = 'synthetic-aux-unexecuted', enabled = true } }\n";
    let next = runtime_text(
        text,
        &settings(),
        "accept_edits",
        true,
        &ExtensionsPrefs::default(),
    )
    .unwrap();
    assert!(
        ConfigSnapshot::parse(&next).is_ok(),
        "runtime overlay made valid inline configuration invalid"
    );
    assert!(next.contains("acceptEdits"));
}

#[test]
fn official_or_injection_off_cannot_keep_a_configured_official_aux_enabled() {
    let text = "[mcp_servers.official-aux]\ncommand = 'synthetic-unexecuted'\nenabled = true\n";
    let next = runtime_text(text, &settings(), "ask", false, &ExtensionsPrefs::default()).unwrap();
    let defs = crate::extensions::parse_mcp_servers_from_toml(&next);
    assert_eq!(
        defs.iter()
            .find(|def| def.name == "official-aux")
            .unwrap()
            .enabled,
        Some(false),
        "official-aux remained enabled when injection was disabled"
    );
}

#[test]
fn invalid_runtime_namespaces_are_rejected_instead_of_silently_overridden() {
    for text in [
        "ui = 1\n",
        "compat = false\n",
        "mcp_servers = 'bad'\n",
        "[ui]\nyolo = true\nyolo = false\n",
    ] {
        assert!(
            runtime_text(text, &settings(), "ask", false, &ExtensionsPrefs::default()).is_err()
        );
    }
}

fn custom_text(vision: bool) -> String {
    format!("[models]\ndefault = 'selected'\n[model.selected]\nmodel = 'deepseek-v4-flash'\nbase_url = 'https://synthetic.invalid/v1'\napi_key = 'synthetic-key-only'\napp_supports_vision = true\napp_models = '[{{\"id\":\"deepseek-v4-flash\",\"name\":\"main\",\"supportsVision\":{vision}}}]'\n")
}

#[test]
fn target_profile_preparation_reaches_actual_capture_and_preserves_global_auth_and_user_files() {
    let _fixture =
        crate::session_migration::test_home::IsolatedAppHome::acquire("runtime-owned-target");
    let owner = uuid::Uuid::new_v4().to_string();
    let pending = super::super::execution_home::PendingHome::create(&owner).unwrap();
    let crate::session_execution::ExecutionSource::AppProfile { generation, .. } = pending.source()
    else {
        panic!("not an App profile");
    };
    let generation = generation.clone();
    let home = pending.home().to_owned();
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::create_dir_all(home.join("hooks")).unwrap();
    fs::write(home.join("config.toml"), custom_text(false)).unwrap();
    fs::write(
        home.join(".claude/settings.json"),
        r#"{"env":{"NOTE":"中文"},"permissions":{"allow":["synthetic-tool"]}}"#,
    )
    .unwrap();
    fs::write(home.join("hooks/user.json"), b"synthetic-user-hook").unwrap();
    fs::write(home.join("auth.json"), b"synthetic-auth-not-a-credential").unwrap();
    let global = agent_config_toml();
    fs::write(&global, b"synthetic-global-decoy").unwrap();
    let settings = AppSettings {
        session_data_mode: "independent".into(),
        official_aux_inject: true,
        ..settings()
    };
    assert!(prepare_pending_runtime(
        &pending,
        &settings,
        "always_approve",
        true,
        true,
        &ExtensionsPrefs::default()
    )
    .unwrap());
    assert_eq!(fs::read(&global).unwrap(), b"synthetic-global-decoy");
    assert_eq!(
        fs::read(home.join("auth.json")).unwrap(),
        b"synthetic-auth-not-a-credential"
    );
    assert_eq!(
        fs::read(home.join("hooks/user.json")).unwrap(),
        b"synthetic-user-hook"
    );
    let claude: serde_json::Value =
        serde_json::from_slice(&fs::read(home.join(".claude/settings.json")).unwrap()).unwrap();
    assert_eq!(claude["env"]["NOTE"], "中文");
    assert_eq!(claude["permissions"]["allow"][0], "synthetic-tool");
    assert_eq!(claude["permissions"]["defaultMode"], "bypassPermissions");
    assert_eq!(
        fs::read_to_string(home.join("hooks/grok-app-block-native-imagine.sh")).unwrap(),
        crate::official_aux::native_media_block_hook_script_body_with(true)
    );
    assert!(
        ProcessModelBinding::capture_for_spawn(
            true,
            false,
            false,
            "deepseek-v4-flash",
            &home,
            &settings,
        )
        .is_err(),
        "an unpublished generation was captured"
    );
    assert_eq!(
        pending
            .publish(&fs::read(home.join("config.toml")).unwrap())
            .unwrap(),
        home
    );
    let (binding, native, identity) = ProcessModelBinding::capture_for_spawn(
        true,
        false,
        false,
        "deepseek-v4-flash",
        &home,
        &settings,
    )
    .unwrap();
    assert!(native.is_none());
    assert_eq!(binding.resolve("deepseek-v4-flash"), "selected");
    assert!(
        matches!(identity.unwrap().execution_source(), crate::session_execution::ExecutionSource::AppProfile { owner_session_id, generation: actual }
        if owner_session_id == owner && actual == generation)
    );
}

#[test]
fn injection_and_image_hook_follow_the_target_model_and_availability_facts() {
    let _fixture =
        crate::session_migration::test_home::IsolatedAppHome::acquire("runtime-main-policy");
    let home = agent_home_dir();
    let settings = AppSettings {
        official_aux_inject: true,
        ..settings()
    };
    fs::create_dir_all(home.join("hooks")).unwrap();
    fs::write(home.join("hooks/user.json"), b"preserved").unwrap();
    for (vision, available, enabled) in [
        (false, true, true),
        (true, true, true),
        (false, false, false),
    ] {
        fs::write(home.join("config.toml"), custom_text(vision)).unwrap();
        assert_eq!(
            prepare_main_runtime(
                &home,
                &settings,
                "ask",
                true,
                available,
                &ExtensionsPrefs::default()
            )
            .unwrap(),
            enabled
        );
        let hook = home.join("hooks/grok-app-block-native-imagine.sh");
        if enabled {
            assert_eq!(
                fs::read_to_string(hook).unwrap(),
                crate::official_aux::native_media_block_hook_script_body_with(!vision)
            );
        } else {
            assert!(!hook.exists());
        }
        assert_eq!(
            fs::read(home.join("hooks/user.json")).unwrap(),
            b"preserved"
        );
    }
    fs::write(home.join("config.toml"), "[models]\ndefault = 'grok'\n[mcp_servers.official-aux]\ncommand = 'synthetic-unexecuted'\nenabled = true\n").unwrap();
    assert!(!prepare_main_runtime(
        &home,
        &settings,
        "ask",
        false,
        true,
        &ExtensionsPrefs::default()
    )
    .unwrap());
    let text = fs::read_to_string(home.join("config.toml")).unwrap();
    assert_eq!(
        crate::extensions::parse_mcp_servers_from_toml(&text)[0].enabled,
        Some(false)
    );
}

#[test]
fn malformed_settings_route_mismatch_and_invalid_file_types_reject_without_config_writes() {
    let _fixture = crate::session_migration::test_home::IsolatedAppHome::acquire("runtime-reject");
    let home = agent_home_dir();
    let text = custom_text(false);
    fs::write(home.join("config.toml"), &text).unwrap();
    assert!(prepare_main_runtime(
        &home,
        &settings(),
        "ask",
        false,
        false,
        &ExtensionsPrefs::default()
    )
    .is_err());
    fs::create_dir_all(home.join(".claude")).unwrap();
    for raw in ["not-json", "[]", r#"{"permissions":false}"#] {
        fs::write(home.join(".claude/settings.json"), raw).unwrap();
        let error = prepare_main_runtime(
            &home,
            &settings(),
            "ask",
            true,
            false,
            &ExtensionsPrefs::default(),
        )
        .unwrap_err();
        assert!(!error.contains("synthetic-key"));
        assert_eq!(fs::read_to_string(home.join("config.toml")).unwrap(), text);
        assert_eq!(
            fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
            raw
        );
    }
    fs::write(home.join(".claude/settings.json"), "{}").unwrap();
    fs::create_dir(home.join("config.toml.tmp")).unwrap();
    assert!(prepare_main_runtime(
        &home,
        &settings(),
        "ask",
        true,
        false,
        &ExtensionsPrefs::default()
    )
    .is_err());
    assert_eq!(fs::read_to_string(home.join("config.toml")).unwrap(), text);
}

#[test]
fn public_permission_and_auth_compat_sync_preserve_literals_and_claude_settings() {
    let _fixture =
        crate::session_migration::test_home::IsolatedAppHome::acquire("runtime-public-sync");
    let home = agent_home_dir();
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::write(
        home.join("config.toml"),
        "ui = { yolo_mode = false }\n[model.carrier]\nnote = '''\n[ui]\nyolo = 'literal'\n'''\n",
    )
    .unwrap();
    fs::write(
        home.join(".claude/settings.json"),
        r#"{"env":{"KEEP":"中文"},"permissions":{"deny":["synthetic-command"]}}"#,
    )
    .unwrap();
    crate::agent_prefs::sync_permission_to_agent_profile("independent", "dont_ask").unwrap();
    crate::agent_home_config::apply_compat_mcp_disabled("independent").unwrap();
    let text = fs::read_to_string(home.join("config.toml")).unwrap();
    assert_eq!(
        ConfigSnapshot::parse(&text)
            .unwrap()
            .model_fields("carrier")
            .unwrap()
            .unwrap()["note"],
        "[ui]\nyolo = 'literal'\n"
    );
    let claude: serde_json::Value =
        serde_json::from_slice(&fs::read(home.join(".claude/settings.json")).unwrap()).unwrap();
    assert_eq!(claude["env"]["KEEP"], "中文");
    assert_eq!(claude["permissions"]["deny"][0], "synthetic-command");
    assert_eq!(claude["permissions"]["defaultMode"], "dontAsk");
    let before = fs::read(home.join("config.toml")).unwrap();
    fs::write(home.join(".claude/settings.json"), "invalid").unwrap();
    assert!(
        crate::agent_prefs::sync_permission_to_agent_profile("independent", "always_approve")
            .is_err()
    );
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), before);
    crate::agent_prefs::sync_permission_to_agent_profile(" SHARED ", "always_approve").unwrap();
    crate::agent_home_config::apply_compat_mcp_disabled(" SHARED ").unwrap();
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), before);
}

#[test]
fn runtime_preparation_refuses_arbitrary_shared_and_auxiliary_homes() {
    let fixture =
        crate::session_migration::test_home::IsolatedAppHome::acquire("runtime-boundaries");
    for home in [
        fixture.path.join("synthetic-user/.grok"),
        crate::official_aux::official_aux_home(),
    ] {
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("config.toml"), b"synthetic-sentinel").unwrap();
        assert!(prepare_main_runtime(
            &home,
            &settings(),
            "ask",
            false,
            false,
            &ExtensionsPrefs::default()
        )
        .is_err());
        assert_eq!(
            fs::read(home.join("config.toml")).unwrap(),
            b"synthetic-sentinel"
        );
        assert!(!home.join(".claude").exists());
    }
}

#[test]
fn published_generation_cannot_be_rewritten_by_runtime_preparation() {
    let _fixture =
        crate::session_migration::test_home::IsolatedAppHome::acquire("runtime-published-freeze");
    let owner = uuid::Uuid::new_v4().to_string();
    let generation = uuid::Uuid::new_v4().to_string();
    let home = crate::paths::app_data_root()
        .join("execution-profiles")
        .join(&owner)
        .join(&generation);
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("config.toml"), custom_text(false)).unwrap();
    fs::write(
        home.join("execution-profile.json"),
        serde_json::json!({"schemaVersion":1,"ownerSessionId":owner,"generation":generation})
            .to_string(),
    )
    .unwrap();
    let before = fs::read(home.join("config.toml")).unwrap();
    assert!(
        prepare_main_runtime(
            &home,
            &settings(),
            "always_approve",
            true,
            false,
            &ExtensionsPrefs::default(),
        )
        .is_err(),
        "a published generation was treated as a mutable preparation directory"
    );
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), before);
    assert!(!home.join(".claude").exists());
}
