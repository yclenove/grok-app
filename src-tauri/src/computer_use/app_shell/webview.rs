//! App-owned WebView and slash-entry harness scenarios.

use std::path::Path;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use serde_json::{json, Value};
use tauri::Manager;

use crate::computer_use::ComputerUseAdapter;
use crate::session_manager::SessionManager;

use super::mcp::McpClient;
use super::{on_main, revoke_session};

const WEBVIEW_IFRAME_HTML: &str = r#"<!doctype html>
<html><meta charset="utf-8"><title>CU-IFRAME</title>
<body>
<iframe src="https://example.com"></iframe>
<a href="https://example.com/file.bin" download>dl</a>
</body></html>"#;

fn eval_main(app: &tauri::AppHandle, script: String) -> Result<String, String> {
    let (tx, rx) = mpsc::channel();
    let cb = move |result: String| {
        let _ = tx.send(result);
    };
    if let Some(window) = app.get_webview_window("main") {
        window
            .eval_with_callback(script, cb)
            .map_err(|e| e.to_string())?;
    } else if let Some(view) = app.get_webview("main") {
        view.eval_with_callback(script, cb)
            .map_err(|e| e.to_string())?;
    } else {
        return Err("main window missing".into());
    }
    let raw = rx
        .recv_timeout(Duration::from_secs(8))
        .map_err(|_| "eval timeout".to_string())?;
    let trimmed = raw.trim();
    if let Ok(value) = serde_json::from_str::<String>(trimmed) {
        Ok(value)
    } else {
        Ok(trimmed.trim_matches('"').to_string())
    }
}

