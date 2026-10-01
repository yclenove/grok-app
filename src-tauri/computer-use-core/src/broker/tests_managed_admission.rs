use super::*;
use crate::browser::{RecordingBrowserWorker, RecordingGotoBarrier};

fn signal(gate: &RecordingGotoBarrier) {
    let (mutex, changed) = &**gate;
    *mutex.lock().unwrap() = true;
    changed.notify_all();
}

#[test]
fn pause_during_profile_authorization_cannot_publish_or_resume_before_open_returns() {
    let root = std::env::temp_dir().join(format!("cu-authorize-admission-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    let started: RecordingGotoBarrier = Default::default();
    let release: RecordingGotoBarrier = Default::default();
    *worker.open_started.lock() = Some(started.clone());
    *worker.open_release.lock() = Some(release.clone());
    let broker = Arc::new(ComputerUseBroker::new(
        Arc::new(crate::fake::FakeAdapter::new()),
        gates::enabled_opts(100),
    ));
    broker.tabs().set_profile_root(root.join("profiles"));
    broker.tabs().set_worker(worker.clone());
    broker.register_managed_browser_adapter().unwrap();
    broker.open_run("s", "run").unwrap();
    let opening_broker = broker.clone();
    let opening = std::thread::spawn(move || {
        opening_broker.authorize_on_surface(
            "s",
            "run",
            SurfaceKind::ManagedBrowser,
            "managed-profile:opening",
        )
    });
    let entered = {
        let (mutex, changed) = &*started;
        let (entered, _) = changed
            .wait_timeout_while(mutex.lock().unwrap(), Duration::from_secs(3), |entered| {
                !*entered
            })
            .unwrap();
        *entered
    };
    let paused = broker.pause("run");
    let resumed = broker.resume("run");
    signal(&release);
    let result = opening.join().unwrap();
    assert!(entered, "profile open must reach the held worker");
    assert!(paused.is_ok(), "{paused:?}");
    assert!(matches!(resumed, Err(BrokerError::LeaseHeld { .. })));
    assert!(
        result.is_err(),
        "late open must not authorize the paused run"
    );
    assert!(!broker.has_authorized_target("run"));
    assert!(broker.tabs().list_managed_for_run("run").is_empty());
    assert!(broker.is_paused("run").unwrap());
    assert_eq!(broker.request_stop("run").unwrap(), StopState::Stopped);
    assert_eq!(*worker.cancel_run_calls.lock(), 1);
    std::fs::remove_dir_all(root).unwrap();
}
