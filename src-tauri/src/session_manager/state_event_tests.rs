//! Real JSON-RPC -> retained State channel -> production runtime mutation.
//! Synthetic home/duplex only; no native AppHandle, credentials or real provider.
use super::*;
use crate::acp_client::{test_transport, AcpClient, AcpEvent};
use crate::session_migration::{mcp_stdio::SessionMcpPolicy, test_home::IsolatedAppHome};
use crate::store;
use serde_json::{json, Value};
use std::fmt::Debug;
use std::future::Future;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROCESS: &str = "synthetic-state-process";
const AGENT: &str = "synthetic-state-owner";

fn home() -> IsolatedAppHome {
    let home = IsolatedAppHome::acquire("state-receipt");
    let mut settings = store::load_settings();
    settings.composer_prefs_scope = "session".into();
    store::save_settings(&settings).unwrap();
    home
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

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

fn live(row: &store::SessionMeta, client: &Arc<AcpClient>) -> LiveSession {
    let mut shell = routing_tests::bare_live_session(&row.id, PROCESS);
    shell.meta = row.clone();
    shell.model_id = row.model_id.clone();
    shell.acp = Some(client.clone());
    shell.backend = "before-state".into();
    shell
}

async fn drive<T, E: Debug>(
    peer: &mut tokio::io::DuplexStream,
    action: impl Future<Output = Result<T, E>>,
    mut result_for: impl FnMut(&Value) -> Result<Value, ()>,
) -> (T, Vec<Value>) {
    let (read, mut write) = tokio::io::split(peer);
    let mut lines = BufReader::new(read).lines();
    let mut action = Box::pin(action);
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut requests = Vec::new();
        loop {
            tokio::select! {
                result = &mut action => return (result.unwrap(), requests),
                line = lines.next_line() => {
                    let request: Value = serde_json::from_str(&line.unwrap().expect("ACP request")).unwrap();
                    let response = match result_for(&request) {
                        Ok(result) => json!({"jsonrpc":"2.0", "id":request["id"], "result":result}),
                        Err(()) => json!({"jsonrpc":"2.0", "id":request["id"],
                            "error":{"code":-32601,"message":"synthetic unavailable method"}}),
                    };
                    requests.push(request);
                    write.write_all(format!("{response}\n").as_bytes()).await.unwrap();
                    write.flush().await.unwrap();
                }
            }
        }
    }).await.expect("in-memory RPC must finish")
}

fn receipt(manager: &SessionManager, event: (Option<String>, AcpEvent)) -> Option<SessionSnapshot> {
    let (envelope, event) = event;
    let AcpEvent::State {
        backend,
        agent_session_id,
        model_id,
    } = event
    else {
        panic!("expected actual ACP State, got {event:?}");
    };
    manager.apply_state_event(
        PROCESS,
        envelope.as_deref(),
        agent_session_id.as_deref(),
        &backend,
        model_id.as_deref(),
    )
}

