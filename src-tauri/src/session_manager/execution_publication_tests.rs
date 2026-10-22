//! Actual capture + duplex open + manager/store commit, all in synthetic homes.
use super::*;
use crate::{
    acp_client::{test_transport, AcpClient},
    session_execution::ExecutionSource,
    session_migration::{mcp_stdio::SessionMcpPolicy, test_home::TestEnvGuard},
    store,
};
use serde_json::{json, Value};
use std::{fs, path::PathBuf, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[path = "main_route_tests.rs"]
mod main_route;
#[path = "execution_migration_tests.rs"]
mod migration;
#[path = "execution_publication_race_tests.rs"]
mod race;

struct Fixture {
    root: PathBuf,
    meta: store::SessionMeta,
    settings: store::AppSettings,
    context: RuntimeWorkspaceContext,
    _env: TestEnvGuard,
}

impl Fixture {
    fn new(shared: bool) -> Self {
        let env = TestEnvGuard::acquire(&["HOME", "USERPROFILE"]);
        let root =
            std::env::temp_dir().join(format!("execution-publication-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("user")).unwrap();
        std::env::set_var("HOME", root.join("user"));
        std::env::set_var("USERPROFILE", root.join("user"));
        std::env::set_var("GROK_APP_HOME", root.join("app"));
        crate::paths::ensure_app_dirs().unwrap();
        let mut settings = store::load_settings();
        settings.composer_prefs_scope = "session".into();
        settings.session_data_mode = if shared { "shared" } else { "independent" }.into();
        store::save_settings(&settings).unwrap();
        let meta = store::create_session(None, None, false).unwrap();
        let context = RuntimeWorkspaceContext::capture(
            &meta,
            crate::paths::general_workspace_dir().to_str().unwrap(),
            &settings,
            false,
        )
        .unwrap();
        Self {
            root,
            meta,
            settings,
            context,
            _env: env,
        }
    }

    fn config(&self, provider: &str, model: &str, native: bool) {
        fs::write(crate::paths::agent_config_toml(), format!(
            "[models]\ndefault = \"{provider}\"\n[model.{provider}]\nmodel = \"{model}\"\nbase_url = \"https://synthetic.invalid/v1\"\napi_key = \"synthetic-not-a-credential\"\napi_backend = \"responses\"\napp_provider_mode = \"{}\"\napp_models = '[{{\"id\":\"grok-4.6\",\"name\":\"6\"}},{{\"id\":\"grok-4.5\",\"name\":\"5\"}}]'\n",
            if native { "grok_build_proxy" } else { "generic" })).unwrap();
    }

    fn manager(&self, client: &Arc<AcpClient>, process: &str) -> SessionManager {
        let manager = SessionManager::new();
        let mut live = routing_tests::bare_live_session(&self.meta.id, process);
        live.fsm.soft_disconnect();
        live.fsm.start_connect().unwrap();
        live.meta = self.meta.clone();
        live.workspace_context = Some(self.context.clone());
        live.acp = Some(client.clone());
        *manager.inner.lock() = Some(live);
        manager
    }

    fn stored(&self) -> store::SessionMeta {
        store::load_sessions_snapshot()
            .unwrap()
            .into_iter()
            .find(|row| row.id == self.meta.id)
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

async fn rpc<T>(
    peer: &mut tokio::io::DuplexStream,
    action: impl std::future::Future<Output = T>,
    response: Value,
) -> T {
    let (read, mut write) = tokio::io::split(peer);
    let mut lines = BufReader::new(read).lines();
    let mut action = Box::pin(action);
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            _ = &mut action => panic!("action finished before its actual ACP request"),
            line = lines.next_line() => {
                let request: Value = serde_json::from_str(&line.unwrap().unwrap()).unwrap();
                write.write_all(format!("{}\n", json!({"jsonrpc":"2.0","id":request["id"],"result":response})).as_bytes()).await.unwrap();
                write.flush().await.unwrap();
                action.await
            }
        }
    }).await.expect("synthetic wire budget")
}

async fn open(client: &AcpClient, peer: &mut tokio::io::DuplexStream, model: Option<&str>) {
    let mut response = json!({"sessionId":"synthetic-opened-agent"});
    if let Some(model) = model {
        response["models"] = json!({"currentModelId":model});
    }
    let (agent, resumed) = rpc(
        peer,
        test_transport::open_with_empty_mcp(client, None, false),
        response,
    )
    .await
    .unwrap();
    assert_eq!(agent, "synthetic-opened-agent");
    assert!(!resumed);
}

#[test]
fn publication_uses_captured_generic_a_not_new_global_or_desired_b() {
    let fixture = Fixture::new(false);
    fixture.config("generic-a", "grok-4.6", false);
    runtime().block_on(async {
        let (client, mut peer) =
            test_transport::client_for_current_route(true, SessionMcpPolicy::default(), None);
        open(&client, &mut peer, Some("generic-a")).await;
        let manager = fixture.manager(&client, "process-a");
        fixture.config("generic-b", "grok-4.5", false);
        store::save_composer_prefs_with_route(
            None,
            Some(&fixture.meta.id),
            Some("grok-4.5".into()),
            Some("medium".into()),
            Some("ask".into()),
            Some("accept_edits".into()),
            Some("generic-b".into()),
        )
        .unwrap();
        store::rename_session(&fixture.meta.id, "Latest title").unwrap();
        let foreign = store::create_session(None, Some("Foreign".into()), false).unwrap();
        let before_foreign = serde_json::to_value(&foreign).unwrap();
        let current = manager
            .publish_opened_execution(
                &fixture.meta,
                &client,
                "process-a",
                "synthetic-opened-agent",
                false,
            )
            .unwrap();
        let receipt = current.agent_execution.unwrap();
        assert_eq!(receipt.source, ExecutionSource::LegacyIndependent);
        assert_eq!(receipt.provider_id.as_deref(), Some("generic-a"));
        assert_eq!(receipt.model_id.as_deref(), Some("grok-4.6"));
        assert_eq!(current.provider_id.as_deref(), Some("generic-b"));
        assert_eq!(current.model_id.as_deref(), Some("grok-4.5"));
        assert_eq!(current.effort.as_deref(), Some("medium"));
        assert_eq!(current.mode.as_deref(), Some("ask"));
        assert_eq!(current.permission_policy.as_deref(), Some("accept_edits"));
        assert_eq!(current.title, "Latest title");
        assert_eq!(
            serde_json::to_value(
                store::load_sessions_snapshot()
                    .unwrap()
                    .into_iter()
                    .find(|row| row.id == foreign.id)
                    .unwrap()
            )
            .unwrap(),
            before_foreign
        );
        assert_eq!(
            manager.inner.lock().as_ref().unwrap().fsm.state(),
            SessionState::Ready
        );
        let serialized = serde_json::to_string(&fixture.stored()).unwrap();
        for secret in [
            "synthetic-not-a-credential",
            "synthetic.invalid",
            "provider_config",
            "home_config",
        ] {
            assert!(!serialized.contains(secret));
        }
        client.kill().await;
    });
}

#[test]
fn no_echo_ack_is_not_actual_request_model_evidence() {
    let fixture = Fixture::new(false);
    fixture.config("generic-a", "grok-4.6", false);
    runtime().block_on(async {
        let (client, mut peer) =
            test_transport::client_for_current_route(true, SessionMcpPolicy::default(), None);
        open(&client, &mut peer, Some("generic-a")).await;
        assert_eq!(
            client
                .execution_receipt("synthetic-opened-agent", "process-a")
                .unwrap()
                .model_id
                .as_deref(),
            Some("grok-4.6")
        );
        rpc(
            &mut peer,
            client.set_model_for("synthetic-opened-agent", "generic-a"),
            json!({}),
        )
        .await
        .unwrap();
        assert!(client
            .execution_receipt("synthetic-opened-agent", "process-a")
            .unwrap()
            .model_id
            .is_none());
        client.kill().await;
    });
}

#[test]
fn failed_replacement_never_readies_or_partially_publishes_connection() {
    let fixture = Fixture::new(false);
    runtime().block_on(async {
        let (client, mut peer) =
            test_transport::client_for_current_route(false, SessionMcpPolicy::default(), None);
        open(&client, &mut peer, Some("grok-4.6")).await;
        let manager = fixture.manager(&client, "process-a");
        let mut blocked = crate::paths::sessions_index_file().into_os_string();
        blocked.push(".tmp");
        fs::create_dir(PathBuf::from(blocked)).unwrap();
        let before = fs::read(crate::paths::sessions_index_file()).unwrap();
        assert!(manager
            .publish_opened_execution(
                &fixture.meta,
                &client,
                "process-a",
                "synthetic-opened-agent",
                false,
            )
            .is_err());
        assert_eq!(
            fs::read(crate::paths::sessions_index_file()).unwrap(),
            before
        );
        {
            let live = manager.inner.lock();
            let live = live.as_ref().unwrap();
            assert_eq!(live.fsm.state(), SessionState::Connecting);
            assert!(live.meta.agent_session_id.is_none());
            assert!(live.meta.agent_execution.is_none());
        }
        client.kill().await;
    });
}

#[test]
fn stale_deleted_rebound_cancelled_or_newer_execution_cannot_publish() {
    for scenario in [
        "deleted",
        "project",
        "agent",
        "source",
        "cancelled",
        "client",
        "process",
        "context",
    ] {
        let fixture = Fixture::new(false);
        runtime().block_on(async {
            let (client, mut peer) =
                test_transport::client_for_current_route(false, SessionMcpPolicy::default(), None);
            open(&client, &mut peer, Some("grok-4.6")).await;
            let manager = fixture.manager(&client, "process-a");
            match scenario {
                "deleted" => store::delete_session(&fixture.meta.id).unwrap(),
                "project" => {
                    let directory = fixture.root.join("other-project");
                    fs::create_dir(&directory).unwrap();
                    let project =
                        store::add_project(directory.to_string_lossy().into_owned(), true).unwrap();
                    store::set_session_project(&fixture.meta.id, Some(project.id)).unwrap();
                }
                "agent" | "source" => {
                    let mut current = fixture.stored();
                    current.agent_session_id = Some("newer-agent".into());
                    if scenario == "source" {
                        current.agent_execution =
                            Some(crate::session_execution::ExecutionReceipt {
                                schema_version: 1,
                                agent_session_id: "newer-agent".into(),
                                source: ExecutionSource::LegacyIndependent,
                                provider_id: None,
                                model_id: None,
                                process_id: Some("newer-process".into()),
                            });
                    }
                    store::update_session_meta(&current).unwrap();
                }
                _ => manager
                    .with_session_mut(&fixture.meta.id, |live| match scenario {
                        "cancelled" => live.fsm.soft_disconnect(),
                        "client" => live.acp = None,
                        "process" => live.process_id = "newer-process".into(),
                        "context" => live.workspace_context = None,
                        _ => unreachable!(),
                    })
                    .unwrap(),
            }
            let before = fs::read(crate::paths::sessions_index_file()).unwrap();
            assert!(
                manager
                    .publish_opened_execution(
                        &fixture.meta,
                        &client,
                        "process-a",
                        "synthetic-opened-agent",
                        false,
                    )
                    .is_err(),
                "{scenario}"
            );
            assert_eq!(
                fs::read(crate::paths::sessions_index_file()).unwrap(),
                before,
                "{scenario}"
            );
            client.kill().await;
        });
    }
}

#[test]
fn shared_remote_and_auxiliary_origins_do_not_acquire_local_custom_identity() {
    for kind in ["shared", "tcp", "ssh", "aux"] {
        let _fixture = Fixture::new(kind == "shared");
        runtime().block_on(async {
            let policy = SessionMcpPolicy {
                tcp: kind == "tcp",
                ssh: kind == "ssh",
                official_aux: kind == "aux",
                ..Default::default()
            };
            let (client, mut peer) = test_transport::client_for_current_route(false, policy, None);
            open(&client, &mut peer, Some("grok-4.6")).await;
            let receipt = client
                .execution_receipt("synthetic-opened-agent", "process-a")
                .unwrap();
            assert_eq!(
                receipt.source,
                match kind {
                    "shared" => ExecutionSource::LegacyShared,
                    "aux" => ExecutionSource::Unavailable,
                    _ => ExecutionSource::Remote,
                }
            );
            assert!(receipt.provider_id.is_none());
            if kind != "shared" {
                assert!(receipt.model_id.is_none());
            }
            client.kill().await;
        });
    }
}

#[test]
fn known_resume_source_cannot_load_colliding_agent_from_another_home() {
    let fixture = Fixture::new(false);
    runtime().block_on(async {
        let (client, _) =
            test_transport::client_for_current_route(false, SessionMcpPolicy::default(), None);
        let mut meta = fixture.meta.clone();
        meta.agent_session_id = Some("colliding-agent".into());
        meta.agent_execution = Some(crate::session_execution::ExecutionReceipt {
            schema_version: 1,
            agent_session_id: "colliding-agent".into(),
            source: ExecutionSource::LegacyShared,
            provider_id: None,
            model_id: None,
            process_id: None,
        });
        assert!(execution_publication::validate_resume(&meta, &client).is_err());
        meta.agent_execution.as_mut().unwrap().source = ExecutionSource::LegacyIndependent;
        execution_publication::validate_resume(&meta, &client).unwrap();
        client.kill().await;
    });
}

#[test]
fn reconnect_same_agent_has_new_receipt_and_old_completion_is_rejected() {
    let fixture = Fixture::new(false);
    runtime().block_on(async {
        let (first, mut peer) =
            test_transport::client_for_current_route(false, SessionMcpPolicy::default(), None);
        open(&first, &mut peer, Some("grok-4.6")).await;
        let manager = fixture.manager(&first, "process-a");
        let previous = manager
            .publish_opened_execution(
                &fixture.meta,
                &first,
                "process-a",
                "synthetic-opened-agent",
                false,
            )
            .unwrap();
        let (next, mut next_peer) =
            test_transport::client_for_current_route(false, SessionMcpPolicy::default(), None);
        open(&next, &mut next_peer, Some("grok-4.5")).await;
        manager
            .with_session_mut(&fixture.meta.id, |live| {
                live.fsm.soft_disconnect();
                live.fsm.start_connect().unwrap();
                live.acp = Some(next.clone());
                live.process_id = "process-b".into();
            })
            .unwrap();
        manager
            .publish_opened_execution(
                &previous,
                &next,
                "process-b",
                "synthetic-opened-agent",
                false,
            )
            .unwrap();
        let before = fs::read(crate::paths::sessions_index_file()).unwrap();
        assert!(manager
            .publish_opened_execution(
                &previous,
                &first,
                "process-a",
                "synthetic-opened-agent",
                false,
            )
            .is_err());
        assert_eq!(
            fs::read(crate::paths::sessions_index_file()).unwrap(),
            before
        );
        let current = fixture.stored();
        assert_eq!(current.agent_session_id, previous.agent_session_id);
        let receipt = current.agent_execution.unwrap();
        assert_eq!(receipt.process_id.as_deref(), Some("process-b"));
        assert_eq!(receipt.model_id.as_deref(), Some("grok-4.5"));
        first.kill().await;
        next.kill().await;
    });
}

#[test]
fn captured_native_model_and_home_are_not_rederived_from_global_settings() {
    let mut fixture = Fixture::new(false);
    fixture.config("native-a", "grok-4.6", true);
    runtime().block_on(async {
        let (client, mut peer) =
            test_transport::client_for_current_route(true, SessionMcpPolicy::default(), None);
        open(&client, &mut peer, Some("grok-4.5")).await;
        fixture.config("native-b", "grok-4.6", true);
        fixture.settings.session_data_mode = "shared".into();
        store::save_settings(&fixture.settings).unwrap();
        let receipt = client
            .execution_receipt("synthetic-opened-agent", "process-a")
            .unwrap();
        assert_eq!(receipt.source, ExecutionSource::LegacyIndependent);
        assert_eq!(receipt.provider_id.as_deref(), Some("native-a"));
        assert_eq!(receipt.model_id.as_deref(), Some("grok-4.5"));
        assert!(client
            .execution_receipt("foreign-agent", "process-a")
            .is_err());
        client.kill().await;
        assert!(client
            .execution_receipt("synthetic-opened-agent", "process-a")
            .is_err());
    });
}

#[test]
fn asynchronous_publication_targets_background_connection_not_busy_foreign_focus() {
    let fixture = Fixture::new(false);
    runtime().block_on(async {
        let (client, mut peer) =
            test_transport::client_for_current_route(false, SessionMcpPolicy::default(), None);
        open(&client, &mut peer, Some("grok-4.6")).await;
        let manager = Arc::new(fixture.manager(&client, "process-a"));
        let target = manager.inner.lock().take().unwrap();
        manager
            .background
            .lock()
            .insert(fixture.meta.id.clone(), target);
        let foreign = store::create_session(None, Some("Busy foreign".into()), false).unwrap();
        let mut busy = routing_tests::bare_live_session(&foreign.id, "foreign-process");
        busy.meta = foreign.clone();
        busy.fsm.begin_stream().unwrap();
        busy.prompt_in_flight = true;
        *manager.inner.lock() = Some(busy);
        let current = manager
            .complete_opened_execution(
                &fixture.meta,
                &client,
                "process-a",
                "synthetic-opened-agent",
                false,
            )
            .await
            .unwrap();
        assert_eq!(current.id, fixture.meta.id);
        assert_eq!(
            manager
                .background
                .lock()
                .get(&fixture.meta.id)
                .unwrap()
                .fsm
                .state(),
            SessionState::Ready
        );
        {
            let live = manager.inner.lock();
            let live = live.as_ref().unwrap();
            assert_eq!(live.app_session_id, foreign.id);
            assert_eq!(live.fsm.state(), SessionState::Streaming);
            assert!(live.prompt_in_flight);
        }
        assert_eq!(
            serde_json::to_value(
                store::load_sessions_snapshot()
                    .unwrap()
                    .into_iter()
                    .find(|row| row.id == foreign.id)
                    .unwrap()
            )
            .unwrap(),
            serde_json::to_value(foreign).unwrap()
        );
        client.kill().await;
    });
}
