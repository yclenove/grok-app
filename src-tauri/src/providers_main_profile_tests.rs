use super::*;
use crate::{
    paths, session_execution::ExecutionSource, session_migration::test_home::IsolatedAppHome,
};
use std::fs;

const CONFIG: &str = "[models]\ndefault = 'other'\n[model.selected]\nmodel = 'target'\nbase_url = 'https://synthetic.invalid/v1'\napi_key = 'synthetic-selected-key'\n[model.other]\nmodel = 'foreign'\nbase_url = 'https://foreign.invalid/v1'\napi_key = 'synthetic-foreign-key'\n[features]\nunknown = ['中文', 'kept']\n[mcp_servers.user]\ncommand = 'unexecuted-synthetic'\nargs = ['中文']\nenabled = true\n";

fn fixture() -> IsolatedAppHome {
    let home = IsolatedAppHome::acquire_with_user_home("main-profile-producer");
    let runtime = paths::agent_home_dir();
    let official = crate::process_util::user_home().join(".grok");
    fs::create_dir_all(&official).unwrap();
    fs::write(
        official.join("config.toml"),
        "[auth]\npreferred_method = 'oidc'\n",
    )
    .unwrap();
    fs::write(
        official.join("auth.json"),
        r#"{"issuer":{"key":"synthetic-oidc-not-a-token"}}"#,
    )
    .unwrap();
    fs::write(runtime.join("config.toml"), CONFIG).unwrap();
    fs::write(
        runtime.join("auth.json"),
        b"foreign-auth-must-not-be-copied",
    )
    .unwrap();
    for (path, bytes) in [
        ("hooks/user.sh", "#!/bin/sh\n# 中文\n"),
        ("skills/demo/SKILL.md", "合成技能，不执行"),
        ("plugins/empty/manifest.json", "{}"),
        ("commands/name.md", "命令"),
        ("workflows/name.md", "流程"),
        ("memory/workspace/note.md", "记忆"),
        ("unknown-resource/data.txt", "未知资源"),
        ("sandbox.toml", "[profiles]\n"),
        (
            ".claude/settings.json",
            r#"{"env":{"LABEL":"中文"},"permissions":{"allow":["synthetic-tool"]}}"#,
        ),
        (
            "sessions/foreign/agent/chat_history.jsonl",
            "not-selected-history",
        ),
        ("logs/trace.txt", "not-an-execution-resource"),
    ] {
        let path = runtime.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fs::create_dir_all(runtime.join("skills/empty")).unwrap();
    home
}

fn request(provider: &str) -> MainProfileRequest {
    MainProfileRequest {
        owner_session_id: uuid::Uuid::new_v4().to_string(),
        provider_id: provider.into(),
        model_id: if provider == "official" {
            "grok-4.6"
        } else {
            "target"
        }
        .into(),
        history: None,
    }
}

fn prepare(
    request: &MainProfileRequest,
    key: Option<&str>,
    inject: bool,
) -> Result<PreparedMainProfile, String> {
    PreparedMainProfile::prepare(
        request,
        &AppSettings {
            official_aux_inject: inject,
            ..AppSettings::default()
        },
        "dont_ask",
        true,
        &ExtensionsPrefs::default(),
        key,
        &AtomicBool::new(false),
    )
}

#[test]
fn custom_producer_preserves_resources_and_captures_only_its_selected_configuration() {
    let _fixture = fixture();
    let request = request("selected");
    let runtime = paths::agent_home_dir();
    let profile = prepare(&request, None, true).unwrap();
    let source = ExecutionSource::from_app_profile_home(&profile.home)
        .unwrap()
        .unwrap();
    assert!(
        matches!(&source, ExecutionSource::AppProfile { owner_session_id, .. } if owner_session_id == &request.owner_session_id)
    );
    assert_eq!(source.home().unwrap(), Some(profile.home.clone()));
    assert!(profile.custom && profile.inject);
    assert!(profile.native.is_none());
    assert_eq!(profile.model_binding.resolve("target"), "selected");
    assert_eq!(
        profile.configuration.as_ref().unwrap().execution_source(),
        source
    );
    assert_eq!(
        profile
            .configuration
            .as_ref()
            .unwrap()
            .profile_cached_token(),
        Some(false)
    );
    let text = fs::read_to_string(profile.home.join("config.toml")).unwrap();
    assert!(text.contains("synthetic-selected-key") && text.contains("中文"));
    assert!(!text.contains("synthetic-foreign-key") && !text.contains("model.other"));
    assert!(!profile.home.join("auth.json").exists());
    assert!(!profile.home.join("sessions").exists());
    assert!(!profile.home.join("logs").exists());
    assert!(profile.home.join("skills/empty").is_dir());
    for path in [
        "hooks/user.sh",
        "skills/demo/SKILL.md",
        "plugins/empty/manifest.json",
        "commands/name.md",
        "workflows/name.md",
        "memory/workspace/note.md",
        "unknown-resource/data.txt",
        "sandbox.toml",
    ] {
        assert_eq!(
            fs::read(profile.home.join(path)).unwrap(),
            fs::read(runtime.join(path)).unwrap()
        );
    }
    let claude: serde_json::Value =
        serde_json::from_slice(&fs::read(profile.home.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(claude["env"]["LABEL"], "中文");
    assert_eq!(claude["permissions"]["allow"][0], "synthetic-tool");
    assert_eq!(claude["permissions"]["defaultMode"], "dontAsk");
    assert_eq!(
        fs::read_to_string(runtime.join("config.toml")).unwrap(),
        CONFIG
    );
    assert_eq!(
        fs::read(runtime.join("auth.json")).unwrap(),
        b"foreign-auth-must-not-be-copied"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(profile.home.join("auth.json"))
                .err()
                .unwrap()
                .kind(),
            std::io::ErrorKind::NotFound
        );
        assert_eq!(
            fs::metadata(&profile.home).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}

#[test]
fn official_producer_takes_auth_only_from_official_source_and_never_injects_aux() {
    let _fixture = fixture();
    let official = crate::process_util::user_home().join(".grok");
    let before = fs::read(official.join("auth.json")).unwrap();
    let profile = prepare(&request("official"), None, true).unwrap();
    assert!(!profile.custom && !profile.inject);
    assert_eq!(
        profile
            .configuration
            .as_ref()
            .unwrap()
            .profile_cached_token(),
        Some(true)
    );
    assert!(profile.native.is_none());
    assert_eq!(profile.model_binding.resolve("grok-4.6"), "grok-4.6");
    let text = fs::read_to_string(profile.home.join("config.toml")).unwrap();
    assert!(!text.contains("synthetic-selected-key") && !text.contains("synthetic-foreign-key"));
    assert!(text.contains("oidc"));
    assert_eq!(fs::read(profile.home.join("auth.json")).unwrap(), before);
    assert_eq!(fs::read(official.join("auth.json")).unwrap(), before);
    assert!(!profile
        .home
        .join("hooks/grok-app-block-native-imagine.sh")
        .exists());
}

#[test]
fn explicit_official_key_cannot_bypass_policy_and_does_not_copy_oidc() {
    let _fixture = fixture();
    let official = crate::process_util::user_home().join(".grok");
    let official_request = request("official");
    assert!(prepare(&official_request, Some("synthetic-official-key"), false).is_err());
    assert!(!paths::app_data_root().join("execution-profiles").exists());
    fs::write(official.join("config.toml"), "").unwrap();
    fs::write(
        official.join("auth.json"),
        "malformed-must-not-be-read-with-key",
    )
    .unwrap();
    let profile = prepare(&official_request, Some("synthetic-official-key"), false).unwrap();
    assert!(!profile.home.join("auth.json").exists());
    assert_eq!(profile.model_binding.resolve("grok-4.6"), "grok-4.6");
    assert_eq!(
        profile
            .configuration
            .as_ref()
            .unwrap()
            .profile_cached_token(),
        Some(false)
    );
    assert_eq!(
        ConfigSnapshot::parse(&fs::read_to_string(profile.home.join("config.toml")).unwrap())
            .unwrap()
            .model_fields("grok-4.6")
            .unwrap()
            .unwrap()["api_key"],
        "synthetic-official-key"
    );
    assert!(prepare(&request("selected"), Some("synthetic-official-key"), false).is_err());
}

#[test]
fn malformed_runtime_resources_leave_an_unpublished_unusable_generation() {
    let _fixture = fixture();
    let request = request("selected");
    fs::write(paths::agent_home_dir().join(".claude/settings.json"), "[]").unwrap();
    assert!(prepare(&request, None, false).is_err());
    let owner = paths::app_data_root()
        .join("execution-profiles")
        .join(&request.owner_session_id);
    let entries = fs::read_dir(owner)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(entries.len(), 1);
    let failed = entries[0].path();
    assert!(!failed.join("execution-profile.json").exists());
    assert!(ExecutionSource::from_app_profile_home(&failed).is_err());
    assert_eq!(
        fs::read_to_string(paths::agent_home_dir().join("config.toml")).unwrap(),
        CONFIG
    );
    assert_eq!(
        fs::read_to_string(paths::agent_home_dir().join(".claude/settings.json")).unwrap(),
        "[]"
    );
    let error = prepare(&request, None, false).err().unwrap();
    assert!(!error.contains("synthetic-selected-key"));
}

#[test]
fn repeated_preparation_allocates_new_generations_without_rewriting_the_previous_one() {
    let _fixture = fixture();
    let request = request("selected");
    let first = prepare(&request, None, false).unwrap();
    let before = fs::read(first.home.join("config.toml")).unwrap();
    fs::write(
        paths::agent_home_dir().join("config.toml"),
        CONFIG.replace("synthetic-selected-key", "synthetic-new-key"),
    )
    .unwrap();
    let second = prepare(&request, None, false).unwrap();
    assert_ne!(first.home, second.home);
    assert_eq!(fs::read(first.home.join("config.toml")).unwrap(), before);
    assert!(fs::read_to_string(second.home.join("config.toml"))
        .unwrap()
        .contains("synthetic-new-key"));
    assert!(
        first.configuration.as_ref().unwrap().execution_source()
            != second.configuration.as_ref().unwrap().execution_source()
    );
}

#[test]
fn malformed_captured_mcp_is_refused_before_publishing_a_profile_marker() {
    let _fixture = fixture();
    let request = request("selected");
    let malformed = CONFIG.replace("args = ['中文']", "args = ['中文', 42]");
    fs::write(paths::agent_config_toml(), &malformed).unwrap();
    let error = prepare(&request, None, false).err().unwrap();
    assert!(!error.contains("synthetic-selected-key"));
    let owner = paths::app_data_root()
        .join("execution-profiles")
        .join(&request.owner_session_id);
    let generations = fs::read_dir(owner)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(generations.len(), 1);
    assert!(!generations[0]
        .path()
        .join("execution-profile.json")
        .exists());
    assert!(ExecutionSource::from_app_profile_home(&generations[0].path()).is_err());
    assert_eq!(
        fs::read_to_string(paths::agent_config_toml()).unwrap(),
        malformed
    );
}

#[test]
fn input_snapshot_rejects_changed_configuration_and_resources() {
    let _fixture = fixture();
    let runtime = paths::agent_home_dir();
    let official = crate::process_util::user_home().join(".grok");
    let captured = Inputs::capture(&runtime, &official, false, false, false).unwrap();
    fs::write(
        runtime.join("config.toml"),
        CONFIG.replace("target", "changed"),
    )
    .unwrap();
    assert!(captured.unchanged().is_err());
    fs::write(runtime.join("config.toml"), CONFIG).unwrap();
    let captured = Inputs::capture(&runtime, &official, false, false, false).unwrap();
    fs::write(runtime.join("skills/demo/SKILL.md"), "资源变化").unwrap();
    assert!(captured.unchanged().is_err());
}

#[test]
fn same_bytes_in_a_replaced_input_file_are_not_the_captured_source() {
    let _fixture = fixture();
    let runtime = paths::agent_home_dir();
    let official = crate::process_util::user_home().join(".grok");
    let captured = Inputs::capture(&runtime, &official, true, false, false).unwrap();
    let source = official.join("auth.json");
    let bytes = fs::read(&source).unwrap();
    fs::rename(&source, official.join("original-auth.kept")).unwrap();
    fs::write(&source, bytes).unwrap();
    assert!(captured.unchanged().is_err());
}

#[test]
fn pending_capability_requires_exclusive_directory_and_is_consumed_by_publication() {
    let _fixture = fixture();
    let request = request("selected");
    let pending = PendingHome::create(&request.owner_session_id).unwrap();
    let home = pending.home().to_owned();
    assert!(ExecutionSource::from_app_profile_home(&home).is_err());
    profile_io::write_new(&home.join("config.toml"), b"[models]\ndefault = 'grok'\n").unwrap();
    fs::write(home.join("execution-profile.json"), "competing-marker").unwrap();
    assert!(pending.check().is_err());
    assert!(pending
        .publish(b"unused-competing-marker-rejected-first")
        .is_err());
    assert_eq!(
        fs::read_to_string(home.join("execution-profile.json")).unwrap(),
        "competing-marker"
    );
    assert!(PendingHome::create("../not-an-owner").is_err());
}

#[test]
fn cancellation_rejects_generation_before_any_publication() {
    let _fixture = fixture();
    assert!(PreparedMainProfile::prepare(
        &request("selected"),
        &AppSettings::default(),
        "ask",
        false,
        &ExtensionsPrefs::default(),
        None,
        &AtomicBool::new(true),
    )
    .is_err());
    assert!(!paths::app_data_root().join("execution-profiles").exists());
}

#[test]
fn cancellation_under_source_lock_prevents_generation_publication() {
    let _fixture = fixture();
    let request = request("selected");
    let cancelled = std::sync::Arc::new(AtomicBool::new(false));
    let worker = crate::agent_home_config::with_config_write_lock(|| {
        let signal = std::sync::Arc::clone(&cancelled);
        let worker = std::thread::spawn(move || {
            PreparedMainProfile::prepare(
                &request,
                &AppSettings::default(),
                "ask",
                false,
                &ExtensionsPrefs::default(),
                None,
                &signal,
            )
        });
        cancelled.store(true, std::sync::atomic::Ordering::Release);
        worker
    });
    assert!(worker.join().unwrap().is_err());
    assert!(!paths::app_data_root().join("execution-profiles").exists());
}

#[test]
fn published_configuration_seal_rejects_changed_bytes_for_all_profile_readers() {
    let _fixture = fixture();
    let profile = prepare(&request("selected"), None, false).unwrap();
    let source = profile.configuration.as_ref().unwrap().execution_source();
    assert!(
        ProcessConfigBinding::from_capture(CONFIG, &profile.home, &AppSettings::default()).is_err(),
        "a captured config different from the published seal was accepted"
    );
    fs::write(profile.home.join("config.toml"), CONFIG).unwrap();
    assert!(source.home().is_err());
    assert!(ExecutionSource::from_app_profile_home(&profile.home).is_err());
    assert!(ProcessModelBinding::capture_for_spawn(
        true,
        false,
        false,
        "target",
        &profile.home,
        &AppSettings::default(),
    )
    .is_err());
}

#[test]
fn mutable_and_sanitized_routes_are_rebound_to_this_generation_and_original_upstream() {
    let _fixture = fixture();
    let runtime = paths::agent_home_dir();
    let request = request("selected");
    fs::write(runtime.join("config.toml"), CONFIG.replace(
        "https://synthetic.invalid/v1", "http://127.0.0.1:7/r/selected/v1",
    ).replace("api_key = 'synthetic-selected-key'", "api_key = 'synthetic-selected-key'\napp_upstream_base_url = 'https://synthetic.invalid/v1'\napp_provider_mode = 'grok_build_proxy'\napi_backend = 'responses'" )).unwrap();
    let original = fs::read(runtime.join("config.toml")).unwrap();
    let profile = prepare(&request, None, false).unwrap();
    let source = profile.configuration.as_ref().unwrap().execution_source();
    let ExecutionSource::AppProfile {
        owner_session_id,
        generation,
    } = source
    else {
        panic!("not a profile");
    };
    let fields =
        ConfigSnapshot::parse(&fs::read_to_string(profile.home.join("config.toml")).unwrap())
            .unwrap()
            .model_fields("selected")
            .unwrap()
            .unwrap();
    let base = &fields["base_url"];
    assert!(base.contains(&format!("/p/{owner_session_id}/{generation}/selected/v1")));
    assert_eq!(
        fields["app_upstream_base_url"],
        "https://synthetic.invalid/v1"
    );
    assert_eq!(profile.native.as_ref().unwrap().base_url, *base);
    assert_eq!(profile.model_binding.resolve("target"), "target");
    assert_eq!(fs::read(runtime.join("config.toml")).unwrap(), original);
}

#[test]
fn redirected_source_resources_are_refused_without_creating_a_generation() {
    let fixture = fixture();
    let external = fixture.path.join("outside");
    fs::create_dir(&external).unwrap();
    let link = paths::agent_home_dir().join("redirect");
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir(&external, &link);
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(&external, &link);
    #[cfg(not(any(unix, windows)))]
    let result: Result<(), std::io::Error> = Err(std::io::ErrorKind::Unsupported.into());
    result.expect("this platform must exercise source symlink rejection");
    assert!(prepare(&request("selected"), None, false).is_err());
    assert!(!paths::app_data_root().join("execution-profiles").exists());
}

fn injected_authorization<'a>(servers: &'a serde_json::Value, name: &str) -> &'a str {
    servers
        .as_array()
        .unwrap()
        .iter()
        .find(|server| server["name"] == name)
        .unwrap()
        .get("headers")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|header| {
            header["name"]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("authorization"))
        })
        .unwrap()["value"]
        .as_str()
        .unwrap()
}

