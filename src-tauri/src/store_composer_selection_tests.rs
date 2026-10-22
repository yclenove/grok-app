//! Exercise the public preference writer/resolver against real synthetic files.
//! Explicit chat choices must not act as global/project model activation.
use super::*;
use crate::session_migration::test_home::IsolatedAppHome;

fn fixture(scope: &str) -> (IsolatedAppHome, Project, SessionMeta, SessionMeta) {
    let home = IsolatedAppHome::acquire_with_user_home("composer-selection");
    let path = home.path.join("project");
    fs::create_dir(&path).unwrap();
    let project = add_project(path.to_string_lossy().into_owned(), true).unwrap();
    let mut settings = load_settings();
    settings.composer_prefs_scope = scope.into();
    settings.model_id = Some("grok-4.6".into());
    settings.effort = Some("low".into());
    settings.mode = "agent".into();
    settings.permission_policy = "ask".into();
    save_settings(&settings).unwrap();
    update_projects(|list| {
        let row = list.iter_mut().find(|row| row.id == project.id).unwrap();
        row.model_id = Some("grok-4.6".into());
        row.effort = Some("low".into());
        Ok(())
    })
    .unwrap();
    let a = create_session(Some(project.id.clone()), Some("A".into()), false).unwrap();
    let b = create_session(Some(project.id.clone()), Some("B".into()), false).unwrap();
    (home, project, a, b)
}

fn stored(id: &str) -> SessionMeta {
    load_sessions_snapshot()
        .unwrap()
        .into_iter()
        .find(|row| row.id == id)
        .unwrap()
}

fn files() -> Vec<Vec<u8>> {
    [
        settings_file(),
        crate::paths::projects_file(),
        sessions_index_file(),
    ]
    .iter()
    .map(|path| fs::read(path).unwrap())
    .collect()
}

fn choice_is_per_chat(scope: &str) {
    let (_home, project, a, old_b) = fixture(scope);
    let a_before = serde_json::to_value(stored(&a.id)).unwrap();
    let settings_before = fs::read(settings_file()).unwrap();
    let projects_before = fs::read(crate::paths::projects_file()).unwrap();
    let selected = save_composer_prefs(
        Some(&project.id),
        Some(&old_b.id),
        Some("grok-4.5".into()),
        Some("medium".into()),
        None,
        None,
    )
    .unwrap();
    assert_eq!(selected.model_id, "grok-4.5");
    assert_eq!(selected.effort, "medium");
    let b = stored(&old_b.id);
    assert_eq!(b.model_id.as_deref(), Some("grok-4.5"));
    assert_eq!(b.effort.as_deref(), Some("medium"));
    assert_eq!(b.project_id, old_b.project_id);
    assert_eq!(b.agent_session_id, old_b.agent_session_id);
    assert_eq!(serde_json::to_value(stored(&a.id)).unwrap(), a_before);
    assert_eq!(fs::read(settings_file()).unwrap(), settings_before);
    assert_eq!(
        fs::read(crate::paths::projects_file()).unwrap(),
        projects_before
    );
    assert_eq!(
        resolve_composer_prefs(None, Some(&a.id)).model_id,
        "grok-4.6"
    );
    for publish in [update_runtime_session_meta, update_connected_session_meta] {
        publish(&old_b).unwrap();
        let reloaded = resolve_composer_prefs(None, Some(&old_b.id));
        assert_eq!(reloaded.model_id, "grok-4.5");
        assert_eq!(reloaded.effort, "medium");
    }
}

fn legacy_model_still_inherits(scope: &str) {
    let (_home, project, mut a, _) = fixture(scope);
    let mut settings = load_settings();
    settings.model_id = Some("grok-4.5".into());
    save_settings(&settings).unwrap();
    let inherited = if scope == "global" {
        "grok-4.5"
    } else {
        "grok-4.6"
    };
    for empty in [None, Some("".into()), Some("  ".into())] {
        a.model_id = empty;
        update_session_meta(&a).unwrap();
        assert_eq!(
            resolve_composer_prefs(Some(&project.id), Some(&a.id)).model_id,
            inherited
        );
    }
    a.model_id = Some("grok-4.5".into());
    update_session_meta(&a).unwrap();
    assert_eq!(
        resolve_composer_prefs(None, Some(&a.id)).model_id,
        "grok-4.5"
    );
}

