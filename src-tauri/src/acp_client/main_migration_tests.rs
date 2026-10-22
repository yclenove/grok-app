use super::*;
use crate::{
    acp_client::test_transport,
    extensions::ExtensionsPrefs,
    providers::{MainHistoryRequest, MainProfileRequest, PreparedMainProfile},
    session_execution::ExecutionReceipt,
    session_migration::test_home::IsolatedAppHome,
    store::{self, AppSettings},
};
use serde_json::{json, Value};
use std::{fs, path::PathBuf, sync::atomic::AtomicBool, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct Fixture {
    meta: SessionMeta,
    request: MainProfileRequest,
    source: PathBuf,
    cwd: PathBuf,
    _home: IsolatedAppHome,
}

impl Fixture {
    fn new(fork: bool, legacy: bool) -> Self {
        let home = IsolatedAppHome::acquire_with_user_home("migration-acp-open");
        fs::write(crate::paths::agent_config_toml(),
            "[models]\ndefault = 'other'\n[model.selected]\nmodel = 'target'\nbase_url = 'https://synthetic.invalid/v1'\napi_key = 'synthetic-key'\n").unwrap();
        let mut meta = store::create_session(None, Some("合成迁移会话".into()), false).unwrap();
        meta.agent_session_id = Some("original-agent".into());
        meta.fork_agent_session = fork;
        if !legacy {
            meta.agent_execution = Some(ExecutionReceipt {
                schema_version: 1,
                agent_session_id: "original-agent".into(),
                source: ExecutionSource::LegacyShared,
                provider_id: Some("old-provider".into()),
                model_id: Some("old-model".into()),
                process_id: Some("old-process".into()),
            });
        }
        store::update_session_meta(&meta).unwrap();
        let cwd = crate::paths::general_workspace_dir();
        let source = ExecutionSource::LegacyShared
            .home()
            .unwrap()
            .unwrap()
            .join("sessions/synthetic-physical-workspace/original-agent");
        fs::create_dir_all(source.join("images")).unwrap();
        fs::write(source.parent().unwrap().join(".cwd"), cwd.to_str().unwrap()).unwrap();
        fs::write(source.join("plan.md"), "中文计划").unwrap();
        fs::write(
            source.join("chat_history.jsonl"),
            "{\"content\":\"中文历史\"}\n",
        )
        .unwrap();
        fs::write(source.join("images/1.png"), "synthetic-media").unwrap();
        let request = MainProfileRequest {
            owner_session_id: meta.id.clone(),
            provider_id: "selected".into(),
            model_id: "target".into(),
            history: Some(MainHistoryRequest {
                session: meta.clone(),
                cwd: cwd.to_str().unwrap().into(),
                legacy_source: ExecutionSource::LegacyShared,
            }),
        };
        Self {
            meta,
            request,
            source,
            cwd,
            _home: home,
        }
    }

    fn prepare(&self) -> PreparedMainProfile {
        PreparedMainProfile::prepare(
            &self.request,
            &AppSettings::default(),
            "ask",
            false,
            &ExtensionsPrefs::default(),
            None,
            &AtomicBool::new(false),
        )
        .unwrap()
    }

    fn client(
        &self,
    ) -> (
        Arc<AcpClient>,
        tokio::io::DuplexStream,
        test_transport::EventReceiver,
    ) {
        test_transport::client_from_main_profile(self.prepare(), self.cwd.clone())
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

async fn exchange<T>(
    peer: &mut tokio::io::DuplexStream,
    action: impl std::future::Future<Output = T>,
    replies: &[(&str, Value)],
) -> T {
    let (read, mut write) = tokio::io::split(peer);
    let mut lines = BufReader::new(read).lines();
    let mut action = Box::pin(action);
    tokio::time::timeout(Duration::from_secs(3), async {
        for (method, response) in replies {
            let line = tokio::select! {
                _ = &mut action => panic!("open ended before its requested wire exchange"),
                line = lines.next_line() => line.unwrap().unwrap(),
            };
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], *method);
            let mut reply = response.clone();
            reply["jsonrpc"] = json!("2.0");
            reply["id"] = request["id"].clone();
            write
                .write_all(format!("{reply}\n").as_bytes())
                .await
                .unwrap();
            write.flush().await.unwrap();
        }
        action.await
    })
    .await
    .expect("isolated migration wire budget")
}

fn success(agent: &str) -> Value {
    // Generic CLI models echo the provider alias. A body-model echo alone
    // deliberately does not certify the provider that owns that model name.
    json!({"result":{"sessionId":agent,"models":{"currentModelId":"selected"}}})
}
fn failure() -> Value {
    json!({"error":{"code":-32601,"message":"synthetic unsupported"}})
}

async fn open(
    client: &AcpClient,
    resume: Option<&str>,
    fork: bool,
) -> Result<(String, bool), crate::error::AgentError> {
    client
        .open_session_with_mcp_servers(resume, fork, &client.cwd().to_string_lossy(), json!([]))
        .await
}

#[test]
fn producer_witness_allows_only_the_matching_meta_and_actual_load_result() {
    let fixture = Fixture::new(false, false);
    runtime().block_on(async {
        let (client, mut peer, _) = fixture.client();
        client.validate_execution_resume(&fixture.meta).unwrap();
        for change in [
            |meta: &mut SessionMeta| meta.id = uuid::Uuid::new_v4().to_string(),
            |meta: &mut SessionMeta| meta.agent_session_id = Some("foreign-agent".into()),
            |meta: &mut SessionMeta| meta.workspace_root_snapshot = Some("changed-roots".into()),
            |meta: &mut SessionMeta| meta.fork_agent_session = true,
            |meta: &mut SessionMeta| meta.agent_execution.as_mut().unwrap().process_id = Some("foreign-process".into()),
        ] {
            let mut meta = fixture.meta.clone(); change(&mut meta);
            assert!(client.validate_execution_resume(&meta).is_err());
        }
        let mut changed_desired = fixture.meta.clone();
        changed_desired.title = "改标题".into(); changed_desired.provider_id = Some("desired-other".into());
        client.validate_execution_resume(&changed_desired).unwrap();
        let opened = exchange(&mut peer, open(&client, Some("original-agent"), false),
            &[("session/load", success("original-agent"))]).await.unwrap();
        assert_eq!(opened, ("original-agent".into(), true));
        client.validate_execution_publication(&fixture.meta, "original-agent").unwrap();
        let receipt = client.execution_receipt("original-agent", "new-process").unwrap();
        assert!(matches!(&receipt.source, ExecutionSource::AppProfile { owner_session_id, .. } if owner_session_id == &fixture.meta.id));
        assert_eq!(receipt.provider_id.as_deref(), Some("selected"));
        let mut current = fixture.meta.clone(); current.agent_execution = Some(receipt); current.fork_agent_session = false;
        client.note_execution_publication(&current).unwrap();
        assert!(client.validate_execution_resume(&fixture.meta).is_err());
        client.validate_execution_resume(&current).unwrap();
        client.kill().await;
        assert!(client.validate_execution_publication(&current, "original-agent").is_err());
    });
}

#[test]
fn legacy_migration_does_not_grant_foreign_rows_with_missing_receipts() {
    let fixture = Fixture::new(false, true);
    runtime().block_on(async {
        let (client, mut peer, _) = fixture.client();
        client.validate_execution_resume(&fixture.meta).unwrap();
        let mut foreign = fixture.meta.clone();
        foreign.id = uuid::Uuid::new_v4().to_string();
        assert!(client.validate_execution_resume(&foreign).is_err());
        exchange(
            &mut peer,
            open(&client, Some("original-agent"), false),
            &[("session/load", success("original-agent"))],
        )
        .await
        .unwrap();
        client
            .validate_execution_publication(&fixture.meta, "original-agent")
            .unwrap();
        assert!(client.note_execution_publication(&fixture.meta).is_err());
        client.validate_execution_resume(&fixture.meta).unwrap();
        client.kill().await;
    });
}

#[test]
fn changed_source_or_copied_history_is_rejected_before_any_acp_request() {
    for destination in [false, true] {
        let fixture = Fixture::new(false, false);
        runtime().block_on(async {
            let profile = fixture.prepare();
            let path = if destination {
                profile
                    .home
                    .join("sessions/synthetic-physical-workspace/original-agent/plan.md")
            } else {
                fixture.source.join("plan.md")
            };
            let (client, mut peer, _) =
                test_transport::client_from_main_profile(profile, fixture.cwd.clone());
            fs::write(path, "改变历史").unwrap();
            assert!(open(&client, Some("original-agent"), false).await.is_err());
            let mut line = String::new();
            assert!(tokio::time::timeout(
                Duration::from_millis(50),
                BufReader::new(&mut peer).read_line(&mut line)
            )
            .await
            .is_err());
            assert!(client.validate_execution_resume(&fixture.meta).is_err());
            client.kill().await;
        });
    }
}

#[test]
fn compact_proof_rejects_same_bytes_replacement_of_history_and_cwd() {
    for (destination, cwd) in [(false, false), (false, true), (true, false), (true, true)] {
        let fixture = Fixture::new(false, false);
        runtime().block_on(async {
            let profile = fixture.prepare();
            let root = if destination {
                profile
                    .home
                    .join("sessions/synthetic-physical-workspace/original-agent")
            } else {
                fixture.source.clone()
            };
            let path = if cwd {
                root.parent().unwrap().join(".cwd")
            } else {
                root.join("plan.md")
            };
            let bytes = fs::read(&path).unwrap();
            fs::rename(&path, fixture._home.path.join("held-original-history-file")).unwrap();
            fs::write(&path, bytes).unwrap();
            let (client, _peer, _) =
                test_transport::client_from_main_profile(profile, fixture.cwd.clone());
            assert!(open(&client, Some("original-agent"), false).await.is_err());
            assert!(matches!(
                &*client.main_migration.as_ref().unwrap().phase.lock(),
                Phase::Revoked
            ));
            client.kill().await;
        });
    }
}

#[test]
fn changed_destination_marker_cannot_authorize_load() {
    let fixture = Fixture::new(false, false);
    runtime().block_on(async {
        let profile = fixture.prepare();
        fs::write(
            profile.home.join("config.toml"),
            "[models]\ndefault = 'wrong'\n",
        )
        .unwrap();
        let (client, _peer, _) =
            test_transport::client_from_main_profile(profile, fixture.cwd.clone());
        assert!(open(&client, Some("original-agent"), false).await.is_err());
        client.kill().await;
    });
}

#[test]
fn mismatched_load_reply_revokes_grant_and_never_publishes_an_agent() {
    let fixture = Fixture::new(false, false);
    runtime().block_on(async {
        let (client, mut peer, _) = fixture.client();
        assert!(exchange(
            &mut peer,
            open(&client, Some("original-agent"), false),
            &[("session/load", success("foreign-agent"))]
        )
        .await
        .is_err());
        assert!(client.agent_session_id().is_none());
        assert!(client
            .execution_receipt("foreign-agent", "process")
            .is_err());
        assert!(client.validate_execution_resume(&fixture.meta).is_err());
        client.kill().await;
    });
}

#[test]
fn fork_and_new_fallback_keep_original_history_untouched() {
    for fallback in [false, true] {
        let fixture = Fixture::new(true, false);
        let before = fs::read(fixture.source.join("chat_history.jsonl")).unwrap();
        runtime().block_on(async {
            let (client, mut peer, _) = fixture.client();
            let replies = if fallback {
                vec![
                    ("session/fork", failure()),
                    ("_x.ai/session/fork", failure()),
                    ("session/new", success("fresh-child")),
                ]
            } else {
                vec![("session/fork", success("forked-child"))]
            };
            let (agent, resumed) = exchange(
                &mut peer,
                open(&client, Some("original-agent"), true),
                &replies,
            )
            .await
            .unwrap();
            assert_eq!(resumed, !fallback);
            assert_ne!(agent, "original-agent");
            client
                .validate_execution_publication(&fixture.meta, &agent)
                .unwrap();
            assert_eq!(
                fs::read(fixture.source.join("chat_history.jsonl")).unwrap(),
                before
            );
            client.kill().await;
        });
    }
}

#[test]
fn failed_load_can_bootstrap_a_new_agent_but_cannot_reuse_the_failed_source_id() {
    for repeated in [false, true] {
        let fixture = Fixture::new(false, false);
        runtime().block_on(async {
            let (client, mut peer, _) = fixture.client();
            let agent = if repeated {
                "original-agent"
            } else {
                "bootstrap-agent"
            };
            let result = exchange(
                &mut peer,
                open(&client, Some("original-agent"), false),
                &[("session/load", failure()), ("session/new", success(agent))],
            )
            .await;
            if repeated {
                assert!(result.is_err());
            } else {
                assert_eq!(result.unwrap(), ("bootstrap-agent".into(), false));
                client
                    .validate_execution_publication(&fixture.meta, agent)
                    .unwrap();
            }
            client.kill().await;
        });
    }
}

#[test]
fn rewind_failure_fresh_child_and_later_same_home_load_use_current_open_identity() {
    let fixture = Fixture::new(true, false);
    runtime().block_on(async {
        let (client, mut peer, _) = fixture.client();
        exchange(
            &mut peer,
            open(&client, Some("original-agent"), true),
            &[("session/fork", success("untrimmed-child"))],
        )
        .await
        .unwrap();
        exchange(
            &mut peer,
            open(&client, None, false),
            &[("session/new", success("bootstrap-child"))],
        )
        .await
        .unwrap();
        client
            .validate_execution_publication(&fixture.meta, "bootstrap-child")
            .unwrap();
        assert!(client
            .execution_receipt("untrimmed-child", "process")
            .is_err());
        let mut current = fixture.meta.clone();
        current.agent_session_id = Some("bootstrap-child".into());
        current.agent_execution = Some(
            client
                .execution_receipt("bootstrap-child", "process")
                .unwrap(),
        );
        current.fork_agent_session = false;
        client.note_execution_publication(&current).unwrap();
        exchange(
            &mut peer,
            open(&client, Some("bootstrap-child"), false),
            &[("session/load", success("bootstrap-child"))],
        )
        .await
        .unwrap();
        client
            .validate_execution_publication(&current, "bootstrap-child")
            .unwrap();
        assert!(client.validate_execution_resume(&fixture.meta).is_err());
        client.kill().await;
    });
}

#[test]
fn dropped_inflight_load_and_late_reply_cannot_reuse_or_publish_migration() {
    let fixture = Fixture::new(false, false);
    runtime().block_on(async {
        let (client, peer, _) = fixture.client();
        let (read, mut write) = tokio::io::split(peer);
        let mut lines = BufReader::new(read).lines();
        let mut action = Box::pin(open(&client, Some("original-agent"), false));
        let request: Value = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::select! {
                _ = &mut action => panic!("open ended before pending load"),
                line = lines.next_line() => serde_json::from_str(&line.unwrap().unwrap()).unwrap(),
            }
        })
        .await
        .unwrap();
        assert_eq!(request["method"], "session/load");
        drop(action);
        assert!(client.validate_execution_resume(&fixture.meta).is_err());
        let reply =
            json!({"jsonrpc":"2.0","id":request["id"],"result":{"sessionId":"original-agent"}});
        write
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .unwrap();
        write.flush().await.unwrap();
        assert!(open(&client, Some("original-agent"), false).await.is_err());
        assert!(client
            .execution_receipt("original-agent", "process")
            .is_err());
        client.kill().await;
    });
}

