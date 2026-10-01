//! Generic controls exercised through the production native-world command path.

use super::*;
use crate::computer_use::adapter::{ActionScope, CaptureOptions};
use crate::computer_use::protocol::{ActionKind, ActionTarget};
use crate::computer_use::webview_host::HostOwnedWebView;

pub(super) fn run() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!("grok-cu-wv-dom-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).map_err(|_| "DOM fixture directory unavailable")?;
    let page = root.join("page.html");
    std::fs::write(&page, r#"<!doctype html><meta charset="utf-8"><title>CU generic DOM fixture</title>
<button>Without id</button><button id="duplicate">Duplicate first</button><button id="duplicate">Duplicate second</button>
<div aria-label="First region" style="height:60px;overflow-y:auto"><div style="height:900px">First content</div></div>
<div aria-label="Second region" style="height:60px;overflow-y:auto"><div style="height:900px">Second content</div></div>
<button aria-label="Replaced control">Original</button>
<script>
globalThis.effects = [0,0,0,0];
document.querySelectorAll('button').forEach((button,index) => button.onclick = () => effects[index]++);
</script>"#).map_err(|_| "DOM fixture page unavailable")?;
    let url = tauri::Url::from_file_path(&page).map_err(|_| "DOM fixture URL unavailable")?;
    let live = HostOwnedWebView::spawn("resource-browser-dom", "dom", url.as_str())?;
    let result = check(&live);
    live.finish_probe(result)
}

fn request(
    observation: &Observation,
    name: &str,
    action: ActionKind,
) -> Result<DispatchRequest, String> {
    let node = observation
        .nodes
        .iter()
        .find(|node| node.name == name)
        .ok_or("generic DOM fixture control missing")?;
    Ok(DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: observation.run_id.clone(),
        action_id: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        target_id: observation.target_id.clone(),
        target_generation: observation.target_generation,
        snapshot_id: observation.snapshot_id.clone(),
        geometry_revision: observation.geometry_revision,
        action,
        target: ActionTarget::Element {
            element_ref: node.node_ref.clone(),
        },
        parameters: if action == ActionKind::Scroll {
            serde_json::json!({"dy":120})
        } else {
            serde_json::json!({})
        },
        scope: ActionScope::Directed,
    })
}

fn check(live: &Arc<HostOwnedWebView>) -> Result<(), String> {
    let adapter = WebViewAdapter::new();
    let target = adapter.bind(
        live.clone(),
        "dom-session",
        "dom-run",
        &serde_json::json!({}),
    )?;
    let model = adapter.observe(&target.target_id)?;
    if model
        .nodes
        .iter()
        .any(|node| !node.node_ref.starts_with(&model.snapshot_id) || node.node_ref.contains('#'))
    {
        return Err("generic DOM observation exposed selector authority".into());
    }
    let preview = adapter.capture_for_run(
        "dom-run",
        &target.target_id,
        CaptureOptions {
            managed_request: None,
            for_model: false,
            screenshot: false,
            cancellation: Default::default(),
        },
    )?;
    if adapter
        .act(&request(&preview, "Without id", ActionKind::Click)?)
        .is_ok()
    {
        return Err("UI preview incorrectly granted model action authority".into());
    }
    let first = request(&model, "Without id", ActionKind::Click)?;
    if !adapter.act(&first)?.applied || adapter.act(&first).is_ok() {
        return Err("no-id action did not preserve exact-once consumption".into());
    }
    let next = adapter.observe(&target.target_id)?;
    if !adapter
        .act(&request(&next, "Duplicate second", ActionKind::Click)?)?
        .applied
    {
        return Err("duplicate id action not applied".into());
    }
    let before_replacement = adapter.observe(&target.target_id)?;
    // Fixture-only mutation in the page world. The Host must not retarget by id/name.
    live.host_script("(() => { const old = document.querySelector('[aria-label=\"Replaced control\"]'); const next = old.cloneNode(true); next.onclick = () => effects[3]++; old.replaceWith(next); return {ok:true}; })()")?;
    if adapter
        .act(&request(
            &before_replacement,
            "Replaced control",
            ActionKind::Click,
        )?)
        .is_ok()
    {
        return Err("replaced DOM object inherited old action authority".into());
    }
    let next = adapter.observe(&target.target_id)?;
    if !adapter
        .act(&request(&next, "Replaced control", ActionKind::Click)?)?
        .applied
    {
        return Err("fresh replacement observation did not restore explicit action".into());
    }
    let scroll = adapter.observe(&target.target_id)?;
    if !adapter
        .act(&request(&scroll, "Second region", ActionKind::Scroll)?)?
        .applied
    {
        return Err("generic DOM scroll not applied".into());
    }
    let actual = parse_script_json(&live.host_script("({effects, first:document.querySelector('[aria-label=\"First region\"]').scrollTop, second:document.querySelector('[aria-label=\"Second region\"]').scrollTop})")?)?;
    if actual != serde_json::json!({"effects":[1,0,1,1],"first":0,"second":120}) {
        return Err("generic DOM independent effect or scroller postcondition failed".into());
    }
    adapter.unbind();
    println!("gate: webview_native_dom no_id=true duplicate_id_exact=true preview_preserves_model=true replacement_stale=true effects=1,0,1,1 scroll=0,120");
    Ok(())
}