fn draft_seeds_without_rewriting_chats(scope: &str) {
    let (_home, project, a, _) = fixture(scope);
    save_composer_prefs(
        None,
        Some(&a.id),
        Some("grok-4.6".into()),
        Some("low".into()),
        None,
        None,
    )
    .unwrap();
    let before = serde_json::to_value(stored(&a.id)).unwrap();
    let prefs = save_composer_prefs(
        Some(&project.id),
        None,
        Some("grok-4.5".into()),
        Some("medium".into()),
        Some("plan".into()),
        None,
    )
    .unwrap();
    // No session row: retain the existing scope and project-inheritance rules.
    // A session-scope draft seeds global defaults but still inherits its project.
    assert_eq!(
        prefs.model_id,
        if scope == "session" {
            "grok-4.6"
        } else {
            "grok-4.5"
        }
    );
    assert_eq!(
        prefs.effort,
        if scope == "project" { "medium" } else { "low" }
    );
    let seed = load_settings();
    assert_eq!(seed.model_id.as_deref(), Some("grok-4.5"));
    assert_eq!(seed.effort.as_deref(), Some("medium"));
    assert_eq!(
        prefs.mode, "agent",
        "draft plan cannot become a sticky default"
    );
    assert_eq!(serde_json::to_value(stored(&a.id)).unwrap(), before);
    assert_eq!(
        resolve_composer_prefs(None, Some(&a.id)).model_id,
        "grok-4.6"
    );
    assert_eq!(resolve_composer_prefs(None, Some(&a.id)).effort, "low");
}

fn invalid_target_cannot_seed_defaults(scope: &str) {
    let (_home, project, a, b) = fixture(scope);
    delete_session(&b.id).unwrap();
    let before = files();
    for target in [b.id.as_str(), "synthetic-unknown-chat", "", "  "] {
        for fields in [
            (Some("grok-4.5".into()), None, None, None),
            (None, Some("medium".into()), None, None),
            (None, None, Some("ask".into()), None),
            (None, None, None, Some("accept_edits".into())),
        ] {
            let result = save_composer_prefs(
                Some(&project.id),
                Some(target),
                fields.0,
                fields.1,
                fields.2,
                fields.3,
            );
            assert!(
                result.is_err(),
                "explicit missing target must fail, not become a draft: {target:?}"
            );
            assert_eq!(files(), before);
        }
    }
    assert_eq!(load_sessions_snapshot().unwrap().len(), 1);
    assert_eq!(stored(&a.id).model_id, a.model_id);
    assert!(!crate::paths::session_dir(&b.id).exists());
}

fn corrupt_index_is_not_recovered_as_a_draft(scope: &str) {
    let (_home, project, a, _) = fixture(scope);
    fs::write(sessions_index_file(), b"synthetic-corrupt-index").unwrap();
    let before = files();
    let result = save_composer_prefs(
        Some(&project.id),
        Some(&a.id),
        Some("grok-4.5".into()),
        Some("medium".into()),
        None,
        None,
    );
    assert!(
        result.is_err(),
        "writer must read the explicit target strictly"
    );
    assert_eq!(
        files(),
        before,
        "no repair/quarantine or default write on this path"
    );
}

fn failed_replacement_cannot_partially_save_a_choice(scope: &str) {
    let (_home, project, a, _) = fixture(scope);
    // Block only this synthetic index's temporary replacement, not OS permissions.
    let mut blocked = sessions_index_file().into_os_string();
    blocked.push(".tmp");
    fs::create_dir(PathBuf::from(blocked)).unwrap();
    let before = files();
    assert!(save_composer_prefs(
        Some(&project.id),
        Some(&a.id),
        Some("grok-4.5".into()),
        Some("medium".into()),
        None,
        None
    )
    .is_err());
    assert_eq!(
        files(),
        before,
        "model and effort commit together or not at all"
    );
}