fn producer(kind: &str) {
    let _home = home();
    runtime().block_on(async {
        let (client, mut peer, mut events) = test_transport::client_with_events(
            false,
            SessionMcpPolicy::default(),
            Some("recent-foreign-agent"),
        );
        if kind.starts_with("model") {
            let (_, requests) = drive(&mut peer, client.set_model_for(AGENT, "grok-4.5"), |_| {
                Ok(match kind {
                    "model-nested" => json!({"models":{"currentModelId":"grok-4.5"}}),
                    "model-flat" => json!({"modelId":"grok-4.5"}),
                    _ => json!({}),
                })
            })
            .await;
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0]["params"]["sessionId"], AGENT);
        } else {
            let resume = (kind != "new").then_some("synthetic-source-agent");
            let fork = kind.starts_with("fork");
            let ((id, loaded), requests) = drive(
                &mut peer,
                test_transport::open_with_empty_mcp(&client, resume, fork),
                |request| {
                    let method = request["method"].as_str().unwrap();
                    if kind.ends_with("fallback") && method != "session/new" {
                        return Err(());
                    }
                    if kind == "fork-extension" && method == "session/fork" {
                        return Err(());
                    }
                    if method == "_x.ai/session/fork" {
                        return Ok(json!({"sessionId":AGENT, "newModelId":"grok-4.5"}));
                    }
                    Ok(json!({"sessionId":AGENT, "models":{"currentModelId":"grok-4.5"}}))
                },
            )
            .await;
            assert_eq!(id, AGENT);
            assert_eq!(
                loaded,
                !matches!(kind, "new" | "load-fallback" | "fork-fallback")
            );
            let expected_methods: &[&str] = match kind {
                "new" => &["session/new"],
                "load" => &["session/load"],
                "fork" => &["session/fork"],
                "load-fallback" => &["session/load", "session/new"],
                "fork-extension" => &["session/fork", "_x.ai/session/fork"],
                "fork-fallback" => &["session/fork", "_x.ai/session/fork", "session/new"],
                _ => unreachable!(),
            };
            assert_eq!(
                requests
                    .iter()
                    .map(|r| r["method"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                expected_methods
            );
            let cwd = crate::paths::general_workspace_dir()
                .to_string_lossy()
                .into_owned();
            for request in requests {
                let params = &request["params"];
                if request["method"] == "_x.ai/session/fork" {
                    assert_eq!(params["sourceSessionId"], "synthetic-source-agent");
                    assert_eq!(params["sourceCwd"], cwd);
                    assert_eq!(params["newCwd"], cwd);
                    assert!(params.get("mcpServers").is_none());
                } else {
                    assert_eq!(params["mcpServers"], json!([]));
                    assert_eq!(params["cwd"], cwd);
                    if request["method"] != "session/new" {
                        assert_eq!(params["sessionId"], "synthetic-source-agent");
                    }
                }
            }
        }
        let (stamp, event) = events.try_recv().expect("actual State producer");
        assert_eq!(stamp.as_deref(), Some(AGENT), "{kind}: owner stamp missing");
        let AcpEvent::State {
            agent_session_id,
            model_id,
            ..
        } = event
        else {
            panic!("expected State");
        };
        assert_eq!(agent_session_id.as_deref(), Some(AGENT));
        assert_eq!(model_id.as_deref(), Some("grok-4.5"));
    });
}

macro_rules! producer_test {
    ($name:ident, $kind:literal) => {
        #[test]
        fn $name() {
            producer($kind);
        }
    };
}
producer_test!(model_nested_echo_is_owner_stamped, "model-nested");
producer_test!(model_flat_echo_is_owner_stamped, "model-flat");
producer_test!(model_no_echo_is_owner_stamped, "model-noecho");
producer_test!(new_receipt_is_owner_stamped, "new");
producer_test!(load_receipt_is_owner_stamped, "load");
producer_test!(fork_receipt_is_owner_stamped, "fork");
producer_test!(fork_extension_receipt_is_owner_stamped, "fork-extension");
producer_test!(load_fallback_new_is_owner_stamped, "load-fallback");
producer_test!(fork_fallback_new_is_owner_stamped, "fork-fallback");

#[test]
fn queued_old_wire_model_does_not_replace_latest_manager_choice() {
    let _home = home();
    runtime().block_on(async {
        let (client, mut peer, mut events) = test_transport::client_with_events(
            false,
            SessionMcpPolicy::default(),
            Some("recent-foreign-agent"),
        );
        let owner = row(AGENT);
        let manager = SessionManager::new();
        *manager.inner.lock() = Some(live(&owner, &client));
        drive(&mut peer, client.set_model_for(AGENT, "grok-4.6"), |_| {
            Ok(json!({}))
        })
        .await;
        let old = events.try_recv().unwrap();
        store::save_composer_prefs(
            None,
            Some(&owner.id),
            Some("grok-4.5".into()),
            None,
            None,
            None,
        )
        .unwrap();
        drive(
            &mut peer,
            manager.set_model("grok-4.5".into(), Some(&owner.id)),
            |_| Ok(json!({})),
        )
        .await;
        let new = events.try_recv().unwrap();
        for event in [new, old] {
            let snapshot = receipt(&manager, event).expect("same owner State accepted");
            assert_eq!(snapshot.session_id.as_deref(), Some(owner.id.as_str()));
            assert_eq!(snapshot.model_id.as_deref(), Some("grok-4.5"));
        }
        assert_eq!(
            workspace_context::session(&owner.id)
                .unwrap()
                .model_id
                .as_deref(),
            Some("grok-4.5")
        );
    });
}

