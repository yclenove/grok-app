//! Official projection and its real preparation/capture consumers, offline only.
use super::*;

fn project(runtime: &str, auth: &str, model: &str, key: Option<&str>) -> Result<String, String> {
    ConfigSnapshot::parse(runtime)?.project_official(model, ConfigSnapshot::parse(auth)?, key)
}

fn table<'a>(snapshot: &'a ConfigSnapshot, name: &str) -> &'a Table {
    let Value::Table(table) = &snapshot.root.fields[name] else {
        panic!("expected a configuration table");
    };
    table
}

#[test]
fn official_projection_removes_foreign_routes_and_keeps_runtime_policy() {
    let runtime = format!(
        "note = '''[model.foreign]\napi_key = 'literal-not-a-route'\n'''\n{}
        [endpoints]\nmodels_base_url = 'https://synthetic-relay.invalid'\n
        models_endpoint = 'https://synthetic-relay.invalid/models'\n
        models_list_url = 'https://synthetic-relay.invalid/other'\n
        xai_api_base_url = 'https://synthetic-relay.invalid/v1'\n
        cli_chat_proxy_base_url = 'https://synthetic-relay.invalid/proxy'\n
        future = {{keep = ['中文', 1]}}\n
        [model_providers.foreign]\napi_key = 'synthetic-foreign-provider'\n
        [auth_provider.foreign]\ncommand = 'synthetic-never-executed'\n
        [features]\nfuture = {{keep = ['中文', 1]}}\n
        [tools]\nallow = ['synthetic-tool']\n
        [[hooks]]\ncommand = ['synthetic-hook', '--never-executed']\n
        [mcp_servers.'synthetic.user.mcp']\ncommand = 'synthetic-mcp'\n",
        projection_fixture("grok_build_proxy")
    );
    let original = ConfigSnapshot::parse(&runtime).unwrap();
    let text = project(&runtime, "", "grok-4.5", None).unwrap();
    let actual = ConfigSnapshot::parse(&text).unwrap();
    assert_eq!(actual.default_model().unwrap(), Some("grok"));
    assert!(actual.model_ids().unwrap().is_empty());
    for name in ["model_providers", "auth_provider"] {
        assert!(!actual.root.fields.contains_key(name));
    }
    for name in ["note", "features", "tools", "hooks", "mcp_servers"] {
        assert_eq!(
            semantic(&original.root.fields[name]),
            semantic(&actual.root.fields[name])
        );
    }
    for (name, value) in &table(&original, "models").fields {
        if ![
            "default",
            "web_search",
            "session_summary",
            "image_description",
            "prompt_suggestion",
        ]
        .contains(&name.as_str())
        {
            assert_eq!(
                semantic(value),
                semantic(&table(&actual, "models").fields[name])
            );
        }
    }
    assert_eq!(table(&actual, "endpoints").fields.len(), 1);
    assert_eq!(
        semantic(&table(&actual, "endpoints").fields["future"]),
        semantic(&table(&original, "endpoints").fields["future"])
    );
    assert!(!text.contains("synthetic-selected-key"));
    assert!(!text.contains("synthetic-foreign-provider"));
    assert!(!text.contains("synthetic-relay.invalid"));
}

#[test]
fn subscription_auth_comes_from_official_source_and_keeps_alias_policy() {
    let runtime = "[auth]\npreferred_method = 'api_key'\nfuture = 'synthetic-custom-auth'\n[grok_com_config]\npreferred_method = 'api_key'\n";
    let official = "[auth]\npreferred_method = 'oidc'\ndisable_api_key_auth = true\nforce_login_team_uuid = ['synthetic-team']\n[auth.oauth2]\nissuer = 'https://synthetic-official.invalid'\n[grok_com_config]\ndisable_api_key_auth = false\nfuture = {keep = ['中文', false]}\n";
    let source = ConfigSnapshot::parse(official).unwrap();
    let text = project(runtime, official, "grok-4.5", None).unwrap();
    let actual = ConfigSnapshot::parse(&text).unwrap();
    for name in ["auth", "grok_com_config"] {
        assert_eq!(
            semantic(&source.root.fields[name]),
            semantic(&actual.root.fields[name])
        );
    }
    assert!(!text.contains("synthetic-custom-auth"));
    assert!(actual.model_ids().unwrap().is_empty());
}

