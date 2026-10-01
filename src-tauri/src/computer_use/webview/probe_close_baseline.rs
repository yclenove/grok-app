//! Owned close controls: no CU dispatch versus an acknowledged exact-once action.
//! These use fresh profiles and do not modify any workstation/network settings.

use super::*;
use crate::computer_use::webview_host::HostOwnedWebView;
use std::time::{Duration, Instant};

pub(crate) fn run(completed_action: bool) -> Result<(), String> {
    let mode = if completed_action {
        "completed"
    } else {
        "idle"
    };
    let root =
        std::env::temp_dir().join(format!("grok-cu-wv-close-{mode}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).map_err(|_| "close baseline directory unavailable")?;
    let page = root.join("page.html");
    std::fs::write(
        &page,
        "<!doctype html><title>CU owned close baseline</title><output id='effects'>0</output>",
    )
    .map_err(|_| "close baseline page unavailable")?;
    let url = tauri::Url::from_file_path(&page).map_err(|_| "close baseline URL unavailable")?;
    let live = HostOwnedWebView::spawn_disposable(
        "resource-browser-close-baseline",
        mode,
        url.as_str(),
        &root.join("profile"),
    )?;
    let result = check(&live, mode, completed_action);
    // Preserve action and cleanup failures; retain every owned profile.
    live.finish_probe(result)
}

fn check(live: &HostOwnedWebView, mode: &str, completed_action: bool) -> Result<(), String> {
    let tracker = ScriptTracker::default();
    if completed_action {
        let operation = tracker.begin(
            "close-baseline",
            "baseline-native",
            &Default::default(),
            &Default::default(),
        )?;
        live.host_script_owned(
            "(() => { const el = document.getElementById('effects'); el.textContent = String(Number(el.textContent) + 1); return {ran: true}; })()",
            live.navigation_generation(),
            operation,
        )?;
    }
    let expected = if completed_action { "1" } else { "0" };
    let effects: serde_json::Value =
        serde_json::from_str(&live.host_script("document.getElementById('effects').textContent")?)
            .map_err(|_| "close baseline independent counter unavailable")?;
    if effects != serde_json::json!(expected) || !tracker.is_idle("close-baseline") {
        return Err("close baseline action did not finish with its expected effect".into());
    }
    let started = Instant::now();
    let cleanup = live.shutdown();
    let elapsed = started.elapsed();
    eprintln!(
        "close-baseline: mode={mode} close_ms={} terminal_cleanup_ok={} effects={expected}",
        elapsed.as_millis(),
        cleanup.is_ok()
    );
    cleanup?;
    if elapsed > Duration::from_secs(5) {
        return Err(format!(
            "normal close baseline missed its five-second deadline: mode={mode} elapsed_ms={}",
            elapsed.as_millis()
        ));
    }
    println!(
        "gate: webview_close_baseline mode={mode} close_ms={} effects={expected} terminal_cleanup=true",
        elapsed.as_millis()
    );
    Ok(())
}
