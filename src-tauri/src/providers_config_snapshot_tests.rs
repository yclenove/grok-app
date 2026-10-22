use super::*;

#[path = "providers_config_official_projection_tests.rs"]
mod official_projection;
#[path = "providers_config_profile_auth_tests.rs"]
mod profile_auth;

fn route(fields: &str) -> String {
    format!("[models]\ndefault = \"real\"\n[model.real]\nmodel = \"target\"\nbase_url = \"https://synthetic.invalid/v1\"\napi_key = \"synthetic-not-a-credential\"\n{fields}")
}

#[test]
fn only_real_table_fields_are_used_by_the_actual_process_binding() {
    let text = format!("note = '''\n[models]\ndefault = \"fake\"\n[model.fake]\napi_key = \"synthetic-other-key\"\n'''\n{}", route("[model.real.extra_headers]\nmodel = \"wrong\"\n"));
    assert!(super::super::ProcessConfigBinding::custom_from_text(&text).unwrap());
    let (binding, native) =
        super::super::ProcessModelBinding::from_text(&text, true, false, "target").unwrap();
    assert!(native.is_none());
    assert_eq!(binding.resolve("target"), "real");
    assert_eq!(
        binding.execution_model(Some("real")),
        (Some("real".into()), Some("target".into()))
    );
}

#[test]
fn native_env_and_binding_share_commented_and_quoted_route_fields() {
    let text =
        route("app_provider_mode = 'grok_build_proxy' # native\napi_backend = \"responses\"\n");
    let (binding, native) =
        super::super::ProcessModelBinding::from_text(&text, true, false, "target").unwrap();
    let native = native.unwrap();
    assert_eq!(native.base_url, "https://synthetic.invalid/v1");
    assert_eq!(native.api_key, "synthetic-not-a-credential");
    assert_eq!(native.model, binding.resolve("target"));
}

#[test]
fn nested_arrays_tables_inline_and_dotted_keys_are_structural() {
    let text = "model.'real.channel' = { model = 'target', base_url = 'https://synthetic.invalid/v1', api_key = 'synthetic-not-a-credential', extra_headers = { 'X-Note' = '[model.fake]' } }\n[[hooks]]\nname = 'first'\n[hooks.config]\nactive = true\n[[hooks]]\nname = 'second'\n[hooks.config]\nactive = false\n[models]\ndefault = 'real.channel'\n";
    let snapshot = ConfigSnapshot::parse(text).unwrap();
    assert_eq!(snapshot.default_model().unwrap(), Some("real.channel"));
    let fields = snapshot.model_fields("real.channel").unwrap().unwrap();
    assert_eq!(fields["model"], "target");
    assert!(!fields.contains_key("X-Note"));
    assert_eq!(snapshot.model_ids().unwrap(), vec!["real.channel"]);
}

#[test]
fn multiline_escapes_unicode_and_crlf_decode_without_promoting_text() {
    let text = "[model.\"real\\u002echannel\"] # header\r\nmodel = \"\"\"\r\n tar\\\r\n  get\"\"\"\r\nbase_url = '''https://synthetic.invalid/v1'''\r\napi_key = 'synthetic-not-a-credential'\r\n";
    let fields = ConfigSnapshot::parse(text)
        .unwrap()
        .model_fields("real.channel")
        .unwrap()
        .unwrap();
    assert_eq!(fields["model"], " target");
    assert_eq!(fields["base_url"], "https://synthetic.invalid/v1");
}

#[test]
fn duplicate_escaped_keys_and_table_redefinitions_fail_closed() {
    for text in [
        route("\"api\\u005fkey\" = 'synthetic-other-key'\n"),
        route("[model.real]\n"),
        "a = {b = 1, b = 2}\n".into(),
        "a = {b = 1}\n[a.c]\nx = 2\n".into(),
        "a.b = 1\n[a]\nx = 2\n".into(),
        "a = 1\n[a.b]\nx = 2\n".into(),
    ] {
        assert!(ConfigSnapshot::parse(&text).is_err());
    }
}