#[test]
fn captured_mcp_header_cannot_be_overridden_by_later_same_name_foreign_credentials() {
    let _fixture = fixture();
    let runtime = paths::agent_home_dir();
    let official = crate::process_util::user_home().join(".grok");
    let config = format!("{CONFIG}\n[mcp_servers.bound]\nurl = 'https://intended.invalid/mcp'\nheaders = {{ Authorization = 'Bearer synthetic-intended' }}\n");
    fs::write(runtime.join("config.toml"), config).unwrap();
    let profile = prepare(&request("selected"), None, false).unwrap();
    fs::write(
        official.join("mcp_credentials.json"),
        serde_json::json!({"bound": {
            "access_token": "synthetic-later-foreign",
            "resource": "https://foreign.invalid/mcp"
        }})
        .to_string(),
    )
    .unwrap();
    let servers = profile.runtime.mcp_servers_for_connect(None, None);
    assert_eq!(
        injected_authorization(&servers, "bound"),
        "Bearer synthetic-intended"
    );
}

#[test]
fn captured_main_and_shared_mcp_credentials_follow_their_definition_source() {
    let _fixture = fixture();
    let runtime = paths::agent_home_dir();
    let official = crate::process_util::user_home().join(".grok");
    fs::write(
        runtime.join("config.toml"),
        format!("{CONFIG}\n[mcp_servers.bound]\nurl = 'https://main.invalid/mcp'\n"),
    )
    .unwrap();
    fs::write(
        runtime.join("mcp_credentials.json"),
        serde_json::json!({"bound": {
            "access_token": "synthetic-main",
            "resource": "https://main.invalid/mcp"
        }})
        .to_string(),
    )
    .unwrap();
    fs::write(
        official.join("config.toml"),
        "[mcp_servers.bound]\nurl = 'https://shared.invalid/mcp'\n[mcp_servers.shared_only]\nurl = 'https://shared-only.invalid/mcp'\n",
    )
    .unwrap();
    fs::write(
        official.join("mcp_credentials.json"),
        serde_json::json!({"servers": {
            "bound": {"access_token": "synthetic-shared-conflict", "resource": "https://shared.invalid/mcp"},
            "shared_only": {"access_token": "synthetic-shared-only", "resource": "https://shared-only.invalid/mcp"}
        }})
        .to_string(),
    )
    .unwrap();
    let profile = prepare(&request("selected"), None, false).unwrap();
    let servers = profile.runtime.mcp_servers_for_connect(None, None);
    assert_eq!(
        injected_authorization(&servers, "bound"),
        "Bearer synthetic-main"
    );
    assert_eq!(
        injected_authorization(&servers, "shared_only"),
        "Bearer synthetic-shared-only"
    );
    fs::write(
        official.join("mcp_credentials.json"),
        serde_json::json!({"servers": {
            "bound": {"access_token": "synthetic-new-account", "resource": "https://main.invalid/mcp"},
            "shared_only": {"access_token": "synthetic-new-account", "resource": "https://shared-only.invalid/mcp"}
        }})
        .to_string(),
    )
    .unwrap();
    let reopened = profile.runtime.mcp_servers_for_connect(None, None);
    assert_eq!(
        injected_authorization(&reopened, "bound"),
        "Bearer synthetic-main"
    );
    assert_eq!(
        injected_authorization(&reopened, "shared_only"),
        "Bearer synthetic-shared-only"
    );
    let rebound = prepare(&request("selected"), None, false).unwrap();
    assert_ne!(profile.home, rebound.home);
    let new_servers = rebound.runtime.mcp_servers_for_connect(None, None);
    assert_eq!(
        injected_authorization(&new_servers, "bound"),
        "Bearer synthetic-main"
    );
    assert_eq!(
        injected_authorization(&new_servers, "shared_only"),
        "Bearer synthetic-new-account"
    );
}

#[test]
fn shared_mcp_authentication_arrival_invalidates_the_unpublished_input_snapshot() {
    let _fixture = fixture();
    let runtime = paths::agent_home_dir();
    let official = crate::process_util::user_home().join(".grok");
    let inputs = Inputs::capture(&runtime, &official, false, false, true).unwrap();
    fs::write(
        official.join("mcp_credentials.json"),
        r#"{"bound":{"access_token":"synthetic-only"}}"#,
    )
    .unwrap();
    assert!(inputs.unchanged().is_err());
    assert!(!paths::app_data_root().join("execution-profiles").exists());
}
