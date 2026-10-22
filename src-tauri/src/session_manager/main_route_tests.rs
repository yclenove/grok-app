//! Normal manager selection, actual producer/open and stale model-RPC guards.
use super::*;
use crate::{
    extensions::ExtensionsPrefs, providers::PreparedMainProfile,
    session_execution::ExecutionReceipt,
};
use std::{sync::atomic::AtomicBool, time::Instant};

fn selected(fixture: &mut Fixture, native: bool) -> store::ComposerPrefs {
    fixture.config("selected", "grok-4.6", native);
    let path = crate::paths::agent_config_toml();
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("default = \"selected\"", "default = \"other\"");
    fs::write(path, format!("{text}\napp_append_prompt = 'SELECTED-RULE'\n[model.other]\nmodel = 'foreign'\nbase_url = 'https://foreign.invalid/v1'\napi_key = 'synthetic-foreign-key'\napp_append_prompt = 'FOREIGN-RULE'\n")).unwrap();
    fixture.meta.provider_id = Some("selected".into());
    fixture.meta.model_id = Some("grok-4.6".into());
    store::update_session_meta(&fixture.meta).unwrap();
    store::resolve_composer_prefs(None, Some(&fixture.meta.id))
}

fn profile(fixture: &Fixture, prefs: &store::ComposerPrefs) -> PreparedMainProfile {
    let request = super::super::main_route::request(
        &fixture.meta,
        prefs,
        &fixture.settings,
        crate::paths::general_workspace_dir().to_str().unwrap(),
        false,
    )
    .unwrap()
    .unwrap();
    PreparedMainProfile::prepare(
        &request,
        &fixture.settings,
        "ask",
        false,
        &ExtensionsPrefs::default(),
        None,
        &AtomicBool::new(false),
    )
    .unwrap()
}

