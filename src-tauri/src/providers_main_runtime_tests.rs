use super::*;
use crate::session_migration::test_home::IsolatedAppHome;
use serde_json::json;

const CONFIG: &str = r#"
[models]
default = 'selected'
[model.selected]
model = 'text-model'
name = 'synthetic relay'
base_url = 'https://synthetic.invalid/v1'
api_key = 'synthetic-key'
app_models = '[{"id":"text-model","name":"Text","supportsVision":false},{"id":"vision-model","name":"Vision","supportsVision":true}]'
[mcp_servers.'用户.server']
command = 'unexecuted-synthetic'
args = ['中文', '"literal"']
env = { 'A.B' = '合成值' }
[mcp_servers.'official-aux']
command = 'unexecuted-user-duplicate'
"#;

fn capture(
    binding: &ProcessModelBinding,
    inject: bool,
    with_user: bool,
    source: &str,
) -> MainRuntime {
    MainRuntime::capture(
        CONFIG,
        source,
        MainMcpSources::default(),
        binding,
        inject,
        &AppSettings {
            official_aux_with_user_mcp: with_user,
            ..AppSettings::default()
        },
        &ExtensionsPrefs::default(),
    )
    .unwrap()
}

fn generic() -> ProcessModelBinding {
    ProcessModelBinding::Generic {
        provider_id: "selected".into(),
        request_model: "text-model".into(),
    }
}

#[test]
fn official_policy_keeps_user_mcp_but_never_calls_official_aux_builder_or_host_vision() {
    let _home = IsolatedAppHome::acquire_with_user_home("main-runtime-official");
    let runtime = capture(&ProcessModelBinding::catalog(), false, false, CONFIG);
    let servers = crate::extensions::compose_main_mcp_servers(
        runtime.mcp_servers_for_connect(None, None),
        runtime.policy,
        || panic!("official main must not inspect auxiliary credentials"),
    );
    assert_eq!(servers.as_array().unwrap().len(), 1);
    assert_eq!(servers[0]["name"], "用户.server");
    let prompt = "描述\n@/synthetic/中文.png";
    let context = runtime.vision_context(Some("text-model"));
    assert!(!context.will_run(prompt));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let prepared =
                crate::models_aux::prepare_agent_prompt_for_context(prompt, None, &context).await;
            assert_eq!(prepared.prompt, prompt);
            assert!(!prepared.ran);
        });
}

#[test]
fn custom_solo_and_combined_mcp_use_the_captured_policy_and_deduplicate_auxiliary() {
    let _home = IsolatedAppHome::acquire_with_user_home("main-runtime-custom-mcp");
    for with_user in [false, true] {
        let runtime = capture(&generic(), true, with_user, CONFIG);
        let calls = std::cell::Cell::new(0);
        let servers = crate::extensions::compose_main_mcp_servers(
            json!([
                {"name":"official-aux","command":"unexecuted-duplicate"},
                {"name":"user","command":"unexecuted-user"}
            ]),
            runtime.policy,
            || {
                calls.set(calls.get() + 1);
                Some(json!({"name":"official-aux","command":"unexecuted-app-aux"}))
            },
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(servers[0]["command"], "unexecuted-app-aux");
        assert_eq!(
            servers.as_array().unwrap().len(),
            if with_user { 2 } else { 1 }
        );
    }
}

#[test]
fn native_model_capabilities_follow_only_the_captured_catalog_and_current_echo() {
    let _home = IsolatedAppHome::acquire_with_user_home("main-runtime-native-vision");
    let runtime = capture(
        &ProcessModelBinding::Native {
            provider_id: "selected".into(),
            models: vec!["text-model".into(), "vision-model".into()],
            fallback: "text-model".into(),
        },
        false,
        false,
        CONFIG,
    );
    let prompt = "描述\n@/synthetic/中文.png";
    assert!(runtime.vision_context(None).will_run(prompt));
    assert!(runtime.vision_context(Some("text-model")).will_run(prompt));
    assert!(!runtime
        .vision_context(Some("vision-model"))
        .will_run(prompt));
    assert!(runtime
        .vision_context(Some("unknown-echo"))
        .will_run(prompt));
}

#[test]
fn text_only_inject_off_strips_pixels_without_using_global_official_or_http_helpers() {
    let _home = IsolatedAppHome::acquire_with_user_home("main-runtime-no-aux");
    let runtime = capture(&generic(), false, false, CONFIG);
    let context = runtime.vision_context(Some("selected"));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let prepared = crate::models_aux::prepare_agent_prompt_for_context(
                "解释\n@/synthetic/中文.png",
                None,
                &context,
            )
            .await;
            assert!(prepared.ran && !prepared.ok);
            assert!(prepared.prompt.contains("Images not described"));
            assert!(crate::models_aux::extract_image_at_paths(&prepared.prompt).is_empty());
        });
}