#[test]
fn malformed_values_never_include_source_in_the_error() {
    for text in [
        "note = '''synthetic-not-a-credential\n[model.fake]\n",
        "note = \"\\qsynthetic-not-a-credential\"\n",
        "note = [1 2]\n",
        "note = {a = 1,}\n",
        "note = 01\n",
        "[model.fake] junk\n",
        "note = \"\\uD800\"\n",
    ] {
        let error = ConfigSnapshot::parse(text)
            .err()
            .expect("invalid input must fail closed");
        assert_eq!(error, invalid());
    }
}

#[test]
fn arrays_of_models_and_non_string_credentials_are_not_routes() {
    for text in [
        "[[model.fake]]\napi_key = 'synthetic-not-a-credential'\n",
        "[model.fake]\napi_key = 1\n",
        "[model.fake]\napi_key = ['synthetic-not-a-credential']\n",
    ] {
        let snapshot = ConfigSnapshot::parse(text).unwrap();
        assert!(snapshot.model_fields("fake").is_err());
    }
}

#[test]
fn unrelated_cli_scalar_types_and_arrays_survive_read_only_parsing() {
    let values = "note = [0xA_B, -12, +1_000, 1.2e-3, true, false, inf, +nan, 1979-05-27, 07:32:00, 1979-05-27 07:32:00, 1979-05-27T07:32:00Z, {a = 'b'}]\n";
    let text = format!("{values}{}", route(""));
    let snapshot = ConfigSnapshot::parse(&text).unwrap();
    assert_eq!(
        snapshot.model_fields("real").unwrap().unwrap()["model"],
        "target"
    );
}

#[test]
fn excessive_nesting_fails_without_stack_overflow() {
    let text = format!("note = {}0{}\n", "[".repeat(65), "]".repeat(65));
    assert!(ConfigSnapshot::parse(&text).is_err());
}

fn dotted_path(parts: usize) -> String {
    std::iter::repeat_n("part", parts)
        .collect::<Vec<_>>()
        .join(".")
}

#[test]
fn parser_rejects_combined_inline_and_dotted_depth_before_building_the_tree() {
    let path = dotted_path(16);
    let mut value = "'synthetic-value'".to_owned();
    for _ in 0..5 {
        value = format!("{{{path} = {value}}}");
    }
    assert!(ConfigSnapshot::parse(&format!("note = {value}\n")).is_err());
}

#[test]
fn parser_counts_table_scope_and_assignment_path_together() {
    let source = format!(
        "[{}]\n{} = 'synthetic-value'\n",
        dotted_path(32),
        dotted_path(32)
    );
    assert!(ConfigSnapshot::parse(&source).is_err());
}

#[test]
fn parser_counts_array_table_containers_when_extending_the_scope() {
    let mut source = String::new();
    for parts in 1..=32 {
        source.push_str(&format!("[[{}]]\n", dotted_path(parts)));
    }
    assert!(ConfigSnapshot::parse(&source).is_err());
}

fn assert_round_trip(source: &str) {
    let snapshot = ConfigSnapshot::parse(source).unwrap();
    let text = write::to_text(&snapshot.root).unwrap();
    let parsed = ConfigSnapshot::parse(&text).unwrap();
    assert_eq!(write::to_text(&parsed.root).unwrap(), text);
}

#[test]
fn structural_bound_keeps_the_deepest_supported_leaf_and_empty_table() {
    assert_round_trip(&format!("{} = 'synthetic-value'\n", dotted_path(63)));
    assert_round_trip(&format!("[{}]\n", dotted_path(63)));
    assert_round_trip(&format!("note = {}0{}\n", "[".repeat(62), "]".repeat(62)));
    let mut source = String::new();
    for parts in 1..=31 {
        source.push_str(&format!("[[{}]]\n", dotted_path(parts)));
    }
    source.push_str("leaf = 'synthetic-value'\n");
    assert_round_trip(&source);
}

