//! Host races at the production worker contract; these use a gated worker
//! double, not a claim of browser launch/close or App UI verification.
use super::*;
use crate::error::BrokerError;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

struct Fixture {
    host: Arc<ExistingTabHost>,
    worker: Arc<RecordingBrowserWorker>,
    root: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("cu-profile-lifecycle-{}", uuid::Uuid::new_v4()));
        let host = Arc::new(ExistingTabHost::new());
        let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
        host.set_profile_root(root.join("profiles"));
        host.set_worker(worker.clone());
        Self { host, worker, root }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        assert!(self
            .root
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("cu-profile-lifecycle-"));
        if self.root.exists() {
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }
}

#[test]
fn stop_during_open_fences_late_publication_and_waits_for_local_open_to_settle() {
    let f = Fixture::new();
    let other = f.host.open_managed_profile("s2", "run-b", "other").unwrap();
    let started = Arc::new((Mutex::new(false), Condvar::new()));
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    *f.worker.open_started.lock() = Some(started.clone());
    *f.worker.open_release.lock() = Some(release.clone());
    let host = f.host.clone();
    let opening = std::thread::spawn(move || host.open_managed_profile("s1", "run-a", "opening"));
    let (lock, cv) = &*started;
    let (entered_guard, _) = cv
        .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(4), |ready| {
            !*ready
        })
        .unwrap();
    let entered = *entered_guard;
    drop(entered_guard);
    // This double acknowledges remote cleanup before its original open result
    // returns. Host must still reject late publication and premature completion.
    let stop = f.host.cancel_run("run-a");
    let cancel_calls = *f.worker.cancel_run_calls.lock();
    let clear = f.host.clear_managed_profile("opening");
    let late_open = f.host.open_managed_profile("s1", "run-a", "late");
    let steal = f.host.open_managed_profile("s2", "run-b", "opening");
    let (lock, cv) = &*release;
    *lock.lock().unwrap() = true;
    cv.notify_all();
    let opened = opening.join().unwrap();
    assert!(entered, "worker did not reach the open gate");
    assert_eq!(
        cancel_calls, 1,
        "Stop skipped a profile that was still opening"
    );
    assert!(clear.is_err());
    assert_eq!(
        *f.worker.clear_calls.lock(),
        0,
        "clear raced an active profile open"
    );
    assert!(
        matches!(stop, Err(BrokerError::BrowserWorker(ref e)) if e.code == "run_cleanup_pending")
    );
    assert!(matches!(late_open, Err(BrokerError::StopRequested)));
    assert!(steal.is_err());
    assert!(matches!(opened, Err(BrokerError::StopRequested)));
    assert!(f.host.list_managed_for_run("run-a").is_empty());
    assert!(f.host.managed_target_info(&other.tab_id).is_some());
    f.host.cancel_run("run-a").unwrap();
    assert_eq!(*f.worker.cancel_run_calls.lock(), 2);
    f.host
        .open_managed_profile("s2", "run-b", "opening")
        .unwrap();
    assert!(f
        .host
        .open_managed_profile("s1", "run-a", "never-resurrect")
        .is_err());
}

#[test]
fn failed_cleanup_revokes_local_targets_and_retains_ownership_for_retry() {
    let f = Fixture::new();
    let tab = f.host.open_managed_profile("s", "run-a", "owned").unwrap();
    *f.worker.cancel_run_error.lock() = Some(WorkerError::unknown(
        500,
        "run_cleanup_pending",
        "close failed",
    ));
    assert!(f.host.cancel_run("run-a").is_err());
    assert!(f.host.managed_target_info(&tab.tab_id).is_none());
    assert!(f.host.observe_managed("run-a", "owned").is_err());
    assert!(f.host.open_managed_profile("s", "run-a", "owned").is_err());
    assert!(f.host.open_managed_profile("s2", "run-b", "owned").is_err());
    f.host.cancel_run("run-a").unwrap();
    assert_eq!(*f.worker.cancel_run_calls.lock(), 2);
    f.host.open_managed_profile("s2", "run-b", "owned").unwrap();
}

