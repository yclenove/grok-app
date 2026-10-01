//! Regression coverage for upstream respawn helpers crossing CU ownership.

use super::recycle_tests::ready_session;
use super::*;
use crate::computer_use::sessions;
use crate::computer_use::test_support::{CountingAdapter, COUNTING_TARGET};
use crate::computer_use::{BrokerOptions, ComputerUseBroker};

struct TestHome {
    previous: Option<std::ffi::OsString>,
    path: std::path::PathBuf,
}

impl TestHome {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("cu-respawn-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("create isolated App home");
        let previous = std::env::var_os("GROK_APP_HOME");
        std::env::set_var("GROK_APP_HOME", &path);
        crate::paths::ensure_app_dirs().expect("initialize isolated App home");
        Self { previous, path }
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var("GROK_APP_HOME", value),
            None => std::env::remove_var("GROK_APP_HOME"),
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

struct CleanupGuard {
    adapter: Arc<CountingAdapter>,
    session: String,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        self.adapter.set_abort_blocked(false);
        sessions::forget_session(&self.session);
    }
}

fn assert_cleanup_precedes_owner_retirement(
    background: bool,
    invalidate: bool,
    replace_owner: bool,
) {
    let _home_lock = crate::paths::APP_HOME_ENV_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let home = TestHome::new();
    tauri::async_runtime::block_on(async {
        let session_id = format!("cu-respawn-{}", uuid::Uuid::new_v4());
        let manager = Arc::new(SessionManager::new());
        let session = ready_session(
            &session_id,
            "original-process",
            "original-agent",
            "official",
        );
        store::save_sessions_index(std::slice::from_ref(&session.meta))
            .expect("persist original endpoint");
        if background {
            manager
                .background
                .lock()
                .insert(session_id.clone(), session);
        } else {
            *manager.inner.lock() = Some(session);
        }
        let adapter = Arc::new(CountingAdapter::default());
        let broker = Arc::new(ComputerUseBroker::new(
            adapter.clone(),
            BrokerOptions {
                feature_enabled: true,
                lease_path: home.path.join("desktop.lease"),
                ..BrokerOptions::default()
            },
        ));
        let ticket = sessions::begin(&broker, &session_id, None, "original-attempt", 1)
            .expect("begin owned authorization");
        broker
            .authorize_target(&ticket.run_id, COUNTING_TARGET)
            .expect("authorize test target");
        sessions::activate(&broker, &ticket).expect("activate owned authorization");
        sessions::complete(&broker, &ticket).expect("complete owned authorization");
        adapter.set_abort_blocked(true);
        let _cleanup_guard = CleanupGuard {
            adapter: adapter.clone(),
            session: session_id.clone(),
        };
        // Publish a real generation-bound cleanup ticket without replacing
        // the process-global broker used by parallel tests.
        let fenced = sessions::fence_revoke_checked_for_test(broker, &session_id);
        assert!(fenced.error.is_none());
        assert!(fenced.cleanup.is_some());
        let task_manager = manager.clone();
        let task_session = session_id.clone();
        let task = tokio::spawn(async move {
            if invalidate {
                task_manager
                    .invalidate_spawn_flags_inner(None, &task_session, "session_provider")
                    .await;
            } else if background {
                task_manager
                    .drop_idle_agent_for_session(&task_session, "permission_policy")
                    .await;
            } else {
                task_manager
                    .detach_live_for_soft_respawn("permission_policy", Some(&task_session))
                    .await;
            }
        });
        tokio::time::timeout(Duration::from_secs(3), async {
            while adapter.aborts() == 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("surface cleanup entered");
        assert!(
            !task.is_finished(),
            "blocked cleanup cannot retire its owner"
        );
        assert!(
            sessions::mcp_desired(&session_id).run_id.is_none(),
            "authority is already fenced"
        );
        assert_eq!(
            manager.with_session_mut(&session_id, |s| s.meta.agent_session_id.clone()),
            Some(Some("original-agent".into())),
            "keep the original MCP endpoint indexed while cleanup is pending"
        );
        assert_eq!(
            store::load_sessions_index()[0].agent_session_id.as_deref(),
            Some("original-agent")
        );
        if invalidate {
            assert!(manager
                .pending_spawn_flag_invalidations
                .lock()
                .contains_key(&session_id));
        }
        if replace_owner {
            let replacement = ready_session(
                &session_id,
                if background {
                    "replacement-process"
                } else {
                    "original-process"
                },
                "replacement-agent",
                "official",
            );
            store::save_sessions_index(std::slice::from_ref(&replacement.meta))
                .expect("persist replacement endpoint");
            if background {
                manager
                    .background
                    .lock()
                    .insert(session_id.clone(), replacement);
            } else {
                *manager.inner.lock() = Some(replacement);
            }
        }
        adapter.set_abort_blocked(false);
        tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .expect("cleanup completes")
            .expect("retirement task succeeds");
        assert_eq!(adapter.aborts(), 1);
        assert_eq!(adapter.releases(), 1);
        if replace_owner {
            manager
                .with_session_mut(&session_id, |replacement| {
                    assert_eq!(
                        replacement.process_id,
                        if background {
                            "replacement-process"
                        } else {
                            "original-process"
                        }
                    );
                    assert_eq!(
                        replacement.meta.agent_session_id.as_deref(),
                        Some("replacement-agent")
                    );
                })
                .expect("replacement stays registered");
            assert_eq!(
                store::load_sessions_index()[0].agent_session_id.as_deref(),
                Some("replacement-agent")
            );
            assert!(manager
                .pending_soft_respawn
                .lock()
                .contains_key(&session_id));
            assert_eq!(
                manager
                    .pending_spawn_flag_invalidations
                    .lock()
                    .contains_key(&session_id),
                invalidate
            );
            return;
        }
        assert!(!manager
            .pending_spawn_flag_invalidations
            .lock()
            .contains_key(&session_id));
        if background {
            assert!(!manager.background.lock().contains_key(&session_id));
        } else {
            assert_eq!(
                manager.inner.lock().as_ref().unwrap().meta.agent_session_id,
                None
            );
        }
        assert_eq!(
            store::load_sessions_index()[0].agent_session_id.as_deref(),
            if invalidate {
                None
            } else {
                Some("original-agent")
            }
        );
    });
}

#[test]
fn spawn_flags_keep_live_endpoint_until_cu_cleanup_finishes() {
    assert_cleanup_precedes_owner_retirement(false, true, false);
}

#[test]
fn spawn_flags_keep_background_endpoint_until_cu_cleanup_finishes() {
    assert_cleanup_precedes_owner_retirement(true, true, false);
}

#[test]
fn idle_agent_keeps_background_owner_until_cu_cleanup_finishes() {
    assert_cleanup_precedes_owner_retirement(true, false, false);
}

#[test]
fn spawn_flags_do_not_reset_a_replacement_background_owner() {
    assert_cleanup_precedes_owner_retirement(true, true, true);
}

#[test]
fn idle_cleanup_does_not_remove_a_replacement_background_owner() {
    assert_cleanup_precedes_owner_retirement(true, false, true);
}

#[test]
fn soft_respawn_keeps_replacement_agent_on_the_same_live_process() {
    assert_cleanup_precedes_owner_retirement(false, false, true);
}

fn assert_stale_reset_preserves_resume(disk_only: bool) {
    let _home_lock = crate::paths::APP_HOME_ENV_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let _home = TestHome::new();
    let session_id = format!("cu-reset-cas-{}", uuid::Uuid::new_v4());
    let manager = SessionManager::new();
    let original = ready_session(&session_id, "process", "original-agent", "official");
    store::save_sessions_index(std::slice::from_ref(&original.meta)).expect("persist original");
    *manager.inner.lock() = Some(original);
    let expected = manager.respawn_owner(&session_id).expect("captured owner");
    assert!(manager.respawn_owner_is_current(&session_id, Some(&expected)));

    // Exercise a replacement after the preliminary owner check, including a
    // disk writer that has not yet published its in-memory endpoint.
    let replacement = ready_session(&session_id, "process", "replacement-agent", "official");
    store::save_sessions_index(std::slice::from_ref(&replacement.meta))
        .expect("persist replacement");
    if !disk_only {
        *manager.inner.lock() = Some(replacement);
    }
    assert!(!manager
        .clear_respawn_owner_agent_id(&session_id, Some(&expected), Some("original-agent"))
        .expect("compare-and-clear"));
    assert_eq!(
        store::load_sessions_index()[0].agent_session_id.as_deref(),
        Some("replacement-agent")
    );
    assert_eq!(
        manager
            .inner
            .lock()
            .as_ref()
            .unwrap()
            .meta
            .agent_session_id
            .as_deref(),
        Some(if disk_only {
            "original-agent"
        } else {
            "replacement-agent"
        })
    );
}

#[test]
fn spawn_flags_preserve_a_newer_persisted_resume_id() {
    assert_stale_reset_preserves_resume(true);
}

#[test]
fn spawn_flags_recheck_owner_before_the_persisted_reset() {
    assert_stale_reset_preserves_resume(false);
}
