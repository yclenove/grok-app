//! Actual context commands on synthetic store rows and an in-memory ACP wire.
use super::*;
use crate::acp_client::test_transport;
use crate::session_migration::{mcp_stdio::SessionMcpPolicy, test_home::IsolatedAppHome};
use crate::store;
use serde_json::{json, Value};
use std::{future::Future, time::Duration};
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
    let mut live = routing_tests::bare_live_session(&row.id, "synthetic-context-process");
    live.meta = row.clone();
    live.model_id = row.model_id.clone();
    live.effort = row.effort.clone();
    live.product_mode = row.mode.clone();
    live.acp = Some(client.clone());
    live
}

fn window() -> u64 {
    crate::models_catalog::list_available_models()
        .models
        .iter()
        .find(|model| model.id == "grok-4.6")
        .and_then(crate::models_catalog::app_default_context_window)
        .unwrap()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

async fn drive(
    peer: tokio::io::DuplexStream,
    operation: impl Future<Output = Result<Value, String>>,
    mut response: impl FnMut(&Value) -> Value,
) -> (Result<Value, String>, Vec<Value>) {
    let (read, mut write) = tokio::io::split(peer);
    let mut lines = BufReader::new(read).lines();
    let mut operation = Box::pin(operation);
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut requests = Vec::new();
        loop {
            tokio::select! {
                result = &mut operation => return (result, requests),
                line = lines.next_line() => {
                    let request: Value = serde_json::from_str(&line.unwrap().expect("ACP request")).unwrap();
                    let payload = response(&request);
                    requests.push(request);
                    write.write_all(format!("{payload}\n").as_bytes()).await.unwrap();
                    write.flush().await.unwrap();
                }
            }
        }
    }).await.expect("synthetic context command must finish")
}

#[test]
fn background_context_targets_exact_owner_and_preserves_another_draft() {
    let _home = IsolatedAppHome::acquire("context-target");
    runtime().block_on(async {
        let (client, peer) = test_transport::client_for(
            false,
            SessionMcpPolicy::default(),
            Some("foreign-recent-agent"),
        );
        let a = row("synthetic-agent-a");
        let b = row("synthetic-agent-b");
        let manager = SessionManager::new();
        *manager.inner.lock() = Some(live(&a, &client));
        manager
            .background
            .lock()
            .insert(b.id.clone(), live(&b, &client));
        let tokens = window();
        *manager.pending_context_window.lock() = Some(tokens);
        let (result, requests) = drive(
            peer,
            async {
                manager
                    .set_context_window(tokens, Some(&b.id))
                    .await
                    .map(|_| json!({}))
            },
            |request| json!({"jsonrpc":"2.0","id":request["id"],"result":{}}),
        )
        .await;
        result.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["method"], "session/set_model");
        assert_eq!(requests[0]["params"]["sessionId"], "synthetic-agent-b");
        assert_eq!(requests[0]["params"]["_meta"]["contextWindow"], tokens);
        assert_eq!(*manager.pending_context_window.lock(), Some(tokens));
        assert_eq!(
            manager
                .inner
                .lock()
                .as_ref()
                .unwrap()
                .meta
                .agent_session_id
                .as_deref(),
            Some("synthetic-agent-a")
        );
    });
}

#[test]
fn context_ack_after_directory_rebind_is_not_reported_as_current_success() {
    let home = IsolatedAppHome::acquire("context-rebind");
    runtime().block_on(async {
        let (client, peer) = test_transport::client_for(false, SessionMcpPolicy::default(), None);
        let a = row("synthetic-agent-a");
        let target_dir = home.path.join("new-project");
        std::fs::create_dir(&target_dir).unwrap();
        let target_project =
            store::add_project(target_dir.to_string_lossy().into_owned(), true).unwrap();
        let manager = SessionManager::new();
        *manager.inner.lock() = Some(live(&a, &client));
        let (result, requests) = drive(
            peer,
            async {
                manager
                    .set_context_window(window(), Some(&a.id))
                    .await
                    .map(|_| json!({}))
            },
            |request| {
                store::update_sessions_index_strict(|rows| {
                    rows.iter_mut()
                        .find(|row| row.id == a.id)
                        .unwrap()
                        .project_id = Some(target_project.id.clone());
                    Ok(())
                })
                .unwrap();
                json!({"jsonrpc":"2.0","id":request["id"],"result":{}})
            },
        )
        .await;
        assert!(result.unwrap_err().contains("binding changed"));
        assert_eq!(requests.len(), 1);
    });
}

#[test]
fn session_context_info_keeps_explicit_owner_on_old_method_fallback() {
    let _home = IsolatedAppHome::acquire("context-info");
    runtime().block_on(async {
        let (client, peer) = test_transport::client_for(false, SessionMcpPolicy::default(), Some("foreign-recent-agent"));
        let a = row("synthetic-agent-a");
        let manager = SessionManager::new();
        *manager.inner.lock() = Some(live(&a, &client));
        let (result, requests) = drive(peer, manager.session_context_info(Some(&a.id)), |request| {
            if request["method"] == "_x.ai/session/info" {
                json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32601,"message":"Method not found"}})
            } else {
                json!({"jsonrpc":"2.0","id":request["id"],"result":{"sessionId":"synthetic-agent-a","context":{"used":1,"total":2}}})
            }
        }).await;
        assert_eq!(result.unwrap()["context"]["used"], 1);
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["method"], "x.ai/session/info");
        for request in requests { assert_eq!(request["params"]["sessionId"], "synthetic-agent-a"); }
    });
}

#[test]
fn fresh_empty_alignment_applies_window_but_resume_does_not() {
    for fresh in [true, false] {
        let _home = IsolatedAppHome::acquire("context-open");
        runtime().block_on(async {
            let (client, peer) =
                test_transport::client_for(false, SessionMcpPolicy::default(), None);
            let a = row("synthetic-agent-a");
            let manager = SessionManager::new();
            *manager.inner.lock() = Some(live(&a, &client));
            let tokens = window();
            *manager.pending_context_window.lock() = Some(tokens);
            let prefs = store::resolve_composer_prefs(None, Some(&a.id));
            let (result, requests) = drive(
                peer,
                async {
                    manager
                        .align_opened_composer(&a.id, &prefs, false, fresh)
                        .await
                        .map(|_| json!({}))
                },
                |request| json!({"jsonrpc":"2.0","id":request["id"],"result":{}}),
            )
            .await;
            result.unwrap();
            let models: Vec<_> = requests
                .iter()
                .filter(|request| request["method"] == "session/set_model")
                .collect();
            assert_eq!(models.len(), 1);
            assert_eq!(models[0]["params"]["sessionId"], "synthetic-agent-a");
            assert_eq!(
                models[0]["params"]["_meta"]["contextWindow"].as_u64(),
                fresh.then_some(tokens)
            );
            assert_eq!(
                *manager.pending_context_window.lock(),
                if fresh { None } else { Some(tokens) }
            );
        });
    }
}