#[test]
fn structural_bound_preserves_mixed_scope_inline_and_array_values() {
    let path = dotted_path(20);
    let source = format!("[{path}]\n{path} = {{{path} = [[['synthetic-value']]]}}\n");
    assert_round_trip(&source);
}

#[test]
fn excessive_structural_depth_always_returns_the_fixed_safe_error() {
    let path = dotted_path(16);
    let mut value = "'synthetic-not-a-credential'".to_owned();
    for _ in 0..128 {
        value = format!("{{{path} = {value}}}");
    }
    for source in [
        format!("{} = 'synthetic-not-a-credential'\n", dotted_path(64)),
        format!("[{}]\n", dotted_path(64)),
        format!("note = {}0{}\n", "[".repeat(63), "]".repeat(63)),
        format!("note = {value}\n"),
    ] {
        match ConfigSnapshot::parse(&source) {
            Err(error) => assert_eq!(error, invalid()),
            Ok(_) => panic!("excessive structural depth must be rejected"),
        }
    }
}

fn semantic(value: &Value) -> serde_json::Value {
    match value {
        Value::String(value) => serde_json::json!({"string": value}),
        Value::Atom(value) => serde_json::json!({"atom": value}),
        Value::Array(values) => values.iter().map(semantic).collect(),
        Value::Table(table) => table
            .fields
            .iter()
            .map(|(key, value)| (key.clone(), semantic(value)))
            .collect(),
        Value::TableArray(tables) => tables
            .iter()
            .map(|table| {
                table
                    .fields
                    .iter()
                    .map(|(key, value)| (key.clone(), semantic(value)))
                    .collect::<serde_json::Value>()
            })
            .collect(),
    }
}

fn projection_fixture(mode: &str) -> String {
    let catalog = serde_json::json!([
        {"id": "target", "name": "First", "contextWindow": 32000, "supportsVision": true},
        {"id": "target-two", "name": "中文模型", "contextWindow": 128000, "supportsVision": false,
         "supportsVideo": true, "efforts": [{"id": "high", "name": "High", "isDefault": true}]}
    ]);
    format!(
        "[models]\ndefault = 'other'\nweb_search = 'other'\nsession_summary = 'other'\nimage_description = 'other'\nprompt_suggestion = 'other'\nallowed_models = ['real', 'target*']\nhidden_models = ['other']\ndisabled_models = ['blocked']\nmax_retries = 4\n[models.extra_headers]\n'X-Global' = 'synthetic-global'\n[model.other]\nmodel = 'target-two'\nbase_url = 'https://other.invalid/v1'\napi_key = 'synthetic-unrelated-key'\n[model.real]\nmodel = 'target'\nbase_url = 'https://synthetic.invalid/v1'\napi_key = 'synthetic-selected-key'\napi_backend = 'responses'\napp_provider_mode = '{}'\napp_models = {}\ncontext_window = 32000\napp_supports_vision = true\n",
        mode,
        super::super::quote(&catalog.to_string()),
    )
}

#[test]
fn canonical_cli_text_preserves_all_value_types_and_nested_array_instances() {
    let source = "\"中文 key\" = \"quote \\\" slash \\\\ line\\n\\t\\u0000\"\nnotes = [0xA_B, -12, +1_000, 1.2e-3, true, false, inf, +nan, 1979-05-27, 07:32:00, 1979-05-27 07:32:00, 1979-05-27T07:32:00Z, {a.b = '中🙂', nested = [1, [2, 3]]}, []]\nempty = {}\n[[future.events]]\nname = 'first'\n[future.events.options]\nenabled = true\n[[future.events.children]]\nname = 'child'\n[[future.events]]\nname = 'second'\n[future.events.options]\nenabled = false\n";
    let original = ConfigSnapshot::parse(source).unwrap();
    let text = write::to_text(&original.root).unwrap();
    assert!(!text.contains('\r'));
    assert!(!text.starts_with('\u{feff}'));
    let parsed = ConfigSnapshot::parse(&text).unwrap();
    let shape = |root: &Table| {
        root.fields
            .iter()
            .map(|(key, value)| (key.clone(), semantic(value)))
            .collect::<serde_json::Value>()
    };
    assert_eq!(shape(&parsed.root), shape(&original.root));
    assert_eq!(write::to_text(&parsed.root).unwrap(), text);
}