#[test]
fn actual_open_uses_captured_mcp_definitions_after_global_config_and_prefs_change() {
    let mut fixture = Fixture::new(true);
    let prefs = selected(&mut fixture, false);
    let path = crate::paths::agent_config_toml();
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{text}\n[mcp_servers.'用户.server']\ncommand = 'unexecuted-selected'\nargs = ['中文']\n[mcp_servers.remote]\nurl = 'https://selected-mcp.invalid/mcp'\nheaders = {{ Authorization = 'Bearer synthetic-config-mcp' }}\n")).unwrap();
    fs::write(path.parent().unwrap().join("mcp_credentials.json"), json!({"remote": {"access_token":"synthetic-selected-mcp", "resource":"https://selected-mcp.invalid/mcp"}}).to_string()).unwrap();
    runtime().block_on(async {
        let prepared = profile(&fixture, &prefs);
        let (client, mut peer, _) = test_transport::client_from_main_profile(prepared, crate::paths::general_workspace_dir());
        fs::write(&path, "[models]\ndefault='official'\n[mcp_servers.foreign]\ncommand='unexecuted-foreign'\n").unwrap();
        fs::write(crate::paths::extensions_file(), r#"{"mcp":{"用户.server":false}}"#).unwrap();
        for credential_home in [path.parent().unwrap().to_path_buf(), crate::process_util::user_home().join(".grok")] {
            fs::create_dir_all(&credential_home).unwrap();
            fs::write(credential_home.join("mcp_credentials.json"), json!({"remote": {"access_token":"synthetic-later-global-mcp", "resource":"https://selected-mcp.invalid/mcp"}}).to_string()).unwrap();
        }
        let context = client.main_vision_context(None).unwrap();
        let prompt = "保留像素\n@/synthetic/中文.png";
        let prepared = crate::models_aux::prepare_agent_prompt_for_context(prompt, None, &context).await;
        assert_eq!(prepared.prompt, prompt);
        assert!(!prepared.ran);
        let (read, mut write) = tokio::io::split(&mut peer);
        let mut lines = BufReader::new(read).lines();
        let action = client.open_session(None, false);
        tokio::pin!(action);
        tokio::time::timeout(Duration::from_secs(4), async {
            tokio::select! {
                _ = &mut action => panic!("open completed before captured MCP wire request"),
                line = lines.next_line() => {
                    let request: Value = serde_json::from_str(&line.unwrap().unwrap()).unwrap();
                    assert_eq!(request["method"], "session/new");
                    let servers = request["params"]["mcpServers"].as_array().unwrap();
                    assert_eq!(servers.len(), 2);
                    let stdio = servers.iter().find(|server| server["name"] == "用户.server").unwrap();
                    assert_eq!(stdio["command"], "unexecuted-selected");
                    let remote = servers.iter().find(|server| server["name"] == "remote").unwrap();
                    assert_eq!(remote["url"], "https://selected-mcp.invalid/mcp");
                    assert_eq!(remote["headers"][0]["value"], "Bearer synthetic-selected-mcp");
                    write.write_all(format!("{}\n", json!({"jsonrpc":"2.0","id":request["id"],"result":{"sessionId":"synthetic-opened-agent","models":{"currentModelId":"selected"}}})).as_bytes()).await.unwrap();
                    write.flush().await.unwrap();
                    action.await.unwrap();
                }
            }
        }).await.unwrap();
        client.kill().await;
    });
}

#[test]
fn owned_main_mcp_changes_queue_live_busy_background_and_parked_actors() {
    for location in ["live", "busy", "background", "parked"] {
        let mut fixture = Fixture::new(false);
        let prefs = selected(&mut fixture, false);
        runtime().block_on(async {
            let (client, _peer, _) = test_transport::client_from_main_profile(
                profile(&fixture, &prefs),
                crate::paths::general_workspace_dir(),
            );
            let manager = fixture.manager(&client, "runtime-process");
            if location == "busy" {
                manager.with_session_mut(&fixture.meta.id, |live| {
                    live.fsm.handshake_ok().unwrap();
                    live.fsm.begin_stream().unwrap();
                    live.prompt_in_flight = true;
                });
            } else if location == "background" {
                let live = manager.inner.lock().take().unwrap();
                manager
                    .background
                    .lock()
                    .insert(fixture.meta.id.clone(), live);
            } else if location == "parked" {
                manager.inner.lock().take();
                manager.parked.lock().insert(
                    fixture.meta.id.clone(),
                    ParkedAgent {
                        turn_open_epoch: 0,
                        workspace_context: Some(fixture.context.clone()),
                        process_id: "runtime-process".into(),
                        app_session_id: fixture.meta.id.clone(),
                        meta: fixture.meta.clone(),
                        acp: client.clone(),
                        last_activity: Instant::now(),
                        model_id: fixture.meta.model_id.clone(),
                        effort: None,
                        product_mode: None,
                        project_path: None,
                        policy: crate::permission::PermissionPolicy::Ask,
                        sandbox_profile: None,
                        needs_history_bootstrap: false,
                        backend: "synthetic-acp".into(),
                    },
                );
            }
            assert_eq!(
                manager.queue_main_runtime_mcp_rebinds(),
                vec![fixture.meta.id.clone()]
            );
            assert_eq!(
                manager
                    .pending_soft_respawn
                    .lock()
                    .get(&fixture.meta.id)
                    .map(String::as_str),
                Some("extensions_mcp")
            );
            assert!(client.is_alive());
            if location == "busy" {
                assert!(manager
                    .with_session_mut(&fixture.meta.id, |live| live.prompt_in_flight
                        && live.fsm.state() == SessionState::Streaming)
                    .unwrap());
            }
            client.kill().await;
        });
    }
}

#[test]
fn main_profile_preserves_user_home_mcp_and_projected_config_name_precedence() {
    let mut fixture = Fixture::new(true);
    let prefs = selected(&mut fixture, false);
    let official = crate::process_util::user_home().join(".grok");
    fs::create_dir_all(&official).unwrap();
    fs::write(official.join("config.toml"), "[mcp_servers.shared]\ncommand='unexecuted-shared'\n[mcp_servers.same]\ncommand='unexecuted-shared-shadowed'\n[model.foreign]\napi_key='synthetic-never-copied'\n").unwrap();
    let path = crate::paths::agent_config_toml();
    let source = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        format!("{source}\n[mcp_servers.same]\ncommand='unexecuted-profile-wins'\n"),
    )
    .unwrap();
    let prepared = profile(&fixture, &prefs);
    let servers = prepared.runtime.mcp_servers_for_connect(None, None);
    let config = fs::read_to_string(prepared.home.join("config.toml")).unwrap();
    assert!(!config.contains("synthetic-never-copied"));
    let entries = servers.as_array().unwrap();
    assert_eq!(
        entries.len(),
        2,
        "shared user MCP disappeared from main capture"
    );
    assert!(entries
        .iter()
        .any(|entry| entry["name"] == "shared" && entry["command"] == "unexecuted-shared"));
    assert!(entries
        .iter()
        .any(|entry| entry["name"] == "same" && entry["command"] == "unexecuted-profile-wins"));
}