#[test]
fn generic_alias_wire_receipt_preserves_composer_catalog_id() {
    let _home = home();
    runtime().block_on(async {
        let (client, mut peer, mut events) = test_transport::client_with_events(
            true,
            SessionMcpPolicy::default(),
            Some("recent-foreign-agent"),
        );
        let owner = row(AGENT);
        let manager = SessionManager::new();
        *manager.inner.lock() = Some(live(&owner, &client));
        let (_, requests) = drive(
            &mut peer,
            manager.set_model("grok-4.6".into(), Some(&owner.id)),
            |request| Ok(json!({"models":{"currentModelId":request["params"]["modelId"]}})),
        )
        .await;
        assert_eq!(requests[0]["params"]["modelId"], "synthetic-history-route");
        let snapshot = receipt(&manager, events.try_recv().unwrap()).unwrap();
        assert_eq!(snapshot.model_id, owner.model_id);
        assert_eq!(
            manager.inner.lock().as_ref().unwrap().meta.model_id,
            owner.model_id
        );
    });
}

fn invalid(kind: &str) {
    let home = home();
    runtime().block_on(async {
        let (client, _peer, _) =
            test_transport::client_with_events(false, SessionMcpPolicy::default(), None);
        let mut owner = row(AGENT);
        if kind == "directory" {
            let dir = home.path.join("project");
            std::fs::create_dir(&dir).unwrap();
            owner.project_id = Some(
                store::add_project(dir.to_string_lossy().into_owned(), true)
                    .unwrap()
                    .id,
            );
            store::update_session_meta(&owner).unwrap();
        }
        let manager = SessionManager::new();
        *manager.inner.lock() = Some(live(&owner, &client));
        match kind {
            "disk-cleared" => {
                store::update_sessions_index_strict(|rows| {
                    rows.iter_mut()
                        .find(|r| r.id == owner.id)
                        .unwrap()
                        .agent_session_id = None;
                    Ok(())
                })
                .unwrap();
            }
            "memory-cleared" => manager.inner.lock().as_mut().unwrap().meta.agent_session_id = None,
            "new-agent" => {
                manager.inner.lock().as_mut().unwrap().meta.agent_session_id =
                    Some("new-agent".into())
            }
            "deleted" => {
                store::delete_session(&owner.id).unwrap();
            }
            "directory" => {
                store::set_session_project(&owner.id, None).unwrap();
            }
            "corrupt" => std::fs::write(crate::paths::sessions_index_file(), b"{broken").unwrap(),
            "ambiguous" => {
                row(AGENT);
            }
            _ => {}
        }
        let before = manager.snapshot();
        let disk = std::fs::read(crate::paths::sessions_index_file()).unwrap();
        let envelope = match kind {
            "wrong-envelope" => Some("other-agent"),
            _ => None,
        };
        let payload = match kind {
            "missing-payload" => None,
            "unknown" => Some("unknown-agent"),
            "blank" => Some(" "),
            _ => Some(AGENT),
        };
        let process = if kind == "wrong-process" {
            "other-process"
        } else {
            PROCESS
        };
        assert!(
            manager
                .apply_state_event(
                    process,
                    envelope,
                    payload,
                    "received-state",
                    Some("wire-old-model")
                )
                .is_none(),
            "{kind}: invalid State accepted"
        );
        let after = manager.snapshot();
        assert_eq!(after.backend, before.backend, "{kind}: backend changed");
        assert_eq!(
            after.agent_session_id, before.agent_session_id,
            "{kind}: identity changed"
        );
        assert_eq!(after.model_id, before.model_id, "{kind}: model changed");
        assert_eq!(
            std::fs::read(crate::paths::sessions_index_file()).unwrap(),
            disk,
            "{kind}: strict receipt rewrote store"
        );
    });
}

