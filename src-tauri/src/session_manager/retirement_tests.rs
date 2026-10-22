//! Exercises the production retirement future with controlled thread interleaving.
//! No AppHandle/WebView, child process, provider or credential access.
use super::*;
use crate::acp_client::test_transport;
use crate::session_migration::mcp_stdio::SessionMcpPolicy;
use std::sync::{mpsc, Mutex as StdMutex};
use std::time::Duration;

struct Pause {
    reached: mpsc::SyncSender<()>,
    proceed: mpsc::Receiver<()>,
}

fn pauses() -> &'static StdMutex<HashMap<String, Pause>> {
    static PAUSES: OnceLock<StdMutex<HashMap<String, Pause>>> = OnceLock::new();
    PAUSES.get_or_init(|| StdMutex::new(HashMap::new()))
}

pub(super) fn pause_before_retirement(session_id: &str) {
    let pause = pauses().lock().unwrap().remove(session_id);
    if let Some(pause) = pause {
        pause.reached.send(()).unwrap();
        pause.proceed.recv_timeout(Duration::from_secs(4)).unwrap();
    }
}

pub(super) fn pause_after_detach(session_id: &str) {
    pause_before_retirement(&format!("{session_id}:detached"));
}

fn focus_switch_during_retirement(target_becomes_busy: bool, newer_reason: bool) {
    let _home = crate::session_migration::test_home::IsolatedAppHome::acquire_with_user_home(
        "session-retirement-focus",
    );
    let transport_runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _entered = transport_runtime.enter();
    let id = uuid::Uuid::new_v4().to_string();
    let manager = Arc::new(SessionManager::new());
    let (target, _target_peer) =
        test_transport::client_for(false, SessionMcpPolicy::default(), None);
    let (other, _other_peer) = test_transport::client_for(false, SessionMcpPolicy::default(), None);
    let mut live = routing_tests::bare_live_session(&id, "target-process");
    live.acp = Some(target.clone());
    *manager.inner.lock() = Some(live);
    manager
        .pending_soft_respawn
        .lock()
        .insert(id.clone(), "extensions_mcp".into());
    let (reached_tx, reached_rx) = mpsc::sync_channel(1);
    let (proceed_tx, proceed_rx) = mpsc::sync_channel(1);
    pauses().lock().unwrap().insert(
        id.clone(),
        Pause {
            reached: reached_tx,
            proceed: proceed_rx,
        },
    );
    let worker_manager = manager.clone();
    let worker_id = id.clone();
    let worker = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(worker_manager.flush_pending_soft_respawn_core(None, &worker_id));
    });
    reached_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    let mut target_session = manager.inner.lock().take().unwrap();
    if target_becomes_busy {
        target_session.fsm.begin_stream().unwrap();
        target_session.prompt_in_flight = true;
    }
    manager.background.lock().insert(id.clone(), target_session);
    let mut other_session = routing_tests::bare_live_session("other-chat", "other-process");
    other_session.acp = Some(other.clone());
    *manager.inner.lock() = Some(other_session);
    if newer_reason {
        manager
            .pending_soft_respawn
            .lock()
            .insert(id.clone(), "effort".into());
    }
    proceed_tx.send(()).unwrap();
    worker.join().unwrap();
    assert!(
        other.is_alive(),
        "retirement killed the newly focused, unrelated process"
    );
    assert!(manager
        .inner
        .lock()
        .as_ref()
        .unwrap()
        .acp
        .as_ref()
        .is_some_and(|client| Arc::ptr_eq(client, &other)));
    if target_becomes_busy {
        assert!(target.is_alive());
        assert!(manager.background.lock().contains_key(&id));
        assert!(manager.pending_soft_respawn.lock().contains_key(&id));
        assert_eq!(
            manager
                .pending_soft_respawn
                .lock()
                .get(&id)
                .map(String::as_str),
            Some(if newer_reason {
                "effort"
            } else {
                "extensions_mcp"
            })
        );
    } else {
        assert!(
            !target.is_alive(),
            "the originally requested idle process was not retired"
        );
        assert!(!manager.background.lock().contains_key(&id));
    }
}

#[test]
fn requested_idle_owner_is_retired_after_focus_moves_to_another_chat() {
    focus_switch_during_retirement(false, false);
}

#[test]
fn requested_owner_that_becomes_busy_after_observation_remains_queued() {
    focus_switch_during_retirement(true, false);
}

#[test]
fn busy_requeue_preserves_the_newer_settings_request() {
    focus_switch_during_retirement(true, true);
}

fn put_in_slot(manager: &SessionManager, live: LiveSession, slot: &str) {
    match slot {
        "live" => *manager.inner.lock() = Some(live),
        "background" => {
            manager
                .background
                .lock()
                .insert(live.app_session_id.clone(), live);
        }
        "parked" => {
            manager.parked.lock().insert(
                live.app_session_id.clone(),
                ParkedAgent {
                    turn_open_epoch: live.turn_open_epoch,
                    workspace_context: live.workspace_context,
                    process_id: live.process_id,
                    app_session_id: live.app_session_id,
                    meta: live.meta,
                    acp: live.acp.unwrap(),
                    last_activity: live.last_activity,
                    model_id: live.model_id,
                    effort: live.effort,
                    product_mode: live.product_mode,
                    project_path: live.project_path,
                    policy: live.policy,
                    sandbox_profile: None,
                    needs_history_bootstrap: live.needs_history_bootstrap,
                    backend: live.backend,
                },
            );
        }
        _ => panic!("unknown synthetic slot"),
    }
}

