//! Real native setup with one deliberately withheld callback/event. Cancellation
//! must reject it before the five-second setup deadline, with no script effect.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::isolated_windows::probe_fault::{Fault, Phase};
use super::*;
use crate::computer_use::webview_host::HostOwnedWebView;

pub(super) fn run() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!("grok-cu-wv-setup-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).map_err(|_| "setup fixture directory unavailable")?;
    let page = root.join("page.html");
    std::fs::write(
        &page,
        "<!doctype html><title>CU owned setup fixture</title><output id='effects'>0</output>",
    )
    .map_err(|_| "setup fixture page unavailable")?;
    let url = tauri::Url::from_file_path(&page).map_err(|_| "setup fixture URL unavailable")?;
    for phase in [
        Phase::FrameReply,
        Phase::ContextEvent,
        Phase::CreateReply,
        Phase::ProcessIdentityReply,
    ] {
        let live =
            HostOwnedWebView::spawn("resource-browser-setup-probe", "owned-setup", url.as_str())?;
        let result = check(&live, phase);
        live.finish_probe(result)?;
    }
    Ok(())
}

fn check(live: &HostOwnedWebView, phase: Phase) -> Result<(), String> {
    let tracker = ScriptTracker::default();
    let operation = tracker.begin(
        "setup-old",
        "owned-view",
        &Default::default(),
        &Default::default(),
    )?;
    let (seen, observed) = mpsc::channel();
    let result = live.setup_fault(
        operation,
        Fault {
            phase,
            observed: seen,
        },
    )?;
    observed
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| format!("native setup fault was not reached: {phase:?}"))?;
    if tracker.is_idle("setup-old") {
        return Err("withheld native setup callback falsely reported idle".into());
    }
    let start = Instant::now();
    tracker.cancel("setup-old");
    let cancelled = result
        .recv_timeout(Duration::from_secs(1))
        .map_err(|_| format!("cancelled native setup waited for its full deadline: {phase:?}"))?;
    if !matches!(cancelled, Err(ref error) if error.contains("cancelled before execution")) {
        return Err("native setup did not acknowledge cancellation before evaluation".into());
    }
    let stop_ms = start.elapsed().as_millis();
    if !tracker.is_idle("setup-old") || effects(live)? != serde_json::json!("0") {
        return Err("cancelled native setup retained occupancy or executed a side effect".into());
    }
    let next = tracker.begin(
        "setup-next",
        "owned-view",
        &Default::default(),
        &Default::default(),
    )?;
    tracker.cancel("setup-old");
    live.host_script_owned(
        "(() => { const el = document.getElementById('effects'); el.textContent = String(Number(el.textContent) + 1); return {ran: true}; })()",
        live.navigation_generation(), next,
    )?;
    if !tracker.is_idle("setup-next") || effects(live)? != serde_json::json!("1") {
        return Err("cancelled setup affected the replacement's exactly-once execution".into());
    }
    println!(
        "gate: webview_setup_cancel phase={phase:?} stop_ms={stop_ms} old_effect=0 next_effect=1"
    );
    Ok(())
}

fn effects(live: &HostOwnedWebView) -> Result<serde_json::Value, String> {
    serde_json::from_str(&live.host_script("document.getElementById('effects').textContent")?)
        .map_err(|_| "setup independent effect readback invalid".into())
}