macro_rules! invalid_test {
    ($name:ident, $kind:literal) => {
        #[test]
        fn $name() {
            invalid($kind);
        }
    };
}
invalid_test!(cleared_disk_identity_is_not_revived, "disk-cleared");
invalid_test!(cleared_memory_identity_is_not_revived, "memory-cleared");
invalid_test!(same_process_new_agent_rejects_old_receipt, "new-agent");
invalid_test!(deleted_session_is_not_revived, "deleted");
invalid_test!(directory_rebind_rejects_old_receipt, "directory");
invalid_test!(corrupt_index_is_not_quarantined_or_rewritten, "corrupt");
invalid_test!(ambiguous_saved_agent_identity_is_rejected, "ambiguous");
invalid_test!(envelope_payload_disagreement_is_rejected, "wrong-envelope");
invalid_test!(
    state_without_payload_identity_is_rejected,
    "missing-payload"
);
invalid_test!(unknown_agent_is_not_adopted, "unknown");
invalid_test!(blank_identity_is_not_adopted, "blank");
invalid_test!(wrong_process_is_rejected, "wrong-process");

#[test]
fn background_wire_receipt_updates_only_its_owner() {
    let _home = home();
    runtime().block_on(async {
        let (client, mut peer, mut events) =
            test_transport::client_with_events(false, SessionMcpPolicy::default(), None);
        let owner = row(AGENT);
        let foreign = row("foreign-state-agent");
        let manager = SessionManager::new();
        *manager.inner.lock() = Some(live(&foreign, &client));
        manager
            .background
            .lock()
            .insert(owner.id.clone(), live(&owner, &client));
        drive(
            &mut peer,
            client.set_model_for(AGENT, "wire-old-model"),
            |_| Ok(json!({})),
        )
        .await;
        let snapshot =
            receipt(&manager, events.try_recv().unwrap()).expect("background State handled");
        assert_eq!(snapshot.session_id.as_deref(), Some(owner.id.as_str()));
        assert_eq!(snapshot.backend, "grok_agent_stdio");
        assert_eq!(snapshot.model_id, owner.model_id);
        let focus = manager.snapshot();
        assert_eq!(focus.session_id.as_deref(), Some(foreign.id.as_str()));
        assert_eq!(focus.agent_session_id, foreign.agent_session_id);
        assert_eq!(focus.backend, "before-state");
        assert_eq!(focus.model_id, foreign.model_id);
    });
}

fn boundary(kind: &str) {
    let home = home();
    runtime().block_on(async {
        let (client, _peer) = test_transport::client_for(false, SessionMcpPolicy::default(), None);
        let (replacement, _replacement_peer) =
            test_transport::client_for(false, SessionMcpPolicy::default(), None);
        let mut owner = row(AGENT);
        let foreign = row("foreign-state-agent");
        if kind == "directory" {
            let dir = home.path.join("project");
            std::fs::create_dir(&dir).unwrap();
            owner.project_id = Some(
                store::add_project(dir.to_string_lossy().into_owned(), true)
                    .unwrap()
                    .id,
            );
            store::update_session_meta(&owner).unwrap();
        }
        let manager = SessionManager::new();
        *manager.inner.lock() = Some(live(&owner, &client));
        let captured = manager
            .capture_state_owner(PROCESS, Some(AGENT), Some(AGENT))
            .unwrap();
        match kind {
            "demote" => {
                let shell = manager.inner.lock().take().unwrap();
                manager.background.lock().insert(owner.id.clone(), shell);
                *manager.inner.lock() = Some(live(&foreign, &client));
            }
            "replacement" => manager.inner.lock().as_mut().unwrap().acp = Some(replacement),
            "process" => {
                manager.inner.lock().as_mut().unwrap().process_id = "replacement-process".into()
            }
            "agent" => {
                manager.inner.lock().as_mut().unwrap().meta.agent_session_id =
                    Some("replacement-agent".into())
            }
            "clear-memory" => manager.inner.lock().as_mut().unwrap().meta.agent_session_id = None,
            "clear-disk" => {
                store::update_sessions_index_strict(|rows| {
                    rows.iter_mut()
                        .find(|r| r.id == owner.id)
                        .unwrap()
                        .agent_session_id = None;
                    Ok(())
                })
                .unwrap();
            }
            "deleted" => {
                store::delete_session(&owner.id).unwrap();
            }
            "directory" => {
                store::set_session_project(&owner.id, None).unwrap();
            }
            "corrupt" => std::fs::write(crate::paths::sessions_index_file(), b"{broken").unwrap(),
            "parked" => {
                manager.try_park_live().unwrap();
            }
            _ => unreachable!(),
        }
        let disk = std::fs::read(crate::paths::sessions_index_file()).unwrap();
        let applied = manager.apply_state_owner(&captured, "received-state");
        if kind == "demote" {
            let snapshot = applied.expect("captured owner can demote without losing receipt");
            assert_eq!(snapshot.session_id.as_deref(), Some(owner.id.as_str()));
            assert_eq!(snapshot.model_id, owner.model_id);
            assert_eq!(snapshot.backend, "received-state");
            assert_eq!(
                manager.snapshot().session_id.as_deref(),
                Some(foreign.id.as_str())
            );
            assert_eq!(manager.snapshot().backend, "before-state");
        } else {
            assert!(
                applied.is_none(),
                "{kind}: obsolete captured owner accepted"
            );
            manager.with_session_mut(&owner.id, |s| assert_eq!(s.backend, "before-state"));
        }
        assert_eq!(
            std::fs::read(crate::paths::sessions_index_file()).unwrap(),
            disk
        );
    });
}

