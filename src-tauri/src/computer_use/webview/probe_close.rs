//! A normal native controller close does not promise a ProcessFailed event.
//! Exercise lost completion recovery in a fresh, exclusively owned profile.

use super::isolated_windows::probe_fault::{Fault, Phase};
use super::*;
use crate::computer_use::webview_host::HostOwnedWebView;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub(crate) fn run() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!("grok-cu-wv-close-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).map_err(|_| "close fixture directory unavailable")?;
    let page = root.join("page.html");
    std::fs::write(&page, "<!doctype html><title>CU owned close fixture</title><button>Owned control</button><output id='effects'>0</output>")
        .map_err(|_| "close fixture page unavailable")?;
    let url = tauri::Url::from_file_path(&page).map_err(|_| "close fixture URL unavailable")?;
    let old = HostOwnedWebView::spawn_disposable(
        "resource-browser-close-probe",
        "close-old",
        url.as_str(),
        &root.join("old-profile"),
    )?;
    let result = check(&old, url.as_str(), &root);
    // An individual renderer's exit is not proof that every profile child exited.
    // Retain the owned profile until complete environment cleanup is verified.
    old.finish_probe(result)
}

fn check(old: &Arc<HostOwnedWebView>, url: &str, root: &std::path::Path) -> Result<(), String> {
    let adapter = WebViewAdapter::new();
    let original = adapter.bind(
        old.clone(),
        "close-session",
        "close-old",
        &serde_json::json!({}),
    )?;
    let tracker = ScriptTracker::default();
    let operation = tracker.begin(
        "close-old",
        "owned-native",
        &Default::default(),
        &Default::default(),
    )?;
    let (seen, observed) = mpsc::channel();
    let result = old.setup_fault(
        operation.clone(),
        Fault {
            phase: Phase::EvaluationReply,
            observed: seen,
        },
    )?;
    observed
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "close fixture did not reach the real evaluation callback")?;
    if tracker.is_idle("close-old") || effects(old)? != serde_json::json!("1") {
        return Err("close fixture did not retain the lost reply and its one effect".into());
    }
    tracker.cancel("close-old");
    if tracker.is_idle("close-old") {
        return Err("logical Stop incorrectly acknowledged physical close".into());
    }
    let started = Instant::now();
    eprintln!("close-probe: closing disposable native controller normally");
    old.shutdown()?;
    eprintln!(
        "close-probe: native owner returned elapsed_ms={}",
        started.elapsed().as_millis()
    );
    // Include native shutdown, not only the final channel wait, in this gate's
    // existing five-second deadline. Diagnostic grace never converts failure.
    let deadline = Duration::from_secs(5);
    let reply = result.recv_timeout(deadline.saturating_sub(started.elapsed()));
    let reply = match reply {
        Ok(reply) if started.elapsed() <= deadline => reply,
        outcome => {
            let original = format!(
                "normal close missed its completion deadline: {outcome:?}; elapsed_ms={}; idle={}",
                started.elapsed().as_millis(),
                tracker.is_idle("close-old")
            );
            eprintln!("close-probe: {original}");
            if matches!(outcome, Err(mpsc::RecvTimeoutError::Timeout)) {
                let late = result.recv_timeout(Duration::from_secs(15));
                eprintln!(
                    "close-probe: diagnostic_only elapsed_ms={} idle={} late={late:?}",
                    started.elapsed().as_millis(),
                    tracker.is_idle("close-old")
                );
            }
            return Err(original);
        }
    };
    if !matches!(reply, Err(ref error) if error.contains("closed") && error.contains("outcome unknown"))
        || !tracker.is_idle("close-old")
        || adapter.target_alive(&original.target_id)
    {
        return Err(
            "normal close did not preserve unknown outcome and retire the exact old binding".into(),
        );
    }
    let close_ms = started.elapsed().as_millis();
    let replacement = HostOwnedWebView::spawn_disposable(
        "resource-browser-close-probe",
        "close-new",
        url,
        &root.join("new-profile"),
    )?;
    let result = check_replacement(
        &replacement,
        &adapter,
        &tracker,
        &operation,
        &original.target_id,
    );
    replacement.finish_probe(result)?;
    println!("gate: webview_normal_close close_ms={close_ms} lost_callback=true old_effect=1 replacement_initial=0 next_effect=1 same_label_isolated=true no_crash=true");
    Ok(())
}

fn check_replacement(
    replacement: &Arc<HostOwnedWebView>,
    adapter: &WebViewAdapter,
    tracker: &ScriptTracker,
    old: &ScriptOperation,
    old_target: &str,
) -> Result<(), String> {
    if effects(replacement)? != serde_json::json!("0") {
        return Err("closed operation was replayed in the replacement".into());
    }
    let next = tracker.begin(
        "close-new",
        "owned-native",
        &Default::default(),
        &Default::default(),
    )?;
    old.finished();
    tracker.cancel("close-old");
    if next.check().is_err() || tracker.is_idle("close-new") {
        return Err("late old completion or Stop settled the replacement".into());
    }
    replacement.host_script_owned(
        "(() => { const el = document.getElementById('effects'); el.textContent = String(Number(el.textContent) + 1); return {ran: true}; })()",
        replacement.navigation_generation(), next,
    )?;
    if !tracker.is_idle("close-new") || effects(replacement)? != serde_json::json!("1") {
        return Err("normal-close replacement action was not exactly once".into());
    }
    let bound = adapter.bind(
        replacement.clone(),
        "replacement-session",
        "replacement-run",
        &serde_json::json!({}),
    )?;
    if bound.target_id == old_target
        || adapter
            .observe_for_run("replacement-run", &bound.target_id)?
            .nodes
            .is_empty()
    {
        return Err("normal-close replacement requires fresh binding and observation".into());
    }
    adapter.unbind();
    Ok(())
}

fn effects(live: &HostOwnedWebView) -> Result<serde_json::Value, String> {
    serde_json::from_str(&live.host_script("document.getElementById('effects').textContent")?)
        .map_err(|_| "close fixture independent readback invalid".into())
}
