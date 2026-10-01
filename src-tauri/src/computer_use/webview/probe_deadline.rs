//! Renderer-enforced termination of a runaway, explicitly owned fixture action.
//! This is a native deadline gate, not evidence of immediate user Stop.

use super::*;
use crate::computer_use::adapter::ActionScope;
use crate::computer_use::protocol::{ActionKind, ActionTarget};
use crate::computer_use::webview_host::HostOwnedWebView;
use std::time::{Duration, Instant};

pub(super) fn run() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!("grok-cu-wv-deadline-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).map_err(|_| "deadline fixture directory unavailable")?;
    let page = root.join("page.html");
    std::fs::write(
        &page,
        r#"<!doctype html><meta charset="utf-8"><title>CU native deadline fixture</title>
<button id="runaway">Runaway owned action</button><button id="next">Next owned action</button>
<output id="entered">0</output><output id="after">0</output><output id="next-effect">0</output>
<script>
document.getElementById('runaway').onclick = () => {
  const entered = document.getElementById('entered');
  entered.textContent = String(Number(entered.textContent) + 1);
  for (;;) { /* The renderer must interrupt this, not just time out its caller. */ }
  document.getElementById('after').textContent = '1';
};
document.getElementById('next').onclick = () => {
  const effect = document.getElementById('next-effect');
  effect.textContent = String(Number(effect.textContent) + 1);
};
</script>"#,
    )
    .map_err(|_| "deadline fixture page unavailable")?;
    let url = tauri::Url::from_file_path(&page).map_err(|_| "deadline fixture URL unavailable")?;
    let live = HostOwnedWebView::spawn(
        "resource-browser-native-deadline",
        "native-deadline",
        url.as_str(),
    )?;
    let result = check(&live);
    live.finish_probe(result)
}

fn check(live: &Arc<HostOwnedWebView>) -> Result<(), String> {
    let adapter = WebViewAdapter::new();
    let target = adapter.bind(
        live.clone(),
        "deadline-session",
        "deadline-run",
        &serde_json::json!({}),
    )?;
    let request = click_request(&adapter, &target, "deadline-run", "Runaway owned action")?;
    let started = Instant::now();
    let outcome = adapter.act(&request);
    if !matches!(outcome, Err(ref error) if error.contains("outcome unknown")) {
        return Err("native runaway action did not return an unknown outcome".into());
    }
    let deadline_ms = started.elapsed().as_millis();
    if adapter.is_idle("deadline-run") {
        return Err("business timeout released a still-executing native action".into());
    }
    adapter.abort("deadline-run", 2)?;
    adapter.release_target(&target.target_id);
    if adapter.is_idle("deadline-run") {
        return Err("logical cancellation was mistaken for native termination".into());
    }
    let completion_deadline = Instant::now() + Duration::from_secs(10);
    while !adapter.is_idle("deadline-run") {
        if Instant::now() >= completion_deadline {
            return Err("renderer deadline did not settle the runaway native action".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let native_ms = started.elapsed().as_millis();
    let before = fixture_state(live)?;
    if before != serde_json::json!({"entered": "1", "after": "0", "next": "0"}) {
        return Err("native termination did not preserve exactly-once partial effects".into());
    }
    if !adapter.list_targets()?.is_empty() {
        return Err("native termination restored retired authority".into());
    }
    let next = adapter.bind(
        live.clone(),
        "next-session",
        "next-run",
        &serde_json::json!({}),
    )?;
    let next_request = click_request(&adapter, &next, "next-run", "Next owned action")?;
    // An old Stop must not enqueue a global termination against this fresh run.
    adapter.abort("deadline-run", 3)?;
    let next_result = adapter.act(&next_request)?;
    if !next_result.applied || next_result.verifiable {
        return Err("post-termination action completion contract invalid".into());
    }
    if adapter.act(&request).is_ok() || adapter.act(&next_request).is_ok() {
        return Err("retired or duplicate action was replayed after native termination".into());
    }
    if fixture_state(live)? != serde_json::json!({"entered": "1", "after": "0", "next": "1"}) {
        return Err("native deadline affected a replacement or replayed the old action".into());
    }
    adapter.unbind();
    println!("gate: webview_native_deadline business_ms={deadline_ms} native_ms={native_ms} entered=1 after=0 next=1 old_stop_isolated=true replay=false");
    Ok(())
}

fn fixture_state(live: &HostOwnedWebView) -> Result<serde_json::Value, String> {
    parse_script_json(&live.host_script(
        "({entered: document.getElementById('entered').textContent, after: document.getElementById('after').textContent, next: document.getElementById('next-effect').textContent})",
    )?)
}

fn click_request(
    adapter: &WebViewAdapter,
    target: &TargetInfo,
    run: &str,
    name: &str,
) -> Result<DispatchRequest, String> {
    let observed = adapter.observe_for_run(run, &target.target_id)?;
    let element_ref = observed
        .nodes
        .iter()
        .find(|node| node.name == name)
        .ok_or("native deadline fixture control missing")?
        .node_ref
        .clone();
    Ok(DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: run.into(),
        action_id: format!("{run}-once"),
        generation: 1,
        target_id: target.target_id.clone(),
        target_generation: target.lifecycle_stamp,
        snapshot_id: observed.snapshot_id,
        geometry_revision: target.lifecycle_stamp,
        action: ActionKind::Click,
        target: ActionTarget::Element { element_ref },
        parameters: serde_json::json!({}),
        scope: ActionScope::Directed,
    })
}
