//! Save real preferences, invoke the real manager, and observe actual ACP lines.
//! Test-only transport/metadata; no CLI, account, SSH or provider is contacted.
use super::*;
use crate::acp_client::test_transport;
use crate::permission::PermissionPolicy;
use crate::session_migration::{mcp_stdio::SessionMcpPolicy, test_home::IsolatedAppHome};
use crate::store;
use serde_json::{json, Value};
use std::future::{poll_fn, Future};
use std::task::Poll;
use std::time::{Duration, Instant};
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
    let mut live = routing_tests::bare_live_session(&row.id, "synthetic-shared-process");
    live.meta = row.clone();
    live.model_id = row.model_id.clone();
    live.effort = row.effort.clone();
    live.product_mode = row.mode.clone();
    live.acp = Some(client.clone());
    live
}

fn parked(row: &store::SessionMeta, client: &Arc<crate::acp_client::AcpClient>) -> ParkedAgent {
    ParkedAgent {
        turn_open_epoch: 0,
        workspace_context: None,
        process_id: "synthetic-shared-process".into(),
        app_session_id: row.id.clone(),
        meta: row.clone(),
        acp: client.clone(),
        last_activity: Instant::now(),
        model_id: row.model_id.clone(),
        effort: row.effort.clone(),
        product_mode: row.mode.clone(),
        project_path: None,
        policy: PermissionPolicy::Ask,
        sandbox_profile: None,
        needs_history_bootstrap: false,
        backend: "synthetic-acp".into(),
    }
}

async fn drive(
    peer: tokio::io::DuplexStream,
    update: impl Future<Output = Result<(), String>>,
) -> Vec<Value> {
    let (read, mut write) = tokio::io::split(peer);
    let mut lines = BufReader::new(read).lines();
    let mut update = Box::pin(update);
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut requests = Vec::new();
        loop {
            tokio::select! {
                result = &mut update => { result.unwrap(); return requests; }
                line = lines.next_line() => {
                    let request: Value = serde_json::from_str(&line.unwrap().expect("ACP line")).unwrap();
                    assert_eq!(request["method"], "session/set_model");
                    let response = json!({"jsonrpc":"2.0", "id":request["id"],
                        "result":{"models":{"currentModelId": request["params"]["modelId"]}}});
                    requests.push(request);
                    write.write_all(format!("{response}\n").as_bytes()).await.unwrap();
                    write.flush().await.unwrap();
                }
            }
        }
    }).await.expect("in-memory model command must finish")
}

fn home() -> IsolatedAppHome {
    home_with_scope("session")
}

fn home_with_scope(scope: &str) -> IsolatedAppHome {
    let home = IsolatedAppHome::acquire("manager-model-target");
    let mut settings = store::load_settings();
    settings.composer_prefs_scope = scope.into();
    store::save_settings(&settings).unwrap();
    home
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn assert_target(kind: &str, scope: &str) {
    let _home = home_with_scope(scope);
    runtime().block_on(async {
        let (client, peer) = test_transport::client_for(
            false,
            SessionMcpPolicy::default(),
            Some("recent-foreign-agent"),
        );
        let a = row("synthetic-agent-a");
        let mut b = row("synthetic-agent-b");
        let manager = SessionManager::new();
        *manager.inner.lock() = Some(live(&a, &client));
        match kind {
            "background" => {
                manager
                    .background
                    .lock()
                    .insert(b.id.clone(), live(&b, &client));
            }
            "parked" => {
                manager
                    .parked
                    .lock()
                    .insert(b.id.clone(), parked(&b, &client));
            }
            "connecting" => {
                b.agent_session_id = None;
                store::update_session_meta(&b).unwrap();
                let mut shell = live(&b, &client);
                shell.fsm.soft_disconnect();
                shell.fsm.start_connect().unwrap();
                manager.background.lock().insert(b.id.clone(), shell);
            }
            "idle" | "draft" | "unknown" => {}
            _ => unreachable!(),
        }
        let target = match kind {
            "draft" => None,
            "unknown" => Some("synthetic-unknown-chat"),
            _ => Some(b.id.as_str()),
        };
        let defaults_before = std::fs::read(crate::paths::settings_file()).unwrap();
        let saved =
            store::save_composer_prefs(None, target, Some("grok-4.5".into()), None, None, None);
        if kind == "unknown" {
            assert!(saved.unwrap_err().contains("session not found"));
        } else {
            saved.unwrap();
        }
        if kind != "draft" {
            assert_eq!(
                std::fs::read(crate::paths::settings_file()).unwrap(),
                defaults_before
            );
        }
        if !matches!(kind, "draft" | "unknown") {
            assert_eq!(
                store::resolve_composer_prefs(None, target).model_id,
                "grok-4.5"
            );
        }
        // The first red exercised the discarded target. Keep the same assertion
        // now that production passes the saved App-session target explicitly.
        let requests = drive(peer, manager.set_model("grok-4.5".into(), target)).await;
        assert_eq!(
            manager.inner.lock().as_ref().unwrap().model_id,
            a.model_id,
            "{kind}: A live changed"
        );
        let stored_a = store::load_sessions_snapshot()
            .unwrap()
            .into_iter()
            .find(|row| row.id == a.id)
            .unwrap();
        assert_eq!(stored_a.model_id, a.model_id, "{kind}: A disk changed");
        if matches!(kind, "background" | "parked") {
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0]["params"]["sessionId"], "synthetic-agent-b");
            assert_eq!(requests[0]["params"]["modelId"], "grok-4.5");
        } else {
            assert!(
                requests.is_empty(),
                "{kind}: foreign wire request {requests:?}"
            );
        }
    });
}

