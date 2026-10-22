//! The selected custom route cannot inherit an official login or bypass policy.
use super::*;

fn project(prefix: &str, mode: &str) -> Result<String, String> {
    ConfigSnapshot::parse(&format!("{prefix}{}", projection_fixture(mode)))?
        .project_custom("real", "target-two")
}

fn namespace<'a>(snapshot: &'a ConfigSnapshot, name: &str) -> &'a Table {
    let Value::Table(table) = &snapshot.root.fields[name] else {
        panic!("auth namespace must be a table");
    };
    table
}

fn api_key_only(snapshot: &ConfigSnapshot, name: &str) {
    assert!(
        matches!(namespace(snapshot, name).fields.get("preferred_method"),
        Some(Value::String(value)) if value == "api_key")
    );
}

#[test]
fn custom_projection_pins_api_key_without_changing_source_or_process_model() {
    for mode in ["generic", "grok_build_proxy"] {
        let source = projection_fixture(mode);
        let text = ConfigSnapshot::parse(&source)
            .unwrap()
            .project_custom("real", "target-two")
            .unwrap();
        let snapshot = ConfigSnapshot::parse(&text).unwrap();
        api_key_only(&snapshot, "auth");
        assert!(!ConfigSnapshot::parse(&source)
            .unwrap()
            .root
            .fields
            .contains_key("auth"));
        let (binding, native) =
            crate::providers::ProcessModelBinding::from_text(&text, true, false, "target-two")
                .unwrap();
        assert_eq!(
            binding.resolve("target-two"),
            if native.is_some() {
                "target-two"
            } else {
                "real"
            }
        );
        assert_eq!(
            snapshot.model_fields("real").unwrap().unwrap()["api_key"],
            "synthetic-selected-key"
        );
    }
}

#[test]
fn official_login_definitions_do_not_enter_custom_profile_but_literals_survive() {
    let prefix = "note = '''[auth]\npreferred_method = 'oidc'\n'''\n\
        [auth]\npreferred_method = 'oidc'\nfuture = {keep = ['中文', 1]}\n\
        auth_provider_command = 'synthetic-official-command'\n\
        [auth.oauth2]\nclient_secret = 'synthetic-official-secret'\n\
        [auth.oidc]\nissuer = 'https://synthetic-issuer.invalid'\n\
        [grok_com_config]\npreferred_method = 'oidc'\ndisable_api_key_auth = false\n\
        auth_provider_command = 'synthetic-legacy-official-command'\n\
        [grok_com_config.oauth2]\nclient_secret = 'synthetic-legacy-official-secret'\n";
    let original = ConfigSnapshot::parse(prefix).unwrap();
    let text = project(prefix, "generic").unwrap();
    let snapshot = ConfigSnapshot::parse(&text).unwrap();
    api_key_only(&snapshot, "auth");
    api_key_only(&snapshot, "grok_com_config");
    assert_eq!(
        semantic(&snapshot.root.fields["note"]),
        semantic(&original.root.fields["note"])
    );
    assert_eq!(
        semantic(&namespace(&snapshot, "auth").fields["future"]),
        semantic(&namespace(&original, "auth").fields["future"])
    );
    for name in ["auth", "grok_com_config"] {
        let fields = &namespace(&snapshot, name).fields;
        for field in ["oidc", "oauth2", "auth_provider_command"] {
            assert!(!fields.contains_key(field));
        }
    }
    assert!(!text.contains("synthetic-official-secret"));
    assert!(!text.contains("synthetic-legacy-official-secret"));
}

#[test]
fn inline_and_quoted_auth_namespaces_have_the_same_policy() {
    let text = project(
        "'auth' = {preferred_method = 'oidc', future = [false, {keep = 'literal'}]}\n\
        'grok_com_config' = {preferred_method = 'oidc', disable_api_key_auth = false}\n",
        "grok_build_proxy",
    )
    .unwrap();
    let snapshot = ConfigSnapshot::parse(&text).unwrap();
    api_key_only(&snapshot, "auth");
    api_key_only(&snapshot, "grok_com_config");
    assert!(namespace(&snapshot, "auth").fields.contains_key("future"));
}

#[test]
fn explicit_api_key_ban_cannot_be_removed_by_channel_projection() {
    for name in ["auth", "grok_com_config"] {
        let error = project(
            &format!("[{name}]\ndisable_api_key_auth = true\n"),
            "generic",
        )
        .expect_err("administrative API key ban must reject custom profile");
        assert_eq!(error, "selected route conflicts with authentication policy");
    }
}

