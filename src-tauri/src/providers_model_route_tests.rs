//! The same resolver used by connect/unpark/control/prewarm followed by the
//! native spawn parser. Deliberately not a replacement for manager/argv/wire tests.
use super::*;
use crate::session_migration::test_home::IsolatedAppHome;

fn config(default: &str, native: bool) -> String {
    let models = encode_app_models(&[
        ProviderModelEntry::named("grok-4.6", "Synthetic 4.6"),
        ProviderModelEntry::named("grok-4.5", "Synthetic 4.5"),
    ]);
    let mut text = String::new();
    for id in ["synthetic-relay-a", "synthetic-relay-b"] {
        text = append_section(
            &text,
            id,
            &[
                ("model".into(), "grok-4.6".into()),
                ("base_url".into(), format!("https://{id}.invalid/v1")),
                ("api_key".into(), "synthetic-not-a-credential".into()),
                ("api_backend".into(), "responses".into()),
                (
                    APP_PROVIDER_MODE_KEY.into(),
                    if native {
                        PROVIDER_MODE_GROK_BUILD_PROXY
                    } else {
                        PROVIDER_MODE_GENERIC
                    }
                    .into(),
                ),
                (APP_MODELS_KEY.into(), models.clone()),
            ],
        );
    }
    set_models_default(&text, default)
}

fn fixture(default: &str, native: bool) -> IsolatedAppHome {
    let home = IsolatedAppHome::acquire("native-model-resolution");
    ensure_agent_home().unwrap();
    write_text(&agent_config_toml(), &config(default, native)).unwrap();
    home
}

#[test]
fn non_default_session_choice_survives_manager_resolver_and_native_spawn() {
    let _home = fixture("synthetic-relay-a", true);
    let mut settings = crate::store::load_settings();
    settings.composer_prefs_scope = "session".into();
    crate::store::save_settings(&settings).unwrap();
    let row = crate::store::create_session(None, None, false).unwrap();
    crate::store::save_composer_prefs(
        None,
        Some(&row.id),
        Some("grok-4.5".into()),
        None,
        None,
        None,
    )
    .unwrap();
    let prefs = crate::store::resolve_composer_prefs(None, Some(&row.id));
    assert_eq!(prefs.model_id, "grok-4.5");
    let resolved = agent_spawn_model_id(&prefs.model_id);
    assert_eq!(
        resolved, "grok-4.5",
        "manager must not erase native catalog choice"
    );
    let native = active_grok_build_proxy_spawn(&resolved).unwrap();
    assert_eq!(native.model, "grok-4.5");
    assert_eq!(native.base_url, "https://synthetic-relay-a.invalid/v1");
}

#[test]
fn native_fallback_is_a_real_model_for_every_apply_path() {
    let _home = fixture("synthetic-relay-a", true);
    for request in ["", "  ", "unknown-model", "synthetic-relay-a"] {
        let resolved = agent_spawn_model_id(request);
        assert_eq!(resolved, "grok-4.6", "request={request:?}");
        assert_eq!(
            active_grok_build_proxy_spawn(&resolved).unwrap().model,
            resolved
        );
    }
}

#[test]
fn native_catalog_choice_trims_input() {
    let _home = fixture("synthetic-relay-a", true);
    assert_eq!(agent_spawn_model_id("  grok-4.5  "), "grok-4.5");
}

#[test]
fn generic_route_keeps_section_alias_even_when_it_lists_official_model_names() {
    let _home = fixture("synthetic-relay-a", false);
    for request in ["grok-4.5", "grok-4.6", "", "unknown-model"] {
        assert_eq!(agent_spawn_model_id(request), "synthetic-relay-a");
        assert!(active_grok_build_proxy_spawn(request).is_none());
    }
}

#[test]
fn official_route_does_not_remap_models_or_acquire_native_binding() {
    let _home = fixture(OFFICIAL_DEFAULT_MODEL, true);
    for request in ["grok-4.5", "grok-4.6"] {
        assert_eq!(agent_spawn_model_id(request), request);
        assert!(active_grok_build_proxy_spawn(request).is_none());
    }
    for request in ["", "synthetic-relay-a", OFFICIAL_DEFAULT_MODEL] {
        assert_eq!(agent_spawn_model_id(request), OFFICIAL_CATALOG_MODEL);
    }
}

#[test]
fn two_active_native_routes_with_the_same_model_do_not_mix_endpoint_binding() {
    let _home = fixture("synthetic-relay-a", true);
    for id in ["synthetic-relay-a", "synthetic-relay-b"] {
        write_text(&agent_config_toml(), &config(id, true)).unwrap();
        let resolved = agent_spawn_model_id("grok-4.5");
        let binding = active_grok_build_proxy_spawn(&resolved).unwrap();
        assert_eq!(binding.model, "grok-4.5");
        assert_eq!(binding.base_url, format!("https://{id}.invalid/v1"));
    }
}
