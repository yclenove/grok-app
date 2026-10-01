//! R4.4: hang/stop/pause/late-result races use barriers, not sleep.

use super::gates::enabled_opts;
use super::*;
use crate::browser::RecordingBrowserWorker;
use crate::fake::FakeAdapter;
use std::sync::{Arc as StdArc, Condvar, Mutex as StdMutex};
use std::thread;

fn wait_flag(flag: &StdArc<(StdMutex<bool>, Condvar)>) {
    let (lock, cv) = &**flag;
    let mut ready = lock.lock().unwrap();
    while !*ready {
        ready = cv.wait(ready).unwrap();
    }
}

fn release_flag(flag: &StdArc<(StdMutex<bool>, Condvar)>) {
    let (lock, cv) = &**flag;
    *lock.lock().unwrap() = true;
    cv.notify_all();
}

#[allow(clippy::type_complexity)]
type ReadyHang = (
    Arc<ComputerUseBroker>,
    Arc<RecordingBrowserWorker>,
    String,
    crate::browser::RecordingGotoBarrier,
    crate::browser::RecordingGotoBarrier,
    std::path::PathBuf,
);

fn ready_hang() -> ReadyHang {
    let fake = Arc::new(FakeAdapter::new());
    let broker = Arc::new(ComputerUseBroker::new(fake, enabled_opts(400)));
    broker.open_run("sess-a", "run-a").expect("open");
    let root = std::env::temp_dir().join(format!("cu-r44-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    broker.tabs().set_profile_root(root.join("profiles"));
    broker.tabs().set_worker(worker.clone());
    broker.tabs().set_staging_root(root.clone());
    let tab = broker
        .tabs()
        .open_managed_profile("sess-a", "run-a", "p1")
        .expect("profile")
        .tab_id;
    let started = StdArc::new((StdMutex::new(false), Condvar::new()));
    let release = StdArc::new((StdMutex::new(false), Condvar::new()));
    *worker.goto_started.lock() = Some(started.clone());
    *worker.goto_release.lock() = Some(release.clone());
    (broker, worker, tab, started, release, root)
}

fn dispatch_nav(
    broker: &ComputerUseBroker,
    tab: &str,
    url: &str,
    action_id: &str,
) -> serde_json::Value {
    crate::tools::dispatch(
        broker,
        &crate::ipc::SessionBinding {
            session_id: "sess-a".into(),
            run_id: "run-a".into(),
        },
        "computer_navigate",
        serde_json::json!({
            "tabId": tab,
            "url": url,
            "actionId": action_id,
            "pageGeneration": 1,
        }),
    )
}

#[test]
fn stop_during_hang_does_not_commit_navigation() {
    let (broker, worker, tab, started, release, root) = ready_hang();
    let pending = {
        let broker = broker.clone();
        let tab = tab.clone();
        thread::spawn(move || {
            dispatch_nav(
                &broker,
                &tab,
                "https://example.test/hang-stop",
                "nav-hang-stop",
            )
        })
    };
    wait_flag(&started);
    let state = broker.request_stop("run-a").expect("stop");
    assert_ne!(state, StopState::Running);
    release_flag(&release);
    let result = pending.join().expect("join");
    assert_eq!(
        result.get("isError"),
        Some(&serde_json::json!(true)),
        "stopped hang must not succeed: {result}"
    );
    match broker.tabs().observe("run-a", &tab) {
        Err(BrokerError::DeadTarget) => {}
        Ok(info) => {
            assert!(
                !info.url.contains("hang-stop"),
                "stop must not leave the hung URL as postcondition, got {}",
                info.url
            );
        }
        other => panic!("unexpected tab state {other:?}"),
    }
    let _ = worker;
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn pause_during_hang_ignores_late_navigation() {
    let (broker, worker, tab, started, release, root) = ready_hang();
    let pending = {
        let broker = broker.clone();
        let tab = tab.clone();
        thread::spawn(move || {
            dispatch_nav(
                &broker,
                &tab,
                "https://example.test/hang-pause",
                "nav-hang-pause",
            )
        })
    };
    wait_flag(&started);
    broker.pause("run-a").expect("pause");
    release_flag(&release);
    let result = pending.join().expect("join");
    assert_eq!(
        result.get("isError"),
        Some(&serde_json::json!(true)),
        "paused hang must not succeed: {result}"
    );
    let info = broker
        .tabs()
        .observe("run-a", &tab)
        .expect("tab still open");
    assert!(
        !info.url.contains("hang-pause"),
        "late navigate after pause must not rewrite the tab, got {}",
        info.url
    );
    assert!(!info.closed);
    let follow = dispatch_nav(
        &broker,
        &tab,
        "https://example.test/after-pause",
        "nav-after-pause",
    );
    assert_eq!(follow.get("isError"), Some(&serde_json::json!(true)));
    assert!(
        !worker
            .gotos
            .lock()
            .iter()
            .any(|row| row.contains("after-pause")),
        "paused run must not dispatch a follow-up navigate"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unknown_after_late_result_blocks_until_observe() {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    fake.set_hang(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(40));
    broker.open_run("sess-a", "run-a").expect("open");
    let tid = fake.fixture_id();
    let gen = broker.authorize_target("run-a", &tid).expect("auth");
    let obs = broker.observe("run-a").expect("obs");
    let root = std::env::temp_dir().join(format!("cu-r44-unk-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    broker.tabs().set_profile_root(root.join("profiles"));
    broker.tabs().set_worker(worker.clone());
    broker.tabs().set_staging_root(root.clone());
    let tab = broker
        .tabs()
        .open_managed_profile("sess-a", "run-a", "p1")
        .expect("profile")
        .tab_id;
    let out = broker.act(super::gates::click_req(
        "run-a",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "late-unknown",
    ));
    assert_eq!(out.kind, crate::protocol::OutcomeKind::Unknown);
    fake.finish_in_flight();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while fake.adapter_in_flight() != 0 && std::time::Instant::now() < until {
        thread::yield_now();
    }
    let blocked = dispatch_nav(&broker, &tab, "https://example.test/blocked", "nav-blocked");
    assert_eq!(blocked.get("isError"), Some(&serde_json::json!(true)));
    let text = blocked["content"][0]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("unknown") && text.contains("observe"),
        "unknown must require observe, got {text}"
    );
    assert!(worker.gotos.lock().is_empty());
    let _ = std::fs::remove_dir_all(root);
}