#[test]
fn explicit_drop_defers_busy_live_and_background_owners() {
    for slot in ["live", "background"] {
        let manager = SessionManager::new();
        let mut live = routing_tests::bare_live_session("target", "process");
        // Even before ACP publication, an in-flight prompt must defer.
        live.prompt_in_flight = true;
        put_in_slot(&manager, live, slot);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(manager.soft_respawn_for_session_core(None, "target", "rules"));
        assert_eq!(
            manager
                .pending_soft_respawn
                .lock()
                .get("target")
                .map(String::as_str),
            Some("rules")
        );
        assert!(manager
            .with_session_mut("target", |s| s.prompt_in_flight)
            .unwrap());
    }
}

#[test]
fn idle_retirement_preserves_shared_process_tenants_in_each_map() {
    let _home = crate::session_migration::test_home::IsolatedAppHome::acquire_with_user_home(
        "retirement-tenants",
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    for target_slot in ["live", "background", "parked"] {
        for tenant_slot in ["live", "background", "parked"] {
            if target_slot == "live" && tenant_slot == "live" {
                continue;
            }
            let manager = SessionManager::new();
            let (client, _peer) =
                test_transport::client_for(false, SessionMcpPolicy::default(), None);
            let mut target = routing_tests::bare_live_session("target", "shared-process");
            target.acp = Some(client.clone());
            put_in_slot(&manager, target, target_slot);
            let mut tenant = routing_tests::bare_live_session("tenant", "shared-process");
            tenant.acp = Some(client.clone());
            put_in_slot(&manager, tenant, tenant_slot);
            runtime.block_on(manager.soft_respawn_for_session_core(None, "target", "rules"));
            assert!(
                client.is_alive(),
                "{target_slot} retirement killed {tenant_slot} tenant"
            );
            assert!(!manager.background.lock().contains_key("target"));
            assert!(!manager.parked.lock().contains_key("target"));
            if target_slot == "live" {
                let live = manager.inner.lock();
                let live = live.as_ref().unwrap();
                assert_eq!(live.app_session_id, "target");
                assert!(live.acp.is_none());
                assert!(live.process_id.is_empty());
                assert!(matches!(
                    live.fsm.state(),
                    crate::session_fsm::SessionState::Disconnected
                ));
            }
            runtime.block_on(client.kill());
        }
    }
}

#[test]
fn idle_target_shutdown_covers_live_background_and_parked_without_erasing_resume_id() {
    let _home = crate::session_migration::test_home::IsolatedAppHome::acquire_with_user_home(
        "retirement-idle-slots",
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    for slot in ["live", "background", "parked"] {
        let manager = SessionManager::new();
        let (client, _peer) = test_transport::client_for(false, SessionMcpPolicy::default(), None);
        let mut live = routing_tests::bare_live_session("target", "target-process");
        live.acp = Some(client.clone());
        live.meta.agent_session_id = Some("resume-agent".into());
        put_in_slot(&manager, live, slot);
        manager
            .pending_soft_respawn
            .lock()
            .insert("target".into(), "extensions_mcp".into());
        runtime.block_on(manager.flush_pending_soft_respawn_core(None, "target"));
        assert!(!client.is_alive(), "idle {slot} process was not stopped");
        assert!(!manager.pending_soft_respawn.lock().contains_key("target"));
        if slot == "live" {
            assert_eq!(
                manager
                    .inner
                    .lock()
                    .as_ref()
                    .unwrap()
                    .meta
                    .agent_session_id
                    .as_deref(),
                Some("resume-agent")
            );
        } else {
            assert!(manager.inner.lock().is_none());
        }
        assert!(manager.background.lock().is_empty());
        assert!(manager.parked.lock().is_empty());
    }
}

#[test]
fn old_retirement_keeps_new_process_and_request_published_after_detach() {
    let _home = crate::session_migration::test_home::IsolatedAppHome::acquire_with_user_home(
        "retirement-new-generation",
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    let manager = Arc::new(SessionManager::new());
    let id = uuid::Uuid::new_v4().to_string();
    let (old, _old_peer) = test_transport::client_for(false, SessionMcpPolicy::default(), None);
    let (new, _new_peer) = test_transport::client_for(false, SessionMcpPolicy::default(), None);
    let mut live = routing_tests::bare_live_session(&id, "old-process");
    live.acp = Some(old.clone());
    *manager.inner.lock() = Some(live);
    let (reached_tx, reached_rx) = mpsc::sync_channel(1);
    let (proceed_tx, proceed_rx) = mpsc::sync_channel(1);
    pauses().lock().unwrap().insert(
        format!("{id}:detached"),
        Pause {
            reached: reached_tx,
            proceed: proceed_rx,
        },
    );
    let worker_manager = manager.clone();
    let worker_id = id.clone();
    let worker = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(worker_manager.soft_respawn_for_session_core(None, &worker_id, "rules"));
    });
    reached_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    // All map guards must be released before process shutdown can yield.
    assert!(manager.runtime_transfer.try_lock().is_some());
    assert!(manager.inner.try_lock().is_some());
    assert!(manager.background.try_lock().is_some());
    assert!(manager.parked.try_lock().is_some());
    let mut live = routing_tests::bare_live_session(&id, "new-process");
    live.acp = Some(new.clone());
    *manager.inner.lock() = Some(live);
    manager
        .pending_soft_respawn
        .lock()
        .insert(id.clone(), "effort".into());
    proceed_tx.send(()).unwrap();
    worker.join().unwrap();
    assert!(!old.is_alive());
    assert!(new.is_alive());
    assert!(manager
        .inner
        .lock()
        .as_ref()
        .unwrap()
        .acp
        .as_ref()
        .is_some_and(|c| Arc::ptr_eq(c, &new)));
    assert_eq!(
        manager
            .pending_soft_respawn
            .lock()
            .get(&id)
            .map(String::as_str),
        Some("effort")
    );
}