#[test]
fn forced_team_login_cannot_be_removed_by_channel_projection() {
    for name in ["auth", "grok_com_config"] {
        for value in [
            "'synthetic-private-team'",
            "['synthetic-private-team']",
            "[]",
        ] {
            let error = project(
                &format!("[{name}]\nforce_login_team_uuid = {value}\n"),
                "generic",
            )
            .expect_err("team restriction must reject custom profile");
            assert_eq!(error, "selected route conflicts with authentication policy");
            assert!(!error.contains("synthetic-private-team"));
        }
    }
}

#[test]
fn invalid_or_unknown_auth_policy_is_not_silently_repaired() {
    for prefix in [
        "auth = 1\n",
        "grok_com_config = false\n",
        "[auth]\npreferred_method = 1\n",
        "[auth]\npreferred_method = 'future-method'\n",
        "[auth]\ndisable_api_key_auth = 'false'\n",
        "[grok_com_config]\ndisable_api_key_auth = [false]\n",
    ] {
        assert!(
            project(prefix, "generic").is_err(),
            "invalid policy must reject projection"
        );
    }
}

#[test]
fn explicit_grok_com_policy_has_the_same_per_field_precedence_as_build() {
    let allowed = "[auth]\ndisable_api_key_auth = true\npreferred_method = 'oidc'\n\
        [grok_com_config]\ndisable_api_key_auth = false\n";
    let text = project(allowed, "generic").unwrap();
    let snapshot = ConfigSnapshot::parse(&text).unwrap();
    api_key_only(&snapshot, "auth");
    api_key_only(&snapshot, "grok_com_config");
    for (name, value) in [("auth", "true"), ("grok_com_config", "false")] {
        assert!(
            matches!(namespace(&snapshot, name).fields.get("disable_api_key_auth"),
            Some(Value::Atom(actual)) if actual == value)
        );
    }
    for denied in [
        "[auth]\ndisable_api_key_auth = false\n[grok_com_config]\ndisable_api_key_auth = true\n",
        "[auth]\ndisable_api_key_auth = true\n[grok_com_config]\npreferred_method = 'api_key'\n",
    ] {
        assert_eq!(
            project(denied, "generic").err().unwrap(),
            "selected route conflicts with authentication policy"
        );
    }
}

#[test]
fn projected_auth_policy_survives_actual_runtime_preparation_and_process_capture() {
    use crate::{extensions::ExtensionsPrefs, paths, providers, store::AppSettings};
    use std::fs;
    let _fixture =
        crate::session_migration::test_home::IsolatedAppHome::acquire("profile-auth-capture");
    for mode in ["generic", "grok_build_proxy"] {
        let source = format!("[auth]\npreferred_method = 'oidc'\n[auth.oauth2]\nclient_secret = 'synthetic-official-secret'\n{}", projection_fixture(mode));
        fs::write(paths::agent_config_toml(), &source).unwrap();
        let owner = uuid::Uuid::new_v4().to_string();
        let pending = providers::execution_home::PendingHome::create(&owner).unwrap();
        let crate::session_execution::ExecutionSource::AppProfile { generation, .. } =
            pending.source()
        else {
            panic!("not a profile");
        };
        let generation = generation.clone();
        let home = pending.home().to_owned();
        fs::write(home.join("config.toml"), project("[auth]\npreferred_method = 'oidc'\n[auth.oauth2]\nclient_secret = 'synthetic-official-secret'\n", mode).unwrap()).unwrap();
        let settings = AppSettings {
            session_data_mode: "independent".into(),
            ..AppSettings::default()
        };
        assert!(!providers::runtime_config::prepare_pending_runtime(
            &pending,
            &settings,
            "ask",
            true,
            false,
            &ExtensionsPrefs::default()
        )
        .unwrap());
        pending
            .publish(&fs::read(home.join("config.toml")).unwrap())
            .unwrap();
        let text = fs::read_to_string(home.join("config.toml")).unwrap();
        let snapshot = ConfigSnapshot::parse(&text).unwrap();
        api_key_only(&snapshot, "auth");
        assert!(!namespace(&snapshot, "auth").fields.contains_key("oauth2"));
        let (binding, native, identity) = providers::ProcessModelBinding::capture_for_spawn(
            true,
            false,
            false,
            "target-two",
            &home,
            &settings,
        )
        .unwrap();
        assert_eq!(
            binding.resolve("target-two"),
            if mode == "generic" {
                "real"
            } else {
                "target-two"
            }
        );
        if let Some(native) = native {
            assert_eq!(native.api_key, "synthetic-selected-key");
        }
        assert!(
            matches!(identity.unwrap().execution_source(), crate::session_execution::ExecutionSource::AppProfile {
            owner_session_id, generation: actual
        } if owner_session_id == owner && actual == generation)
        );
        assert!(!home.join("auth.json").exists());
        assert_eq!(
            fs::read_to_string(paths::agent_config_toml()).unwrap(),
            source
        );
    }
}
