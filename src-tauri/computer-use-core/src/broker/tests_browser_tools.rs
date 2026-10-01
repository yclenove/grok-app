//! R5: model browser tools enter tools::dispatch and Broker dispatch.

use super::gates::enabled_opts;
use super::*;
use crate::browser::RecordingBrowserWorker;
use crate::fake::FakeAdapter;

fn ready() -> (
    ComputerUseBroker,
    Arc<RecordingBrowserWorker>,
    String,
    std::path::PathBuf,
) {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake, enabled_opts(400));
    broker.open_run("sess-a", "run-a").expect("open");
    let root = std::env::temp_dir().join(format!("cu-r5-{}", Uuid::new_v4()));
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

fn binding() -> crate::ipc::SessionBinding {
    crate::ipc::SessionBinding {
        session_id: "sess-a".into(),
        run_id: "run-a".into(),
    }
}

fn call(broker: &ComputerUseBroker, name: &str, args: serde_json::Value) -> serde_json::Value {
    crate::tools::dispatch(broker, &binding(), name, args)
}

fn is_err(result: &serde_json::Value) -> bool {
    result.get("isError") == Some(&serde_json::json!(true))
}

fn payload(result: &serde_json::Value) -> serde_json::Value {
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    serde_json::from_str(text).unwrap_or(serde_json::json!({}))
}

#[test]
fn legacy_observe_respects_admission_budget_and_invalidates_generic_snapshot() {
    let (mut broker, worker, tab, _root) = ready();
    broker.observe_budget = 1;
    let inflight = {
        let mut state = broker.inner.lock();
        let run = state.runs.get_mut("run-a").unwrap();
        run.snapshot_id = Some("old-generic".into());
        run.last_unknown = true;
        run.in_flight.store(true, Ordering::SeqCst);
        run.in_flight.clone()
    };
    let permit = InFlightClear(inflight);
    let args = serde_json::json!({"tabId":tab,"pageGeneration":1});
    let busy = call(&broker, "browser_observe", args.clone());
    assert!(is_err(&busy), "{busy}");
    assert!(worker.capture_options.lock().is_empty());
    assert_eq!(broker.inner.lock().runs["run-a"].observe_count, 0);
    drop(permit);
    let observation = call(&broker, "browser_observe", args.clone());
    assert!(!is_err(&observation), "{observation}");
    assert_eq!(worker.capture_options.lock().len(), 1);
    {
        let state = broker.inner.lock();
        let run = &state.runs["run-a"];
        assert!(run.snapshot_id.is_none());
        assert!(!run.last_unknown);
    }
    let exhausted = call(&broker, "browser_observe", args);
    assert!(is_err(&exhausted), "{exhausted}");
    assert_eq!(worker.capture_options.lock().len(), 1);
}

#[test]
fn browser_wait_cannot_bypass_the_shared_read_budget() {
    let (mut broker, worker, tab, _root) = ready();
    broker.observe_budget = 2;
    assert!(!is_err(&call(
        &broker,
        "browser_observe",
        serde_json::json!({"tabId":tab,"pageGeneration":1})
    )));
    let mut args = serde_json::json!({
        "tabId":tab,"pageGeneration":1,"snapshotId":"rec-snap","elementRef":"rec-ref",
        "actionId":"wait-one","kind":"wait","parameters":{"nameEquals":"+1","timeoutMs":100},
    });
    let first = call(&broker, "browser_act", args.clone());
    assert!(!is_err(&first), "{first}");
    assert_eq!(*worker.act_calls.lock(), 1);
    assert!(!is_err(&call(&broker, "browser_act", args.clone())));
    assert_eq!(
        *worker.act_calls.lock(),
        1,
        "replay must not re-read the condition"
    );
    args["actionId"] = serde_json::json!("wait-two");
    let exhausted = call(&broker, "browser_act", args);
    assert!(is_err(&exhausted), "{exhausted}");
    assert!(exhausted.to_string().contains("budget exhausted"));
    assert_eq!(*worker.act_calls.lock(), 1);
}