#[test]
fn explicit_vision_helper_is_captured_outside_the_main_projection_and_mutable_proxy() {
    let _home = IsolatedAppHome::acquire_with_user_home("main-runtime-captured-helper");
    let source = format!(
        "{}{}",
        CONFIG.replace(
            "default = 'selected'",
            "default = 'foreign'\nimage_description = 'helper'"
        ),
        r#"
[model.helper]
model = 'helper-model'
base_url = 'http://127.0.0.1:7/r/helper/v1'
app_upstream_base_url = 'https://helper.invalid/v1'
api_key = 'synthetic-helper-key'
api_backend = 'responses'
"#
    );
    let runtime = capture(&generic(), false, false, &source);
    let helper = runtime.vision_endpoint.as_ref().unwrap();
    assert_eq!(helper.base_url, "https://helper.invalid/v1");
    assert_eq!(helper.model, "helper-model");
    assert_eq!(helper.api_backend, "responses");
    assert!(helper.api_key == "synthetic-helper-key");
    assert!(!runtime.policy.inject_official_aux);
    assert!(
        capture(&ProcessModelBinding::catalog(), false, false, &source)
            .vision_endpoint
            .is_none()
    );
}

#[test]
fn native_sibling_capabilities_keep_original_channel_flags_after_selected_row_projection() {
    let _home = IsolatedAppHome::acquire_with_user_home("main-runtime-channel-flags");
    let source = CONFIG
        .replace(
            "name = 'synthetic relay'",
            "name = 'synthetic relay'\napp_supports_vision = true",
        )
        .replace(
            r#""id":"vision-model","name":"Vision","supportsVision":true"#,
            r#""id":"vision-model","name":"Vision""#,
        );
    let fields = ConfigSnapshot::parse(&source)
        .unwrap()
        .model_fields("selected")
        .unwrap()
        .unwrap();
    let catalog: Vec<super::super::ProviderModelEntry> =
        serde_json::from_str(fields.get(super::super::APP_MODELS_KEY).unwrap()).unwrap();
    assert_eq!(catalog.len(), 2);
    assert_eq!(catalog[0].supports_vision, Some(false));
    assert_eq!(catalog[1].supports_vision, None);
    let projected = source.replace("app_supports_vision = true", "app_supports_vision = false");
    let runtime = MainRuntime::capture(
        &projected,
        &source,
        MainMcpSources::default(),
        &ProcessModelBinding::Native {
            provider_id: "selected".into(),
            models: vec!["text-model".into(), "vision-model".into()],
            fallback: "text-model".into(),
        },
        false,
        &AppSettings::default(),
        &ExtensionsPrefs::default(),
    )
    .unwrap();
    let prompt = "描述\n@/synthetic/中文.png";
    assert!(runtime.vision_context(Some("text-model")).will_run(prompt));
    assert!(!runtime
        .vision_context(Some("vision-model"))
        .will_run(prompt));
    assert!(runtime.matches_model_policy("text-model"));
    assert!(!runtime.matches_model_policy("vision-model"));
}

#[test]
fn structural_mcp_reader_preserves_quoted_inline_and_dotted_fields_and_rejects_invalid_types() {
    let snapshot = ConfigSnapshot::parse(CONFIG).unwrap();
    let defs = snapshot.mcp_definitions().unwrap();
    let def = defs.iter().find(|def| def.name == "用户.server").unwrap();
    assert_eq!(def.args.as_ref().unwrap(), &["中文", "\"literal\""]);
    assert_eq!(def.env.as_ref().unwrap().get("A.B").unwrap(), "合成值");
    let dotted = ConfigSnapshot::parse("mcp_servers.test.command = 'unexecuted'\nmcp_servers.test.headers.Authorization = 'synthetic-only'\n").unwrap();
    assert!(dotted.mcp_definitions().unwrap()[0]
        .headers
        .as_ref()
        .unwrap()
        .contains_key("Authorization"));
    for text in [
        "[mcp_servers.test]\ncommand = 1\n",
        "[mcp_servers.test]\nargs = ['ok', 1]\n",
        "[mcp_servers.test]\nenv = { KEY = 2 }\n",
        "[mcp_servers.test]\nheaders = 'synthetic-secret'\n",
    ] {
        let error = ConfigSnapshot::parse(text)
            .unwrap()
            .mcp_definitions()
            .err()
            .unwrap();
        assert!(!error.contains("synthetic-secret"));
    }
}

#[test]
fn session_bound_pr_tools_keep_captured_route_and_user_mcp() {
    let home = IsolatedAppHome::acquire_with_user_home("main-runtime-session-pr");
    for binding in [ProcessModelBinding::catalog(), generic()] {
        let runtime = capture(&binding, false, false, CONFIG);
        // A later global config must not replace this actor's captured MCP set.
        let global = crate::paths::agent_config_toml();
        std::fs::create_dir_all(global.parent().unwrap()).unwrap();
        std::fs::write(
            &global,
            "[mcp_servers.foreign]\ncommand='unexecuted-foreign'\n",
        )
        .unwrap();
        let cwd = home.path.to_str().unwrap();
        let servers = runtime.mcp_servers_for_connect(Some(cwd), Some("bound-chat"));
        let entries = servers.as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e["name"] == "用户.server"));
        assert!(!entries
            .iter()
            .any(|e| e["name"] == "foreign" || e["name"] == "official-aux"));
        let pr = entries.iter().find(|e| e["name"] == "pr-monitor").unwrap();
        let env = pr["env"].as_array().unwrap();
        assert!(env
            .iter()
            .any(|e| e["name"] == "GROK_APP_SESSION_ID" && e["value"] == "bound-chat"));
        assert!(env
            .iter()
            .any(|e| e["name"] == "GROK_APP_PROJECT_PATH" && e["value"] == cwd));
        assert_eq!(
            runtime
                .mcp_servers_for_connect(Some(cwd), None)
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