macro_rules! target_test {
    ($name:ident, $kind:literal) => {
        #[test]
        fn $name() {
            assert_target($kind, "session");
        }
    };
}
target_test!(idle_target_does_not_change_active_chat, "idle");
target_test!(background_target_does_not_change_active_chat, "background");
target_test!(parked_target_does_not_change_active_chat, "parked");
target_test!(connecting_target_does_not_change_active_chat, "connecting");
target_test!(draft_target_does_not_change_active_chat, "draft");
target_test!(unknown_target_does_not_change_active_chat, "unknown");

macro_rules! scoped_target_test {
    ($name:ident, $kind:literal, $scope:literal) => {
        #[test]
        fn $name() {
            assert_target($kind, $scope);
        }
    };
}
scoped_target_test!(global_idle_target, "idle", "global");
scoped_target_test!(global_background_target, "background", "global");
scoped_target_test!(global_parked_target, "parked", "global");
scoped_target_test!(global_connecting_target, "connecting", "global");
scoped_target_test!(global_draft_target, "draft", "global");
scoped_target_test!(global_unknown_target, "unknown", "global");
scoped_target_test!(project_idle_target, "idle", "project");
scoped_target_test!(project_background_target, "background", "project");
scoped_target_test!(project_parked_target, "parked", "project");
scoped_target_test!(project_connecting_target, "connecting", "project");
scoped_target_test!(project_draft_target, "draft", "project");
scoped_target_test!(project_unknown_target, "unknown", "project");

fn assert_active_choice(ssh: bool, relay_mode: Option<&str>, stale_argument: bool) {
    let _home = home();
    if let Some(mode) = relay_mode {
        std::fs::create_dir_all(crate::paths::agent_home_dir()).unwrap();
        std::fs::write(crate::paths::agent_config_toml(), format!(
            "[models]\ndefault = \"synthetic-relay\"\n[model.synthetic-relay]\nmodel = \"grok-4.6\"\nbase_url = \"https://synthetic.invalid/v1\"\napi_key = \"synthetic-not-a-credential\"\napi_backend = \"responses\"\napp_provider_mode = \"{mode}\"\napp_models = '[{{\"id\":\"grok-4.6\",\"name\":\"Synthetic 4.6\"}},{{\"id\":\"grok-4.5\",\"name\":\"Synthetic 4.5\"}}]'\n"
        )).unwrap();
    }
    runtime().block_on(async {
        let (client, peer) = test_transport::client_for_current_route(
            relay_mode.is_some() && !ssh,
            SessionMcpPolicy {
                ssh,
                ..Default::default()
            },
            Some("recent-foreign-agent"),
        );
        let a = row("synthetic-agent-a");
        let manager = SessionManager::new();
        *manager.inner.lock() = Some(live(&a, &client));
        store::save_composer_prefs(None, Some(&a.id), Some("grok-4.5".into()), None, None, None)
            .unwrap();
        let argument = if stale_argument {
            "grok-4.6"
        } else {
            "grok-4.5"
        };
        let requests = drive(peer, manager.set_model(argument.into(), Some(&a.id))).await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["params"]["sessionId"], "synthetic-agent-a");
        assert_eq!(requests[0]["params"]["modelId"], "grok-4.5");
        assert_eq!(
            manager.inner.lock().as_ref().unwrap().model_id.as_deref(),
            Some("grok-4.5")
        );
        assert_eq!(
            store::resolve_composer_prefs(None, Some(&a.id)).model_id,
            "grok-4.5"
        );
    });
}

