//! R4.1: every browser write is admitted by one Broker-owned dispatch.
//! Failed gates must produce the matching Broker error and zero extra worker calls.

use super::gates::{click_req, enabled_opts};
use super::*;
use crate::browser::RecordingBrowserWorker;
use crate::fake::FakeAdapter;
use crate::protocol::OutcomeKind;
use std::time::{Duration, Instant};

struct Ready {
    broker: ComputerUseBroker,
    worker: Arc<RecordingBrowserWorker>,
    tab: String,
    gen: u64,
    root: std::path::PathBuf,
}

fn ready() -> Result<Ready, String> {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker
        .open_run("sess-a", "run-a")
        .map_err(|e| e.to_string())?;
    broker
        .authorize_target("run-a", &fake.fixture_id())
        .map_err(|e| e.to_string())?;
    let root = std::env::temp_dir().join(format!("cu-r41-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    broker.tabs().set_profile_root(root.join("profiles"));
    broker.tabs().set_worker(worker.clone());
    broker.tabs().set_staging_root(root.clone());
    let info = broker
        .tabs()
        .open_managed_profile("sess-a", "run-a", "p1")
        .map_err(|e| e.to_string())?;
    Ok(Ready {
        broker,
        worker,
        tab: info.tab_id,
        gen: info.document_generation.max(1),
        root,
    })
}

fn binding() -> crate::ipc::SessionBinding {
    crate::ipc::SessionBinding {
        session_id: "sess-a".into(),
        run_id: "run-a".into(),
    }
}

fn nav_args(tab: &str, url: &str, action_id: &str, page_generation: u64) -> serde_json::Value {
    serde_json::json!({
        "tabId": tab,
        "url": url,
        "actionId": action_id,
        "pageGeneration": page_generation,
    })
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
        nav_args(tab, url, action_id, page_generation),
    )
}

fn is_tool_error(result: &serde_json::Value) -> bool {
    result.get("isError") == Some(&serde_json::json!(true))
}

fn tool_text(result: &serde_json::Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn tools_rs_does_not_call_raw_tab_writes() {
    let src = include_str!("../tools.rs");
    assert!(
        !src.contains("navigate_with_action"),
        "tools.rs must not call tabs.navigate_with_action"
    );
    assert!(
        !src.contains("stage_download"),
        "tools.rs must not call tabs.stage_download"
    );
    assert!(
        !src.contains("act_managed"),
        "tools.rs must not call tabs.act_managed"
    );
    assert!(
        !src.contains(".tabs."),
        "tools.rs must not reach ExistingTabHost directly"
    );
}

#[test]
fn paused_navigate_is_rejected_with_zero_worker_dispatch() {
    let ready = ready().expect("ready");
    ready.broker.pause("run-a").expect("pause");
    let before = ready.worker.gotos.lock().len();
    let result = dispatch_nav(
        &ready.broker,
        &ready.tab,
        "https://example.test/paused",
        "nav-paused",
        ready.gen,
    );
    assert!(
        is_tool_error(&result),
        "paused navigate must error: {result}"
    );
    let text = tool_text(&result);
    assert!(
        text.contains("paused"),
        "paused gate must name pause, got {text}"
    );
    assert_eq!(
        ready.worker.gotos.lock().len(),
        before,
        "paused navigate must not reach the worker"
    );
    let _ = std::fs::remove_dir_all(&ready.root);
}

#[test]
fn stopped_navigate_is_rejected_with_zero_worker_dispatch() {
    let ready = ready().expect("ready");
    let before = ready.worker.gotos.lock().len();
    ready.broker.request_stop("run-a").expect("stop");
    let result = dispatch_nav(
        &ready.broker,
        &ready.tab,
        "https://example.test/stopped",
        "nav-stopped",
        ready.gen,
    );
    assert!(
        is_tool_error(&result),
        "stopped navigate must error: {result}"
    );
    let text = tool_text(&result);
    assert!(
        text.contains("stop"),
        "stop gate must name stop, got {text}"
    );
    assert_eq!(
        ready.worker.gotos.lock().len(),
        before,
        "stopped navigate must not reach the worker"
    );
    let _ = std::fs::remove_dir_all(&ready.root);
}

#[test]
fn feature_disabled_navigate_is_zero_dispatch() {
    let ready = ready().expect("ready");
    ready.broker.set_feature_enabled(false);
    let before = ready.worker.gotos.lock().len();
    let result = dispatch_nav(
        &ready.broker,
        &ready.tab,
        "https://example.test/off",
        "nav-off",
        ready.gen,
    );
    assert!(
        is_tool_error(&result),
        "disabled navigate must error: {result}"
    );
    assert_eq!(ready.worker.gotos.lock().len(), before);
    let _ = std::fs::remove_dir_all(&ready.root);
}

#[test]
fn unauthorized_tab_is_zero_dispatch() {
    let ready = ready().expect("ready");
    let before = ready.worker.gotos.lock().len();
    let result = dispatch_nav(
        &ready.broker,
        "missing-tab",
        "https://example.test/other",
        "nav-other",
        ready.gen,
    );
    assert!(is_tool_error(&result), "missing tab must error: {result}");
    assert_eq!(ready.worker.gotos.lock().len(), before);
    let _ = std::fs::remove_dir_all(&ready.root);
}

#[test]
fn stale_page_generation_is_zero_dispatch() {
    let ready = ready().expect("ready");
    let before = ready.worker.gotos.lock().len();
    let result = dispatch_nav(
        &ready.broker,
        &ready.tab,
        "https://example.test/stale",
        "nav-stale",
        ready.gen.saturating_add(9),
    );
    assert!(
        is_tool_error(&result),
        "stale pageGeneration must error: {result}"
    );
    let text = tool_text(&result);
    assert!(
        text.to_ascii_lowercase().contains("generation") || text.contains("identity"),
        "stale generation must be an identity error, got {text}"
    );
    assert_eq!(ready.worker.gotos.lock().len(), before);
    let _ = std::fs::remove_dir_all(&ready.root);
}

#[test]
fn unknown_without_observe_blocks_browser_write() {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    fake.set_hang(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(40));
    broker.open_run("sess-a", "run-a").expect("open");
    let tid = fake.fixture_id();
    let gen = broker.authorize_target("run-a", &tid).expect("auth");
    let obs = broker.observe("run-a").expect("obs");
    let root = std::env::temp_dir().join(format!("cu-r41-unk-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    broker.tabs().set_profile_root(root.join("profiles"));
    broker.tabs().set_worker(worker.clone());
    broker.tabs().set_staging_root(root.clone());
    let tab = broker
        .tabs()
        .open_managed_profile("sess-a", "run-a", "p1")
        .expect("profile")
        .tab_id;
    let out = broker.act(click_req(
        "run-a",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "hang-1",
    ));
    assert_eq!(out.kind, OutcomeKind::Unknown, "timeout must be unknown");
    fake.finish_in_flight();
    let until = Instant::now() + Duration::from_secs(2);
    while fake.adapter_in_flight() != 0 && Instant::now() < until {
        std::thread::yield_now();
    }
    let before = worker.gotos.lock().len();
    let result = dispatch_nav(
        &broker,
        &tab,
        "https://example.test/after-unknown",
        "nav-after-unknown",
        1,
    );
    assert!(
        is_tool_error(&result),
        "unknown must block browser writes: {result}"
    );
    let text = tool_text(&result);
    assert!(
        text.contains("unknown") && text.contains("observe"),
        "unknown gate must require observe, got {text}"
    );
    assert_eq!(worker.gotos.lock().len(), before);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn action_budget_is_shared_with_browser_writes() {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    let mut opts = enabled_opts(400);
    opts.action_budget = 1;
    let broker = ComputerUseBroker::new(fake.clone(), opts);
    broker.open_run("sess-a", "run-a").expect("open");
    let tid = fake.fixture_id();
    let gen = broker.authorize_target("run-a", &tid).expect("auth");
    let obs = broker.observe("run-a").expect("obs");
    let root = std::env::temp_dir().join(format!("cu-r41-bud-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    broker.tabs().set_profile_root(root.join("profiles"));
    broker.tabs().set_worker(worker.clone());
    broker.tabs().set_staging_root(root.clone());
    let tab = broker
        .tabs()
        .open_managed_profile("sess-a", "run-a", "p1")
        .expect("profile")
        .tab_id;
    let desktop = broker.act(click_req(
        "run-a",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "desk-1",
    ));
    assert_ne!(desktop.kind, OutcomeKind::Rejected, "{:?}", desktop.reason);
    let before = worker.gotos.lock().len();
    let result = dispatch_nav(
        &broker,
        &tab,
        "https://example.test/budget",
        "nav-budget",
        1,
    );
    assert!(
        is_tool_error(&result),
        "exhausted budget must error: {result}"
    );
    let text = tool_text(&result);
    assert!(
        text.contains("budget"),
        "budget gate must name budget, got {text}"
    );
    assert_eq!(worker.gotos.lock().len(), before);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn desktop_in_flight_blocks_browser_write() {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    fake.set_hang(true);
    fake.set_abort_does_not_quiesce(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("sess-a", "run-a").expect("open");
    let tid = fake.fixture_id();
    let gen = broker.authorize_target("run-a", &tid).expect("auth");
    let obs = broker.observe("run-a").expect("obs");
    let root = std::env::temp_dir().join(format!("cu-r41-if-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    broker.tabs().set_profile_root(root.join("profiles"));
    broker.tabs().set_worker(worker.clone());
    broker.tabs().set_staging_root(root.clone());
    let tab = broker
        .tabs()
        .open_managed_profile("sess-a", "run-a", "p1")
        .expect("profile")
        .tab_id;
    let req = click_req(
        "run-a",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "hang-if",
    );
    let broker = Arc::new(broker);
    let pending = broker.clone();
    let handle = std::thread::spawn(move || pending.act(req));
    let until = Instant::now() + Duration::from_secs(2);
    while fake.adapter_in_flight() == 0 && Instant::now() < until {
        std::thread::yield_now();
    }
    assert!(
        fake.adapter_in_flight() > 0,
        "desktop act must be in flight"
    );
    let before = worker.gotos.lock().len();
    let result = dispatch_nav(
        &broker,
        &tab,
        "https://example.test/inflight",
        "nav-inflight",
        1,
    );
    assert!(
        is_tool_error(&result),
        "in-flight must block navigate: {result}"
    );
    let text = tool_text(&result);
    assert!(
        text.contains("lease")
            || text.contains("held")
            || text.contains("in-flight")
            || text.contains("in_flight"),
        "in-flight gate must be lease/held, got {text}"
    );
    assert_eq!(worker.gotos.lock().len(), before);
    fake.finish_in_flight();
    let _ = handle.join();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn fingerprint_conflict_does_not_execute_a_second_time() {
    let ready = ready().expect("ready");
    let first = dispatch_nav(
        &ready.broker,
        &ready.tab,
        "https://example.test/one",
        "nav-same",
        ready.gen,
    );
    assert!(
        !is_tool_error(&first),
        "first navigate must succeed: {first}"
    );
    let after_first = ready.worker.gotos.lock().len();
    assert!(after_first >= 1, "first navigate must hit the worker");
    let second = dispatch_nav(
        &ready.broker,
        &ready.tab,
        "https://example.test/two",
        "nav-same",
        ready.gen,
    );
    assert!(
        is_tool_error(&second),
        "same actionId different url must conflict: {second}"
    );
    let text = tool_text(&second);
    assert!(
        text.contains("fingerprint") || text.contains("conflict"),
        "conflict must name fingerprint, got {text}"
    );
    assert_eq!(
        ready.worker.gotos.lock().len(),
        after_first,
        "conflict must not dispatch again"
    );
    let _ = std::fs::remove_dir_all(&ready.root);
}

#[test]
fn same_fingerprint_replays_without_second_dispatch() {
    let ready = ready().expect("ready");
    let first = dispatch_nav(
        &ready.broker,
        &ready.tab,
        "https://example.test/replay",
        "nav-replay",
        ready.gen,
    );
    assert!(
        !is_tool_error(&first),
        "first navigate must succeed: {first}"
    );
    let after_first = ready.worker.gotos.lock().len();
    let second = dispatch_nav(
        &ready.broker,
        &ready.tab,
        "https://example.test/replay",
        "nav-replay",
        ready.gen,
    );
    assert!(
        !is_tool_error(&second),
        "same id+fingerprint must replay: {second}"
    );
    assert_eq!(
        ready.worker.gotos.lock().len(),
        after_first,
        "replay must not hit the worker again"
    );
    let _ = std::fs::remove_dir_all(&ready.root);
}

#[test]
fn download_without_current_snapshot_is_zero_dispatch() {
    let ready = ready().expect("ready");
    let before = *ready.worker.downloads.lock();
    let result = crate::tools::dispatch(
        &ready.broker,
        &binding(),
        "computer_download",
        serde_json::json!({
            "tabId": ready.tab,
            "filename": "report.bin",
            "actionId": "dl-stale",
            "pageGeneration": ready.gen,
            "snapshotId": "not-current",
            "elementRef": "rec-ref",
        }),
    );
    assert!(
        is_tool_error(&result),
        "stale snapshot must error: {result}"
    );
    assert_eq!(*ready.worker.downloads.lock(), before);
    let _ = std::fs::remove_dir_all(&ready.root);
}

#[test]
fn download_after_observe_reaches_worker_once() {
    let ready = ready().expect("ready");
    let obs = ready
        .broker
        .tabs()
        .observe_managed("run-a", "p1")
        .expect("observe");
    let element = obs
        .nodes
        .iter()
        .find(|n| !n.element_ref.is_empty())
        .map(|n| n.element_ref.clone())
        .expect("ref");
    let before = *ready.worker.downloads.lock();
    let result = crate::tools::dispatch(
        &ready.broker,
        &binding(),
        "computer_download",
        serde_json::json!({
            "tabId": ready.tab,
            "filename": "report.bin",
            "actionId": "dl-ok",
            "pageGeneration": obs.page_generation.max(1),
            "snapshotId": obs.snapshot_id,
            "elementRef": element,
        }),
    );
    assert!(
        !is_tool_error(&result),
        "current snapshot download must succeed: {result}"
    );
    assert_eq!(*ready.worker.downloads.lock(), before + 1);
    let _ = std::fs::remove_dir_all(&ready.root);
}

#[test]
fn navigate_trace_omits_url_query() {
    let ready = ready().expect("ready");
    let result = dispatch_nav(
        &ready.broker,
        &ready.tab,
        "https://example.test/path?secret=token-value",
        "nav-trace",
        ready.gen,
    );
    assert!(
        !is_tool_error(&result),
        "https navigate must succeed: {result}"
    );
    let traces = ready.broker.traces(Some("run-a"), false);
    let blob = traces
        .iter()
        .map(|t| format!("{}:{}", t.kind, t.detail))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        traces
            .iter()
            .any(|t| t.kind.contains("browser") || t.kind.contains("navigate")),
        "navigate must emit a trace, got {blob}"
    );
    assert!(
        !blob.contains("secret=") && !blob.contains("token-value"),
        "trace must omit URL query, got {blob}"
    );
    let _ = std::fs::remove_dir_all(&ready.root);
}

#[test]
fn paused_broker_navigate_wrapper_is_zero_dispatch() {
    let ready = ready().expect("ready");
    ready.broker.pause("run-a").expect("pause");
    let before = ready.worker.gotos.lock().len();
    let err = ready
        .broker
        .browser_navigate("run-a", &ready.tab, "https://example.test/wrap", "nav-wrap")
        .expect_err("paused wrapper must fail");
    assert!(
        err.to_string().contains("paused"),
        "wrapper must fail closed on pause, got {err}"
    );
    assert_eq!(ready.worker.gotos.lock().len(), before);
    let _ = std::fs::remove_dir_all(&ready.root);
}

#[test]
fn paused_typed_act_upload_open_and_popup_are_zero_dispatch() {
    let ready = ready().expect("ready");
    let opens_before = *ready.worker.opens.lock();
    ready.broker.pause("run-a").expect("pause");
    let act_before = *ready.worker.act_calls.lock();
    let up_before = *ready.worker.uploads.lock();
    let err = ready
        .broker
        .browser_act_managed(
            "run-a",
            "p1",
            "act-paused",
            "click",
            crate::browser::ManagedLocator {
                element_ref: Some("rec-ref".into()),
            },
            serde_json::json!({}),
        )
        .expect_err("paused act must fail");
    assert!(
        err.to_string().contains("paused"),
        "act must fail closed on pause, got {err}"
    );
    assert_eq!(*ready.worker.act_calls.lock(), act_before);
    let staged = ready.root.join("note.txt");
    std::fs::write(&staged, b"hi").expect("stage");
    let err = ready
        .broker
        .browser_upload("run-a", &ready.tab, "up-paused", "rec-ref", &staged)
        .expect_err("paused upload must fail");
    assert!(
        err.to_string().contains("paused"),
        "upload must fail closed on pause, got {err}"
    );
    assert_eq!(*ready.worker.uploads.lock(), up_before);
    let err = ready
        .broker
        .browser_open_tab("sess-a", "run-a", "p2", "open-paused")
        .expect_err("paused open must fail");
    assert!(
        err.to_string().contains("paused"),
        "open must fail closed on pause, got {err}"
    );
    assert_eq!(*ready.worker.opens.lock(), opens_before);
    let err = ready
        .broker
        .browser_popup("run-a", "p1", "pop-paused", "rec-ref")
        .expect_err("paused popup must fail");
    assert!(
        err.to_string().contains("paused"),
        "popup must fail closed on pause, got {err}"
    );
    assert_eq!(*ready.worker.act_calls.lock(), act_before);
    let _ = std::fs::remove_dir_all(&ready.root);
}