pub(super) async fn slash_open(app: &tauri::AppHandle, action: &str) -> Result<(), String> {
    let js = format!(
        "(function(){{ try {{ var fn = window.__grokOpenComputerUseFromSlash; if (typeof fn !== 'function') return 'missing-hook:' + String(location.href||'') + ':' + String(document.readyState||'') + ':' + String(typeof fn); return fn({action:?}) ? 'ok' : 'rejected'; }} catch (e) {{ return 'eval-error:' + String(e); }} }})()"
    );
    let mut last = String::from("missing-hook");
    for _ in 0..16 {
        let handle = app.clone();
        let script = js.clone();
        let raw = tauri::async_runtime::spawn_blocking(move || eval_main(&handle, script))
            .await
            .map_err(|e| e.to_string())??;
        let text = raw.trim().trim_matches('"').to_string();
        if text == "ok" {
            return Ok(());
        }
        if text == "rejected" {
            return Err("rejected".into());
        }
        last = text;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(last)
}

pub(super) async fn bind_side_on_main(
    app: &tauri::AppHandle,
    sid: &str,
    run: &str,
    label: &str,
) -> Result<crate::computer_use::TargetInfo, String> {
    let handle = app.clone();
    let sid = sid.to_string();
    let run = run.to_string();
    let label = label.to_string();
    on_main(app, move || {
        crate::computer_use::product_webview().bind_side_browser(handle, &label, &sid, &run)
    })
    .await?
}

pub(super) async fn webview_rounds(
    app: &tauri::AppHandle,
    sid: &str,
    label: &str,
    rounds: u32,
) -> Result<(), String> {
    let wv = crate::computer_use::product_webview();
    for i in 1..=rounds {
        if !wv.list_targets().unwrap_or_default().is_empty() {
            return Err(format!("product webview not empty before round {i}"));
        }
        let round_result = async {
            let listed = production_bind_webview(app, sid, label).await?;
            if listed.kind != "webview" && !listed.target_id.starts_with("wv|") {
                return Err(format!("round {i} expected webview, got {:?}", listed.kind));
            }
            let run_id = crate::computer_use::sessions::current(sid)
                .ok_or_else(|| format!("round {i} missing run after bind"))?;
            let listed_cmd = crate::commands::computer_use_list_targets(
                Some(sid.to_string()),
                None,
                Some("app-webview".into()),
            )
            .await
            .unwrap_or_default();
            if listed_cmd.is_empty() {
                return Err(format!(
                    "round {i} list_targets app-webview empty after bind"
                ));
            }
            let entry = crate::computer_use::mcp_acp_entry(sid, &run_id)?;
            let mut mcp = McpClient::spawn(&entry)?;
            mcp.initialize()?;
            let status = mcp.tool("computer_status", json!({}))?;
            if status.get("backend").and_then(Value::as_str) != Some("webview") {
                return Err(format!("round {i} MCP status backend mismatch: {status}"));
            }
            let obs = mcp.tool("computer_observe", json!({}))?;
            if obs.get("targetId").and_then(Value::as_str) != Some(listed.target_id.as_str()) {
                return Err(format!("round {i} MCP observe target mismatch"));
            }
            let node_ref = obs
                .get("nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|node| {
                    node.get("ref")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value.contains("#go"))
                })
                .and_then(|node| node.get("ref"))
                .and_then(Value::as_str)
                .ok_or_else(|| format!("round {i} MCP observe missing #go"))?;
            let count_before = obs
                .get("nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|node| {
                    node.get("ref")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value.contains("#count"))
                })
                .and_then(|node| node.get("name"))
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| format!("round {i} MCP observe has invalid #count"))?;
            let target_generation = obs
                .get("targetGeneration")
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("round {i} MCP observe missing target generation"))?;
            let snapshot_id = obs
                .get("snapshotId")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("round {i} MCP observe missing snapshot"))?;
            let geometry_revision = obs
                .get("geometryRevision")
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("round {i} MCP observe missing geometry"))?;
            let acted = mcp.tool(
                "computer_act",
                json!({
                    "version": crate::computer_use::PROTOCOL_VERSION,
                    "runId": run_id,
                    "actionId": format!("wv-mcp-click-{i}"),
                    "targetId": listed.target_id,
                    "targetGeneration": target_generation,
                    "snapshotId": snapshot_id,
                    "geometryRevision": geometry_revision,
                    "action": "click",
                    "target": {"elementRef": node_ref},
                    "parameters": {},
                }),
            )?;
            if acted.get("kind").and_then(Value::as_str) != Some("verified")
                || acted.get("executed").and_then(Value::as_bool) != Some(true)
            {
                return Err(format!("round {i} MCP act not verified: {acted}"));
            }
            let obs2 = mcp.tool("computer_observe", json!({}))?;
            let count = obs2
                .get("nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|node| {
                    node.get("ref")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value.contains("#count"))
                })
                .and_then(|node| node.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let mark = obs2
                .get("nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|node| {
                    node.get("ref")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value.contains("#mark"))
                })
                .and_then(|node| node.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let expected_count = count_before.saturating_add(1).to_string();
            if count != expected_count || mark != "clicked" {
                return Err(format!(
                    "round {i} MCP DOM oracle failed before={count_before} count={count:?} mark={mark:?}"
                ));
            }
            let stopped = mcp.tool("computer_stop", json!({}))?;
            if stopped.get("stopState").and_then(Value::as_str) != Some("stopped") {
                return Err(format!("round {i} MCP stop incomplete: {stopped}"));
            }
            Ok(())
        }
        .await;
        revoke_session(sid).await;
        crate::computer_use::set_session_enabled(sid, true);
        let cleanup_result = if wv.list_targets().unwrap_or_default().is_empty() {
            Ok(())
        } else {
            Err(format!("round {i} Broker cleanup left WebView bound"))
        };
        if let Err(error) = round_result {
            return match cleanup_result {
                Ok(()) => Err(error),
                Err(cleanup) => Err(format!("{error}; cleanup failed: {cleanup}")),
            };
        }
        cleanup_result?;
    }
    Ok(())
}

pub(super) async fn production_bind_webview(
    app: &tauri::AppHandle,
    sid: &str,
    label: &str,
) -> Result<crate::computer_use::TargetInfo, String> {
    let mgr = app.state::<Arc<SessionManager>>();
    let authorization = crate::commands::computer_use_authorize_surface(
        app.clone(),
        app.get_webview_window("main")
            .ok_or("probe main window missing")?
            .as_ref()
            .window(),
        mgr,
        crate::commands::ComputerAuthorizationRequest {
            session_id: Some(sid.to_string()),
            run_id: None,
            attempt_id: format!("appshell-webview-{}", uuid::Uuid::new_v4()),
            selector_revision: 1,
            surface: "app-webview".into(),
            target_id: label.to_string(),
        },
    )
    .await?;
    let bound = authorization.target;
    Ok(crate::computer_use::TargetInfo {
        target_id: bound.target_id,
        title: bound.title,
        app_name: bound.app_name,
        kind: bound.kind,
        pid: None,
        backend: "webview".into(),
        execution_mode: "host-owned".into(),
        replay_policy: "never".into(),
        lifecycle_stamp: 1,
        display_id: label.into(),
        coordinate_space: "image-pixels".into(),
        scope_label: sid.into(),
    })
}

