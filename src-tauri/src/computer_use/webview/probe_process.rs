//! Crash only a disposable WebView2 profile owned by this fixture. A withheld
//! native completion tests recovery without pretending that cancellation is exit.

use super::isolated_windows::probe_fault::{Fault, Phase};
use super::*;
use crate::computer_use::webview_host::HostOwnedWebView;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub(crate) fn run() -> Result<(), String> {
    eprintln!("renderer-probe: creating disposable fixture");
    let root = std::env::temp_dir().join(format!("grok-cu-wv-process-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).map_err(|_| "process fixture directory unavailable")?;
    let page = root.join("page.html");
    std::fs::write(
        &page,
        "<!doctype html><title>CU disposable renderer fixture</title><button>Owned control</button><output id='effects'>0</output>",
    )
    .map_err(|_| "process fixture page unavailable")?;
    let url = tauri::Url::from_file_path(&page).map_err(|_| "process fixture URL unavailable")?;
    let live = HostOwnedWebView::spawn_disposable(
        "resource-browser-process-probe",
        "owned-process",
        url.as_str(),
        &root.join("profile"),
    )?;
    let result = check(&live, url.as_str());
    eprintln!("renderer-probe: shutting down owned fixture");
    // Do not remove a profile until native browser + child exit is proven.
    live.finish_probe(result)
}

fn check(live: &Arc<HostOwnedWebView>, url: &str) -> Result<(), String> {
    eprintln!("renderer-probe: binding native document");
    let adapter = WebViewAdapter::new();
    let original = adapter.bind(
        live.clone(),
        "process-session",
        "process-old",
        &serde_json::json!({}),
    )?;
    let original_generation = live.navigation_generation();
    let tracker = ScriptTracker::default();
    let operation = tracker.begin(
        "process-old",
        "owned-native",
        &Default::default(),
        &Default::default(),
    )?;
    let (seen, observed) = mpsc::channel();
    let result = live.setup_fault(
        operation.clone(),
        Fault {
            phase: Phase::EvaluationReply,
            observed: seen,
        },
    )?;
    eprintln!("renderer-probe: waiting for actual evaluation callback");
    observed
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "native evaluation callback fault not reached")?;
    if tracker.is_idle("process-old") || effects(live)? != serde_json::json!("1") {
        return Err("lost evaluation callback did not retain occupancy and its one effect".into());
    }
    tracker.cancel("process-old");
    if tracker.is_idle("process-old") {
        return Err("logical cancellation incorrectly released lost callback occupancy".into());
    }
    let start = Instant::now();
    eprintln!("renderer-probe: crashing disposable renderer");
    live.crash_disposable_renderer()?;
    let outcome = result.recv_timeout(Duration::from_secs(5)).map_err(|_| {
        let failure = format!(
            "renderer exit did not settle lost native callback; generation_changed={} idle={}",
            live.navigation_generation() != original_generation,
            tracker.is_idle("process-old")
        );
        eprintln!(
            "renderer-probe: deadline_failed; observing late exit without retry or pass conversion"
        );
        let late = result.recv_timeout(Duration::from_secs(10));
        eprintln!(
            "renderer-probe: late_reply={} elapsed_ms={} idle={}",
            late.is_ok(),
            start.elapsed().as_millis(),
            tracker.is_idle("process-old")
        );
        failure
    })?;
    if !matches!(outcome, Err(ref error) if error.contains("renderer exited") && error.contains("outcome unknown"))
    {
        return Err("renderer crash did not preserve the unknown action outcome".into());
    }
    if !tracker.is_idle("process-old") || live.navigation_generation() == original_generation {
        return Err("renderer exit retained occupancy or old document authority".into());
    }
    if adapter.target_alive(&original.target_id) || !adapter.list_targets()?.is_empty() {
        return Err("renderer crash left the original binding usable".into());
    }
    let exit_ms = start.elapsed().as_millis();
    eprintln!("renderer-probe: explicitly reloading after confirmed exit");
    live.navigate(url)?;
    live.fixture_ready()?;
    if effects(live)? != serde_json::json!("0") {
        return Err("old action was replayed while recovering the renderer".into());
    }
    let next = tracker.begin(
        "process-next",
        "owned-native",
        &Default::default(),
        &Default::default(),
    )?;
    operation.finished(); // A very late old native completion must not settle this new ticket.
    tracker.cancel("process-old");
    if tracker.is_idle("process-next") || next.check().is_err() {
        return Err("old native completion/Stop touched replacement ownership".into());
    }
    live.host_script_owned(
        "(() => { const el = document.getElementById('effects'); el.textContent = String(Number(el.textContent) + 1); return {ran: true}; })()",
        live.navigation_generation(), next,
    )?;
    eprintln!("renderer-probe: checking new action and explicit binding");
    if !tracker.is_idle("process-next") || effects(live)? != serde_json::json!("1") {
        return Err("replacement renderer did not execute exactly one fresh action".into());
    }
    let replacement = adapter.bind(
        live.clone(),
        "replacement-session",
        "replacement-run",
        &serde_json::json!({}),
    )?;
    let observation = adapter.observe_for_run("replacement-run", &replacement.target_id)?;
    if replacement.target_id == original.target_id || observation.nodes.is_empty() {
        return Err(
            "explicit recovery failed to create fresh native binding and observation".into(),
        );
    }
    adapter.unbind();
    println!("gate: webview_renderer_exit exit_ms={exit_ms} lost_callback=true old_effect=1 recovery_effect=0 next_effect=1 stale_completion_isolated=true explicit_rebind=true");
    Ok(())
}

fn effects(live: &HostOwnedWebView) -> Result<serde_json::Value, String> {
    serde_json::from_str(&live.host_script("document.getElementById('effects').textContent")?)
        .map_err(|_| "process independent readback invalid".into())
}