fn pending_migrations_reject_target_before_any_writes(scope: &str, corrupt: bool) {
    let (_home, project, a, _) = fixture(scope);
    if corrupt {
        fs::write(sessions_index_file(), b"synthetic-corrupt-index").unwrap();
    } else {
        delete_session(&a.id).unwrap();
    }
    // Real settings-load migrations normally rewrite the global seed and run
    // a recovering index reader. A rejected target must not initiate them.
    let mut settings: AppSettings = read_json(&settings_file());
    settings.model_id = Some("grok-4.5".into());
    settings.official_model_default_migrated = false;
    settings.official_effort_xhigh_rows_migrated = false;
    save_settings(&settings).unwrap();
    let paths = [
        settings_file(),
        crate::paths::projects_file(),
        sessions_index_file(),
    ];
    let before: Vec<_> = paths.iter().map(|path| fs::read(path).ok()).collect();
    let targets = if corrupt {
        vec![a.id.as_str()]
    } else {
        vec![a.id.as_str(), "synthetic-unknown-chat", "", "  "]
    };
    for target in targets {
        assert!(composer_effort_before_update(Some(&project.id), Some(target)).is_err());
        let after_observation: Vec<_> = paths.iter().map(|path| fs::read(path).ok()).collect();
        assert!(
            after_observation == before,
            "pre-write command observation triggered migration"
        );
        assert!(save_composer_prefs(
            Some(&project.id),
            Some(target),
            Some("grok-4.6".into()),
            None,
            None,
            None
        )
        .is_err());
        let after: Vec<_> = paths.iter().map(|path| fs::read(path).ok()).collect();
        assert!(
            after == before,
            "rejected target triggered settings migration or index quarantine"
        );
    }
}

fn pending_migrations_missing(scope: &str) {
    pending_migrations_reject_target_before_any_writes(scope, false);
}

fn pending_migrations_corrupt(scope: &str) {
    pending_migrations_reject_target_before_any_writes(scope, true);
}

fn raw_row(id: &str) -> serde_json::Value {
    let rows: serde_json::Value =
        serde_json::from_slice(&fs::read(sessions_index_file()).unwrap()).unwrap();
    rows.as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap()
        .clone()
}

fn provider_survives_observation(scope: &str, connected: bool) {
    let (_home, project, a, mut old_b) = fixture(scope);
    save_composer_prefs(
        Some(&project.id),
        Some(&old_b.id),
        Some("shared-model".into()),
        Some("medium".into()),
        None,
        None,
    )
    .unwrap();
    // Inject the forward-compatible on-disk field without depending on the
    // missing Rust field. The red must exercise the actual public writer.
    let mut rows: serde_json::Value =
        serde_json::from_slice(&fs::read(sessions_index_file()).unwrap()).unwrap();
    for row in rows.as_array_mut().unwrap() {
        row["providerId"] = if row["id"] == a.id {
            "official".into()
        } else {
            "relay-b".into()
        };
    }
    fs::write(
        sessions_index_file(),
        serde_json::to_vec_pretty(&rows).unwrap(),
    )
    .unwrap();
    let a_before = raw_row(&a.id);
    let defaults_before = files()[..2].to_vec();
    // Force a genuine observation write, not a no-op that leaves unknown JSON
    // fields on disk while the public snapshot has already lost them.
    old_b.title = "Observed by the previous connection".into();
    if connected {
        update_connected_session_meta(&old_b).unwrap();
    } else {
        update_runtime_session_meta(&old_b).unwrap();
    }
    assert_eq!(raw_row(&old_b.id)["providerId"], "relay-b");
    assert_eq!(
        serde_json::to_value(stored(&old_b.id)).unwrap()["providerId"],
        "relay-b",
        "public snapshot and serde must retain the desired channel"
    );
    assert_eq!(raw_row(&a.id), a_before, "foreign chat must not change");
    assert_eq!(files()[..2], defaults_before);
    assert_eq!(stored(&old_b.id).model_id.as_deref(), Some("shared-model"));
    assert_eq!(stored(&old_b.id).effort.as_deref(), Some("medium"));
}

fn runtime_provider_intent(scope: &str) {
    provider_survives_observation(scope, false);
}

fn connected_provider_intent(scope: &str) {
    provider_survives_observation(scope, true);
}

