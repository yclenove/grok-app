//! Process-wide Computer Use feature transitions.
//!
//! The persisted preference, Broker dispatch gate, session grants, MCP
//! credentials, and surface cleanup must move through one ordered path.  In
//! particular, feature-off fences local authority before any filesystem I/O.

use super::{sessions, ComputerUseBroker};
use crate::session_manager::SessionManager;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

static FEATURE_CHOICE: AtomicU64 = AtomicU64::new(0);
static STOPPING: AtomicBool = AtomicBool::new(false);

fn workers() -> &'static super::owned_tasks::OwnedTasks<Result<(), String>> {
    static WORKERS: OnceLock<super::owned_tasks::OwnedTasks<Result<(), String>>> = OnceLock::new();
    WORKERS.get_or_init(Default::default)
}

pub(super) fn stop() {
    STOPPING.store(true, Ordering::SeqCst);
    FEATURE_CHOICE.fetch_add(1, Ordering::SeqCst);
    workers().stop();
}

pub(super) async fn join() -> Result<(), String> {
    workers().join().await
}

pub(super) fn choice_revision() -> u64 {
    FEATURE_CHOICE.load(Ordering::SeqCst)
}

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
    let broker = super::ensure_request_endpoint()?;
    persist_feature_transition_with_broker(manager, broker, enabled, persist).await
}

pub(super) async fn persist_feature_transition_with_broker<F>(
    manager: Arc<SessionManager>,
    broker: Arc<ComputerUseBroker>,
    enabled: bool,
    persist: F,
) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String> + Send + 'static,
{
    if STOPPING.load(Ordering::SeqCst) {
        return Err("Computer Use is shutting down".into());
    }
    let revision = FEATURE_CHOICE
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |revision| {
            revision.checked_add(1)
        })
        .map_err(|_| "Computer Use preference revision exhausted")?
        + 1;
    // Closing authority must not wait behind an in-progress preference write
    // or native preparation. Its persistence still joins the ordered writer.
    if !enabled {
        fence_feature_off(&manager, &broker);
    }
    // Keep the original I/O and transition lock owned if the command future
    // is dropped. An explicit later choice wins at both publication checks.
    workers()
        .run(
            async move {
                let _transition = transition_lock().lock().await;
                if STOPPING.load(Ordering::SeqCst) || choice_revision() != revision {
                    return Err("Computer Use preference transition superseded".into());
                }
                tauri::async_runtime::spawn_blocking(persist)
                    .await
                    .map_err(|_| "Computer Use preference writer did not join")??;
                if STOPPING.load(Ordering::SeqCst) || choice_revision() != revision {
                    return Err("Computer Use preference transition superseded".into());
                }
                if enabled {
                    broker.supersede_access_approvals_for_explicit_enable();
                    broker.set_feature_enabled(true);
                    super::set_feature_enabled(true);
                }
                Ok(())
            },
            || {},
        )
        .await?
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
    async fn feature_off_fences_while_an_older_enable_is_still_writing() {
        let _feature_guard = super::super::feature::TEST_LOCK.lock().await;
        let broker = broker(Arc::new(CountingAdapter::default()));
        super::super::set_feature_enabled(true);
        let manager = Arc::new(SessionManager::new());
        let entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let slow = entered.clone();
        let gate = release.clone();
        let enable = tokio::spawn(persist_feature_transition_with_broker(
            manager.clone(),
            broker.clone(),
            true,
            move || {
                slow.store(true, Ordering::SeqCst);
                while !gate.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(())
            },
        ));
        tokio::time::timeout(Duration::from_secs(3), async {
            while !entered.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("original preference write entered");
        let off = tokio::spawn(persist_feature_transition_with_broker(
            manager,
            broker.clone(),
            false,
            || Ok(()),
        ));
        tokio::time::timeout(Duration::from_secs(3), async {
            while broker.feature_enabled() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("off did not wait for the older store writer");
        assert!(
            !off.is_finished(),
            "new preference write still waits for the original writer"
        );
        release.store(true, Ordering::SeqCst);
        assert!(
            enable.await.unwrap().is_err(),
            "superseded enable must never publish"
        );
        off.await.unwrap().unwrap();
        assert!(!broker.feature_enabled());
        assert!(!super::super::feature::feature_enabled());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn feature_off_fences_before_slow_persist_and_releases_each_target_once() {
        let _feature_guard = super::super::feature::TEST_LOCK.lock().await;
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