#[test]
fn canonical_strings_escape_del_in_values_and_quoted_keys() {
    let snapshot = ConfigSnapshot::parse("\"key\\u007f\" = \"value\\u007f\"\n").unwrap();
    let text = write::to_text(&snapshot.root).unwrap();
    assert!(!text.contains('\u{007f}'));
    let parsed = ConfigSnapshot::parse(&text).unwrap();
    assert_eq!(
        semantic(&parsed.root.fields["key\u{007f}"]),
        semantic(&snapshot.root.fields["key\u{007f}"])
    );
}

#[test]
fn projected_request_model_and_capabilities_reach_the_real_process_binding() {
    let text = ConfigSnapshot::parse(&projection_fixture("generic"))
        .unwrap()
        .project_custom("real", "target-two")
        .unwrap();
    let snapshot = ConfigSnapshot::parse(&text).unwrap();
    assert_eq!(snapshot.default_model().unwrap(), Some("real"));
    assert_eq!(snapshot.model_ids().unwrap(), ["real"]);
    let fields = snapshot.model_fields("real").unwrap().unwrap();
    assert_eq!(fields["model"], "target-two");
    assert_eq!(fields["context_window"], "128000");
    assert_eq!(fields["app_supports_vision"], "false");
    let efforts: Vec<super::super::ProviderEffortEntry> =
        serde_json::from_str(&fields["app_efforts"]).unwrap();
    assert_eq!(efforts.len(), 1);
    assert_eq!(efforts[0].id, "high");
    assert!(efforts[0].is_default);
    let (binding, native) =
        super::super::ProcessModelBinding::from_text(&text, true, false, "target-two").unwrap();
    assert!(native.is_none());
    assert_eq!(binding.resolve("target-two"), "real");
    assert_eq!(
        binding.execution_model(Some("real")),
        (Some("real".into()), Some("target-two".into()))
    );
    assert!(!text.contains("synthetic-unrelated-key"));
}

#[test]
fn native_projection_preserves_real_catalog_ids_and_the_selected_key() {
    let text = ConfigSnapshot::parse(&projection_fixture("grok_build_proxy"))
        .unwrap()
        .project_custom("real", "target-two")
        .unwrap();
    let (binding, native) =
        super::super::ProcessModelBinding::from_text(&text, true, false, "target-two").unwrap();
    let native = native.unwrap();
    assert_eq!(binding.resolve("target-two"), "target-two");
    assert_eq!(binding.resolve("unknown"), "target-two");
    assert_eq!(native.model, "target-two");
    assert_eq!(native.api_key, "synthetic-selected-key");
    assert_eq!(native.base_url, "https://synthetic.invalid/v1");
    assert_eq!(
        binding.execution_model(Some("target-two")),
        (Some("real".into()), Some("target-two".into()))
    );
    assert!(!text.contains("synthetic-unrelated-key"));
}