#[test]
fn provider_selection_survives_serde_roundtrip() {
    let (_home, _, a, _) = fixture("global");
    for provider in ["official", "generic-a", "generic-b", "native-a", "native-b"] {
        let mut value = serde_json::to_value(&a).unwrap();
        value["providerId"] = provider.into();
        let decoded: SessionMeta = serde_json::from_value(value).unwrap();
        assert_eq!(
            serde_json::to_value(decoded).unwrap()["providerId"],
            provider
        );
    }
}

#[test]
fn legacy_missing_provider_has_no_explicit_selection() {
    let (_home, _, a, _) = fixture("global");
    let mut value = serde_json::to_value(&a).unwrap();
    value.as_object_mut().unwrap().remove("providerId");
    let legacy: SessionMeta = serde_json::from_value(value).unwrap();
    assert!(serde_json::to_value(legacy)
        .unwrap()
        .get("providerId")
        .is_none());
}

fn route_catalog() -> Vec<u8> {
    let mut text = String::from("[models]\ndefault = \"generic-a\"\n");
    for (provider, mode, key) in [
        ("generic-a", "generic", "synthetic-key-not-a-credential"),
        ("generic-b", "generic", "synthetic-key-not-a-credential"),
        (
            "native-a",
            "grok_build_proxy",
            "synthetic-key-not-a-credential",
        ),
        (
            "native-b",
            "grok_build_proxy",
            "synthetic-key-not-a-credential",
        ),
        ("missing-key", "generic", ""),
    ] {
        text.push_str(&format!(
            "[model.{provider}]\nmodel = \"grok-4.6\"\nbase_url = \"https://synthetic.invalid/v1\"\napi_key = \"{key}\"\napi_backend = \"responses\"\napp_provider_mode = \"{mode}\"\napp_models = '[{{\"id\":\"grok-4.6\",\"name\":\"6\"}},{{\"id\":\"grok-4.5\",\"name\":\"5\"}}]'\n"
        ));
    }
    let bytes = text.into_bytes();
    fs::write(crate::paths::agent_config_toml(), &bytes).unwrap();
    bytes
}