#[test]
fn explicit_official_key_never_uses_a_colliding_relay_or_changes_catalog_id() {
    let runtime = "[model.'grok-4.5']\nmodel = 'foreign-body'\nbase_url = 'https://synthetic-relay.invalid/v1'\napi_key = 'synthetic-foreign-key'\n[auth]\npreferred_method = 'oidc'\n";
    for (requested, expected) in [
        ("grok-4.5", "grok-4.5"),
        (
            "synthetic-future-official-catalog-id",
            "synthetic-future-official-catalog-id",
        ),
        ("grok", crate::providers::OFFICIAL_CATALOG_MODEL),
    ] {
        let text = project(
            runtime,
            "",
            requested,
            Some("synthetic-explicit-official-key"),
        )
        .unwrap();
        let actual = ConfigSnapshot::parse(&text).unwrap();
        assert_eq!(actual.model_ids().unwrap(), [expected]);
        let fields = actual.model_fields(expected).unwrap().unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields["api_key"], "synthetic-explicit-official-key");
        assert!(
            matches!(&table(&actual, "auth").fields["preferred_method"], Value::String(method) if method == "api_key")
        );
        let (binding, native) =
            crate::providers::ProcessModelBinding::from_text(&text, false, false, requested)
                .unwrap();
        assert!(native.is_none());
        assert_eq!(binding.resolve(requested), expected);
        assert!(!crate::providers::ProcessConfigBinding::custom_from_text(&text).unwrap());
        assert!(!text.contains("synthetic-foreign-key"));
        assert!(!text.contains("synthetic-relay.invalid"));
    }
}

#[test]
fn official_key_respects_effective_bans_team_restrictions_and_oidc_preference() {
    for policy in [
        "[auth]\ndisable_api_key_auth = true\n",
        "[grok_com_config]\ndisable_api_key_auth = true\n",
        "[auth]\nforce_login_team_uuid = ['synthetic-private-team']\n",
        "[grok_com_config]\nforce_login_team_uuid = 'synthetic-private-team'\n",
        "[auth]\npreferred_method = 'oidc'\n",
        "[grok_com_config]\npreferred_method = 'oidc'\n",
        "[auth]\ndisable_api_key_auth = false\n[grok_com_config]\ndisable_api_key_auth = true\n",
    ] {
        let error = project("", policy, "grok-4.5", Some("synthetic-official-key"))
            .expect_err("an explicit official key must not bypass auth policy");
        assert_eq!(error, "selected route conflicts with authentication policy");
    }
    let override_policy = "[auth]\npreferred_method = 'oidc'\ndisable_api_key_auth = true\n[grok_com_config]\npreferred_method = 'api_key'\ndisable_api_key_auth = false\n";
    let text = project(
        "",
        override_policy,
        "grok-4.5",
        Some("synthetic-official-key"),
    )
    .unwrap();
    let actual = ConfigSnapshot::parse(&text).unwrap();
    for name in ["auth", "grok_com_config"] {
        assert!(
            matches!(&table(&actual, name).fields["preferred_method"], Value::String(value) if value == "api_key")
        );
    }
    assert!(
        matches!(&table(&actual, "auth").fields["disable_api_key_auth"], Value::Atom(value) if value == "true")
    );
    assert!(
        matches!(&table(&actual, "grok_com_config").fields["disable_api_key_auth"], Value::Atom(value) if value == "false")
    );
}

#[test]
fn official_projection_preserves_unremoved_role_pins_and_rejects_bad_structure() {
    let text = project("[models]\nweb_search = 'synthetic-native-search-id'\nsession_summary = 'foreign'\n[model.foreign]\napi_key = 'synthetic-foreign-key'\n", "", "grok-4.5", None).unwrap();
    let actual = ConfigSnapshot::parse(&text).unwrap();
    assert!(
        matches!(&table(&actual, "models").fields["web_search"], Value::String(value) if value == "synthetic-native-search-id")
    );
    assert!(!table(&actual, "models")
        .fields
        .contains_key("session_summary"));
    for runtime in [
        "model = []\n",
        "model_providers = false\n",
        "auth_provider = 'synthetic-secret'\n",
        "models = 'bad'\n",
        "endpoints = []\n",
        "[models]\nweb_search = 123\n",
    ] {
        assert!(project(runtime, "", "grok-4.5", None).is_err());
    }
    for auth in [
        "auth = 'synthetic-secret'\n",
        "[auth]\npreferred_method = false\n",
        "[grok_com_config]\ndisable_api_key_auth = 'true'\n",
    ] {
        assert!(project("", auth, "grok-4.5", None).is_err());
    }
    for requested in ["", " ", " grok-4.5", "grok-4.5 "] {
        assert!(project("", "", requested, None).is_err());
    }
    assert!(project("", "", "grok-4.5", Some(" ")).is_err());
}

