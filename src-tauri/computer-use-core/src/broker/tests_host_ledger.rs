//! R4.2: Host action ledger is the restart authority.

use super::gates::enabled_opts;
use super::*;
use crate::browser::RecordingBrowserWorker;
use crate::fake::FakeAdapter;

fn binding() -> crate::ipc::SessionBinding {
    crate::ipc::SessionBinding {
        session_id: "sess-a".into(),
        run_id: "run-a".into(),
    }
}

fn dispatch_nav(
    broker: &ComputerUseBroker,
    tab: &str,
    url: &str,
    action_id: &str,
    page_generation: u64,
) -> serde_json::Value {
    crate::tools::dispatch(
        broker,
        &binding(),
        "computer_navigate",
        serde_json::json!({
            "tabId": tab,
            "url": url,
            "actionId": action_id,
            "pageGeneration": page_generation,
        }),
    )
}

fn is_tool_error(result: &serde_json::Value) -> bool {
    result.get("isError") == Some(&serde_json::json!(true))
}

#[test]
fn worker_restart_replays_host_ledger_without_second_execution() {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake, enabled_opts(400));
    broker.open_run("sess-a", "run-a").expect("open");
    let root = std::env::temp_dir().join(format!("cu-r42-restart-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    broker.tabs().set_profile_root(root.join("profiles"));
    broker.tabs().set_worker(worker.clone());
    broker.tabs().set_staging_root(root.clone());
    let info = broker
        .tabs()
        .open_managed_profile("sess-a", "run-a", "p1")
        .expect("profile");
    let first = dispatch_nav(
        &broker,
        &info.tab_id,
        "https://example.test/once",
        "nav-restart",
        info.document_generation.max(1),
    );
    assert!(
        !is_tool_error(&first),
        "seed navigate must succeed: {first}"
    );
    assert_eq!(worker.gotos.lock().len(), 1);

    let worker2 = Arc::new(RecordingBrowserWorker::new(root.join("worker2")));
    broker.tabs().set_worker(worker2.clone());
    let replay = dispatch_nav(
        &broker,
        &info.tab_id,
        "https://example.test/once",
        "nav-restart",
        info.document_generation.max(1),
    );
    assert!(
        !is_tool_error(&replay),
        "Host ledger must replay after worker swap: {replay}"
    );
    assert_eq!(
        worker2.gotos.lock().len(),
        0,
        "restarted worker must not execute a known actionId"
    );
    assert_eq!(worker.gotos.lock().len(), 1);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn full_host_ledger_rejects_unseen_ids_but_still_replays_known() {
    let fake = Arc::new(FakeAdapter::new());
    let mut opts = enabled_opts(400);
    opts.action_budget = 2;
    let broker = ComputerUseBroker::new(fake, opts);
    broker.open_run("sess-a", "run-a").expect("open");
    let root = std::env::temp_dir().join(format!("cu-r42-full-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    broker.tabs().set_profile_root(root.join("profiles"));
    broker.tabs().set_worker(worker.clone());
    broker.tabs().set_staging_root(root.clone());
    let info = broker
        .tabs()
        .open_managed_profile("sess-a", "run-a", "p1")
        .expect("profile");
    let gen = info.document_generation.max(1);
    for (id, path) in [("nav-a", "a"), ("nav-b", "b")] {
        let result = dispatch_nav(
            &broker,
            &info.tab_id,
            &format!("https://example.test/{path}"),
            id,
            gen,
        );
        assert!(!is_tool_error(&result), "{id} must succeed: {result}");
    }
    let after_two = worker.gotos.lock().len();
    let third = dispatch_nav(
        &broker,
        &info.tab_id,
        "https://example.test/c",
        "nav-c",
        gen,
    );
    assert!(
        is_tool_error(&third),
        "unseen id over budget must fail: {third}"
    );
    assert!(
        tool_text(&third).contains("budget"),
        "full ledger must name budget, got {}",
        tool_text(&third)
    );
    assert_eq!(worker.gotos.lock().len(), after_two);
    let replay = dispatch_nav(
        &broker,
        &info.tab_id,
        "https://example.test/a",
        "nav-a",
        gen,
    );
    assert!(
        !is_tool_error(&replay),
        "known id must still replay when full: {replay}"
    );
    assert_eq!(
        worker.gotos.lock().len(),
        after_two,
        "replay of a known id must not evict-and-reexecute"
    );
    let _ = std::fs::remove_dir_all(root);
}

fn tool_text(result: &serde_json::Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn persist_restore_does_not_restore_grant_or_auto_execute() {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("sess-a", "run-a").expect("open");
    let root = std::env::temp_dir().join(format!("cu-r42-persist-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    broker.tabs().set_profile_root(root.join("profiles"));
    broker.tabs().set_worker(worker.clone());
    broker.tabs().set_staging_root(root.clone());
    let info = broker
        .tabs()
        .open_managed_profile("sess-a", "run-a", "p1")
        .expect("profile");
    let first = dispatch_nav(
        &broker,
        &info.tab_id,
        "https://example.test/persisted?token=secret-value",
        "nav-persist",
        info.document_generation.max(1),
    );
    assert!(
        !is_tool_error(&first),
        "seed navigate must succeed: {first}"
    );
    let snap = broker.persist_run("run-a").expect("persist");
    assert!(
        snap.consumed_action_ids
            .iter()
            .any(|id| id == "nav-persist"),
        "persist must record consumed action ids, got {:?}",
        snap.consumed_action_ids
    );
    let raw = serde_json::to_string(&snap).expect("json");
    assert!(
        !raw.contains("secret-value") && !raw.contains("token="),
        "audit summary must omit URL query values: {raw}"
    );

    let restored = ComputerUseBroker::new(fake, enabled_opts(400));
    restored.restore_run(snap).expect("restore");
    assert!(
        restored.authorized_target("run-a").is_err(),
        "restore must not keep authorization"
    );
    assert!(
        restored.model_snapshot_id("run-a").is_none(),
        "restore must not keep snapshot"
    );

    let root2 = root.join("restored");
    let worker2 = Arc::new(RecordingBrowserWorker::new(root2.clone()));
    restored.tabs().set_profile_root(root2.join("profiles"));
    restored.tabs().set_worker(worker2.clone());
    restored.tabs().set_staging_root(root2.clone());
    let tab2 = restored
        .tabs()
        .open_managed_profile("sess-a", "run-a", "p2")
        .expect("new profile")
        .tab_id;
    let retry = dispatch_nav(
        &restored,
        &tab2,
        "https://example.test/persisted",
        "nav-persist",
        1,
    );
    assert!(
        is_tool_error(&retry),
        "restored run must not execute a persisted actionId: {retry}"
    );
    assert_eq!(
        worker2.gotos.lock().len(),
        0,
        "restore must not auto-execute or replay old browser writes"
    );
    let _ = std::fs::remove_dir_all(root);
}