#[test]
fn model_catalog_includes_browser_tools_and_rejects_host_only() {
    for name in [
        "browser_list_tabs",
        "browser_open",
        "browser_observe",
        "browser_act",
    ] {
        assert!(
            crate::tools::MODEL_TOOLS.contains(&name),
            "{name} must be a model tool"
        );
        assert!(
            !crate::tools::HOST_ONLY_TOOLS.contains(&name),
            "{name} must not be host-only"
        );
    }
    let (broker, worker, _tab, root) = ready();
    for name in crate::tools::HOST_ONLY_TOOLS {
        let result = call(&broker, name, serde_json::json!({}));
        assert!(is_err(&result), "host-only {name} must fail: {result}");
    }
    assert_eq!(*worker.act_calls.lock(), 0);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn list_tabs_does_not_leak_foreign_run_or_query() {
    let (broker, worker, tab, root) = ready();
    broker
        .tabs()
        .navigate_with_action(
            "run-a",
            &tab,
            1,
            "seed-nav",
            "https://example.test/a?secret=token",
        )
        .ok();
    let other = broker
        .tabs()
        .open_managed_profile("sess-b", "run-b", "other")
        .expect("other");
    let listed = call(&broker, "browser_list_tabs", serde_json::json!({}));
    assert!(!is_err(&listed), "list must succeed: {listed}");
    let body = payload(&listed);
    let tabs = body["tabs"].as_array().cloned().unwrap_or_default();
    assert!(
        tabs.iter().any(|row| row["tabId"] == tab),
        "own tab missing: {body}"
    );
    assert!(
        tabs.iter().all(|row| row["tabId"] != other.tab_id),
        "foreign tab leaked: {body}"
    );
    let blob = body.to_string();
    assert!(
        !blob.contains("secret=") && !blob.contains("token"),
        "list must omit URL query: {blob}"
    );
    assert!(!blob.contains("pageId"));
    let _ = worker;
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn open_does_not_create_a_profile_or_grant() {
    let (broker, worker, tab, root) = ready();
    let opens = *worker.opens.lock();
    let missing = call(
        &broker,
        "browser_open",
        serde_json::json!({"tabId": "not-a-tab"}),
    );
    assert!(is_err(&missing), "unknown tab must fail: {missing}");
    assert_eq!(
        *worker.opens.lock(),
        opens,
        "open must not create a profile"
    );
    let ok = call(&broker, "browser_open", serde_json::json!({"tabId": tab}));
    assert!(!is_err(&ok), "authorized tab must open: {ok}");
    assert_eq!(payload(&ok)["tabId"], tab);
    assert_eq!(*worker.opens.lock(), opens);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn observe_text_omits_png_bytes() {
    let (broker, _worker, tab, root) = ready();
    let result = call(
        &broker,
        "browser_observe",
        serde_json::json!({"tabId": tab, "pageGeneration": 1}),
    );
    assert!(!is_err(&result), "observe must succeed: {result}");
    let content = result["content"].as_array().cloned().unwrap_or_default();
    let text = content
        .iter()
        .find(|part| part["type"] == "text")
        .and_then(|part| part["text"].as_str())
        .unwrap_or("");
    assert!(
        !text.contains("pngBase64") && !text.contains("png_base64") && !text.contains("iVBOR"),
        "observe text must omit png bytes: {text}"
    );
    assert!(
        text.contains("snapshotId") || text.contains("elementRef") || text.contains("rec-snap")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn act_without_identity_is_zero_dispatch() {
    let (broker, worker, tab, root) = ready();
    let before = *worker.act_calls.lock();
    let missing = call(
        &broker,
        "browser_act",
        serde_json::json!({
            "tabId": tab,
            "actionId": "act-1",
            "kind": "click",
            "pageGeneration": 1
        }),
    );
    assert!(is_err(&missing), "missing snapshot must fail: {missing}");
    assert_eq!(*worker.act_calls.lock(), before);
    let stale = call(
        &broker,
        "browser_act",
        serde_json::json!({
            "tabId": tab,
            "actionId": "act-2",
            "kind": "click",
            "pageGeneration": 1,
            "snapshotId": "not-current",
            "elementRef": "rec-ref"
        }),
    );
    assert!(is_err(&stale), "stale snapshot must fail: {stale}");
    assert_eq!(*worker.act_calls.lock(), before);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn act_after_observe_reaches_worker_once() {
    let (broker, worker, tab, root) = ready();
    let obs = call(
        &broker,
        "browser_observe",
        serde_json::json!({"tabId": tab, "pageGeneration": 1}),
    );
    assert!(!is_err(&obs), "observe must succeed: {obs}");
    let before = *worker.act_calls.lock();
    let acted = call(
        &broker,
        "browser_act",
        serde_json::json!({
            "tabId": tab,
            "actionId": "act-ok",
            "kind": "click",
            "pageGeneration": 1,
            "snapshotId": "rec-snap",
            "elementRef": "rec-ref"
        }),
    );
    assert!(!is_err(&acted), "typed act must succeed: {acted}");
    assert_eq!(*worker.act_calls.lock(), before + 1);
    let _ = std::fs::remove_dir_all(root);
}
