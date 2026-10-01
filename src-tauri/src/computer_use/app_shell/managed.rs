//! Managed-browser harness rounds and the owned loopback oracle.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::Manager;

use crate::session_manager::SessionManager;
use crate::store;

use super::mcp::McpClient;
use super::revoke_session;

const MANAGED_ORACLE_HTML: &str = r#"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"/><title>CU-F3</title></head>
<body>
  <h1>managed-product</h1>
  <button id="inc" type="button">+1</button>
  <output id="count">0</output>
  <script>
    let n = 0;
    document.getElementById('inc').onclick = () => {
      n += 1;
      document.getElementById('count').textContent = String(n);
      fetch('/oracle', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ count: n }) }).catch(() => {});
    };
  </script>
</body></html>"#;

pub(super) async fn managed_product_rounds(
    app: &tauri::AppHandle,
    sid: &str,
    rounds: u32,
) -> Result<u32, String> {
    crate::computer_use::set_session_enabled(sid, true);
    let broker = crate::computer_use::ensure_host_runtime();
    crate::computer_use::browser_supervisor::ensure_product_attached(&broker)
        .map_err(|e| format!("managed worker attach: {e}"))?;
    let oracle = Arc::new(AtomicU64::new(0));
    let (page_url, stop, thread) = serve_managed_oracle(oracle.clone())?;
    let result = managed_product_rounds_inner(app, sid, rounds, &page_url, &oracle).await;
    stop.store(true, Ordering::SeqCst);
    let _ = thread.join();
    result
}

