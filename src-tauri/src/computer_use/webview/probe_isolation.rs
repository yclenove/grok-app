//! Actual native world separation, with a deliberately hostile owned page.

use super::*;
use crate::computer_use::adapter::ActionScope;
use crate::computer_use::protocol::{ActionKind, ActionTarget};
use crate::computer_use::webview_host::HostOwnedWebView;

pub(super) fn run() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!("grok-cu-wv-isolated-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).map_err(|_| "isolated fixture directory unavailable")?;
    let page = root.join("page.html");
    std::fs::write(
        &page,
        r#"<!doctype html><meta charset="utf-8"><title>CU native isolation fixture</title>
<button id="inc">Native control</button><output id="count">0</output>
<script>
globalThis.pageQueries = 0;
globalThis.pageClicks = 0;
document.getElementById('inc').onclick = () => {
  const count = document.getElementById('count');
  count.textContent = String(Number(count.textContent) + 1);
};
Document.prototype.querySelectorAll = function () { globalThis.pageQueries++; return []; };
HTMLElement.prototype.click = function () { globalThis.pageClicks++; };
</script>"#,
    )
    .map_err(|_| "isolated fixture page unavailable")?;
    let url = tauri::Url::from_file_path(&page).map_err(|_| "isolated fixture URL unavailable")?;
    let live = HostOwnedWebView::spawn("resource-browser-isolated", "isolated", url.as_str())?;
    let result = check(&live, url.as_str());
    live.finish_probe(result)
}

fn check(live: &Arc<HostOwnedWebView>, url: &str) -> Result<(), String> {
    let adapter = WebViewAdapter::new();
    let target = adapter.bind(
        live.clone(),
        "isolated-session",
        "isolated-run",
        &serde_json::json!({}),
    )?;
    let observation = adapter.observe_for_run("isolated-run", &target.target_id)?;
    let element = observation
        .nodes
        .iter()
        .find(|node| node.name == "Native control")
        .ok_or("isolated snapshot was intercepted by page querySelectorAll")?;
    let result = adapter.act(&DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "isolated-run".into(),
        action_id: "isolated-click-once".into(),
        generation: 1,
        target_id: target.target_id.clone(),
        target_generation: target.lifecycle_stamp,
        snapshot_id: observation.snapshot_id,
        geometry_revision: target.lifecycle_stamp,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: element.node_ref.clone(),
        },
        parameters: serde_json::json!({}),
        scope: ActionScope::Directed,
    })?;
    if !result.applied || result.verifiable {
        return Err("isolated native action returned an invalid completion contract".into());
    }
    let bound = adapter.bound_target(&target.target_id)?;
    let epoch = bound.execution_epoch.load(Ordering::SeqCst);
    // These fixed diagnostics are probe-only; the model has no eval entry point.
    let marker = "(globalThis.__cu_isolation_probe = (globalThis.__cu_isolation_probe || 0) + 1, {marker: globalThis.__cu_isolation_probe})";
    for expected in 1..=5 {
        let result =
            parse_script_json(&adapter.host_script(&bound, epoch, &Default::default(), marker)?)?;
        if result.get("marker").and_then(serde_json::Value::as_i64) != Some(expected) {
            return Err("isolated native context was not stable within its document".into());
        }
        adapter.observe_for_run("isolated-run", &target.target_id)?;
    }
    let page_state = parse_script_json(&live.host_script(
        "({effect: document.getElementById('count').textContent, queries: globalThis.pageQueries, clicks: globalThis.pageClicks, hostVisible: typeof globalThis.__cu_isolation_probe !== 'undefined'})",
    )?)?;
    if page_state
        != serde_json::json!({"effect": "1", "queries": 0, "clicks": 0, "hostVisible": false})
    {
        return Err("native page-world separation or exact-once effect failed".into());
    }
    live.navigate(url)?;
    if adapter.observe(&target.target_id).is_ok() {
        return Err("old native world survived as authorized after reload".into());
    }
    let next = adapter.bind(
        live.clone(),
        "new-session",
        "new-run",
        &serde_json::json!({}),
    )?;
    let next_bound = adapter.bound_target(&next.target_id)?;
    let reset = parse_script_json(&adapter.host_script(
        &next_bound,
        0,
        &Default::default(),
        "({reset: typeof globalThis.__cu_isolation_probe === 'undefined'})",
    )?)?;
    if reset.get("reset") != Some(&serde_json::json!(true)) {
        return Err("replacement document reused the retired native world".into());
    }
    adapter.observe_for_run("new-run", &next.target_id)?;
    adapter.unbind();
    println!("gate: webview_native_isolation page_query_interceptions=0 page_click_interceptions=0 effect=1 marker_hidden=true context_reused=5 reload_reset=true");
    Ok(())
}
