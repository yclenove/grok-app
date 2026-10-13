//! Native App-owned side-browser selection evidence. No real model/TaskGrant
//! approval is claimed: this fixed debug harness supplies the Host binding.

use super::super::webview::{WebViewAdapter, WebViewSelection};
use super::{on_main, ComputerUseAdapter};
use crate::computer_use::adapter::{ActionScope, DispatchRequest, TargetInfo};
use crate::computer_use::protocol::{ActionKind, ActionTarget};
use serde_json::{json, Value};

mod group;
pub(super) mod task;

const LABEL: &str = "resource-browser-selection-fixture";
const SESSION: &str = "selection-host-fixture";
const RUN: &str = "selection-host-run";
const HTML: &str = r#"<!doctype html><meta charset="utf-8"><title>Selected native page</title>
<label>Note<input aria-label="Note" id="note"></label>
<button onclick="saved.textContent=note.value; saves.textContent=Number(saves.textContent)+1">Save</button>
<button onclick="count.textContent=Number(count.textContent)+1">Increase</button>
<output id="saved">未保存</output><output id="saves">0</output><output id="count">0</output>"#;

pub(super) async fn run(app: &tauri::AppHandle) -> Value {
    let page = crate::paths::app_data_root().join("selection-fixture.html");
    let setup = std::fs::write(&page, HTML)
        .map_err(|_| "selection fixture file unavailable".to_string())
        .and_then(|()| {
            tauri::Url::from_file_path(&page)
                .map_err(|_| "selection fixture URL unavailable".into())
        });
    let result = match setup {
        Ok(url) => check(app, &url).await,
        Err(error) => Err(error),
    };
    crate::computer_use::product_webview().unbind_for_run(SESSION, RUN);
    crate::computer_use::product_webview().unbind_for_run(group::SESSION, group::RUN);
    let first_close = close(app, LABEL).await;
    let group_close = close(app, group::LABEL).await;
    let idle = crate::computer_use::product_webview().is_idle(RUN)
        && crate::computer_use::product_webview().is_idle(group::RUN);
    let ok = result.is_ok() && first_close.is_ok() && group_close.is_ok() && idle;
    let (payload, error) = match result {
        Ok(payload) => (Some(payload), None),
        Err(error) => (None, Some(error)),
    };
    json!({
        "ok": ok,
        "mode": "webview-selection", "platform": std::env::consts::OS,
        "nativeProductSideBrowser": true, "hostBindingSynthetic": true,
        "realModel": false, "taskGrantPath": false,
        "result": payload, "error": error, "closeError": first_close.err(),
        "groupCloseError": group_close.err(),
        "adapterIdle": idle, "loginDataImported": false,
    })
}

async fn select(app: &tauri::AppHandle, label: &str) -> Result<WebViewSelection, String> {
    let handle = app.clone();
    let label = label.to_string();
    on_main(app, move || {
        WebViewAdapter::select_side_browser(handle, &label)
    })
    .await?
}

async fn create(app: &tauri::AppHandle, label: &str, url: &tauri::Url) -> Result<(), String> {
    let handle = app.clone();
    let label = label.to_string();
    let url = url.to_string();
    on_main(app, move || {
        crate::side_browser_host::create(&handle, label, url, "main".into(), 8.0, 8.0, 480.0, 320.0)
    })
    .await?
}

async fn close(app: &tauri::AppHandle, label: &str) -> Result<(), String> {
    let handle = app.clone();
    let label = label.to_string();
    tauri::async_runtime::spawn_blocking(move || crate::side_browser_host::close(&handle, label))
        .await
        .map_err(|_| "native side browser close worker did not join")?
}

async fn state(app: &tauri::AppHandle, label: &str) -> Result<Value, String> {
    let handle = app.clone();
    let label = label.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        // Independent read-only oracle. Every business input below uses the
        // adapter's fixed typed action with a fresh model snapshot/ref.
        let raw = crate::side_browser_host::eval(&handle, label,
            "({note:document.getElementById('saved').textContent,saves:Number(document.getElementById('saves').textContent),count:Number(document.getElementById('count').textContent)})".into())?;
        let value: Value = serde_json::from_str(&raw).map_err(|_| "native oracle response invalid")?;
        let value = if let Value::String(value) = value {
            serde_json::from_str(&value).map_err(|_| "native oracle envelope invalid")?
        } else { value };
        if !value.get("note").is_some_and(Value::is_string)
            || !value.get("saves").is_some_and(Value::is_u64)
            || !value.get("count").is_some_and(Value::is_u64)
        {
            return Err("native oracle controls not ready".into());
        }
        Ok(value)
    }).await.map_err(|_| "native oracle worker did not join")?
}

