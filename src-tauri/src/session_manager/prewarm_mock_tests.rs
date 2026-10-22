use super::*;
use std::ffi::OsString;
use std::sync::atomic::Ordering;

struct AcpEnv(Option<OsString>);

impl AcpEnv {
    fn set(value: &str) -> Self {
        let previous = std::env::var_os("GROK_APP_ACP");
        std::env::set_var("GROK_APP_ACP", value);
        Self(previous)
    }
}

impl Drop for AcpEnv {
    fn drop(&mut self) {
        match &self.0 {
            Some(value) => std::env::set_var("GROK_APP_ACP", value),
            None => std::env::remove_var("GROK_APP_ACP"),
        }
    }
}

#[test]
fn mock_skips_regular_and_forced_prewarm_without_reserving() {
    let _lock = crate::paths::APP_HOME_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _env = AcpEnv::set("mock");
    for force in [false, true] {
        let mgr = SessionManager::new();
        assert_eq!(mgr.reserve_prewarm(force), None, "force={force}");
        assert_eq!(mgr.prewarm_epoch.load(Ordering::SeqCst), 0);
        assert!(matches!(*mgr.prewarm.lock(), PrewarmState::None));
        assert!(mgr.pending_children.lock().is_empty());
    }
}

#[test]
fn mock_case_insensitive_gate_does_not_wait_for_prewarm_mutex() {
    let _lock = crate::paths::APP_HOME_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _env = AcpEnv::set("MoCk");
    let mgr = Arc::new(SessionManager::new());
    let guard = mgr.prewarm.lock();
    let other = Arc::clone(&mgr);
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let _ = tx.send(other.reserve_prewarm(true));
    });
    let result = rx.recv_timeout(Duration::from_secs(2));
    drop(guard);
    handle.join().unwrap();
    assert_eq!(
        result.expect("mock must return before acquiring any prewarm lock"),
        None
    );
    assert_eq!(mgr.prewarm_epoch.load(Ordering::SeqCst), 0);
}

#[test]
fn mock_preserves_existing_spawning_state_and_epoch() {
    let _lock = crate::paths::APP_HOME_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _env = AcpEnv::set("mock");
    let mgr = SessionManager::new();
    let since = Instant::now();
    *mgr.prewarm.lock() = PrewarmState::Spawning { since };
    mgr.prewarm_epoch.store(42, Ordering::SeqCst);
    assert_eq!(mgr.reserve_prewarm(true), None);
    assert_eq!(mgr.prewarm_epoch.load(Ordering::SeqCst), 42);
    assert!(matches!(*mgr.prewarm.lock(), PrewarmState::Spawning { since: t } if t == since));
}

#[test]
fn real_transport_reserves_once_and_keeps_existing_spawning_gate() {
    let _lock = crate::paths::APP_HOME_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _env = AcpEnv::set("real");
    for force in [false, true] {
        let mgr = SessionManager::new();
        assert_eq!(mgr.reserve_prewarm(force), Some(1));
        assert!(matches!(*mgr.prewarm.lock(), PrewarmState::Spawning { .. }));
        assert_eq!(mgr.reserve_prewarm(force), None);
        assert_eq!(mgr.prewarm_epoch.load(Ordering::SeqCst), 1);
    }
}
