//! Inspect the exact production Command; never probe, authenticate or spawn it.
use super::*;
use crate::providers::ProcessModelBinding;
use crate::session_migration::test_home::{IsolatedAppHome, TestEnvGuard};
use std::collections::BTreeMap;

fn command(
    opts: &SpawnOptions,
    settings: &AppSettings,
    version: Option<&str>,
    model: &str,
    native: Option<&GrokBuildProxySpawn>,
    ssh: bool,
) -> Command {
    command_with_home(
        opts,
        settings,
        version,
        model,
        native,
        ssh,
        Path::new("synthetic agent home"),
    )
}

fn command_with_home(
    opts: &SpawnOptions,
    settings: &AppSettings,
    version: Option<&str>,
    model: &str,
    native: Option<&GrokBuildProxySpawn>,
    ssh: bool,
    grok_home: &Path,
) -> Command {
    let sandbox = if should_apply_sandbox(version) {
        SandboxSpawnSpec::from_setting(
            opts.sandbox_profile
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(&settings.sandbox_profile),
        )
    } else {
        None
    };
    let max_turns = MaxTurnsSpawnSpec::from_setting(resolve_max_agent_turns(
        opts.max_agent_turns,
        settings.max_agent_turns,
    ));
    let spec = AgentCommandSpec {
        options: opts,
        settings,
        version,
        model,
        sandbox: sandbox.as_ref(),
        max_turns: max_turns.as_ref(),
        disallowed_tools: &settings.disallowed_tools,
        ssh,
        session_data_mode: &settings.session_data_mode,
        grok_home,
        native,
    };
    let mut cmd = Command::new("synthetic-cli-never-executed");
    cmd.env_clear();
    spec.apply_args(&mut cmd);
    spec.apply_env(&mut cmd);
    cmd
}

fn argv(cmd: &Command) -> Vec<String> {
    cmd.as_std()
        .get_args()
        .map(|value| value.to_str().unwrap().to_owned())
        .collect()
}