async fn ready(app: &tauri::AppHandle, label: &str) -> Result<Value, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(6);
    loop {
        if let Ok(state) = state(app, label).await {
            return Ok(state);
        }
        if std::time::Instant::now() >= deadline {
            return Err("native fixture did not become ready".into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

async fn act(
    run: &str,
    target: TargetInfo,
    kind: ActionKind,
    name: &'static str,
    parameters: Value,
) -> Result<(), String> {
    let run = run.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let adapter = crate::computer_use::product_webview();
        let observed = adapter.observe_for_run(&run, &target.target_id)?;
        let element = observed
            .nodes
            .iter()
            .find(|node| node.name == name)
            .ok_or("native typed control unavailable")?;
        let result = adapter.act(&DispatchRequest {
            managed_request: None,
            cancellation: Default::default(),
            run_id: run,
            action_id: uuid::Uuid::new_v4().to_string(),
            generation: 1,
            target_id: target.target_id,
            target_generation: observed.target_generation,
            snapshot_id: observed.snapshot_id,
            geometry_revision: observed.geometry_revision,
            action: kind,
            target: ActionTarget::Element {
                element_ref: element.node_ref.clone(),
            },
            parameters,
            scope: ActionScope::Directed,
        })?;
        if !result.applied {
            return Err("native typed action did not apply".into());
        }
        Ok(())
    })
    .await
    .map_err(|_| "native typed worker did not join")?
}

async fn check(app: &tauri::AppHandle, url: &tauri::Url) -> Result<Value, String> {
    create(app, LABEL, url).await?;
    let initial = ready(app, LABEL).await?;
    let first = select(app, LABEL).await?;
    if !crate::computer_use::product_webview()
        .list_targets()?
        .is_empty()
        || initial != json!({"note":"未保存","saves":0,"count":0})
        || state(app, LABEL).await? != initial
    {
        return Err("selection bound a run or produced business input".into());
    }
    let handle = app.clone();
    on_main(app, move || {
        crate::side_browser_host::reload(&handle, LABEL.into())
    })
    .await??;
    let after_reload = ready(app, LABEL).await?;
    if first.title().is_ok()
        || crate::computer_use::product_webview()
            .bind_selected(&first, SESSION, RUN)
            .is_ok()
    {
        return Err("same-URL reload authorized the previously selected document".into());
    }
    if after_reload != initial {
        return Err("reload replayed an old business effect".into());
    }
    let second = select(app, LABEL).await?;
    close(app, LABEL).await?;
    create(app, LABEL, url).await?; // Same routing label, different native instance.
    let replacement_initial = ready(app, LABEL).await?;
    if replacement_initial != initial {
        return Err("replacement inherited business effects".into());
    }
    if second.title().is_ok()
        || crate::computer_use::product_webview()
            .bind_selected(&second, SESSION, RUN)
            .is_ok()
    {
        return Err("same-label replacement inherited the selected native identity".into());
    }
    let fresh = select(app, LABEL).await?;
    let target = crate::computer_use::product_webview().bind_selected(&fresh, SESSION, RUN)?;
    act(
        RUN,
        target.clone(),
        ActionKind::SetValue,
        "Note",
        json!({"text":"内嵌选择验证 你好"}),
    )
    .await?;
    act(RUN, target.clone(), ActionKind::Click, "Save", json!({})).await?;
    act(
        RUN,
        target.clone(),
        ActionKind::Click,
        "Increase",
        json!({}),
    )
    .await?;
    let final_state = state(app, LABEL).await?;
    if final_state != json!({"note":"内嵌选择验证 你好","saves":1,"count":1}) {
        return Err("native note/save/count postconditions differ".into());
    }
    crate::computer_use::product_webview().release_target_for_run(RUN, &target.target_id);
    if !crate::computer_use::product_webview()
        .list_targets()?
        .is_empty()
    {
        return Err("exact run release left the selected target live".into());
    }
    let group = group::check(app, url).await?;
    Ok(
        json!({"metadataOnly":true,"sameUrlReloadRejected":true,"sameLabelReplacementRejected":true,
        "initial":initial,"replacementInitial":replacement_initial,"final":final_state,
        "typedActions":3,"releasedTarget":true,"group":group}),
    )
}
