//! Real native callback arriving after the Host caller's response deadline.
//! Uses only its own fixture; it does not terminate or navigate a borrowed page.

use super::*;
use crate::computer_use::adapter::ActionScope;
use crate::computer_use::protocol::{ActionKind, ActionTarget};
use crate::computer_use::webview_host::HostOwnedWebView;
use std::time::{Duration, Instant};

pub(super) fn run() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!("grok-cu-wv-timeout-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).map_err(|_| "timeout fixture directory unavailable")?;
    let page = root.join("page.html");
    std::fs::write(
        &page,
        r#"<!doctype html><meta charset="utf-8"><title>CU native timeout fixture</title>
<button id="slow">Slow owned action</button><output id="effect">0</output>
<script>
document.getElementById('slow').onclick = () => {
  const end = performance.now() + 10000;
  while (performance.now() < end) { /* Deliberately exceeds the 8s Host reply deadline. */ }
  const effect = document.getElementById('effect');
  effect.textContent = String(Number(effect.textContent) + 1);
};
</script>"#,
    )
    .map_err(|_| "timeout fixture page unavailable")?;
    let url = tauri::Url::from_file_path(&page).map_err(|_| "timeout fixture URL unavailable")?;
    let live = HostOwnedWebView::spawn(
        "resource-browser-native-timeout",
        "native-timeout",
        url.as_str(),
    )?;
    let result = check(&live);
    live.finish_probe(result)
}

fn check(live: &Arc<HostOwnedWebView>) -> Result<(), String> {
    let adapter = WebViewAdapter::new();
    let target = adapter.bind(
        live.clone(),
        "timeout-session",
        "timeout-run",
        &serde_json::json!({}),
    )?;
    let observed = adapter.observe(&target.target_id)?;
    let element_ref = observed
        .nodes
        .iter()
        .find(|node| node.name == "Slow owned action")
        .ok_or("timeout fixture control missing")?
        .node_ref
        .clone();
    let started = Instant::now();
    let outcome = adapter.act(&DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "timeout-run".into(),
        action_id: "slow-once".into(),
        generation: 1,
        target_id: target.target_id.clone(),
        target_generation: target.lifecycle_stamp,
        snapshot_id: observed.snapshot_id,
        geometry_revision: target.lifecycle_stamp,
        action: ActionKind::Click,
        target: ActionTarget::Element { element_ref },
        parameters: serde_json::json!({}),
        scope: ActionScope::Directed,
    });
    if !matches!(outcome, Err(ref error) if error.contains("outcome unknown")) {
        return Err("native timeout fixture did not reach its response deadline".into());
    }
    let deadline_ms = started.elapsed().as_millis();
    if adapter.is_idle("timeout-run") {
        return Err("native timed-out action falsely reported idle".into());
    }
    adapter.abort("timeout-run", 2)?;
    adapter.release_target(&target.target_id);
    if adapter.is_idle("timeout-run") {
        return Err("native cancellation/unbind falsely released pending action".into());
    }
    let completion_deadline = Instant::now() + Duration::from_secs(8);
    while !adapter.is_idle("timeout-run") {
        if Instant::now() >= completion_deadline {
            return Err("native late callback did not settle the original owner".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let elapsed_ms = started.elapsed().as_millis();
    let state = parse_script_json(
        &live.host_script("({effect: document.getElementById('effect').textContent})")?,
    )?;
    if state.get("effect").and_then(serde_json::Value::as_str) != Some("1") {
        return Err("native timed-out action effect was not exactly once".into());
    }
    if !adapter.list_targets()?.is_empty() {
        return Err("native completion unexpectedly restored the revoked binding".into());
    }
    let next = adapter.bind(
        live.clone(),
        "new-session",
        "new-run",
        &serde_json::json!({}),
    )?;
    adapter.observe_for_run("new-run", &next.target_id)?;
    adapter.unbind();
    println!("gate: webview_native_timeout deadline_ms={deadline_ms} callback_ms={elapsed_ms} effect=1 old_binding_retired=true new_explicit_binding=true");
    Ok(())
}