#[test]
fn opening_another_agent_cwd_or_fork_mode_cannot_borrow_the_ready_witness() {
    let fixture = Fixture::new(false, false);
    runtime().block_on(async {
        let (client, _peer, _) = fixture.client();
        assert!(open(&client, Some("foreign-agent"), false).await.is_err());
        assert!(open(&client, Some("original-agent"), true).await.is_err());
        assert!(client
            .open_session_with_mcp_servers(Some("original-agent"), false, "wrong-cwd", json!([]))
            .await
            .is_err());
        client.validate_execution_resume(&fixture.meta).unwrap();
        client.kill().await;
    });
}

#[test]
fn dropped_verification_wait_revokes_the_grant_before_queued_worker_runs() {
    let fixture = Fixture::new(false, false);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (client, _peer, _) = fixture.client();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let occupied = tokio::task::spawn_blocking(move || {
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        });
        started_rx.await.unwrap();
        let mut action = Box::pin(open(&client, Some("original-agent"), false));
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(action.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert!(matches!(
            &*client.main_migration.as_ref().unwrap().phase.lock(),
            Phase::Opening { .. }
        ));
        drop(action);
        assert!(matches!(
            &*client.main_migration.as_ref().unwrap().phase.lock(),
            Phase::Revoked
        ));
        release_tx.send(()).unwrap();
        occupied.await.unwrap();
        assert!(open(&client, Some("original-agent"), false).await.is_err());
        assert!(client.validate_execution_resume(&fixture.meta).is_err());
        client.kill().await;
    });
}
