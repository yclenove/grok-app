//! Actual shared post-open consumer, real store transactions and ACP lines.
//! Synthetic home/duplex only: no CLI, provider, SSH, browser or native window.
use super::*;
use crate::acp_client::test_transport;
use crate::permission::PermissionPolicy;
use crate::session_migration::{mcp_stdio::SessionMcpPolicy, test_home::IsolatedAppHome};
use crate::store;
use serde_json::{json, Value};
use std::future::Future;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn row(agent: &str) -> store::SessionMeta {
    let mut row = store::create_session(None, None, false).unwrap();
    row.model_id = Some("grok-4.6".into());
    row.agent_session_id = Some(agent.into());
    row.effort = Some("low".into());
    row.mode = Some("agent".into());
    row.permission_policy = Some("ask".into());
    store::update_session_meta(&row).unwrap();
    row
}

fn live(row: &store::SessionMeta, client: &Arc<crate::acp_client::AcpClient>) -> LiveSession {
    let mut live = routing_tests::bare_live_session(&row.id, "synthetic-connected-process");
    live.meta = row.clone();
    live.model_id = row.model_id.clone();
    live.effort = row.effort.clone();
    live.product_mode = row.mode.clone();
    live.acp = Some(client.clone());
    live
}

async fn drive(
    peer: tokio::io::DuplexStream,
    update: impl Future<Output = Result<(), String>>,
    mut before_response: impl FnMut(&Value),
    reject: Option<&str>,
) -> (Result<(), String>, Vec<Value>) {
    let (read, mut write) = tokio::io::split(peer);
    let mut lines = BufReader::new(read).lines();
    let mut update = Box::pin(update);
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut requests = Vec::new();
        let mut read_open = true;
        loop {
            tokio::select! {
                result = &mut update => return (result, requests),
                line = lines.next_line(), if read_open => {
                    let Some(line) = line.unwrap() else { read_open = false; continue; };
                    let request: Value = serde_json::from_str(&line).unwrap();
                    let method = request["method"].as_str().unwrap();
                    assert!(matches!(method, "session/set_mode" | "session/set_model"));
                    before_response(&request);
                    let response = if reject == Some(method) {
                        json!({"jsonrpc":"2.0", "id":request["id"],
                            "error":{"code":-32601,"message":"synthetic unsupported method"}})
                    } else { json!({"jsonrpc":"2.0", "id":request["id"], "result":{}}) };
                    requests.push(request);
                    write.write_all(format!("{response}\n").as_bytes()).await.unwrap();
                    write.flush().await.unwrap();
                }
            }
        }
    })
    .await
    .expect("actual in-memory post-open path must finish")
}