#[test]
fn native_switch_between_text_and_vision_models_cannot_reuse_stale_media_hooks() {
    for (start, next) in [
        ("text-model", "vision-model"),
        ("vision-model", "text-model"),
    ] {
        let mut fixture = Fixture::new(false);
        fs::write(crate::paths::agent_config_toml(), format!(r#"
[models]
default = 'selected'
[model.selected]
model = '{start}'
base_url = 'https://synthetic.invalid/v1'
api_key = 'synthetic-key'
api_backend = 'responses'
app_provider_mode = 'grok_build_proxy'
app_models = '[{{"id":"text-model","name":"Text","supportsVision":false}},{{"id":"vision-model","name":"Vision","supportsVision":true}}]'
"#)).unwrap();
        fixture.meta.provider_id = Some("selected".into());
        fixture.meta.model_id = Some(start.into());
        store::update_session_meta(&fixture.meta).unwrap();
        let prefs = store::resolve_composer_prefs(None, Some(&fixture.meta.id));
        runtime().block_on(async {
            let request = super::super::main_route::request(
                &fixture.meta,
                &prefs,
                &fixture.settings,
                crate::paths::general_workspace_dir().to_str().unwrap(),
                false,
            )
            .unwrap()
            .unwrap();
            let settings = store::AppSettings {
                official_aux_inject: true,
                ..fixture.settings.clone()
            };
            let prepared = PreparedMainProfile::prepare(
                &request,
                &settings,
                "ask",
                true,
                &ExtensionsPrefs::default(),
                None,
                &AtomicBool::new(false),
            )
            .unwrap();
            assert!(prepared.inject);
            assert_eq!(
                fs::read_to_string(prepared.home.join("hooks/grok-app-block-native-imagine.sh"))
                    .unwrap(),
                crate::official_aux::native_media_block_hook_script_body_with(
                    start == "text-model"
                )
            );
            let (client, _peer, _) = test_transport::client_from_main_profile(
                prepared,
                crate::paths::general_workspace_dir(),
            );
            let mut changed = prefs.clone();
            changed.model_id = next.into();
            let eligible =
                super::super::main_route::matches_client(&fixture.meta, &changed, &client).unwrap();
            client.kill().await;
            assert!(
                !eligible,
                "native capability switch reused hooks prepared for {start}"
            );
        });
    }
}

#[test]
fn normal_selection_produces_and_opens_the_explicit_channel_without_global_activation() {
    let mut fixture = Fixture::new(true);
    let prefs = selected(&mut fixture, false);
    let config = fs::read(crate::paths::agent_config_toml()).unwrap();
    let settings = fs::read(crate::paths::settings_file()).unwrap();
    runtime().block_on(async {
        let prepared = profile(&fixture, &prefs);
        let rules = prepared.extra_rules(Some("用户规则")).unwrap();
        assert!(rules.contains("用户规则"));
        assert!(rules.contains("SELECTED-RULE"));
        assert!(!rules.contains("FOREIGN-RULE"));
        assert!(!rules.contains("official-aux__"));
        let (client, mut peer, _) = test_transport::client_from_main_profile(
            prepared,
            crate::paths::general_workspace_dir(),
        );
        assert!(super::super::main_route::matches_client(&fixture.meta, &prefs, &client).unwrap());
        let manager = fixture.manager(&client, "selected-process");
        open(&client, &mut peer, Some("selected")).await;
        let published = manager
            .publish_opened_execution(
                &fixture.meta,
                &client,
                "selected-process",
                "synthetic-opened-agent",
                false,
            )
            .unwrap();
        assert_eq!(published.provider_id.as_deref(), Some("selected"));
        assert_eq!(
            published
                .agent_execution
                .as_ref()
                .unwrap()
                .provider_id
                .as_deref(),
            Some("selected")
        );
        assert!(matches!(
            published.agent_execution.as_ref().unwrap().source,
            ExecutionSource::AppProfile { .. }
        ));
        assert_eq!(fs::read(crate::paths::agent_config_toml()).unwrap(), config);
        assert_eq!(fs::read(crate::paths::settings_file()).unwrap(), settings);
        client.kill().await;
    });
}

#[test]
fn normal_reconnect_migrates_the_recorded_profile_history_to_the_next_owned_generation() {
    let mut fixture = Fixture::new(true);
    let prefs = selected(&mut fixture, false);
    runtime().block_on(async {
        let prepared = profile(&fixture, &prefs);
        let previous_home = prepared.home.clone();
        let (client, mut peer, _) = test_transport::client_from_main_profile(
            prepared,
            crate::paths::general_workspace_dir(),
        );
        open(&client, &mut peer, Some("selected")).await;
        let manager = fixture.manager(&client, "first-process");
        fixture.meta = manager
            .publish_opened_execution(
                &fixture.meta,
                &client,
                "first-process",
                "synthetic-opened-agent",
                false,
            )
            .unwrap();
        client.kill().await;
        // Duplex does not write a CLI history. Seed its synthetic recorded home
        // explicitly, then exercise the normal request + real migration writer.
        let history = previous_home.join("sessions/synthetic-workspace/synthetic-opened-agent");
        fs::create_dir_all(&history).unwrap();
        fs::write(
            history.parent().unwrap().join(".cwd"),
            crate::paths::general_workspace_dir().to_str().unwrap(),
        )
        .unwrap();
        fs::write(history.join("chat_history.jsonl"), "中文历史\n").unwrap();
        let next = profile(&fixture, &prefs);
        assert_ne!(next.home, previous_home);
        assert_eq!(
            fs::read(
                next.home
                    .join("sessions/synthetic-workspace/synthetic-opened-agent/chat_history.jsonl")
            )
            .unwrap(),
            "中文历史\n".as_bytes()
        );
        let (client, mut peer, _) =
            test_transport::client_from_main_profile(next, crate::paths::general_workspace_dir());
        let manager = fixture.manager(&client, "next-process");
        rpc(
            &mut peer,
            test_transport::open_with_empty_mcp(&client, Some("synthetic-opened-agent"), false),
            json!({"sessionId":"synthetic-opened-agent","models":{"currentModelId":"selected"}}),
        )
        .await
        .unwrap();
        let current = manager
            .publish_opened_execution(
                &fixture.meta,
                &client,
                "next-process",
                "synthetic-opened-agent",
                false,
            )
            .unwrap();
        assert_ne!(
            current.agent_execution.as_ref().unwrap().source,
            fixture.meta.agent_execution.as_ref().unwrap().source
        );
        assert_eq!(
            fs::read(history.join("chat_history.jsonl")).unwrap(),
            "中文历史\n".as_bytes()
        );
        client.kill().await;
    });
}

#[test]
fn normal_history_selection_uses_observed_receipt_or_unmodified_legacy_baseline() {
    let mut fixture = Fixture::new(true);
    let mut prefs = selected(&mut fixture, false);
    fixture.meta.agent_session_id = Some("source-agent".into());
    fixture.meta.provider_id = Some("official".into());
    prefs.model_id = "grok-4.6".into();
    let cwd = crate::paths::general_workspace_dir();
    let request = super::super::main_route::request(
        &fixture.meta,
        &prefs,
        &fixture.settings,
        cwd.to_str().unwrap(),
        false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(request.provider_id, "official");
    assert_eq!(
        request.history.unwrap().legacy_source,
        ExecutionSource::LegacyIndependent
    );
    fixture.meta.agent_execution = Some(ExecutionReceipt {
        schema_version: 1,
        agent_session_id: "source-agent".into(),
        source: ExecutionSource::LegacyShared,
        provider_id: Some("old-channel".into()),
        model_id: Some("old-model".into()),
        process_id: None,
    });
    let request = super::super::main_route::request(
        &fixture.meta,
        &prefs,
        &fixture.settings,
        cwd.to_str().unwrap(),
        false,
    )
    .unwrap()
    .unwrap();
    let history = request.history.unwrap();
    assert_eq!(history.legacy_source, ExecutionSource::Unavailable);
    assert_eq!(
        history.session.agent_execution.unwrap().source,
        ExecutionSource::LegacyShared
    );
    assert_eq!(history.cwd, cwd.to_str().unwrap());
}

#[test]
fn local_channel_rejects_remote_transport_and_incomplete_selection_without_fallback() {
    let mut fixture = Fixture::new(false);
    let prefs = selected(&mut fixture, false);
    let request = |meta: &store::SessionMeta, settings: &store::AppSettings, ssh| {
        super::super::main_route::request(meta, &prefs, settings, "synthetic-cwd", ssh)
    };
    assert!(request(&fixture.meta, &fixture.settings, true).is_err());
    let mut settings = fixture.settings.clone();
    settings.acp_server_addr = Some("127.0.0.1:9".into());
    assert!(request(&fixture.meta, &settings, false).is_err());
    fixture.meta.model_id = None;
    assert!(request(&fixture.meta, &fixture.settings, false).is_err());
    fixture.meta.provider_id = None;
    assert!(request(&fixture.meta, &settings, true).unwrap().is_none());
}

#[test]
fn generic_pinned_model_change_queues_rebind_for_live_busy_background_and_parked_owners() {
    for location in ["live", "busy", "background", "parked"] {
        let mut fixture = Fixture::new(false);
        let prefs = selected(&mut fixture, false);
        runtime().block_on(async {
            let prepared = profile(&fixture, &prefs);
            let (client, _peer, _) = test_transport::client_from_main_profile(
                prepared,
                crate::paths::general_workspace_dir(),
            );
            let manager = fixture.manager(&client, "route-process");
            if location == "busy" {
                manager.with_session_mut(&fixture.meta.id, |live| {
                    live.fsm.handshake_ok().unwrap();
                    live.fsm.begin_stream().unwrap();
                    live.prompt_in_flight = true;
                });
            } else if location == "background" {
                let live = manager.inner.lock().take().unwrap();
                manager
                    .background
                    .lock()
                    .insert(fixture.meta.id.clone(), live);
            } else if location == "parked" {
                manager.inner.lock().take();
                manager.parked.lock().insert(
                    fixture.meta.id.clone(),
                    ParkedAgent {
                        turn_open_epoch: 0,
                        workspace_context: Some(fixture.context.clone()),
                        process_id: "route-process".into(),
                        app_session_id: fixture.meta.id.clone(),
                        meta: fixture.meta.clone(),
                        acp: client.clone(),
                        last_activity: Instant::now(),
                        model_id: fixture.meta.model_id.clone(),
                        effort: None,
                        product_mode: None,
                        project_path: None,
                        policy: crate::permission::PermissionPolicy::Ask,
                        sandbox_profile: None,
                        needs_history_bootstrap: false,
                        backend: "synthetic-acp".into(),
                    },
                );
            }
            let mut changed = prefs.clone();
            changed.model_id = "grok-4.5".into();
            manager
                .queue_main_route_rebind(&fixture.meta, &changed)
                .await
                .unwrap();
            assert_eq!(
                manager
                    .pending_soft_respawn
                    .lock()
                    .get(&fixture.meta.id)
                    .map(String::as_str),
                Some("session_channel")
            );
            assert!(client.is_alive());
            if location == "busy" {
                assert!(manager
                    .with_session_mut(&fixture.meta.id, |live| live.prompt_in_flight
                        && live.fsm.state() == SessionState::Streaming)
                    .unwrap());
            }
            client.kill().await;
        });
    }
}

#[test]
fn native_catalog_switch_is_allowed_only_for_the_same_owned_channel() {
    let mut fixture = Fixture::new(false);
    let prefs = selected(&mut fixture, true);
    runtime().block_on(async {
        let (client, _peer, _) = test_transport::client_from_main_profile(
            profile(&fixture, &prefs),
            crate::paths::general_workspace_dir(),
        );
        let mut changed = prefs.clone();
        changed.model_id = "grok-4.5".into();
        assert!(
            super::super::main_route::matches_client(&fixture.meta, &changed, &client).unwrap()
        );
        let mut foreign = fixture.meta.clone();
        foreign.id = uuid::Uuid::new_v4().to_string();
        assert!(!super::super::main_route::matches_client(&foreign, &changed, &client).unwrap());
        foreign.id = fixture.meta.id.clone();
        foreign.provider_id = Some("other".into());
        assert!(!super::super::main_route::matches_client(&foreign, &changed, &client).unwrap());
        client.kill().await;
    });
}

#[test]
fn persisted_generic_model_change_never_sends_old_alias_as_a_successful_live_switch() {
    let mut fixture = Fixture::new(false);
    let prefs = selected(&mut fixture, false);
    runtime().block_on(async {
        let (client, mut peer, _) = test_transport::client_from_main_profile(
            profile(&fixture, &prefs),
            crate::paths::general_workspace_dir(),
        );
        open(&client, &mut peer, Some("selected")).await;
        let manager = fixture.manager(&client, "route-process");
        manager.with_session_mut(&fixture.meta.id, |live| {
            live.fsm.handshake_ok().unwrap();
            live.meta.agent_session_id = Some("synthetic-opened-agent".into());
        });
        store::save_composer_prefs(
            None,
            Some(&fixture.meta.id),
            Some("grok-4.5".into()),
            None,
            None,
            None,
        )
        .unwrap();
        manager
            .set_model("grok-4.5".into(), Some(&fixture.meta.id))
            .await
            .unwrap();
        let mut byte = [0_u8];
        use tokio::io::AsyncReadExt;
        assert!(
            tokio::time::timeout(Duration::from_millis(30), peer.read(&mut byte))
                .await
                .is_err()
        );
        assert_eq!(
            manager
                .pending_soft_respawn
                .lock()
                .get(&fixture.meta.id)
                .map(String::as_str),
            Some("session_channel")
        );
        client.kill().await;
    });
}

#[test]
fn matching_channel_and_model_cannot_reuse_a_different_recorded_execution_generation() {
    let mut fixture = Fixture::new(false);
    let prefs = selected(&mut fixture, false);
    runtime().block_on(async {
        let (client, mut peer, _) = test_transport::client_from_main_profile(
            profile(&fixture, &prefs),
            crate::paths::general_workspace_dir(),
        );
        open(&client, &mut peer, Some("selected")).await;
        let mut observed = fixture.meta.clone();
        observed.agent_session_id = Some("synthetic-opened-agent".into());
        observed.agent_execution = Some(
            client
                .execution_receipt("synthetic-opened-agent", "actual-process")
                .unwrap(),
        );
        assert!(super::super::main_route::matches_client(&observed, &prefs, &client).unwrap());
        observed.agent_execution.as_mut().unwrap().source = ExecutionSource::AppProfile {
            owner_session_id: observed.id.clone(),
            generation: uuid::Uuid::new_v4().to_string(),
        };
        let eligible =
            super::super::main_route::matches_client(&observed, &prefs, &client).unwrap();
        client.kill().await;
        assert!(
            !eligible,
            "matching desired route borrowed a stale execution generation"
        );
    });
}

#[test]
fn matching_profile_cannot_reuse_a_different_recorded_agent_on_the_same_generation() {
    let mut fixture = Fixture::new(false);
    let prefs = selected(&mut fixture, false);
    runtime().block_on(async {
        let (client, mut peer, _) = test_transport::client_from_main_profile(
            profile(&fixture, &prefs),
            crate::paths::general_workspace_dir(),
        );
        open(&client, &mut peer, Some("selected")).await;
        let mut observed = fixture.meta.clone();
        observed.agent_session_id = Some("newer-recorded-agent".into());
        let mut receipt = client
            .execution_receipt("synthetic-opened-agent", "actual-process")
            .unwrap();
        receipt.agent_session_id = "newer-recorded-agent".into();
        observed.agent_execution = Some(receipt);
        let eligible =
            super::super::main_route::matches_client(&observed, &prefs, &client).unwrap();
        client.kill().await;
        assert!(
            !eligible,
            "matching desired route borrowed another recorded agent"
        );
    });
}
