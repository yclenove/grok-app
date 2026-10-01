use super::*;
use crate::execution::ActionCancellation;
use std::sync::Arc;

struct Fixture {
    host: ExistingTabHost,
    root: std::path::PathBuf,
    worker: Arc<RecordingBrowserWorker>,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("cu-request-binding-{}", uuid::Uuid::new_v4()));
        let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
        let host = ExistingTabHost::new();
        host.set_profile_root(root.join("profiles"));
        host.set_worker(worker.clone());
        Self { host, root, worker }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn admitted_requests_and_cleanup_keep_the_original_worker_after_replacement() {
    let f = Fixture::new();
    let request = f
        .host
        .admit_managed_request("run-a", ActionCancellation::default())
        .unwrap();
    let replacement = Arc::new(RecordingBrowserWorker::new(f.root.clone()));
    f.host.set_worker(replacement.clone());
    let scoped = f.host.for_managed_request(&request).unwrap();
    scoped.open_managed_profile("s", "run-a", "one").unwrap();
    assert!(scoped.open_managed_profile("s", "run-b", "wrong").is_err());
    assert_eq!(*f.worker.opens.lock(), 1);
    assert_eq!(*replacement.opens.lock(), 0);
    f.host.pause_managed_run("run-a").unwrap();
    assert!(f.host.managed_run_idle("run-a"));
    f.host.resume_managed_run("run-a").unwrap();
    // The original token is deliberately not cancelled in this Host test:
    // revision fencing alone must reject the old request.
    assert!(scoped.open_managed_profile("s", "run-a", "stale").is_err());
    assert!(f.host.for_managed_request(&request).is_err());
    let next = f
        .host
        .admit_managed_request("run-a", ActionCancellation::default())
        .unwrap();
    assert_eq!(next.identity.revision().get(), 2);
    f.host
        .for_managed_request(&next)
        .unwrap()
        .open_managed_profile("s", "run-a", "two")
        .unwrap();
    assert_eq!(*f.worker.opens.lock(), 2);
    f.host.cancel_run("run-a").unwrap();
    assert!(f.host.managed_run_idle("run-a"));
    assert_eq!(*f.worker.cancel_run_calls.lock(), 1);
    assert_eq!(*replacement.cancel_run_calls.lock(), 0);
    let other = f
        .host
        .admit_managed_request("run-b", ActionCancellation::default())
        .unwrap();
    f.host
        .for_managed_request(&other)
        .unwrap()
        .open_managed_profile("s2", "run-b", "three")
        .unwrap();
    assert_eq!(*replacement.opens.lock(), 1);
    f.host.cancel_run("run-b").unwrap();
}

#[test]
fn cancelled_admission_cannot_bind_or_publish_a_new_profile() {
    let f = Fixture::new();
    let token = ActionCancellation::default();
    let request = f.host.admit_managed_request("run", token.clone()).unwrap();
    let scoped = f.host.for_managed_request(&request).unwrap();
    token.cancel();
    assert!(f.host.admit_managed_request("run", token).is_err());
    assert!(f.host.for_managed_request(&request).is_err());
    assert!(scoped
        .open_managed_profile("s", "run", "cancelled")
        .is_err());
    assert_eq!(*f.worker.opens.lock(), 0);
}

#[test]
fn lost_resume_acknowledgement_stays_fenced_and_stop_uses_independent_control() {
    let f = Fixture::new();
    let request = f
        .host
        .admit_managed_request("run", ActionCancellation::default())
        .unwrap();
    f.host
        .for_managed_request(&request)
        .unwrap()
        .open_managed_profile("s", "run", "one")
        .unwrap();
    f.host.pause_managed_run("run").unwrap();
    *f.worker.resume_error.lock() = Some(WorkerError::transport("lost resume reply"));
    assert!(f.host.resume_managed_run("run").is_err());
    assert!(!f.host.managed_run_idle("run"));
    assert!(f.host.resume_managed_run("run").is_err());
    assert!(f.host.pause_managed_run("run").is_err());
    assert!(f
        .host
        .admit_managed_request("run", ActionCancellation::default())
        .is_err());
    assert_eq!(*f.worker.resume_calls.lock(), 1);
    f.host.cancel_run("run").unwrap();
    assert!(f.host.managed_run_idle("run"));
    assert_eq!(*f.worker.cancel_run_calls.lock(), 1);
    assert!(f
        .host
        .admit_managed_request("run", ActionCancellation::default())
        .is_err());
}

#[test]
fn later_pause_revokes_the_admitted_resume_before_any_remote_transition() {
    let f = Fixture::new();
    f.host
        .admit_managed_request("run", ActionCancellation::default())
        .unwrap();
    f.host.pause_managed_run("run").unwrap();
    let old = f.host.prepare_managed_resume("run").unwrap().unwrap();
    f.host.pause_managed_run("run").unwrap();
    assert!(f.host.resume_managed_request(&old).is_err());
    assert_eq!(*f.worker.resume_calls.lock(), 0);
    assert!(f.host.managed_run_idle("run"));
    let current = f.host.prepare_managed_resume("run").unwrap().unwrap();
    f.host.resume_managed_request(&current).unwrap();
    assert_eq!(*f.worker.resume_calls.lock(), 1);
}