async fn managed_product_rounds_inner(
    app: &tauri::AppHandle,
    sid: &str,
    rounds: u32,
    page_url: &str,
    oracle: &Arc<AtomicU64>,
) -> Result<u32, String> {
    let mgr = app.state::<Arc<SessionManager>>();
    let listed = crate::commands::computer_use_list_targets(
        Some(sid.to_string()),
        None,
        Some("managed-browser".into()),
    )
    .await
    .map_err(|e| format!("list managed: {e}"))?;
    let _ = listed;

    let bad = crate::commands::computer_use_authorize_surface(
        app.clone(),
        app.get_webview_window("main")
            .ok_or("probe main window missing")?
            .as_ref()
            .window(),
        mgr.clone(),
        crate::commands::ComputerAuthorizationRequest {
            session_id: Some(sid.to_string()),
            run_id: None,
            attempt_id: "appshell-managed-invalid".into(),
            selector_revision: 1,
            surface: "managed-browser".into(),
            target_id: "managed-profile:../nope".into(),
        },
    )
    .await;
    if bad.is_ok() {
        return Err("invalid profile name must fail closed".into());
    }
    revoke_session(sid).await;
    crate::computer_use::set_session_enabled(sid, true);

    let mut ok = 0u32;
    for i in 1..=rounds {
        oracle.store(0, Ordering::SeqCst);
        revoke_session(sid).await;
        crate::computer_use::set_session_enabled(sid, true);
        let authorization = crate::commands::computer_use_authorize_surface(
            app.clone(),
            app.get_webview_window("main")
                .ok_or("probe main window missing")?
                .as_ref()
                .window(),
            mgr.clone(),
            crate::commands::ComputerAuthorizationRequest {
                session_id: Some(sid.to_string()),
                run_id: None,
                attempt_id: format!("appshell-managed-{i}"),
                selector_revision: u64::from(i) + 1,
                surface: "managed-browser".into(),
                target_id: "managed-profile:alice".into(),
            },
        )
        .await
        .map_err(|e| format!("round {i} atomic managed authorize: {e}"))?;
        let tab = authorization.target;
        if !tab.target_id.starts_with("managed:") {
            return Err(format!(
                "round {i} expected managed tab, got {}",
                tab.target_id
            ));
        }
        let run = authorization.run_id;

        let broker = crate::computer_use::ensure_host_runtime();
        let _navigated = {
            let b = broker.clone();
            let run2 = run.clone();
            let tab_id = tab.target_id.clone();
            let url = page_url.to_string();
            tauri::async_runtime::spawn_blocking(move || {
                b.browser_navigate(&run2, &tab_id, &url, &format!("nav-{i}"))
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("round {i} navigate: {e}"))?
        };

        let entry = crate::computer_use::mcp_acp_entry(sid, &run)
            .map_err(|e| format!("round {i} mcp inject: {e}"))?;
        let mut mcp = McpClient::spawn(&entry).map_err(|e| format!("round {i} mcp spawn: {e}"))?;
        let mcp_result = (|| {
            mcp.initialize()?;
            let listed = mcp.call("tools/list", json!({}))?;
            let names: Vec<String> = listed
                .get("tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|t| t.get("name").and_then(Value::as_str).map(str::to_string))
                .collect();
            if !names.iter().any(|n| n == "computer_observe") {
                return Err("computer_observe missing after authorize".into());
            }
            if names.iter().any(|n| n == "computer_authorize") {
                return Err("host-only computer_authorize leaked onto model tools".into());
            }
            let status = mcp.tool("computer_status", json!({}))?;
            if status.get("backend").and_then(Value::as_str) != Some("playwright-managed") {
                return Err(format!("round {i} wrong managed backend: {status}"));
            }
            let targets = mcp.tool("computer_list_targets", json!({}))?;
            if targets
                .as_array()
                .is_none_or(|rows| rows.len() != 1 || rows[0]["targetId"] != tab.target_id)
            {
                return Err(format!("round {i} wrong generic target list: {targets}"));
            }
            let observed = mcp.tool("computer_observe", json!({}))?;
            if observed.get("runId").and_then(Value::as_str) != Some(run.as_str())
                || observed.get("targetId").and_then(Value::as_str) != Some(tab.target_id.as_str())
            {
                return Err(format!(
                    "round {i} generic observation identity: {observed}"
                ));
            }
            let snapshot = observed
                .get("snapshotId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let target_generation = observed
                .get("targetGeneration")
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("round {i} missing targetGeneration"))?;
            let geometry_revision = observed
                .get("geometryRevision")
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("round {i} missing geometryRevision"))?;
            let nodes = observed
                .get("nodes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let plus = nodes.iter().find(|n| {
                n.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.contains("+1"))
            });
            let Some(plus) = plus else {
                return Err(format!("round {i} missing +1 node: {nodes:?}"));
            };
            let element_ref = plus
                .get("ref")
                .and_then(Value::as_str)
                .ok_or("missing generic node ref")?
                .to_string();
            let outcome = mcp.tool(
                "computer_act",
                json!({
                    "version": grok_computer_use_core::protocol::PROTOCOL_VERSION,
                    "actionId": format!("click-{i}"),
                    "runId": run,
                    "targetId": tab.target_id,
                    "targetGeneration": target_generation,
                    "snapshotId": snapshot,
                    "geometryRevision": geometry_revision,
                    "action": "click",
                    "target": { "elementRef": element_ref },
                    "parameters": {}
                }),
            )?;
            if outcome.get("kind").and_then(Value::as_str) != Some("applied") {
                return Err(format!(
                    "round {i} managed action must require re-observe: {outcome}"
                ));
            }
            let after = mcp.tool("computer_observe", json!({}))?;
            if after.get("snapshotId").and_then(Value::as_str) == Some(snapshot.as_str()) {
                return Err(format!("round {i} managed snapshot did not advance"));
            }
            Ok::<_, String>(())
        })();
        mcp.kill();
        mcp_result.map_err(|e| format!("round {i} mcp: {e}"))?;

        let mut hits = 0u64;
        for _ in 0..80 {
            hits = oracle.load(Ordering::SeqCst);
            if hits == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if hits != 1 {
            return Err(format!("round {i} independent oracle count={hits}"));
        }
        let _ = crate::commands::computer_use_pause(Some(sid.to_string()), Some(run.clone())).await;
        let _ = crate::commands::computer_use_stop(
            app.state::<Arc<SessionManager>>(),
            Some(sid.to_string()),
            Some(run.clone()),
        )
        .await;
        revoke_session(sid).await;
        crate::computer_use::set_session_enabled(sid, true);
        ok += 1;
    }

    let sid_b = format!("{sid}-b");
    crate::computer_use::set_session_enabled(&sid_b, true);
    crate::computer_use::set_session_surface(
        &sid_b,
        grok_computer_use_core::surface::ComputerUseSurface::LocalInteractive,
    );
    let broker = crate::computer_use::ensure_host_runtime();
    let run_b = uuid::Uuid::new_v4().to_string();
    broker
        .open_run(&sid_b, &run_b)
        .map_err(|e| format!("dual-session open_run: {e}"))?;
    let bob = {
        let b = broker.clone();
        let run = run_b.clone();
        let sess = sid_b.clone();
        tauri::async_runtime::spawn_blocking(move || {
            b.tabs().open_managed_profile(&sess, &run, "bob")
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("dual-session bob: {e}"))?
    };
    let alice_tabs = broker.tabs().list_for_run(
        crate::computer_use::sessions::current(sid)
            .as_deref()
            .unwrap_or(""),
    );
    if alice_tabs.iter().any(|t| t.tab_id == bob.tab_id) {
        return Err("session A listed session B managed tab".into());
    }
    let _ = broker.request_stop(&run_b);
    crate::computer_use::sessions::revoke(&sid_b);
    Ok(ok)
}

fn serve_managed_oracle(
    oracle: Arc<AtomicU64>,
) -> Result<(String, Arc<AtomicBool>, std::thread::JoinHandle<()>), String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = stop.clone();
    let body = MANAGED_ORACLE_HTML.as_bytes().to_vec();
    let thread = std::thread::spawn(move || {
        while !stop_t.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let req = read_http_request(&mut stream);
                    if req.starts_with("POST /oracle") {
                        let raw = req.split("\r\n\r\n").nth(1).unwrap_or("");
                        if let Ok(value) = serde_json::from_str::<Value>(raw.trim()) {
                            if let Some(count) = value.get("count").and_then(Value::as_u64) {
                                oracle.store(count, Ordering::SeqCst);
                            }
                        }
                        let _ = stream
                            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
                    } else {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(header.as_bytes());
                        let _ = stream.write_all(&body);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(15));
                }
                Err(_) => break,
            }
        }
    });
    Ok((format!("http://127.0.0.1:{port}/form.html"), stop, thread))
}

fn read_http_request(stream: &mut TcpStream) -> String {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(2000)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 512];
    for _ in 0..64 {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}