#[test]
fn projection_preserves_unknown_features_tools_and_model_restrictions() {
    let unknown = "note = '''\n[model.fake]\napi_key = 'inside-a-string'\n'''\n[features]\nsubagents = true\nfuture = [true, {nested = ['中文/🙂', 3]}, [1, 2]]\n[permissions]\ndefault = 'ask'\n[mcp_servers.synthetic]\ncommand = 'synthetic-never-executed'\nargs = ['--one', '--two']\nenabled = false\n[mcp_servers.synthetic.env]\nSYNTHETIC = 'value'\n[[hooks.events]]\nmatcher = 'read_file'\n[hooks.events.config]\nkind = 'first'\n[[hooks.events]]\nmatcher = 'write_file'\n[hooks.events.config]\nkind = 'second'\n";
    let source = format!("{unknown}{}", projection_fixture("generic"));
    let original = ConfigSnapshot::parse(&source).unwrap();
    let projected = ConfigSnapshot::parse(&source)
        .unwrap()
        .project_custom("real", "target-two")
        .unwrap();
    let projected = ConfigSnapshot::parse(&projected).unwrap();
    for field in ["note", "features", "permissions", "mcp_servers", "hooks"] {
        assert_eq!(
            semantic(&original.root.fields[field]),
            semantic(&projected.root.fields[field])
        );
    }
    let models = |snapshot: &ConfigSnapshot| match &snapshot.root.fields["models"] {
        Value::Table(table) => table
            .fields
            .iter()
            .filter(|(name, _)| {
                ![
                    "default",
                    "web_search",
                    "session_summary",
                    "image_description",
                    "prompt_suggestion",
                ]
                .contains(&name.as_str())
            })
            .map(|(key, value)| (key.clone(), semantic(value)))
            .collect::<serde_json::Value>(),
        _ => panic!("models must remain a table"),
    };
    assert_eq!(models(&original), models(&projected));
    assert_eq!(projected.model_ids().unwrap(), ["real"]);
    assert!(projected.model_fields("fake").unwrap().is_none());
}

#[test]
fn only_explicit_model_provider_and_auth_namespaces_survive_projection() {
    let references = "model_provider = 'gateway.with.dot'\nauth_provider = 'direct-helper'\n[model.real.extra_headers]\n'X-Model' = 'model-header'\n[model.real.query_params]\n'version' = 'model-query'\n[model.real.env_http_headers]\n'X-Env' = 'SYNTHETIC_MODEL_ENV'\n[model_providers.'gateway.with.dot']\nauth_provider = 'inherited-helper'\nfuture = [1, {x = 'retained'}]\n[model_providers.'gateway.with.dot'.extra_headers]\n'X-Tenant' = 'provider-header'\n[model_providers.'gateway.with.dot'.query_params]\n'version' = 'provider-query'\n[model_providers.'gateway.with.dot'.env_http_headers]\n'X-Token' = 'SYNTHETIC_PROVIDER_ENV'\n[model_providers.foreign]\napi_key = 'synthetic-foreign-provider'\n[auth_provider.direct-helper]\ncommand = ['synthetic-never-executed', '--direct']\n[auth_provider.inherited-helper]\ncommand = 'synthetic-never-executed'\n[auth_provider.foreign]\ncommand = 'synthetic-foreign-auth'\n";
    let source = format!("{}{references}", projection_fixture("generic"));
    let original = ConfigSnapshot::parse(&source).unwrap();
    let text = ConfigSnapshot::parse(&source)
        .unwrap()
        .project_custom("real", "target-two")
        .unwrap();
    let parsed = ConfigSnapshot::parse(&text).unwrap();
    for (namespace, expected) in [
        ("model_providers", vec!["gateway.with.dot"]),
        ("auth_provider", vec!["direct-helper", "inherited-helper"]),
    ] {
        let (Value::Table(original), Value::Table(actual)) = (
            &original.root.fields[namespace],
            &parsed.root.fields[namespace],
        ) else {
            panic!("namespaces must be tables");
        };
        assert_eq!(
            actual.fields.keys().map(String::as_str).collect::<Vec<_>>(),
            expected
        );
        for name in expected {
            assert_eq!(
                semantic(&actual.fields[name]),
                semantic(&original.fields[name])
            );
        }
    }
    assert!(!text.contains("synthetic-foreign-provider"));
    assert!(!text.contains("synthetic-foreign-auth"));
    let (Value::Table(original_models), Value::Table(actual_models)) =
        (&original.root.fields["model"], &parsed.root.fields["model"])
    else {
        panic!("model namespace must be a table");
    };
    let (Value::Table(original), Value::Table(actual)) = (
        &original_models.fields["real"],
        &actual_models.fields["real"],
    ) else {
        panic!("selected model must be a table");
    };
    for name in ["extra_headers", "query_params", "env_http_headers"] {
        assert_eq!(
            semantic(&actual.fields[name]),
            semantic(&original.fields[name])
        );
    }
}