pub(super) async fn webview_fail_reclaim(
    app: &tauri::AppHandle,
    sid: &str,
    label: &str,
    home: &Path,
) -> Result<String, String> {
    crate::computer_use::set_session_enabled(sid, true);
    let wv = crate::computer_use::product_webview();
    wv.unbind();
    let dead = crate::commands::computer_use_authorize_surface(
        app.clone(),
        app.get_webview_window("main")
            .ok_or("probe main window missing")?
            .as_ref()
            .window(),
        app.state::<Arc<SessionManager>>(),
        crate::commands::ComputerAuthorizationRequest {
            session_id: Some(sid.to_string()),
            run_id: None,
            attempt_id: "appshell-webview-dead".into(),
            selector_revision: 1,
            surface: "app-webview".into(),
            target_id: "not-a-side-browser".into(),
        },
    )
    .await;
    if dead.is_ok() {
        return Err("dead webview label must fail closed".into());
    }
    if !wv.list_targets().unwrap_or_default().is_empty() {
        return Err("failed bind left a webview target".into());
    }
    let iframe = home.join("computer-use").join("oracle-iframe.html");
    std::fs::write(&iframe, WEBVIEW_IFRAME_HTML).map_err(|e| e.to_string())?;
    let iframe_url = format!(
        "file:///{}",
        iframe.display().to_string().replace('\\', "/")
    );
    let iframe_label = "resource-browser-cu-iframe".to_string();
    let created = {
        let app2 = app.clone();
        let label2 = iframe_label.clone();
        let url2 = iframe_url.clone();
        on_main(app, move || {
            crate::side_browser_host::create(
                &app2,
                label2,
                url2,
                "main".into(),
                8.0,
                8.0,
                220.0,
                160.0,
            )
        })
        .await
    };
    match created {
        Ok(Ok(())) => {}
        Ok(Err(e)) | Err(e) => return Err(format!("iframe side browser: {e}")),
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
    let iframe_bind = production_bind_webview(app, sid, &iframe_label).await;
    match iframe_bind {
        Ok(bound) => {
            let obs = wv.observe(&bound.target_id);
            wv.unbind();
            revoke_session(sid).await;
            crate::computer_use::set_session_enabled(sid, true);
            if obs.is_ok() {
                return Err("cross-origin iframe observe must fail closed".into());
            }
        }
        Err(_) => {
            wv.unbind();
            revoke_session(sid).await;
            crate::computer_use::set_session_enabled(sid, true);
        }
    }
    let recovered = production_bind_webview(app, sid, label).await?;
    if recovered.target_id.is_empty() {
        return Err("next bind after fail did not recover".into());
    }
    let gen_before = recovered.lifecycle_stamp;
    crate::commands::computer_use_unbind_webview(
        app.state::<Arc<SessionManager>>(),
        Some(sid.to_string()),
        None,
    )
    .await?;
    revoke_session(sid).await;
    crate::computer_use::set_session_enabled(sid, true);
    let rebound = production_bind_webview(app, sid, label).await?;
    if rebound.target_id == recovered.target_id && rebound.lifecycle_stamp == gen_before {
        // new run id is encoded in wv|...|session|run; empty unbind must not flash the old bind
        if wv.list_targets().unwrap_or_default().len() != 1 {
            return Err("rebind listed wrong target count".into());
        }
    }
    crate::commands::computer_use_unbind_webview(
        app.state::<Arc<SessionManager>>(),
        Some(sid.to_string()),
        None,
    )
    .await?;
    revoke_session(sid).await;
    crate::computer_use::set_session_enabled(sid, true);
    Ok("fail-closed+reclaim".into())
}