fn env(cmd: &Command) -> BTreeMap<String, String> {
    cmd.as_std()
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_str().unwrap().to_owned(),
                value.unwrap().to_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn full_default_command_keeps_official_argv_and_exact_explicit_environment() {
    let settings = AppSettings::default();
    let cmd = command(
        &SpawnOptions::default(),
        &settings,
        Some("1.0.0"),
        "grok-4.6",
        None,
        false,
    );
    assert_eq!(
        argv(&cmd),
        [
            "--no-auto-update",
            "--compaction-mode",
            "summary",
            "--permission-mode",
            "default",
            "--sandbox",
            "workspace",
            "--no-memory",
            "agent",
            "--no-leader",
            "--model",
            "grok-4.6",
            "stdio"
        ]
    );
    let expected = [
        ("GROK_COMPACTION_MODE", "summary"),
        ("GROK_MEMORY", "0"),
        ("GROK_SUBAGENT_WORKTREE_SNAPSHOT", "0"),
        ("GROK_TWO_PASS_COMPACTION", "0"),
        ("GROK_HOME", "synthetic agent home"),
        ("GROK_CLAUDE_MCPS_ENABLED", "false"),
        ("GROK_CURSOR_MCPS_ENABLED", "false"),
        ("GROK_SANDBOX", "workspace"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(env(&cmd), expected);
}

#[test]
fn full_customized_command_keeps_all_top_level_and_agent_options_in_order() {
    let settings = AppSettings {
        compaction_mode: "segments".into(),
        compaction_detail: "balanced".into(),
        max_agent_turns: Some(9),
        background_wait_policy: "timeout".into(),
        background_wait_timeout_sec: 42,
        disable_web_search: true,
        no_ask_user: true,
        allowed_tools: vec!["read_file".into(), "write".into()],
        disallowed_tools: vec!["run_terminal_command".into()],
        todo_gate_enabled: true,
        plan_enabled: false,
        preferred_agent: " synthetic reviewer ".into(),
        agent_profile_path: " /synthetic/profile with spaces.md ".into(),
        agents_json: r#" { "reviewer": { "prompt": "synthetic" } } "#.into(),
        subagents_enabled: false,
        experimental_memory: true,
        subagent_worktree_snapshot_enabled: true,
        two_pass_compaction_enabled: true,
        use_leader: true,
        ..AppSettings::default()
    };
    let opts = SpawnOptions {
        permission_policy: Some("accept_edits".into()),
        product_mode: Some("plan".into()),
        sandbox_profile: Some(" WORKSPACE ".into()),
        max_agent_turns: Some(21),
        json_schema: Some(r#" {"type":"object"} "#.into()),
        extra_rules: Some(" synthetic rules ".into()),
        system_prompt_override: Some(" synthetic system prompt ".into()),
        no_ask_user: Some(false),
        effort: Some(" max ".into()),
        plugin_dirs: vec![
            " /synthetic/plugin one ".into(),
            "/synthetic/plugin two".into(),
        ],
        folder_trust: true,
        fork_session: true,
        ..SpawnOptions::default()
    };
    let cmd = command(
        &opts,
        &settings,
        Some("1.0.0"),
        "synthetic-route",
        None,
        false,
    );
    assert_eq!(
        argv(&cmd),
        [
            "--no-auto-update",
            "--trust",
            "--compaction-mode",
            "segments",
            "--compaction-detail",
            "balanced",
            "--permission-mode",
            "plan",
            "--sandbox",
            "workspace",
            "--max-turns",
            "21",
            "--background-wait-timeout",
            "42",
            "--json-schema",
            r#"{"type":"object"}"#,
            "--rules",
            "synthetic rules",
            "--system-prompt-override",
            "synthetic system prompt",
            "--disable-web-search",
            "--tools",
            "read_file,write",
            "--disallowed-tools",
            "run_terminal_command",
            "--todo-gate",
            "--no-plan",
            "--agent",
            "synthetic reviewer",
            "--agents",
            r#"{"reviewer":{"prompt":"synthetic"}}"#,
            "--no-subagents",
            "--experimental-memory",
            "agent",
            "--leader",
            "--agent-profile",
            "/synthetic/profile with spaces.md",
            "--model",
            "synthetic-route",
            "--reasoning-effort",
            "max",
            "--plugin-dir",
            "/synthetic/plugin one",
            "--plugin-dir",
            "/synthetic/plugin two",
            "stdio"
        ]
    );
    let vars = env(&cmd);
    for (key, value) in [
        ("GROK_COMPACTION_MODE", "segments"),
        ("GROK_COMPACTION_DETAIL", "balanced"),
        ("GROK_MEMORY", "1"),
        ("GROK_SUBAGENTS", "0"),
        ("GROK_TWO_PASS_COMPACTION", "1"),
        ("GROK_SUBAGENT_WORKTREE_SNAPSHOT", "1"),
        ("GROK_SANDBOX", "workspace"),
    ] {
        assert_eq!(vars.get(key).map(String::as_str), Some(value));
    }
    assert!(!vars.contains_key("XAI_API_KEY"));
    assert!(!argv(&cmd).iter().any(|arg| arg == "--fork-session"));
}

fn config(mode: &str) -> String {
    format!("[models]\ndefault = \"synthetic-route\"\n[model.synthetic-route]\nmodel = \"grok-4.6\"\nbase_url = \"https://synthetic.invalid/v1\"\napi_key = \"synthetic-key-not-a-credential\"\napi_backend = \"responses\"\napp_provider_mode = \"{mode}\"\napp_models = '[{{\"id\":\"grok-4.6\",\"name\":\"6\"}},{{\"id\":\"grok-4.5\",\"name\":\"5\"}}]'\n")
}

#[test]
fn normal_producer_result_reaches_actual_command_and_removes_inherited_foreign_overrides() {
    let _fixture = IsolatedAppHome::acquire_with_user_home("main-producer-command");
    std::fs::write(crate::paths::agent_config_toml(), config("generic")).unwrap();
    let request = crate::providers::MainProfileRequest {
        owner_session_id: uuid::Uuid::new_v4().to_string(),
        provider_id: "synthetic-route".into(),
        model_id: "grok-4.5".into(),
        history: None,
    };
    let profile = crate::providers::PreparedMainProfile::prepare(
        &request,
        &AppSettings::default(),
        "ask",
        false,
        &crate::extensions::ExtensionsPrefs::default(),
        None,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    let options = SpawnOptions {
        model_id: Some(request.model_id.clone()),
        main_profile: Some(request),
        ..SpawnOptions::default()
    };
    let cmd = command_with_home(
        &options,
        &AppSettings::default(),
        Some("1.0.0"),
        &profile.model_binding.resolve("grok-4.5"),
        profile.native.as_ref(),
        false,
        &profile.home,
    );
    assert!(argv(&cmd)
        .windows(2)
        .any(|pair| pair == ["--model", "synthetic-route"]));
    let settings = AppSettings::default();
    let model = profile.model_binding.resolve("grok-4.5");
    let mut inherited = Command::new("synthetic-cli-never-executed");
    AgentCommandSpec {
        options: &options,
        settings: &settings,
        version: Some("1.0.0"),
        model: &model,
        sandbox: None,
        max_turns: None,
        disallowed_tools: &[],
        ssh: false,
        session_data_mode: &settings.session_data_mode,
        grok_home: &profile.home,
        native: profile.native.as_ref(),
    }
    .apply_env(&mut inherited);
    let variables = inherited
        .as_std()
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_str().unwrap(),
                value.and_then(std::ffi::OsStr::to_str),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(variables["GROK_HOME"], profile.home.to_str());
    for key in [
        "GROK_MODELS_BASE_URL",
        "GROK_MODELS_LIST_URL",
        "GROK_CLI_CHAT_PROXY_BASE_URL",
        "XAI_API_KEY",
        "GROK_WEB_SEARCH_MODEL",
        "GROK_IMAGE_DESCRIPTION_MODEL",
    ] {
        assert_eq!(variables[key], None, "a foreign override can be inherited");
    }
    assert!(profile
        .configuration
        .unwrap()
        .execution_source()
        .home()
        .unwrap()
        .is_some());
}

#[test]
fn captured_native_command_keeps_non_default_model_and_child_only_key_after_config_edit() {
    let _home = IsolatedAppHome::acquire("spawn-command-native");
    let settings = AppSettings {
        session_data_mode: "independent".into(),
        ..AppSettings::default()
    };
    let text = config("grok_build_proxy");
    std::fs::write(crate::paths::agent_config_toml(), &text).unwrap();
    let (binding, native, _) = ProcessModelBinding::capture_for_spawn(
        true,
        false,
        false,
        "grok-4.5",
        &crate::paths::agent_home_dir(),
        &settings,
    )
    .unwrap();
    // No global default read is allowed during argv/env construction.
    std::fs::write(
        crate::paths::agent_config_toml(),
        text.replace("synthetic.invalid", "foreign.invalid"),
    )
    .unwrap();
    let opts = SpawnOptions {
        effort: Some("xhigh".into()),
        ..SpawnOptions::default()
    };
    let model = binding.resolve("grok-4.5");
    let cmd = command_with_home(
        &opts,
        &settings,
        Some("1.0.0"),
        &model,
        native.as_ref(),
        false,
        &crate::paths::agent_home_dir(),
    );
    let vars = env(&cmd);
    assert_eq!(
        vars.get("GROK_HOME").unwrap(),
        crate::paths::agent_home_dir().to_str().unwrap()
    );
    assert!(argv(&cmd)
        .windows(2)
        .any(|pair| pair == ["--model", "grok-4.5"]));
    for (key, value) in [
        ("GROK_MODELS_BASE_URL", "https://synthetic.invalid/v1"),
        (
            "GROK_MODELS_LIST_URL",
            "https://synthetic.invalid/v1/models",
        ),
        (
            "GROK_CLI_CHAT_PROXY_BASE_URL",
            "https://synthetic.invalid/v1",
        ),
        ("XAI_API_KEY", "synthetic-key-not-a-credential"),
    ] {
        assert_eq!(vars.get(key).map(String::as_str), Some(value));
    }
    assert!(!argv(&cmd).iter().any(|arg| arg.contains("synthetic-key")));
    let unrelated = Command::new("unrelated-never-executed");
    assert!(unrelated.as_std().get_envs().next().is_none());
}

#[test]
fn prepared_runtime_and_captured_native_route_reach_the_same_actual_command() {
    let _home = IsolatedAppHome::acquire("runtime-capture-command");
    let home = crate::paths::agent_home_dir();
    let settings = AppSettings {
        session_data_mode: "independent".into(),
        official_aux_inject: false,
        subagents_enabled: false,
        experimental_memory: false,
        todo_gate_enabled: true,
        ..AppSettings::default()
    };
    std::fs::write(home.join("config.toml"), config("grok_build_proxy")).unwrap();
    assert!(!crate::providers::prepare_main_runtime(
        &home,
        &settings,
        "dont_ask",
        true,
        false,
        &crate::extensions::ExtensionsPrefs::default()
    )
    .unwrap());
    let (binding, native, identity) =
        ProcessModelBinding::capture_for_spawn(true, false, false, "grok-4.5", &home, &settings)
            .unwrap();
    assert!(identity.is_some());
    let options = SpawnOptions {
        permission_policy: Some("dont_ask".into()),
        effort: Some("high".into()),
        ..SpawnOptions::default()
    };
    let model = binding.resolve("grok-4.5");
    let cmd = command_with_home(
        &options,
        &settings,
        Some("1.0.0"),
        &model,
        native.as_ref(),
        false,
        &home,
    );
    let args = argv(&cmd);
    for pair in [
        ["--permission-mode", "dontAsk"],
        ["--model", "grok-4.5"],
        ["--reasoning-effort", "high"],
    ] {
        assert!(args.windows(2).any(|actual| actual == pair));
    }
    assert!(args.iter().any(|arg| arg == "--todo-gate"));
    assert!(args.iter().any(|arg| arg == "--no-subagents"));
    assert!(args.iter().any(|arg| arg == "--no-memory"));
    let variables = env(&cmd);
    assert_eq!(variables["GROK_HOME"], home.to_str().unwrap());
    assert_eq!(
        variables["GROK_CLI_CHAT_PROXY_BASE_URL"],
        "https://synthetic.invalid/v1"
    );
    assert_eq!(variables["XAI_API_KEY"], "synthetic-key-not-a-credential");
    assert!(!args
        .iter()
        .any(|arg| arg == "synthetic-key-not-a-credential"));
    assert!(!home.join("auth.json").exists());
}

#[test]
fn captured_generic_command_keeps_alias_and_never_injects_native_environment() {
    let _home = IsolatedAppHome::acquire("spawn-command-generic");
    let settings = AppSettings {
        session_data_mode: "independent".into(),
        ..AppSettings::default()
    };
    std::fs::write(crate::paths::agent_config_toml(), config("generic")).unwrap();
    let (binding, native, _) = ProcessModelBinding::capture_for_spawn(
        true,
        false,
        false,
        "grok-4.5",
        &crate::paths::agent_home_dir(),
        &settings,
    )
    .unwrap();
    let model = binding.resolve("grok-4.5");
    let cmd = command_with_home(
        &SpawnOptions::default(),
        &settings,
        Some("1.0.0"),
        &model,
        native.as_ref(),
        false,
        &crate::paths::agent_home_dir(),
    );
    assert!(argv(&cmd)
        .windows(2)
        .any(|pair| pair == ["--model", "synthetic-route"]));
    let vars = env(&cmd);
    assert_eq!(
        vars.get("GROK_HOME").unwrap(),
        crate::paths::agent_home_dir().to_str().unwrap()
    );
    for key in [
        "XAI_API_KEY",
        "GROK_MODELS_BASE_URL",
        "GROK_MODELS_LIST_URL",
        "GROK_CLI_CHAT_PROXY_BASE_URL",
    ] {
        assert!(!vars.contains_key(key));
    }
}

fn main_profile_fixture(
    mode: &str,
) -> (
    std::path::PathBuf,
    crate::session_execution::ExecutionSource,
    String,
) {
    let source = crate::session_execution::ExecutionSource::AppProfile {
        owner_session_id: uuid::Uuid::new_v4().to_string(),
        generation: uuid::Uuid::new_v4().to_string(),
    };
    let crate::session_execution::ExecutionSource::AppProfile {
        owner_session_id,
        generation,
    } = &source
    else {
        unreachable!()
    };
    let home = crate::paths::app_data_root()
        .join("execution-profiles")
        .join(owner_session_id)
        .join(generation);
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("execution-profile.json"),
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1, "ownerSessionId": owner_session_id, "generation": generation,
        }))
        .unwrap(),
    )
    .unwrap();
    let text = config(mode)
        .replace("synthetic-route", "profile-route")
        .replace("synthetic.invalid", "profile.invalid");
    std::fs::write(home.join("config.toml"), &text).unwrap();
    (home, source, text)
}

#[test]
fn main_profile_capture_and_actual_generic_command_use_the_target_home_not_global_decoy() {
    let _root = IsolatedAppHome::acquire("main-profile-generic-capture");
    let settings = AppSettings::default();
    std::fs::write(crate::paths::agent_config_toml(), config("generic")).unwrap();
    let (home, source, text) = main_profile_fixture("generic");
    let (binding, native, identity) =
        ProcessModelBinding::capture_for_spawn(true, false, false, "grok-4.5", &home, &settings)
            .unwrap();
    assert_eq!(identity.unwrap().execution_source(), source);
    assert_eq!(binding.resolve("grok-4.5"), "profile-route");
    assert!(native.is_none());
    let cmd = command_with_home(
        &SpawnOptions::default(),
        &settings,
        Some("1.0.0"),
        &binding.resolve("grok-4.5"),
        None,
        false,
        &home,
    );
    assert!(argv(&cmd)
        .windows(2)
        .any(|pair| pair == ["--model", "profile-route"]));
    assert_eq!(env(&cmd)["GROK_HOME"], home.to_str().unwrap());
    assert_eq!(
        std::fs::read_to_string(home.join("config.toml")).unwrap(),
        text
    );
    assert_eq!(
        std::fs::read_to_string(crate::paths::agent_config_toml()).unwrap(),
        config("generic")
    );
}

#[test]
fn main_profile_native_env_and_private_identity_share_the_same_captured_bytes() {
    let _root = IsolatedAppHome::acquire("main-profile-native-capture");
    let settings = AppSettings::default();
    std::fs::write(
        crate::paths::agent_config_toml(),
        config("grok_build_proxy"),
    )
    .unwrap();
    let (home, source, original) = main_profile_fixture("grok_build_proxy");
    let (binding, native, identity) =
        ProcessModelBinding::capture_for_spawn(true, false, false, "grok-4.5", &home, &settings)
            .unwrap();
    assert_eq!(identity.unwrap().execution_source(), source);
    std::fs::write(crate::paths::agent_config_toml(), b"not valid TOML").unwrap();
    std::fs::write(
        home.join("config.toml"),
        original.replace("profile.invalid", "later.invalid"),
    )
    .unwrap();
    let cmd = command_with_home(
        &SpawnOptions::default(),
        &settings,
        Some("1.0.0"),
        &binding.resolve("grok-4.5"),
        native.as_ref(),
        false,
        &home,
    );
    let vars = env(&cmd);
    assert_eq!(vars["GROK_MODELS_BASE_URL"], "https://profile.invalid/v1");
    assert_eq!(
        vars["GROK_CLI_CHAT_PROXY_BASE_URL"],
        "https://profile.invalid/v1"
    );
    assert_eq!(
        vars["GROK_MODELS_LIST_URL"],
        "https://profile.invalid/v1/models"
    );
    assert_eq!(vars["GROK_HOME"], home.to_str().unwrap());
    assert!(argv(&cmd)
        .windows(2)
        .any(|pair| pair == ["--model", "grok-4.5"]));
}

#[test]
fn main_profile_native_command_preserves_its_frozen_sanitize_endpoint() {
    let _root = IsolatedAppHome::acquire("main-profile-native-sanitize");
    let settings = AppSettings::default();
    let (home, source, original) = main_profile_fixture("grok_build_proxy");
    let crate::session_execution::ExecutionSource::AppProfile {
        owner_session_id,
        generation,
    } = source
    else {
        unreachable!()
    };
    let port = crate::relay_stream_proxy::ensure_started_blocking().unwrap();
    let base =
        format!("http://127.0.0.1:{port}/p/{owner_session_id}/{generation}/profile-route/v1");
    let text = original.replace("https://profile.invalid/v1", &base)
        + "app_upstream_base_url = \"https://profile.invalid/v1\"\n";
    std::fs::write(home.join("config.toml"), text).unwrap();
    let (binding, native, _identity) =
        ProcessModelBinding::capture_for_spawn(true, false, false, "grok-4.5", &home, &settings)
            .unwrap();
    let cmd = command_with_home(
        &SpawnOptions::default(),
        &settings,
        Some("1.0.0"),
        &binding.resolve("grok-4.5"),
        native.as_ref(),
        false,
        &home,
    );
    let vars = env(&cmd);
    assert_eq!(vars["GROK_CLI_CHAT_PROXY_BASE_URL"], base);
    assert_eq!(vars["GROK_MODELS_BASE_URL"], base);
    assert_eq!(vars["GROK_MODELS_LIST_URL"], format!("{base}/models"));
}

#[test]
fn missing_main_profile_config_never_captures_the_global_route() {
    let _root = IsolatedAppHome::acquire("main-profile-config-missing");
    let settings = AppSettings::default();
    std::fs::write(crate::paths::agent_config_toml(), config("generic")).unwrap();
    let (home, _, _) = main_profile_fixture("generic");
    std::fs::rename(home.join("config.toml"), home.join("preserved-config.toml")).unwrap();
    assert!(ProcessModelBinding::capture_for_spawn(
        true, false, false, "grok-4.5", &home, &settings
    )
    .is_err());
}

#[test]
fn invalid_main_profile_marker_never_downgrades_ownership_to_unavailable() {
    let _root = IsolatedAppHome::acquire("main-profile-marker-invalid");
    let settings = AppSettings::default();
    std::fs::write(crate::paths::agent_config_toml(), config("generic")).unwrap();
    let (home, _, _) = main_profile_fixture("generic");
    std::fs::write(home.join("execution-profile.json"), b"{}").unwrap();
    assert!(ProcessModelBinding::capture_for_spawn(
        true, false, false, "grok-4.5", &home, &settings
    )
    .is_err());
}

#[test]
fn main_profile_capture_survives_deleted_global_config_without_changing_its_identity() {
    let _root = IsolatedAppHome::acquire("main-profile-no-global");
    let settings = AppSettings::default();
    std::fs::write(crate::paths::agent_config_toml(), config("generic")).unwrap();
    let (home, source, _) = main_profile_fixture("generic");
    let (first, _, first_identity) =
        ProcessModelBinding::capture_for_spawn(true, false, false, "grok-4.5", &home, &settings)
            .unwrap();
    std::fs::rename(
        crate::paths::agent_config_toml(),
        crate::paths::agent_home_dir().join("preserved-global.toml"),
    )
    .unwrap();
    let (second, _, second_identity) =
        ProcessModelBinding::capture_for_spawn(true, false, false, "grok-4.5", &home, &settings)
            .unwrap();
    assert!(first == second);
    assert!(first_identity == second_identity);
    assert_eq!(second_identity.unwrap().execution_source(), source);
}

#[test]
fn missing_wrong_or_future_main_profile_markers_return_only_safe_errors() {
    let _root = IsolatedAppHome::acquire("main-profile-marker-boundaries");
    let settings = AppSettings::default();
    std::fs::write(crate::paths::agent_config_toml(), config("generic")).unwrap();
    let (home, _, _) = main_profile_fixture("generic");
    let marker = home.join("execution-profile.json");
    let valid: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&marker).unwrap()).unwrap();
    std::fs::rename(&marker, home.join("preserved-marker.json")).unwrap();
    let mut wrong_owner = valid.clone();
    wrong_owner["ownerSessionId"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
    let mut wrong_generation = valid.clone();
    wrong_generation["generation"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
    let mut future = valid;
    future["schemaVersion"] = serde_json::json!(99);
    for bytes in [
        None,
        Some(serde_json::to_vec(&wrong_owner).unwrap()),
        Some(serde_json::to_vec(&wrong_generation).unwrap()),
        Some(serde_json::to_vec(&future).unwrap()),
        Some(vec![b' '; 16 * 1024 + 1]),
    ] {
        if let Some(bytes) = bytes {
            std::fs::write(&marker, bytes).unwrap();
        }
        let error = match ProcessModelBinding::capture_for_spawn(
            true, false, false, "grok-4.5", &home, &settings,
        ) {
            Err(error) => error,
            Ok(_) => panic!("unowned main profile must fail closed"),
        };
        assert!(!error.contains("synthetic-key"));
        assert!(!error.contains(home.to_str().unwrap()));
        assert!(!error.contains("profile.invalid"));
    }
}

#[test]
fn main_profile_namespace_rejects_non_uuid_extra_and_parent_components() {
    let _root = IsolatedAppHome::acquire("main-profile-path-boundaries");
    let settings = AppSettings::default();
    std::fs::write(crate::paths::agent_config_toml(), config("generic")).unwrap();
    let (home, _, _) = main_profile_fixture("generic");
    for invalid in [
        home.join("extra"),
        home.join("../other-generation"),
        crate::paths::app_data_root().join("execution-profiles/not-a-uuid/not-a-generation"),
        crate::paths::app_data_root().join("execution-profiles"),
    ] {
        assert!(ProcessModelBinding::capture_for_spawn(
            true, false, false, "grok-4.5", &invalid, &settings
        )
        .is_err());
    }
}

#[test]
fn main_profile_route_class_and_target_config_are_validated_without_global_repair() {
    let _root = IsolatedAppHome::acquire("main-profile-config-boundaries");
    let settings = AppSettings::default();
    let global = config("generic");
    std::fs::write(crate::paths::agent_config_toml(), &global).unwrap();
    let (home, _, _) = main_profile_fixture("generic");
    assert!(ProcessModelBinding::capture_for_spawn(
        false, false, false, "grok-4.5", &home, &settings
    )
    .is_err());
    std::fs::write(home.join("config.toml"), b"not valid TOML").unwrap();
    assert!(ProcessModelBinding::capture_for_spawn(
        true, false, false, "grok-4.5", &home, &settings
    )
    .is_err());
    assert_eq!(
        std::fs::read_to_string(crate::paths::agent_config_toml()).unwrap(),
        global
    );
}

#[cfg(unix)]
#[test]
fn main_profile_config_cannot_be_a_redirect_to_global_decoy() {
    let _root = IsolatedAppHome::acquire("main-profile-config-redirect");
    let settings = AppSettings::default();
    std::fs::write(crate::paths::agent_config_toml(), config("generic")).unwrap();
    let (home, _, _) = main_profile_fixture("generic");
    std::fs::rename(home.join("config.toml"), home.join("preserved-config.toml")).unwrap();
    std::os::unix::fs::symlink(crate::paths::agent_config_toml(), home.join("config.toml"))
        .unwrap();
    assert!(ProcessModelBinding::capture_for_spawn(
        true, false, false, "grok-4.5", &home, &settings
    )
    .is_err());
}

#[test]
fn override_home_and_ssh_commands_do_not_acquire_local_relay_or_profile() {
    let _home = IsolatedAppHome::acquire("spawn-command-boundary");
    let settings = AppSettings {
        agent_profile_path: "/synthetic/local profile.md".into(),
        ..AppSettings::default()
    };
    std::fs::write(
        crate::paths::agent_config_toml(),
        config("grok_build_proxy"),
    )
    .unwrap();
    for ssh in [false, true] {
        let (binding, native, identity) = ProcessModelBinding::capture_for_spawn(
            false,
            ssh,
            !ssh,
            "grok-4.5",
            Path::new("synthetic official aux home"),
            &settings,
        )
        .unwrap();
        assert!(native.is_none());
        assert!(identity.is_none());
        let model = binding.resolve("grok-4.5");
        let cmd = command(
            &SpawnOptions::default(),
            &settings,
            Some("1.0.0"),
            &model,
            native.as_ref(),
            ssh,
        );
        let args = argv(&cmd);
        assert!(args.windows(2).any(|pair| pair == ["--model", "grok-4.5"]));
        assert_eq!(args.iter().any(|arg| arg == "--agent-profile"), !ssh);
        assert_eq!(env(&cmd).contains_key("GROK_HOME"), !ssh);
        assert!(!env(&cmd).contains_key("XAI_API_KEY"));
        assert!(args.contains(&if ssh { "--leader" } else { "--no-leader" }.to_owned()));
    }
}

#[test]
fn complete_command_preserves_version_soft_omission_and_invalid_effort_rejection() {
    let settings = AppSettings {
        compaction_mode: "segments".into(),
        background_wait_policy: "no_wait".into(),
        subagent_worktree_snapshot_enabled: true,
        two_pass_compaction_enabled: true,
        ..AppSettings::default()
    };
    let opts = SpawnOptions {
        effort: Some("not an effort".into()),
        ..SpawnOptions::default()
    };
    for version in [Some("0.2.112"), None] {
        let cmd = command(&opts, &settings, version, "grok-4.6", None, false);
        let args = argv(&cmd);
        for flag in [
            "--compaction-mode",
            "--compaction-detail",
            "--no-wait-for-background",
            "--reasoning-effort",
        ] {
            assert!(!args.iter().any(|arg| arg == flag));
        }
        let vars = env(&cmd);
        assert_eq!(
            vars.get("GROK_COMPACTION_MODE").map(String::as_str),
            Some("segments")
        );
        assert_eq!(
            vars.contains_key("GROK_TWO_PASS_COMPACTION"),
            version.is_none()
        );
        assert_eq!(
            vars.contains_key("GROK_SUBAGENT_WORKTREE_SNAPSHOT"),
            version.is_none()
        );
    }
}

#[test]
fn explicit_off_sandbox_does_not_inherit_the_default_workspace_sandbox() {
    let settings = AppSettings::default();
    assert_eq!(settings.sandbox_profile, "workspace");
    for profile in [" off ", "OFF"] {
        let opts = SpawnOptions {
            sandbox_profile: Some(profile.into()),
            ..SpawnOptions::default()
        };
        let cmd = command(&opts, &settings, Some("1.0.0"), "grok-4.6", None, false);
        assert!(!argv(&cmd).iter().any(|arg| arg == "--sandbox"));
        assert!(!env(&cmd).contains_key("GROK_SANDBOX"));
    }
    for version in [Some("1.0.0"), Some("0.2.111")] {
        let opts = SpawnOptions {
            sandbox_profile: Some("  ".into()),
            ..SpawnOptions::default()
        };
        let cmd = command(&opts, &settings, version, "grok-4.6", None, false);
        assert_eq!(
            env(&cmd).contains_key("GROK_SANDBOX"),
            version == Some("1.0.0")
        );
    }
}

#[test]
fn yolo_overrides_plan_without_moving_agent_options_after_stdio() {
    let opts = SpawnOptions {
        permission_policy: Some("always_approve".into()),
        product_mode: Some("plan".into()),
        ..SpawnOptions::default()
    };
    let cmd = command(
        &opts,
        &AppSettings::default(),
        Some("1.0.0"),
        "grok-4.6",
        None,
        false,
    );
    let args = argv(&cmd);
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--permission-mode", "bypassPermissions"]));
    let agent = args.iter().position(|arg| arg == "agent").unwrap();
    let yolo = args
        .iter()
        .position(|arg| arg == "--always-approve")
        .unwrap();
    assert!(agent < yolo && yolo < args.len() - 1);
    assert_eq!(args.last().map(String::as_str), Some("stdio"));
}

#[test]
fn command_auto_wake_merges_synthetic_overlay_without_mutating_parent_environment() {
    let _guard = TestEnvGuard::acquire(&["GROK_CONFIG"]);
    let inherited = r#"{"features":{"synthetic_keep":true},"synthetic_top":"keep"}"#;
    std::env::set_var("GROK_CONFIG", inherited);
    let settings = AppSettings {
        auto_wake_enabled: true,
        ..AppSettings::default()
    };
    let cmd = command(
        &SpawnOptions::default(),
        &settings,
        Some("1.0.0"),
        "grok-4.6",
        None,
        false,
    );
    let vars = env(&cmd);
    let overlay: serde_json::Value =
        serde_json::from_str(vars.get("GROK_CONFIG").unwrap()).unwrap();
    assert_eq!(overlay["features"]["auto_wake"], true);
    assert_eq!(overlay["features"]["synthetic_keep"], true);
    assert_eq!(overlay["synthetic_top"], "keep");
    assert_eq!(std::env::var("GROK_CONFIG").unwrap(), inherited);
}