#[test]
fn unreferenced_credentials_are_removed_and_inline_auth_is_retained_only_when_referenced() {
    let extras = "[model_providers.foreign]\napi_key = 'synthetic-foreign-provider'\n[auth_provider.foreign]\ncommand = 'synthetic-foreign-auth'\n";
    let text = ConfigSnapshot::parse(&format!("{}{extras}", projection_fixture("generic")))
        .unwrap()
        .project_custom("real", "target-two")
        .unwrap();
    let parsed = ConfigSnapshot::parse(&text).unwrap();
    assert!(!parsed.root.fields.contains_key("model_providers"));
    assert!(!parsed.root.fields.contains_key("auth_provider"));
    let source = format!("{}model_provider = 'gateway'\n[model_providers.gateway.auth]\ncommand = ['synthetic-never-executed', '--inline']\nenv = {{SYNTHETIC = 'retained'}}\n", projection_fixture("generic"));
    let original = ConfigSnapshot::parse(&source).unwrap();
    let text = ConfigSnapshot::parse(&source)
        .unwrap()
        .project_custom("real", "target-two")
        .unwrap();
    let parsed = ConfigSnapshot::parse(&text).unwrap();
    assert_eq!(
        semantic(&parsed.root.fields["model_providers"]),
        semantic(&original.root.fields["model_providers"])
    );
    assert!(!parsed.root.fields.contains_key("auth_provider"));
}

#[test]
fn auxiliary_pins_follow_the_projected_route_without_widening_restrictions() {
    for (mode, expected) in [("generic", "real"), ("grok_build_proxy", "target-two")] {
        let source = projection_fixture(mode);
        let text = ConfigSnapshot::parse(&source)
            .unwrap()
            .project_custom("real", "target-two")
            .unwrap();
        let parsed = ConfigSnapshot::parse(&text).unwrap();
        let Value::Table(models) = &parsed.root.fields["models"] else {
            panic!("models must remain a table");
        };
        for role in [
            "web_search",
            "session_summary",
            "image_description",
            "prompt_suggestion",
        ] {
            assert!(matches!(&models.fields[role], Value::String(value) if value == expected));
        }
    }
    let text = ConfigSnapshot::parse(&route(""))
        .unwrap()
        .project_custom("real", "target")
        .unwrap();
    let parsed = ConfigSnapshot::parse(&text).unwrap();
    let Value::Table(models) = &parsed.root.fields["models"] else {
        panic!("models must remain a table");
    };
    assert_eq!(
        models.fields.len(),
        1,
        "absent auxiliary pins are not manufactured"
    );
}

#[test]
fn projection_rejects_missing_ambiguous_and_dotted_reference_lookalikes() {
    for extra in [
        "model_provider = 'missing'\n",
        "auth_provider = 'missing'\n",
        "model_provider = 42\n",
        "model_provider = 'gateway.extra_headers'\n[model_providers.gateway.extra_headers]\nx = 'synthetic'\n",
        "model_provider = 'gateway'\n[[model_providers.gateway]]\napi_key = 'synthetic'\n",
        "auth_provider = 'helper'\n[[auth_provider.helper]]\ncommand = 'synthetic-never-executed'\n",
    ] {
        let source = format!("{}{extra}", projection_fixture("generic"));
        let result = ConfigSnapshot::parse(&source).unwrap().project_custom("real", "target-two");
        let error = result.expect_err("unavailable references must not select a fallback");
        assert!(!error.contains("synthetic-selected-key"));
        assert!(!error.contains("synthetic.invalid"));
    }
}