fn route_choice_is_atomic_and_chat_owned(scope: &str) {
    let (_home, project, a, mut old_b) = fixture(scope);
    let config_before = route_catalog();
    let defaults_before = files()[..2].to_vec();
    let a_before = raw_row(&a.id);
    old_b.provider_id = Some("generic-a".into());
    for provider in ["official", "generic-a", "generic-b", "native-a", "native-b"] {
        let prefs = save_composer_prefs_with_route(
            Some(&project.id),
            Some(&old_b.id),
            Some(" grok-4.6 ".into()),
            Some("medium".into()),
            None,
            None,
            Some(format!(" {provider} ")),
        )
        .unwrap();
        let b = stored(&old_b.id);
        assert_eq!(b.provider_id.as_deref(), Some(provider));
        assert_eq!(b.model_id.as_deref(), Some("grok-4.6"));
        assert_eq!(prefs.model_id, "grok-4.6");
        assert_eq!(prefs.effort, "medium");
        assert_eq!(b.project_id, old_b.project_id);
        assert_eq!(b.agent_session_id, old_b.agent_session_id);
        assert_eq!(raw_row(&a.id), a_before);
        assert_eq!(files()[..2], defaults_before);
        assert_eq!(
            fs::read(crate::paths::agent_config_toml()).unwrap(),
            config_before
        );
        for publish in [update_runtime_session_meta, update_connected_session_meta] {
            publish(&old_b).unwrap();
            assert_eq!(stored(&old_b.id).provider_id.as_deref(), Some(provider));
            assert_eq!(stored(&old_b.id).model_id.as_deref(), Some("grok-4.6"));
        }
        let serialized = serde_json::to_string(&stored(&old_b.id)).unwrap();
        for private in [
            "synthetic-key-not-a-credential",
            "synthetic.invalid",
            "apiKey",
            "extraHeaders",
        ] {
            assert!(
                !serialized.contains(private),
                "route intent must be credential-free"
            );
        }
    }
    // Same request id as official, but a custom ladder may include xhigh on 4.5.
    let custom = save_composer_prefs_with_route(
        None,
        Some(&old_b.id),
        Some("grok-4.5".into()),
        Some("xhigh".into()),
        None,
        None,
        Some("generic-b".into()),
    )
    .unwrap();
    assert_eq!(custom.effort, "xhigh");
    let official = save_composer_prefs_with_route(
        None,
        Some(&old_b.id),
        Some("grok-4.5".into()),
        Some("xhigh".into()),
        None,
        None,
        Some("OFFICIAL".into()),
    )
    .unwrap();
    assert_eq!(official.effort, "high");
    assert_eq!(stored(&old_b.id).provider_id.as_deref(), Some("official"));
    // Existing model-only API updates retain the explicit channel.
    save_composer_prefs(
        None,
        Some(&old_b.id),
        Some("grok-4.6".into()),
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(stored(&old_b.id).provider_id.as_deref(), Some("official"));
}

fn invalid_routes_cannot_write_defaults(scope: &str) {
    let (_home, project, a, b) = fixture(scope);
    let config_before = route_catalog();
    delete_session(&b.id).unwrap();
    let mut settings: AppSettings = read_json(&settings_file());
    settings.official_model_default_migrated = false;
    settings.official_effort_xhigh_rows_migrated = false;
    save_settings(&settings).unwrap();
    let before = files();
    for (target, provider, model) in [
        (Some(b.id.as_str()), "generic-b", Some("grok-4.5")),
        (Some("unknown-chat"), "generic-b", Some("grok-4.5")),
        (Some(""), "generic-b", Some("grok-4.5")),
        (Some("  "), "generic-b", Some("grok-4.5")),
        (None, "generic-b", Some("grok-4.5")),
        (Some(a.id.as_str()), "missing-provider", Some("grok-4.6")),
        (Some(a.id.as_str()), "missing-key", Some("grok-4.6")),
        (Some(a.id.as_str()), "grok", Some("grok-4.6")),
        (Some(a.id.as_str()), "", Some("grok-4.6")),
        (Some(a.id.as_str()), "  ", Some("grok-4.6")),
        (Some(a.id.as_str()), "generic-b", None),
        (Some(a.id.as_str()), "generic-b", Some("  ")),
        (Some(a.id.as_str()), "generic-b", Some("unknown-model")),
        (Some(a.id.as_str()), "native-b", Some("unknown-model")),
        (Some(a.id.as_str()), "official", None),
    ] {
        assert!(
            save_composer_prefs_with_route(
                Some(&project.id),
                target,
                model.map(str::to_owned),
                Some("medium".into()),
                None,
                None,
                Some(provider.into()),
            )
            .is_err(),
            "invalid target or route must fail closed"
        );
        assert_eq!(
            files(),
            before,
            "invalid choice initiated migration or partial save"
        );
        assert_eq!(
            fs::read(crate::paths::agent_config_toml()).unwrap(),
            config_before
        );
    }
    assert_eq!(load_sessions_snapshot().unwrap().len(), 1);
}

fn route_replacement_failure_is_atomic(scope: &str) {
    let (_home, project, a, _) = fixture(scope);
    let config_before = route_catalog();
    let mut blocked = sessions_index_file().into_os_string();
    blocked.push(".tmp");
    fs::create_dir(PathBuf::from(blocked)).unwrap();
    let before = files();
    assert!(save_composer_prefs_with_route(
        Some(&project.id),
        Some(&a.id),
        Some("grok-4.5".into()),
        Some("medium".into()),
        None,
        None,
        Some("generic-b".into()),
    )
    .is_err());
    assert_eq!(
        files(),
        before,
        "provider/model/effort must commit together"
    );
    assert_eq!(
        fs::read(crate::paths::agent_config_toml()).unwrap(),
        config_before
    );
}

#[test]
fn route_corrupt_index_is_not_repaired_or_recreated() {
    let (_home, _, a, _) = fixture("global");
    let config_before = route_catalog();
    fs::write(sessions_index_file(), b"synthetic-corrupt-index").unwrap();
    let before = files();
    assert!(save_composer_prefs_with_route(
        None,
        Some(&a.id),
        Some("grok-4.6".into()),
        None,
        None,
        None,
        Some("generic-b".into()),
    )
    .is_err());
    assert_eq!(files(), before);
    assert_eq!(
        fs::read(crate::paths::agent_config_toml()).unwrap(),
        config_before
    );
}

#[test]
fn malformed_route_config_never_changes_session_intent() {
    let (_home, _, a, _) = fixture("global");
    let original = String::from_utf8(route_catalog()).unwrap();
    let before = files();
    for config in [
        "synthetic-corrupt-config".to_string(),
        original.replace("api_backend = \"responses\"", "api_backend = \"unknown\""),
        original.replace("https://synthetic.invalid/v1", "file:///synthetic"),
        original.replace(
            "app_provider_mode = \"generic\"",
            "app_provider_mode = \"unknown\"",
        ),
        original.replace("app_models = '[{", "app_models = 'bad[{"),
        format!("{original}[model.generic-b]\nbase_url = \"https://synthetic.invalid/v1\"\n"),
    ] {
        fs::write(crate::paths::agent_config_toml(), &config).unwrap();
        assert!(save_composer_prefs_with_route(
            None,
            Some(&a.id),
            Some("grok-4.6".into()),
            None,
            None,
            None,
            Some("generic-b".into()),
        )
        .is_err());
        assert_eq!(files(), before);
        assert_eq!(
            fs::read_to_string(crate::paths::agent_config_toml()).unwrap(),
            config
        );
    }
}

#[test]
fn fork_inherits_desired_channel_alongside_model() {
    let (_home, _, a, _) = fixture("global");
    route_catalog();
    save_composer_prefs_with_route(
        None,
        Some(&a.id),
        Some("grok-4.5".into()),
        Some("medium".into()),
        None,
        None,
        Some("generic-b".into()),
    )
    .unwrap();
    let source_before = raw_row(&a.id);
    let child = fork_session(&a.id, None, Some("Synthetic fork".into()), false).unwrap();
    assert_eq!(child.provider_id.as_deref(), Some("generic-b"));
    assert_eq!(child.model_id.as_deref(), Some("grok-4.5"));
    assert_eq!(child.effort.as_deref(), Some("medium"));
    assert_eq!(raw_row(&a.id), source_before);
    assert_eq!(stored(&child.id).provider_id, child.provider_id);
}

macro_rules! scopes {
    ($global:ident, $project:ident, $session:ident, $check:ident) => {
        #[test]
        fn $global() {
            $check("global");
        }
        #[test]
        fn $project() {
            $check("project");
        }
        #[test]
        fn $session() {
            $check("session");
        }
    };
}

scopes!(
    global_chat_choice,
    project_chat_choice,
    session_chat_choice,
    choice_is_per_chat
);
scopes!(
    global_legacy,
    project_legacy,
    session_legacy,
    legacy_model_still_inherits
);
scopes!(
    global_draft,
    project_draft,
    session_draft,
    draft_seeds_without_rewriting_chats
);
scopes!(
    global_invalid_target,
    project_invalid_target,
    session_invalid_target,
    invalid_target_cannot_seed_defaults
);
scopes!(
    global_corrupt_index,
    project_corrupt_index,
    session_corrupt_index,
    corrupt_index_is_not_recovered_as_a_draft
);
scopes!(
    global_failed_replace,
    project_failed_replace,
    session_failed_replace,
    failed_replacement_cannot_partially_save_a_choice
);
scopes!(
    global_unmigrated_missing,
    project_unmigrated_missing,
    session_unmigrated_missing,
    pending_migrations_missing
);
scopes!(
    global_unmigrated_corrupt,
    project_unmigrated_corrupt,
    session_unmigrated_corrupt,
    pending_migrations_corrupt
);
scopes!(
    global_runtime_provider_intent,
    project_runtime_provider_intent,
    session_runtime_provider_intent,
    runtime_provider_intent
);
scopes!(
    global_connected_provider_intent,
    project_connected_provider_intent,
    session_connected_provider_intent,
    connected_provider_intent
);
scopes!(
    global_atomic_route_choice,
    project_atomic_route_choice,
    session_atomic_route_choice,
    route_choice_is_atomic_and_chat_owned
);
scopes!(
    global_invalid_route,
    project_invalid_route,
    session_invalid_route,
    invalid_routes_cannot_write_defaults
);
scopes!(
    global_failed_route_replace,
    project_failed_route_replace,
    session_failed_route_replace,
    route_replacement_failure_is_atomic
);
