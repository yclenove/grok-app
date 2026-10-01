//! Explicitly isolated desktop fixture and mixed-surface harness rounds.

#[cfg(target_os = "windows")]
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::Manager;

use crate::computer_use::ComputerUseAdapter;
use crate::session_manager::SessionManager;

use super::mcp::McpClient;
use super::revoke_session;
use super::webview::bind_side_on_main;

#[cfg(target_os = "windows")]
pub(super) fn spawn_isolated_desktop_fixture() -> Result<(String, Child), String> {
    let title = format!("GrokCuFixture-appshell-{}", std::process::id());
    let script = r#"
Add-Type -AssemblyName System.Windows.Forms
$form = New-Object System.Windows.Forms.Form
$form.Text = $env:GROK_CU_FIXTURE_TITLE
$form.Width = 480
$form.Height = 320
$form.StartPosition = 'Manual'
$form.Left = 48
$form.Top = 48
$btn = New-Object System.Windows.Forms.Button
$btn.Text = 'Count'
$btn.Width = 100
$btn.Height = 32
$btn.Left = 16
$btn.Top = 16
$form.Controls.Add($btn)
[void]$form.ShowDialog()
"#;
    let mut child = Command::new("powershell.exe")
        .args([
            "-STA",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .env("GROK_CU_FIXTURE_TITLE", &title)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn fixture process: {e}"))?;
    let adapter = crate::computer_use::windows_adapter::WindowsAdapter::new();
    let start = Instant::now();
    loop {
        if adapter
            .list_targets()
            .unwrap_or_default()
            .iter()
            .any(|t| t.title.contains(&title) && !t.app_name.eq_ignore_ascii_case("grok-app"))
        {
            return Ok((title, child));
        }
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let listed: Vec<String> = adapter
                .list_targets()
                .unwrap_or_default()
                .into_iter()
                .map(|t| format!("{} ({})", t.title, t.app_name))
                .collect();
            return Err(format!(
                "isolated fixture {title} not listed within 10s; have {listed:?}"
            ));
        }
        std::thread::sleep(Duration::from_millis(120));
    }
}

fn pick_harness_desktop_target(
    targets: &[crate::commands::ComputerTargetDto],
    fixture_title: &str,
) -> Result<crate::commands::ComputerTargetDto, String> {
    if fixture_title.is_empty() || !fixture_title.contains("GrokCuFixture") {
        return Err("harness requires an isolated GrokCuFixture title".into());
    }
    let hit = targets
        .iter()
        .find(|t| t.title.contains(fixture_title))
        .cloned()
        .ok_or_else(|| {
            format!("isolated fixture {fixture_title:?} not listed; refusing first window")
        })?;
    let title_l = hit.title.to_ascii_lowercase();
    let app_l = hit.app_name.to_ascii_lowercase();
    if title_l.contains("chatgpt")
        || title_l.contains("google chrome")
        || title_l.contains("wechat")
        || title_l.contains("微信")
        || app_l.contains("chrome")
        || app_l.contains("msedge")
    {
        return Err(format!(
            "refusing daily app as desktop target: {} ({})",
            hit.title, hit.app_name
        ));
    }
    Ok(hit)
}

pub(super) async fn one_round(
    app: &tauri::AppHandle,
    sid: &str,
    label: &str,
    round: u32,
    fixture_title: Option<&str>,
) -> Result<String, String> {
    crate::commands::computer_use_set_feature(app.state::<Arc<SessionManager>>(), true).await?;
    let broker = crate::computer_use::ensure_host_runtime();
    revoke_session(sid).await;
    crate::computer_use::set_session_enabled(sid, true);

    let needle = fixture_title.ok_or_else(|| {
        format!("round {round} isolated GrokCuFixture missing (no desktop fixture)")
    })?;
    let desktop = crate::commands::computer_use_list_targets(
        Some(sid.to_string()),
        None,
        Some("desktop".into()),
    )
    .await?;
    let t = pick_harness_desktop_target(&desktop, needle)?;
    {
        let run_d = uuid::Uuid::new_v4().to_string();
        broker
            .open_run(sid, &run_d)
            .map_err(|e| format!("desktop open_run: {e}"))?;
        broker
            .authorize_target(&run_d, &t.target_id)
            .map_err(|e| format!("desktop authorize: {e}"))?;
        let obs = broker
            .observe_preview(&run_d)
            .map_err(|e| format!("desktop observe: {e}"))?;
        if obs.snapshot_id.is_empty() {
            return Err(format!(
                "round {round} desktop observe produced empty snapshot"
            ));
        }
        broker
            .pause(&run_d)
            .map_err(|e| format!("desktop pause: {e}"))?;
        broker
            .takeover(&run_d)
            .map_err(|e| format!("desktop takeover: {e}"))?;
        broker
            .resume(&run_d)
            .map_err(|e| format!("desktop resume: {e}"))?;
        broker
            .set_preview_visible(&run_d, false)
            .map_err(|e| format!("desktop preview: {e}"))?;
        let _ = broker.request_stop(&run_d);
        revoke_session(sid).await;
        crate::computer_use::set_session_enabled(sid, true);
    }

    let run_m = uuid::Uuid::new_v4().to_string();
    broker
        .open_run(sid, &run_m)
        .map_err(|e| format!("managed open_run: {e}"))?;
    {
        let b = broker.clone();
        let sid_owned = sid.to_string();
        let run_owned = run_m.clone();
        match tauri::async_runtime::spawn_blocking(move || {
            b.tabs()
                .open_managed_profile(&sid_owned, &run_owned, "alice")
        })
        .await
        {
            Ok(Ok(tab)) if !tab.tab_id.trim().is_empty() => {
                let _ = broker.authorize_target(&run_m, &tab.tab_id);
                let b2 = broker.clone();
                let run2 = run_m.clone();
                let tab_id = tab.tab_id.clone();
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    b2.browser_navigate(&run2, &tab_id, "about:blank", "nav")
                })
                .await;
            }
            _ => {}
        }
    }

    let run_w = uuid::Uuid::new_v4().to_string();
    broker
        .open_run(sid, &run_w)
        .map_err(|e| format!("webview open_run: {e}"))?;
    let bound = bind_side_on_main(app, sid, &run_w, label).await?;
    if bound.kind != "webview" && !bound.target_id.starts_with("wv|") {
        return Err(format!(
            "round {round} expected webview bind, got {}",
            bound.kind
        ));
    }
    broker
        .authorize_on_surface(
            sid,
            &run_w,
            crate::computer_use::SurfaceKind::WebView,
            &bound.target_id,
        )
        .map_err(|error| format!("round {round} WebView authorize: {error}"))?;
    let wv_obs = broker
        .observe(&run_w)
        .map_err(|error| format!("round {round} WebView Broker observe: {error}"))?;
    if !wv_obs.nodes.iter().any(|n| n.node_ref.contains("#go")) {
        return Err(format!("round {round} webview DOM missing #go"));
    }
    crate::computer_use::product_webview().unbind();
    revoke_session(sid).await;
    crate::computer_use::set_session_enabled(sid, true);

    if let Ok(entry) = crate::computer_use::mcp_acp_entry(sid, &run_m) {
        if let Ok(mut mcp) = McpClient::spawn(&entry) {
            let _ = mcp.initialize();
            let _ = mcp.call("tools/list", json!({}));
            mcp.kill();
        }
    }

    Ok(format!(
        "round {round} pid={} desktop={} webview={} managed={}",
        std::process::id(),
        t.title,
        bound.target_id,
        run_m
    ))
}
