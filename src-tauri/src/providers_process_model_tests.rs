//! Pure capture/parser tests: no file, provider, child or user home is accessed.
use super::*;

fn config(mode: &str) -> String {
    format!("[models]\ndefault = \"synthetic-route\"\n[model.synthetic-route]\nmodel = \"grok-4.6\"\nbase_url = \"https://synthetic.invalid/v1\"\napi_key = \"synthetic-key-not-a-credential\"\napi_backend = \"responses\"\napp_provider_mode = \"{mode}\"\napp_models = '[{{\"id\":\"grok-4.6\",\"name\":\"6\"}},{{\"id\":\"grok-4.5\",\"name\":\"5\"}}]'\n")
}

#[test]
fn native_non_default_model_matches_environment_model() {
    let (binding, native) =
        ProcessModelBinding::from_text(&config("grok_build_proxy"), true, false, " grok-4.5 ")
            .unwrap();
    let native = native.unwrap();
    assert_eq!(binding.resolve(" grok-4.5 "), "grok-4.5");
    assert_eq!(native.model, binding.resolve("grok-4.5"));
    assert_eq!(native.base_url, "https://synthetic.invalid/v1");
    assert_eq!(native.models_url, "https://synthetic.invalid/v1/models");
}

#[test]
fn native_unknown_empty_and_alias_use_configured_real_fallback() {
    let (binding, _) =
        ProcessModelBinding::from_text(&config("grok_build_proxy"), true, false, "").unwrap();
    for requested in ["", " ", "missing-model", "synthetic-route"] {
        assert_eq!(binding.resolve(requested), "grok-4.6");
    }
}

#[test]
fn generic_models_remain_section_aliases() {
    let (binding, native) =
        ProcessModelBinding::from_text(&config("generic"), true, false, "grok-4.5").unwrap();
    assert!(native.is_none());
    for requested in ["", "grok-4.5", "missing-model"] {
        assert_eq!(binding.resolve(requested), "synthetic-route");
    }
}

#[test]
fn official_binding_does_not_acquire_native_environment() {
    let (binding, native) =
        ProcessModelBinding::from_text(&config("grok_build_proxy"), false, false, "grok-4.5")
            .unwrap();
    assert!(native.is_none());
    assert_eq!(binding.resolve("grok-4.5"), "grok-4.5");
    assert_eq!(binding.resolve("synthetic-route"), OFFICIAL_CATALOG_MODEL);
}

#[test]
fn official_key_only_override_does_not_unblock_nested_or_array_route_fields() {
    for suffix in [
        "env_key = ['SYNTHETIC_ROUTE_KEY']\n",
        "[model.future.extra_headers]\n'X-Route' = 'synthetic-header'\n",
        "[model.future.query_params]\nroute = 'synthetic-route'\n",
        "model_provider = 'synthetic-provider'\n",
        "future = {route = 'synthetic-unknown'}\n",
    ] {
        let source = format!("[model.future]\napi_key = 'synthetic-key'\n{suffix}");
        let (binding, native) =
            ProcessModelBinding::from_text(&source, false, false, "future").unwrap();
        assert!(native.is_none());
        assert_eq!(binding.resolve("future"), OFFICIAL_CATALOG_MODEL);
    }
    let (binding, _) = ProcessModelBinding::from_text(
        "[model.future]\napi_key = 'synthetic-key'\n",
        false,
        false,
        "future",
    )
    .unwrap();
    assert_eq!(binding.resolve("future"), "future");
}

#[test]
fn ssh_catalog_never_acquires_local_native_environment() {
    let (binding, native) =
        ProcessModelBinding::from_text(&config("grok_build_proxy"), true, true, "grok-4.5")
            .unwrap();
    assert!(native.is_none());
    assert_eq!(binding.resolve("grok-4.5"), "grok-4.5");
    assert_eq!(binding.resolve("synthetic-route"), OFFICIAL_CATALOG_MODEL);
}

#[test]
fn explicit_native_missing_key_fails_closed() {
    let text =
        config("grok_build_proxy").replace("api_key = \"synthetic-key-not-a-credential\"\n", "");
    assert!(ProcessModelBinding::from_text(&text, true, false, "grok-4.5").is_err());
}

#[test]
fn missing_custom_section_cannot_become_official() {
    assert!(ProcessModelBinding::from_text(
        "[models]\ndefault = \"missing\"\n",
        true,
        false,
        "grok-4.5"
    )
    .is_err());
}

#[test]
fn tcp_and_override_catalogs_do_not_map_unknown_remote_names() {
    let binding = ProcessModelBinding::catalog();
    assert_eq!(binding.resolve(" remote-model "), "remote-model");
    assert_eq!(binding.resolve(""), OFFICIAL_CATALOG_MODEL);
}

#[test]
fn generic_echo_maps_alias_to_captured_body_but_does_not_guess_colliding_provider() {
    let (binding, _) =
        ProcessModelBinding::from_text(&config("generic"), true, false, "grok-4.5").unwrap();
    assert_eq!(
        binding.execution_model(Some("synthetic-route")),
        (Some("synthetic-route".into()), Some("grok-4.6".into()))
    );
    assert_eq!(
        binding.execution_model(Some("grok-4.6")),
        (None, Some("grok-4.6".into()))
    );
    for echo in [None, Some("unknown-alias"), Some("grok-4.5")] {
        assert_eq!(binding.execution_model(echo), (None, None));
    }
}