#[test]
fn uncertain_open_without_registered_tab_still_requires_remote_cleanup() {
    let f = Fixture::new();
    *f.worker.open_error.lock() = Some(WorkerError::timeout("lost open response"));
    assert!(f
        .host
        .open_managed_profile("s", "run-a", "uncertain")
        .is_err());
    assert!(f.host.list_managed_for_run("run-a").is_empty());
    assert!(f
        .host
        .open_managed_profile("s2", "run-b", "uncertain")
        .is_err());
    *f.worker.cancel_run_error.lock() = Some(WorkerError::unknown(
        500,
        "run_cleanup_pending",
        "close failed",
    ));
    assert!(f.host.cancel_run("run-a").is_err());
    f.host.cancel_run("run-a").unwrap();
    assert_eq!(*f.worker.cancel_run_calls.lock(), 2);
    f.host
        .open_managed_profile("s2", "run-b", "uncertain")
        .unwrap();
}

#[test]
fn stop_before_first_open_prevents_dispatch() {
    let f = Fixture::new();
    f.host.cancel_run("run-a").unwrap();
    assert!(matches!(
        f.host.open_managed_profile("s", "run-a", "late"),
        Err(BrokerError::StopRequested)
    ));
    assert_eq!(*f.worker.opens.lock(), 0);
    assert_eq!(*f.worker.cancel_run_calls.lock(), 0);
}

#[test]
fn clear_revokes_grants_and_blocks_reopen_until_the_owned_cleanup_finishes() {
    let f = Fixture::new();
    let tab = f
        .host
        .open_managed_profile("s", "run-a", "clearing")
        .unwrap();
    let other = f.host.open_managed_profile("s2", "run-b", "other").unwrap();
    let started = Arc::new((Mutex::new(false), Condvar::new()));
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    *f.worker.clear_started.lock() = Some(started.clone());
    *f.worker.clear_release.lock() = Some(release.clone());
    let host = f.host.clone();
    let clearing = std::thread::spawn(move || host.clear_managed_profile("clearing"));
    let (lock, cv) = &*started;
    let (ready, _) = cv
        .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(4), |ready| {
            !*ready
        })
        .unwrap();
    let entered = *ready;
    drop(ready);
    let target = f.host.managed_target_info(&tab.tab_id);
    let reopen = f.host.open_managed_profile("s", "run-a", "clearing");
    let steal = f.host.open_managed_profile("s2", "run-b", "clearing");
    let stop = f.host.cancel_run("run-a");
    let (lock, cv) = &*release;
    *lock.lock().unwrap() = true;
    cv.notify_all();
    clearing.join().unwrap().unwrap();
    assert!(entered);
    assert!(target.is_none());
    assert!(reopen.is_err());
    assert!(steal.is_err());
    assert!(
        matches!(stop, Err(BrokerError::BrowserWorker(ref e)) if e.code == "run_cleanup_pending")
    );
    assert!(f.host.managed_target_info(&other.tab_id).is_some());
    f.host.cancel_run("run-a").unwrap();
    f.host
        .open_managed_profile("s2", "run-b", "clearing")
        .unwrap();
    assert!(f.root.join("profiles/clearing").is_dir());
}

#[test]
fn failed_clear_keeps_profile_fenced_until_an_explicit_successful_retry() {
    let f = Fixture::new();
    let tab = f.host.open_managed_profile("s", "run-a", "retry").unwrap();
    *f.worker.clear_error.lock() = Some(WorkerError::timeout("lost clear reply"));
    assert!(f.host.clear_managed_profile("retry").is_err());
    assert!(f.host.managed_target_info(&tab.tab_id).is_none());
    assert!(f.host.open_managed_profile("s", "run-a", "retry").is_err());
    assert!(f.host.open_managed_profile("s2", "run-b", "retry").is_err());
    assert!(f.root.join("profiles/retry").exists());
    f.host.clear_managed_profile("retry").unwrap();
    assert!(!f.root.join("profiles/retry").exists());
    assert!(f.host.is_closed(&tab.tab_id));
    f.host.open_managed_profile("s2", "run-b", "retry").unwrap();
    assert_eq!(*f.worker.clear_calls.lock(), 2);
}

#[test]
fn local_setup_failure_releases_the_reservation_without_dispatching() {
    let f = Fixture::new();
    std::fs::create_dir_all(&f.root).unwrap();
    let blocked_root = f.root.join("profiles");
    std::fs::write(&blocked_root, b"owned fixture blocks directory creation").unwrap();
    assert!(f.host.open_managed_profile("s", "run-a", "setup").is_err());
    assert_eq!(*f.worker.opens.lock(), 0);
    std::fs::remove_file(blocked_root).unwrap();
    f.host.open_managed_profile("s2", "run-b", "setup").unwrap();
    assert_eq!(*f.worker.opens.lock(), 1);
}