macro_rules! boundary_test {
    ($name:ident, $kind:literal) => {
        #[test]
        fn $name() {
            boundary($kind);
        }
    };
}
boundary_test!(captured_live_owner_can_demote_before_mutation, "demote");
boundary_test!(
    replaced_client_same_process_cannot_receive_old_captured_state,
    "replacement"
);
boundary_test!(changed_process_cannot_receive_old_captured_state, "process");
boundary_test!(changed_agent_cannot_receive_old_captured_state, "agent");
boundary_test!(
    memory_identity_cleared_after_capture_stays_cleared,
    "clear-memory"
);
boundary_test!(
    disk_identity_cleared_after_capture_stays_cleared,
    "clear-disk"
);
boundary_test!(deletion_after_capture_rejects_mutation, "deleted");
boundary_test!(directory_rebind_after_capture_rejects_mutation, "directory");
boundary_test!(
    corruption_after_capture_rejects_mutation_without_repair,
    "corrupt"
);
boundary_test!(parked_owner_is_not_rescued_by_state_receipt, "parked");

#[test]
fn queued_model_state_after_real_effort_alignment_cannot_restore_identity() {
    let _home = home();
    runtime().block_on(async {
        let (client, mut peer, mut events) = test_transport::client_with_events(
            false,
            SessionMcpPolicy::default(),
            Some("recent-foreign-agent"),
        );
        let owner = row(AGENT);
        let manager = SessionManager::new();
        *manager.inner.lock() = Some(live(&owner, &client));
        let starting = store::resolve_composer_prefs(None, Some(&owner.id));
        store::save_composer_prefs(
            None,
            Some(&owner.id),
            None,
            Some("medium".into()),
            None,
            None,
        )
        .unwrap();
        drive(
            &mut peer,
            manager.align_connected_composer(&owner.id, &starting, false),
            |_| Ok(json!({})),
        )
        .await;
        assert!(manager.snapshot().agent_session_id.is_none());
        assert!(workspace_context::session(&owner.id)
            .unwrap()
            .agent_session_id
            .is_none());
        let pending = events
            .try_recv()
            .expect("real model alignment queued State");
        assert!(receipt(&manager, pending).is_none());
        assert!(manager.snapshot().agent_session_id.is_none());
        let current = workspace_context::session(&owner.id).unwrap();
        assert_eq!(current.effort.as_deref(), Some("medium"));
        assert!(current.agent_session_id.is_none());
        assert!(manager.pending_soft_respawn.lock().contains_key(&owner.id));
    });
}