#[test]
fn official_preparation_and_process_capture_use_projected_home_without_global_relay() {
    use crate::{extensions::ExtensionsPrefs, paths, providers, store::AppSettings};
    use std::fs;
    let _fixture =
        crate::session_migration::test_home::IsolatedAppHome::acquire("official-profile-capture");
    let decoy = projection_fixture("grok_build_proxy");
    fs::write(paths::agent_config_toml(), &decoy).unwrap();
    for key in [None, Some("synthetic-official-key")] {
        let owner = uuid::Uuid::new_v4().to_string();
        let pending = providers::execution_home::PendingHome::create(&owner).unwrap();
        let crate::session_execution::ExecutionSource::AppProfile { generation, .. } =
            pending.source()
        else {
            panic!("not a profile");
        };
        let generation = generation.clone();
        let home = pending.home().to_owned();
        fs::write(
            home.join("config.toml"),
            project(&decoy, "", "grok-4.5", key).unwrap(),
        )
        .unwrap();
        let settings = AppSettings {
            session_data_mode: "independent".into(),
            official_aux_inject: true,
            ..AppSettings::default()
        };
        assert!(!providers::runtime_config::prepare_pending_runtime(
            &pending,
            &settings,
            "ask",
            false,
            true,
            &ExtensionsPrefs::default()
        )
        .unwrap());
        pending
            .publish(&fs::read(home.join("config.toml")).unwrap())
            .unwrap();
        let (binding, native, identity) = providers::ProcessModelBinding::capture_for_spawn(
            false, false, false, "grok-4.5", &home, &settings,
        )
        .unwrap();
        assert_eq!(binding.resolve("grok-4.5"), "grok-4.5");
        assert!(native.is_none());
        assert!(
            matches!(identity.unwrap().execution_source(), crate::session_execution::ExecutionSource::AppProfile { owner_session_id, generation: actual } if owner_session_id == owner && actual == generation)
        );
        let text = fs::read_to_string(home.join("config.toml")).unwrap();
        assert!(!text.contains("synthetic-selected-key"));
        assert!(!home
            .join("hooks/grok-app-block-native-imagine.json")
            .exists());
        assert!(!home.join("auth.json").exists());
        assert_eq!(
            fs::read_to_string(paths::agent_config_toml()).unwrap(),
            decoy
        );
    }
}

#[test]
fn runtime_auth_restrictions_survive_replacing_login_definitions() {
    let ban = "[auth]\ndisable_api_key_auth = true\npreferred_method = 'api_key'\n";
    let official = "[auth]\npreferred_method = 'oidc'\ndisable_api_key_auth = false\n";
    let text = project(ban, official, "grok-4.5", None).unwrap();
    let actual = ConfigSnapshot::parse(&text).unwrap();
    assert!(
        matches!(&table(&actual, "grok_com_config").fields["disable_api_key_auth"], Value::Atom(value) if value == "true")
    );
    assert!(project(ban, "", "grok-4.5", Some("synthetic-official-key")).is_err());
    let runtime = "[auth]\nforce_login_team_uuid = ['synthetic-team-a', 'synthetic-team-b']\n";
    for (policy, expected) in [
        ("", vec!["synthetic-team-a", "synthetic-team-b"]),
        (
            "[auth]\nforce_login_team_uuid = ['synthetic-team-b', 'synthetic-team-c']\n",
            vec!["synthetic-team-b"],
        ),
        (
            "[auth]\nforce_login_team_uuid = 'synthetic-team-c'\n",
            vec![],
        ),
    ] {
        let text = project(runtime, policy, "grok-4.5", None).unwrap();
        let actual = ConfigSnapshot::parse(&text).unwrap();
        let Value::Array(teams) =
            &table(&actual, "grok_com_config").fields["force_login_team_uuid"]
        else {
            panic!("effective team restriction must be explicit");
        };
        assert_eq!(
            teams
                .iter()
                .map(|team| match team {
                    Value::String(team) => team.as_str(),
                    _ => panic!("team must be a string"),
                })
                .collect::<Vec<_>>(),
            expected
        );
        assert!(project(runtime, policy, "grok-4.5", Some("synthetic-official-key")).is_err());
    }
    // Build's explicit field overrides its alias before combining the inputs.
    let runtime =
        "[auth]\ndisable_api_key_auth = true\n[grok_com_config]\ndisable_api_key_auth = false\n";
    assert!(project(runtime, "", "grok-4.5", Some("synthetic-official-key")).is_ok());
    for policy in [
        "[auth]\nforce_login_team_uuid = 1\n",
        "[grok_com_config]\nforce_login_team_uuid = [false]\n",
    ] {
        assert!(project("", policy, "grok-4.5", None).is_err());
    }
}
