//! Process-wide Computer Use feature transitions.
//!
//! The persisted preference, Broker dispatch gate, session grants, MCP
//! credentials, and surface cleanup must move through one ordered path.  In
//! particular, feature-off fences local authority before any filesystem I/O.

use super::{sessions, ComputerUseBroker};
use crate::session_manager::SessionManager;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

pub(super) fn transition_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn fence_feature_off(manager: &Arc<SessionManager>, broker: &Arc<ComputerUseBroker>) {
    // The Broker gate is the authorization/dispatch authority.  Close it
    // first, so an authorization racing this transition either opened before
    // the fence and is included below, or fails to open after the fence.
    let broker_cleanups = broker.fence_feature_enabled(false);
    super::set_feature_enabled(false);
    broker.tabs().revoke_pairing();

    let tracked_sessions = sessions::tracked_sessions();
    let tracked_runs = tracked_sessions
        .iter()
        .filter_map(|session| sessions::current(session))
        .collect::<HashSet<_>>();
    let orphan_cleanups = broker_cleanups
        .into_iter()
        .filter(|ticket| !tracked_runs.contains(ticket.run_id()))
        .collect::<Vec<_>>();

    // Session fencing revokes the loopback bearer and publishes the
    // desired-absent MCP catalog synchronously.  Network and adapter work is
    // handed off only after that local authority boundary has closed.
    for session_id in tracked_sessions {
        let plan = manager.fence_computer_use_stop_with_broker(Arc::clone(broker), &session_id);
        manager.spawn_fenced_computer_use_stop(session_id, plan, None);
    }

    if !orphan_cleanups.is_empty() {
        let cleanup_broker = Arc::clone(broker);
        tauri::async_runtime::spawn_blocking(move || {
            for cleanup in orphan_cleanups {
                if let Err(error) = cleanup_broker.finish_stop_cleanup(&cleanup) {
                    tracing::warn!(%error, "Computer Use orphan cleanup after feature-off failed");
                }
            }
        });
    }
}

/// Persist and apply the process-wide product flag under one transition lock.
///
/// Enabling is opened only after persistence succeeds. Disabling is fenced
/// before persistence so a slow or failed store write cannot leave a live
/// authorization window. Cleanup is already owned by background workers if
/// the caller is cancelled while the store write is in progress.
pub(crate) async fn persist_feature_transition<F>(
    manager: Arc<SessionManager>,
    enabled: bool,
    persist: F,
) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String> + Send + 'static,
{
    let _transition = transition_lock().lock().await;
    let broker = super::ensure_host_runtime();
    persist_feature_transition_with_broker(manager, broker, enabled, persist).await
}

async fn persist_feature_transition_with_broker<F>(
    manager: Arc<SessionManager>,
    broker: Arc<ComputerUseBroker>,
    enabled: bool,
    persist: F,
) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String> + Send + 'static,
{
    if enabled {
        tauri::async_runtime::spawn_blocking(persist)
            .await
            .map_err(|error| error.to_string())??;
        super::set_feature_enabled(true);
        broker.set_feature_enabled(true);
        return Ok(());
    }

    fence_feature_off(&manager, &broker);
    tauri::async_runtime::spawn_blocking(persist)
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computer_use::test_support::{CountingAdapter, COUNTING_TARGET};
    use grok_computer_use_core::broker::{BrokerOptions, StopState};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    fn broker(adapter: Arc<CountingAdapter>) -> Arc<ComputerUseBroker> {
        Arc::new(ComputerUseBroker::new(
            adapter,
            BrokerOptions {
                feature_enabled: true,
                lease_path: std::env::temp_dir().join(format!(
                    "cu-feature-lifecycle-{}.lease",
                    uuid::Uuid::new_v4()
                )),
                ..BrokerOptions::default()
            },
        ))
    }

    async fn wait_for_cleanup(adapter: &CountingAdapter, count: u64) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if adapter.aborts() == count && adapter.releases() == count {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("feature-off cleanup completed");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn feature_off_fences_before_slow_persist_and_releases_each_target_once() {
        super::super::set_feature_enabled(true);
        let adapter = Arc::new(CountingAdapter::default());
        adapter.set_abort_blocked(true);
        let broker = broker(Arc::clone(&adapter));
        let manager = Arc::new(SessionManager::new());
        let session = format!("feature-off-tracked-{}", uuid::Uuid::new_v4());
        let ticket = sessions::begin(&broker, &session, None, "feature-off", 1)
            .expect("begin tracked authorization");
        broker
            .authorize_target(&ticket.run_id, COUNTING_TARGET)
            .expect("authorize tracked target");
        sessions::activate(&broker, &ticket).expect("publish tracked authorization");
        sessions::complete(&broker, &ticket).expect("complete tracked authorization");

        let orphan_run = format!("feature-off-orphan-{}", uuid::Uuid::new_v4());
        broker
            .open_run("orphan-session", &orphan_run)
            .expect("open orphan run");
        broker
            .authorize_target(&orphan_run, COUNTING_TARGET)
            .expect("authorize orphan target");

        let persist_entered = Arc::new(AtomicBool::new(false));
        let allow_persist = Arc::new(AtomicBool::new(false));
        let transition = {
            let manager = Arc::clone(&manager);
            let broker = Arc::clone(&broker);
            let persist_entered = Arc::clone(&persist_entered);
            let allow_persist = Arc::clone(&allow_persist);
            tokio::spawn(async move {
                persist_feature_transition_with_broker(manager, broker, false, move || {
                    persist_entered.store(true, Ordering::SeqCst);
                    while !allow_persist.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Ok(())
                })
                .await
            })
        };

        tokio::time::timeout(Duration::from_secs(2), async {
            while !persist_entered.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("persist barrier entered");

        assert!(!broker.feature_enabled());
        assert_eq!(
            broker.stop_state(&ticket.run_id).unwrap(),
            StopState::StopRequested
        );
        assert!(sessions::mcp_desired(&session).run_id.is_none());
        assert!(!sessions::is_current(&ticket));
        let late_session = format!("feature-off-late-{}", uuid::Uuid::new_v4());
        assert!(sessions::begin(&broker, &late_session, None, "late", 1).is_err());

        allow_persist.store(true, Ordering::SeqCst);
        transition
            .await
            .expect("join feature transition")
            .expect("persist feature-off");
        adapter.set_abort_blocked(false);
        wait_for_cleanup(&adapter, 2).await;

        persist_feature_transition_with_broker(
            Arc::clone(&manager),
            Arc::clone(&broker),
            false,
            || Ok(()),
        )
        .await
        .expect("repeated feature-off is idempotent");
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(adapter.aborts(), 2);
        assert_eq!(adapter.releases(), 2);

        sessions::forget_session(&session);
        sessions::forget_session(&late_session);
        super::super::set_feature_enabled(false);
    }
}
