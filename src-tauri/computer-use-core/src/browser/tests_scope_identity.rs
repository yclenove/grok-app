//! Synthetic worker identity responses; not production App or user consent.
use super::*;
use crate::adapter::{ComputerUseAdapter, NativeScopeIdentity};
use crate::execution::ActionCancellation;
use std::sync::Arc;

struct Fixture {
    host: Arc<ExistingTabHost>,
    worker: Arc<RecordingBrowserWorker>,
    root: std::path::PathBuf,
    tab: TabInfo,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("cu-scope-{}", uuid::Uuid::new_v4()));
        let host = Arc::new(ExistingTabHost::new());
        let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
        host.set_worker(worker.clone());
        host.set_profile_root(root.join("profiles"));
        let request = host
            .admit_managed_request("run-a", ActionCancellation::default())
            .unwrap();
        let tab = host
            .for_managed_request(&request)
            .unwrap()
            .open_managed_profile("session-a", "run-a", "task-a")
            .unwrap();
        *worker.scope_witness.lock() = Some(ManagedScopeIdentity {
            owner: "run-a".into(),
            profile: "task-a".into(),
            page_id: "rec-page".into(),
            worker_instance_id: uuid::Uuid::new_v4().to_string(),
            context_instance_id: uuid::Uuid::new_v4().to_string(),
        });
        Self {
            host,
            worker,
            root,
            tab,
        }
    }
    fn adapter(&self) -> ManagedBrowserAdapter {
        ManagedBrowserAdapter::new(self.host.clone())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.host.cancel_run("run-a").unwrap();
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn retained_scope_checks_exact_worker_metadata_without_observe_or_input() {
    let f = Fixture::new();
    let first = f.adapter().scope_identity(&f.tab.tab_id).unwrap();
    assert!(matches!(first, NativeScopeIdentity::ManagedPage { .. }));
    assert_eq!(first.target_id().as_deref(), Some(f.tab.tab_id.as_str()));
    f.worker
        .set_actual_url(Some("https://owned.test/changed".into()));
    f.host.list_managed_pages("run-a", "task-a").unwrap();
    assert_eq!(f.adapter().scope_identity(&f.tab.tab_id).unwrap(), first);
    let info = f
        .adapter()
        .scope_target_info("run-a", &f.tab.tab_id)
        .unwrap();
    assert_eq!(info.target_id, f.tab.tab_id);
    assert!(f
        .adapter()
        .scope_target_info("another-run", &f.tab.tab_id)
        .is_err());
    assert_eq!(*f.worker.opens.lock(), 1);
    assert!(f.worker.capture_options.lock().is_empty());
    assert_eq!(*f.worker.act_calls.lock(), 0);
    assert!(f.worker.gotos.lock().is_empty());
}

#[test]
fn replacement_global_worker_cannot_answer_original_run_scope_queries() {
    let f = Fixture::new();
    let original = f.adapter().scope_identity(&f.tab.tab_id).unwrap();
    let replacement = Arc::new(RecordingBrowserWorker::new(f.root.clone()));
    f.host.set_worker(replacement.clone());
    assert_eq!(f.adapter().scope_identity(&f.tab.tab_id).unwrap(), original);
    assert_eq!(*replacement.scope_calls.lock(), 0);
    f.worker
        .scope_witness
        .lock()
        .as_mut()
        .unwrap()
        .context_instance_id = uuid::Uuid::new_v4().to_string();
    assert_ne!(f.adapter().scope_identity(&f.tab.tab_id).unwrap(), original);
}

#[test]
fn unavailable_or_mismatched_worker_witnesses_never_fall_back_to_live_names() {
    let f = Fixture::new();
    let valid = f.worker.scope_witness.lock().clone().unwrap();
    for field in ["owner", "profile", "page", "context", "worker", "missing"] {
        let mut witness = valid.clone();
        match field {
            "owner" => witness.owner = "another-run".into(),
            "profile" => witness.profile = "another-profile".into(),
            "page" => witness.page_id = "another-page".into(),
            "context" => witness.context_instance_id.clear(),
            "worker" => witness.worker_instance_id.clear(),
            _ => {}
        }
        *f.worker.scope_witness.lock() = (field != "missing").then_some(witness);
        assert!(f.adapter().target_alive(&f.tab.tab_id));
        assert!(
            f.adapter().scope_identity(&f.tab.tab_id).is_err(),
            "{field}"
        );
    }
}

#[test]
fn pause_and_stop_reject_late_scope_witnesses_without_waiting_on_worker() {
    for stop in [false, true] {
        let f = Fixture::new();
        let started = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        *f.worker.scope_started.lock() = Some(started.clone());
        *f.worker.scope_release.lock() = Some(release.clone());
        let adapter = f.adapter();
        let target = f.tab.tab_id.clone();
        let query = std::thread::spawn(move || adapter.scope_identity(&target));
        let (lock, cv) = &*started;
        let (ready, _) = cv
            .wait_timeout_while(
                lock.lock().unwrap(),
                std::time::Duration::from_secs(3),
                |ready| !*ready,
            )
            .unwrap();
        assert!(*ready, "original query did not start");
        drop(ready);
        if stop {
            f.host.cancel_run("run-a").unwrap();
        } else {
            f.host.pause_managed_run("run-a").unwrap();
        }
        let (lock, cv) = &*release;
        *lock.lock().unwrap() = true;
        cv.notify_all();
        assert!(
            query.join().unwrap().is_err(),
            "late witness published after fence"
        );
        assert!(f.adapter().scope_identity(&f.tab.tab_id).is_err());
        assert_eq!(*f.worker.scope_calls.lock(), 1);
    }
}

#[test]
fn nonexistent_or_unadmitted_legacy_pages_do_not_create_scope_owners() {
    let f = Fixture::new();
    assert!(f
        .adapter()
        .scope_identity("managed:other:rec-page")
        .is_err());
    let legacy = f
        .host
        .open_managed_profile("legacy-session", "legacy-run", "legacy")
        .unwrap();
    assert!(f.adapter().scope_identity(&legacy.tab_id).is_err());
    assert_eq!(*f.worker.scope_calls.lock(), 0);
    f.host.cancel_run("legacy-run").unwrap();
}