fn assert_scenario(kind: &str) {
    let home = IsolatedAppHome::acquire("connected-composer-consumer");
    let mut settings = store::load_settings();
    settings.composer_prefs_scope = "session".into();
    store::save_settings(&settings).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (client, peer) = test_transport::client_for(
                false,
                SessionMcpPolicy::default(),
                Some("recent-foreign-agent"),
            );
            let mut a = row("synthetic-opened-agent");
            let foreign = row("synthetic-foreign-agent");
            if kind == "directory" {
                let directory = home.path.join("project");
                std::fs::create_dir(&directory).unwrap();
                let project =
                    store::add_project(directory.to_string_lossy().into_owned(), true).unwrap();
                a.project_id = Some(project.id);
                store::update_session_meta(&a).unwrap();
            }
            let manager = SessionManager::new();
            *manager.inner.lock() = Some(live(&a, &client));
            let starting = store::resolve_composer_prefs(a.project_id.as_deref(), Some(&a.id));
            let skip_mode = matches!(kind, "skip-mode" | "fork-new-mode");
            if !matches!(
                kind,
                "owner" | "no-flags" | "deleted" | "directory" | "missing-owner"
            ) {
                store::save_composer_prefs(
                    a.project_id.as_deref(),
                    Some(&a.id),
                    (kind != "during-mode").then(|| "grok-4.5".into()),
                    None,
                    (kind != "skip-mode").then(|| "ask".into()),
                    None,
                )
                .unwrap();
            }
            match kind {
                "background" => {
                    let owner = manager.inner.lock().take().unwrap();
                    manager.background.lock().insert(a.id.clone(), owner);
                    *manager.inner.lock() = Some(live(&foreign, &client));
                }
                "missing-owner" => {
                    *manager.inner.lock() = Some(live(&foreign, &client));
                }
                "deleted" => {
                    store::delete_session(&a.id).unwrap();
                }
                "directory" => {
                    store::set_session_project(&a.id, None).unwrap();
                }
                "spawn-flags" => {
                    store::save_composer_prefs(
                        None,
                        Some(&a.id),
                        None,
                        Some("medium".into()),
                        None,
                        Some("accept_edits".into()),
                    )
                    .unwrap();
                }
                _ => {}
            }
            let mut changed = false;
            let reject = match kind {
                "reject-mode" => Some("session/set_mode"),
                "reject-model" => Some("session/set_model"),
                _ => None,
            };
            // This is the same production path used by cold/warm/unpark. The
            // connection lock is already owned: a recursive lock would time out.
            let _connect = manager.connect_lock.lock().await;
            let (result, requests) = drive(
                peer,
                manager.align_connected_composer(&a.id, &starting, skip_mode),
                |request| {
                    let trigger = if matches!(kind, "during-model-mode" | "during-model-model") {
                        "session/set_model"
                    } else {
                        "session/set_mode"
                    };
                    if changed || request["method"] != trigger {
                        return;
                    }
                    changed = true;
                    match kind {
                        "during-mode" => {
                            store::save_composer_prefs(
                                None,
                                Some(&a.id),
                                Some("grok-4.5".into()),
                                None,
                                None,
                                None,
                            )
                            .unwrap();
                        }
                        "during-model-mode" | "during-model-model" => {
                            store::save_composer_prefs(
                                None,
                                Some(&a.id),
                                (kind == "during-model-model").then(|| "grok-4.6".into()),
                                None,
                                (kind == "during-model-mode").then(|| "agent".into()),
                                None,
                            )
                            .unwrap();
                        }
                        "replaced-process" => {
                            manager
                                .with_session_mut(&a.id, |s| {
                                    s.process_id = "synthetic-replacement-process".into();
                                })
                                .unwrap();
                        }
                        "lost-owner" => {
                            *manager.inner.lock() = Some(live(&foreign, &client));
                        }
                        "demoted" => {
                            let owner = manager.inner.lock().take().unwrap();
                            manager.background.lock().insert(a.id.clone(), owner);
                            *manager.inner.lock() = Some(live(&foreign, &client));
                        }
                        _ => {}
                    }
                },
                reject,
            )
            .await;
            if matches!(kind, "deleted" | "directory" | "missing-owner") {
                assert!(result.is_err());
                assert!(
                    requests.is_empty(),
                    "{kind}: RPC to stale owner {requests:?}"
                );
                if kind == "deleted" {
                    assert!(!crate::paths::session_dir(&a.id).exists());
                    assert!(store::load_sessions_snapshot()
                        .unwrap()
                        .iter()
                        .all(|row| row.id != a.id));
                }
                return;
            }
            assert!(requests
                .iter()
                .all(|request| request["params"]["sessionId"] == "synthetic-opened-agent"));
            if kind == "lost-owner" {
                assert!(result.is_err());
                assert_eq!(requests.len(), 1, "must recheck owner after mode RPC");
                assert_eq!(
                    manager.inner.lock().as_ref().unwrap().model_id,
                    foreign.model_id
                );
                return;
            }
            if kind == "replaced-process" {
                assert!(result.is_err());
                assert_eq!(
                    requests.len(),
                    1,
                    "cannot send model RPC into replacement process"
                );
                assert!(
                    !manager.pending_soft_respawn.lock().contains_key(&a.id),
                    "a stale completion must not recycle the replacement process"
                );
                return;
            }
            let model = requests
                .iter()
                .rfind(|request| request["method"] == "session/set_model")
                .unwrap();
            let expected_model = if matches!(kind, "owner" | "no-flags" | "during-model-model") {
                "grok-4.6"
            } else {
                "grok-4.5"
            };
            assert_eq!(
                model["params"]["modelId"], expected_model,
                "{kind}: stale model wire"
            );
            if kind == "skip-mode" {
                assert_eq!(
                    requests.len(),
                    1,
                    "fork mode is inherited when intent is unchanged"
                );
            } else if kind != "reject-mode" {
                let mode = requests
                    .iter()
                    .rfind(|request| request["method"] == "session/set_mode")
                    .unwrap();
                let expected_mode = if matches!(kind, "owner" | "no-flags" | "during-model-mode") {
                    "agent"
                } else {
                    "ask"
                };
                assert_eq!(
                    mode["params"]["modeId"], expected_mode,
                    "{kind}: stale mode wire"
                );
            }
            if reject.is_some() {
                assert!(result.is_err());
                assert!(
                    manager.pending_soft_respawn.lock().contains_key(&a.id),
                    "failed alignment must not silently permit a prompt"
                );
            } else {
                result.unwrap();
                assert_eq!(
                    manager
                        .with_session_mut(&a.id, |session| session.model_id.clone())
                        .unwrap()
                        .as_deref(),
                    Some(expected_model)
                );
            }
            if kind == "spawn-flags" {
                assert!(manager.pending_soft_respawn.lock().contains_key(&a.id));
                let saved = workspace_context::session(&a.id).unwrap();
                assert!(saved.agent_session_id.is_none());
                assert_eq!(saved.effort.as_deref(), Some("medium"));
                assert_eq!(saved.permission_policy.as_deref(), Some("accept_edits"));
                manager
                    .with_session_mut(&a.id, |session| {
                        assert_eq!(
                            session.effort.as_deref(),
                            Some("low"),
                            "cannot pretend fresh effort was spawned"
                        );
                        assert_eq!(session.policy, PermissionPolicy::Ask);
                        assert!(session.meta.agent_session_id.is_none());
                        assert!(session.needs_history_bootstrap);
                    })
                    .unwrap();
            } else if reject.is_none() {
                assert!(!manager.pending_soft_respawn.lock().contains_key(&a.id));
            }
            if matches!(kind, "background" | "demoted") {
                assert_eq!(
                    manager.inner.lock().as_ref().unwrap().model_id,
                    foreign.model_id
                );
                assert_eq!(
                    store::load_sessions_snapshot()
                        .unwrap()
                        .into_iter()
                        .find(|row| row.id == foreign.id)
                        .unwrap()
                        .model_id,
                    foreign.model_id
                );
            }
        });
}