#[test]
fn active_wire_uses_explicit_owner_not_recent_agent() {
    assert_active_choice(false, None, false);
}
#[test]
fn later_saved_intent_wins_over_queued_old_argument() {
    assert_active_choice(false, None, true);
}
#[test]
fn ssh_wire_does_not_use_local_relay_alias() {
    assert_active_choice(true, Some("generic"), false);
}
#[test]
fn native_non_default_choice_reaches_actual_manager_wire() {
    assert_active_choice(false, Some("grok_build_proxy"), false);
}

#[test]
fn model_application_waits_for_in_flight_connect() {
    let _home = home();
    runtime().block_on(async {
        let (client, _peer) = test_transport::client_for(false, SessionMcpPolicy::default(), None);
        let a = row("synthetic-agent-a");
        let manager = SessionManager::new();
        *manager.inner.lock() = Some(live(&a, &client));
        store::save_composer_prefs(None, Some(&a.id), Some("grok-4.5".into()), None, None, None)
            .unwrap();
        let _connect = manager.connect_lock.lock().await;
        let mut update = Box::pin(manager.set_model("grok-4.5".into(), Some(&a.id)));
        let pending = poll_fn(|cx| Poll::Ready(update.as_mut().poll(cx).is_pending())).await;
        assert!(pending);
        assert_eq!(manager.inner.lock().as_ref().unwrap().model_id, a.model_id);
    });
}

fn relay_config(id: &str, mode: &str) {
    std::fs::create_dir_all(crate::paths::agent_home_dir()).unwrap();
    std::fs::write(crate::paths::agent_config_toml(), format!(
        "[models]\ndefault = \"{id}\"\n[model.{id}]\nmodel = \"grok-4.6\"\nbase_url = \"https://synthetic.invalid/v1\"\napi_key = \"synthetic-not-a-credential\"\napi_backend = \"responses\"\napp_provider_mode = \"{mode}\"\napp_models = '[{{\"id\":\"grok-4.6\",\"name\":\"Synthetic 4.6\"}},{{\"id\":\"grok-4.5\",\"name\":\"Synthetic 4.5\"}}]'\n"
    )).unwrap();
}

fn assert_bound_route(custom: bool, mode: &str, policy: SessionMcpPolicy, expected: &str) {
    let _home = home();
    relay_config("synthetic-bound-route", mode);
    if !custom && !policy.official_aux && !policy.tcp && !policy.ssh {
        // A real local official process is initialized while the default is
        // official. Keep the relay catalog installed, but activate it only
        // AFTER capture below; a contradictory spawn class must fail closed.
        let path = crate::paths::agent_config_toml();
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("default = \"synthetic-bound-route\"", "default = \"grok\"");
        std::fs::write(path, text).unwrap();
    }
    runtime().block_on(async {
        let (client, peer) =
            test_transport::client_for_current_route(custom, policy, Some("recent-foreign-agent"));
        let a = row("synthetic-agent-a");
        let manager = SessionManager::new();
        *manager.inner.lock() = Some(live(&a, &client));
        // Another composer/default changed after this process was initialized.
        relay_config("synthetic-foreign-route", "generic");
        store::save_composer_prefs(None, Some(&a.id), Some("grok-4.5".into()), None, None, None)
            .unwrap();
        let requests = drive(peer, manager.set_model("grok-4.5".into(), Some(&a.id))).await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["params"]["sessionId"], "synthetic-agent-a");
        assert_eq!(requests[0]["params"]["modelId"], expected);
    });
}

#[test]
fn generic_process_keeps_its_bound_provider_after_default_changes() {
    assert_bound_route(
        true,
        "generic",
        SessionMcpPolicy::default(),
        "synthetic-bound-route",
    );
}

#[test]
fn native_process_keeps_real_model_after_default_changes() {
    assert_bound_route(
        true,
        "grok_build_proxy",
        SessionMcpPolicy::default(),
        "grok-4.5",
    );
}

#[test]
fn official_process_does_not_switch_to_a_later_local_relay() {
    assert_bound_route(false, "generic", SessionMcpPolicy::default(), "grok-4.5");
}

#[test]
fn official_aux_override_process_ignores_local_relay_default() {
    assert_bound_route(
        false,
        "generic",
        SessionMcpPolicy {
            official_aux: true,
            ..Default::default()
        },
        "grok-4.5",
    );
}

#[test]
fn tcp_process_uses_remote_catalog_not_local_relay_alias() {
    assert_bound_route(
        false,
        "generic",
        SessionMcpPolicy {
            tcp: true,
            ..Default::default()
        },
        "grok-4.5",
    );
}
