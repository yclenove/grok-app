//! R4.3: cancel-actions vs cancel-run vs borrowed tabs.

use super::gates::enabled_opts;
use super::*;
use crate::browser::{RecordingBrowserWorker, WorkerError};
use crate::fake::FakeAdapter;

fn ready_managed() -> (
    ComputerUseBroker,
    Arc<RecordingBrowserWorker>,
    String,
    std::path::PathBuf,
) {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake, enabled_opts(400));
    broker.open_run("sess-a", "run-a").expect("open");
    let root = std::env::temp_dir().join(format!("cu-r43-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    broker.tabs().set_profile_root(root.join("profiles"));
    broker.tabs().set_worker(worker.clone());
    broker.tabs().set_staging_root(root.clone());
    let tab = broker
        .tabs()
        .open_managed_profile("sess-a", "run-a", "p1")
        .expect("profile")
        .tab_id;
    (broker, worker, tab, root)
}

fn grant_user_tab(broker: &ComputerUseBroker, tab_id: &str) {
    broker.tabs().set_installed_extension_id("pw-ext-installed");
    let token = broker.tabs().handshake_pairing().expect("pair");
    broker
        .tabs()
        .share_and_grant(crate::browser::TabAttachment {
            session: "sess-a",
            run_id: "run-a",
            tab_id,
            title: "Docs",
            url: "https://example.test/",
            origin: "chrome-extension://pw-ext-installed/",
            extension_id: Some("pw-ext-installed"),
            pairing_token: Some(&token),
            home_index: Some(3),
            document_generation: Some(1),
            connection_generation: Some(1),
            focused: true,
        })
        .expect("grant");
}

fn dispatch_nav(broker: &ComputerUseBroker, tab: &str, action_id: &str) -> serde_json::Value {
    crate::tools::dispatch(
        broker,
        &crate::ipc::SessionBinding {
            session_id: "sess-a".into(),
            run_id: "run-a".into(),
        },
        "computer_navigate",
        serde_json::json!({
            "tabId": tab,
            "url": "https://example.test/after",
            "actionId": action_id,
            "pageGeneration": 1,
        }),
    )
}

#[test]
fn pause_keeps_app_owned_profile_open_and_blocks_writes() {
    let (broker, worker, tab, root) = ready_managed();
    broker.pause("run-a").expect("pause");
    let info = broker.tabs().observe("run-a", &tab).expect("observe tab");
    assert!(!info.closed, "pause must not close App-owned tabs");
    assert_eq!(
        *worker.cancel_run_calls.lock(),
        0,
        "pause must not call worker cancel_run"
    );
    let result = dispatch_nav(&broker, &tab, "nav-paused");
    assert_eq!(result.get("isError"), Some(&serde_json::json!(true)));
    assert!(worker.gotos.lock().is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn pause_revokes_borrowed_grant_without_closing_user_tab() {
    let (broker, worker, _managed, root) = ready_managed();
    grant_user_tab(&broker, "tab-user");
    let before = broker
        .tabs()
        .observe("run-a", "tab-user")
        .expect("user tab");
    assert!(before.borrowed && before.user_owned && !before.closed);
    broker.pause("run-a").expect("pause");
    match broker.tabs().observe("run-a", "tab-user") {
        Err(BrokerError::TargetUnauthorized) => {}
        Err(BrokerError::DeadTarget) => panic!("pause must not close the user tab"),
        other => panic!("pause must revoke the borrowed grant, got {other:?}"),
    }
    assert_eq!(*worker.cancel_run_calls.lock(), 0);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn stop_closes_app_owned_and_keeps_user_tab() {
    let (broker, worker, managed, root) = ready_managed();
    grant_user_tab(&broker, "tab-user");
    let state = broker.request_stop("run-a").expect("stop");
    assert_ne!(state, StopState::Running);
    match broker.tabs().observe("run-a", &managed) {
        Err(BrokerError::DeadTarget) => {}
        Ok(info) if info.closed => {}
        other => panic!("stop must close App-owned tabs, got {other:?}"),
    }
    match broker.tabs().observe("run-a", "tab-user") {
        Err(BrokerError::DeadTarget) => panic!("stop must not close borrowed user tabs"),
        Err(_) => {}
        Ok(info) => {
            assert!(!info.closed, "stop must not close borrowed user tabs");
            assert!(!info.borrowed, "stop must revoke the borrowed grant");
        }
    }
    assert!(
        *worker.cancel_run_calls.lock() >= 1,
        "stop must call worker cancel_run"
    );
    let result = dispatch_nav(&broker, &managed, "nav-stopped");
    assert_eq!(result.get("isError"), Some(&serde_json::json!(true)));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cancel_run_failure_stays_stop_requested_and_blocks_writes() {
    let (broker, worker, tab, root) = ready_managed();
    *worker.cancel_run_error.lock() = Some(WorkerError::unknown(
        500,
        "cancel_failed",
        "worker close failed",
    ));
    let err = broker
        .request_stop("run-a")
        .expect_err("cleanup failure must not look like success");
    assert!(
        err.to_string().contains("browser cancellation"),
        "cleanup failure must be explicit, got {err}"
    );
    assert_eq!(
        broker.stop_state("run-a").expect("state"),
        StopState::StopRequested,
        "cleanup failure must not report stopped"
    );
    let before = worker.gotos.lock().len();
    let result = dispatch_nav(&broker, &tab, "nav-after-fail");
    assert_eq!(result.get("isError"), Some(&serde_json::json!(true)));
    assert_eq!(worker.gotos.lock().len(), before);
    let _ = std::fs::remove_dir_all(root);
}