macro_rules! scenario {
    ($name:ident, $kind:literal) => {
        #[test]
        fn $name() {
            assert_scenario($kind);
        }
    };
}
scenario!(
    late_model_and_mode_reach_memory_and_actual_wire,
    "late-choice"
);
scenario!(explicit_owner_does_not_use_recent_foreign_agent, "owner");
scenario!(background_owner_does_not_change_live_focus, "background");
scenario!(model_saved_during_mode_rpc_is_reread, "during-mode");
scenario!(deleted_owner_gets_no_rpc_and_is_not_revived, "deleted");
scenario!(changed_directory_binding_gets_no_stale_rpc, "directory");
scenario!(
    missing_owner_does_not_fall_back_to_live_focus,
    "missing-owner"
);
scenario!(owner_is_rechecked_after_mode_rpc, "lost-owner");
scenario!(demoted_owner_keeps_its_explicit_agent_target, "demoted");
scenario!(
    mode_rejection_keeps_model_apply_but_requires_respawn,
    "reject-mode"
);
scenario!(
    model_rejection_cannot_silently_allow_next_prompt,
    "reject-model"
);
scenario!(
    unchanged_fork_mode_is_inherited_but_model_is_fresh,
    "skip-mode"
);
scenario!(new_fork_mode_intent_overrides_the_skip, "fork-new-mode");
scenario!(changed_spawn_flags_are_not_claimed_applied, "spawn-flags");
scenario!(unchanged_flags_do_not_request_respawn, "no-flags");
scenario!(mode_saved_during_model_rpc_is_applied, "during-model-mode");
scenario!(
    model_saved_during_model_rpc_is_applied,
    "during-model-model"
);
scenario!(
    replacement_process_is_not_targeted_or_recycled,
    "replaced-process"
);
