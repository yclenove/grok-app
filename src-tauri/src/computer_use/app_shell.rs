//! Isolated debug App-shell E3 harness.
//!
//! Runs only when `GROK_APP_INSTANCE_ID` is a non-shipping suffix **and**
//! `GROK_CU_APP_HARNESS=1`. Not compiled into release binaries.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::Manager;

use crate::computer_use::ComputerUseAdapter;
use crate::session_manager::SessionManager;
use crate::store;

const ORACLE_HTML: &str = r#"<!doctype html>
<html><meta charset="utf-8"><title>CU-D5</title>
<body>
<p id="mark">idle</p>
<button id="go">go</button>
<input id="name" value="">
<span id="count">0</span>
<div id="scroller" style="height:40px;overflow:auto"><div style="height:400px">x</div></div>
<script>
document.getElementById('go').onclick = function () {
  var c = document.getElementById('count');
  c.textContent = String(Number(c.textContent || '0') + 1);
  document.getElementById('mark').textContent = 'clicked';
};
</script>
</body></html>"#;

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

const WEBVIEW_IFRAME_HTML: &str = r#"<!doctype html>
<html><meta charset="utf-8"><title>CU-IFRAME</title>
<body>
<iframe src="https://example.com"></iframe>
<a href="https://example.com/file.bin" download>dl</a>
</body></html>"#;

pub fn maybe_spawn(app: &tauri::AppHandle) {
    if std::env::var("GROK_CU_APP_HARNESS").ok().as_deref() != Some("1") {
        return;
    }
    if crate::paths::isolated_app_instance_id().is_none() {
        tracing::warn!("GROK_CU_APP_HARNESS ignored without isolated identity");
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(900)).await;
        let report = run(app.clone()).await;
        write_report(&report);
        let _ = tauri::async_runtime::spawn_blocking(crate::computer_use::shutdown_product).await;
        let ok = report.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        std::process::exit(if ok { 0 } else { 2 });
    });
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

async fn revoke_session(sid: &str) {
    let sid = sid.to_string();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        crate::computer_use::sessions::revoke(&sid);
    })
    .await;
}

fn report_path() -> PathBuf {
    if let Ok(p) = std::env::var("GROK_CU_APP_HARNESS_OUT") {
        return PathBuf::from(p);
    }
    crate::paths::app_data_root().join("harness-report.json")
}

fn write_report(v: &Value) {
    let path = report_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        &path,
        serde_json::to_vec_pretty(v).unwrap_or_else(|_| b"{}".to_vec()),
    );
}

async fn on_main<T: Send + 'static>(
    app: &tauri::AppHandle,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .map_err(|e| e.to_string())?;
    rx.await.map_err(|e| e.to_string())
}

async fn run(app: tauri::AppHandle) -> Value {
    let mut errors: Vec<String> = Vec::new();
    let mut scenarios: Vec<Value> = Vec::new();
    let push = |scenarios: &mut Vec<Value>, name: &str, ok: bool, detail: String| {
        scenarios.push(json!({"name": name, "ok": ok, "detail": detail}));
    };

    let identity = crate::paths::isolated_app_instance_id();
    let home = crate::paths::app_data_root();
    let acp_stub_ledger = home.join("computer-use").join("acp-stub-ledger.jsonl");
    let _ = std::fs::remove_file(&acp_stub_ledger);
    std::env::set_var("GROK_CU_ACP_STUB_LEDGER", &acp_stub_ledger);
    let lease = home.join("computer-use").join("desktop.lease");
    let home_s = home
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    if identity.is_none() {
        errors.push("isolated identity missing at runtime".into());
    }
    if home_s.contains("/.grok/")
        || home_s.ends_with("/.grok")
        || home_s.contains("appdata/roaming/grok-app")
            && !home_s.contains("cu-d6")
            && !home_s.contains("grok-cu-d6")
    {
        // Isolated home must not be the shipping AppData path unless the test
        // explicitly chose a subdirectory named for this run.
        if !home_s.contains("cu-d6") && !home_s.contains("48h") {
            errors.push(format!(
                "home looks like shipping App data: {}",
                home.display()
            ));
        }
    }
    push(
        &mut scenarios,
        "identity",
        identity.is_some() && !errors.iter().any(|e| e.contains("home looks")),
        format!(
            "id={:?} home={} lease_under_home={}",
            identity,
            home.display(),
            lease.starts_with(&home) || !lease.exists()
        ),
    );

    let settings = store::load_settings();
    let feature_off = !settings.computer_use_enabled;
    if !feature_off {
        errors.push("feature was not default-off on fresh home".into());
    }
    push(
        &mut scenarios,
        "feature_default_off",
        feature_off,
        format!("computer_use_enabled={}", settings.computer_use_enabled),
    );

    let issues = crate::computer_use::runtime::diagnose();
    let issue_s = issues
        .iter()
        .map(|i| format!("{}:{}", i.component, i.code))
        .collect::<Vec<_>>()
        .join(",");
    if issues.is_empty() {
        push(
            &mut scenarios,
            "runtime_repair",
            true,
            "already valid".into(),
        );
    } else if let Err(e) = repair_runtime(&app) {
        errors.push(format!("runtime repair: {e}"));
        push(
            &mut scenarios,
            "runtime_repair",
            false,
            format!("issues={issue_s} repair={e}"),
        );
    } else {
        push(
            &mut scenarios,
            "runtime_repair",
            true,
            format!("repaired issues={issue_s}"),
        );
    }

    match crate::commands::computer_use_set_feature(app.state::<Arc<SessionManager>>(), true).await
    {
        Ok(()) => push(&mut scenarios, "feature_on", true, "set".into()),
        Err(e) => {
            errors.push(format!("feature on: {e}"));
            push(&mut scenarios, "feature_on", false, e);
        }
    }

    if let Err(e) = configure_isolated_settings() {
        errors.push(format!("settings: {e}"));
        push(&mut scenarios, "isolated_settings", false, e);
    } else {
        push(
            &mut scenarios,
            "isolated_settings",
            true,
            "independent+stub".into(),
        );
    }

    let session = match store::create_session(None, Some("d6-shell".into()), false) {
        Ok(m) => m,
        Err(e) => {
            errors.push(format!("create session: {e}"));
            return finish(
                false,
                &home,
                identity.as_deref(),
                AppShellRoundCounts::default(),
                scenarios,
                errors,
            );
        }
    };
    let sid = session.id.clone();

    let mgr = app.state::<Arc<SessionManager>>();
    let connect = tokio::time::timeout(
        Duration::from_secs(20),
        crate::commands::session_connect(
            app.clone(),
            mgr.clone(),
            None,
            Some(sid.clone()),
            None,
            None,
        ),
    )
    .await;
    match connect {
        Ok(Ok(snap)) => push(
            &mut scenarios,
            "local_chat_connect",
            true,
            format!("state={:?} backend={}", snap.state, snap.backend),
        ),
        Ok(Err(e)) => {
            errors.push(format!("connect: {e}"));
            push(&mut scenarios, "local_chat_connect", false, e);
        }
        Err(_) => {
            errors.push("connect timed out".into());
            push(
                &mut scenarios,
                "local_chat_connect",
                false,
                "timeout 20s".into(),
            );
        }
    }

    let off_list =
        crate::commands::computer_use_list_targets(Some(sid.clone()), None, Some("desktop".into()))
            .await;
    match crate::commands::computer_use_set_feature(app.state::<Arc<SessionManager>>(), false).await
    {
        Ok(()) => {
            let listed = crate::commands::computer_use_list_targets(
                Some(sid.clone()),
                None,
                Some("desktop".into()),
            )
            .await
            .unwrap_or_default();
            let ok = listed.is_empty();
            if !ok {
                errors.push("feature-off still listed targets".into());
            }
            push(
                &mut scenarios,
                "feature_off_lists_empty",
                ok,
                format!("n={}", listed.len()),
            );
        }
        Err(e) => {
            errors.push(format!("feature off: {e}"));
            push(&mut scenarios, "feature_off_lists_empty", false, e);
        }
    }
    let _ =
        crate::commands::computer_use_set_feature(app.state::<Arc<SessionManager>>(), true).await;
    let _ = off_list;

    for surface in ["desktop", "managed-browser", "existing-tabs", "app-webview"] {
        match crate::commands::computer_use_list_targets(
            Some(sid.clone()),
            None,
            Some(surface.into()),
        )
        .await
        {
            Ok(rows) => push(
                &mut scenarios,
                &format!("list_{surface}"),
                true,
                format!("n={}", rows.len()),
            ),
            Err(e) => {
                errors.push(format!("list {surface}: {e}"));
                push(&mut scenarios, &format!("list_{surface}"), false, e);
            }
        }
    }

    let oracle = home.join("computer-use").join("oracle.html");
    if let Err(e) = std::fs::create_dir_all(oracle.parent().unwrap_or(Path::new(".")))
        .map_err(|e| e.to_string())
        .and_then(|_| std::fs::write(&oracle, ORACLE_HTML).map_err(|e| e.to_string()))
    {
        errors.push(format!("oracle file: {e}"));
    }
    let url = format!(
        "file:///{}",
        oracle.display().to_string().replace('\\', "/")
    );
    let label = "resource-browser-cu-d6".to_string();
    let created = {
        let app2 = app.clone();
        let label2 = label.clone();
        let url2 = url.clone();
        on_main(&app, move || {
            crate::side_browser_host::create(
                &app2,
                label2,
                url2,
                "main".into(),
                8.0,
                8.0,
                420.0,
                320.0,
            )
        })
        .await
    };
    match created {
        Ok(Ok(())) => push(&mut scenarios, "side_browser_create", true, label.clone()),
        Ok(Err(e)) | Err(e) => {
            errors.push(format!("side browser: {e}"));
            push(&mut scenarios, "side_browser_create", false, e);
        }
    }
    tokio::time::sleep(Duration::from_millis(700)).await;

    let wv_rounds = env_u32("GROK_CU_WEBVIEW_ROUNDS", 20);
    match tokio::time::timeout(
        Duration::from_secs(480),
        webview_rounds(&app, &sid, &label, wv_rounds),
    )
    .await
    {
        Ok(Ok(())) => push(
            &mut scenarios,
            "webview_bind_rounds",
            true,
            format!("{wv_rounds}"),
        ),
        Ok(Err(e)) => {
            errors.push(format!("webview rounds: {e}"));
            push(&mut scenarios, "webview_bind_rounds", false, e);
        }
        Err(_) => {
            errors.push("webview rounds timed out".into());
            push(
                &mut scenarios,
                "webview_bind_rounds",
                false,
                "timeout 480s".into(),
            );
        }
    }

    match tokio::time::timeout(
        Duration::from_secs(45),
        webview_fail_reclaim(&app, &sid, &label, &home),
    )
    .await
    {
        Ok(Ok(detail)) => push(&mut scenarios, "webview_fail_reclaim", true, detail),
        Ok(Err(e)) => {
            errors.push(format!("webview fail+reclaim: {e}"));
            push(&mut scenarios, "webview_fail_reclaim", false, e);
        }
        Err(_) => {
            errors.push("webview fail+reclaim timed out".into());
            push(
                &mut scenarios,
                "webview_fail_reclaim",
                false,
                "timeout 45s".into(),
            );
        }
    }

    let managed_rounds = env_u32("GROK_CU_MANAGED_ROUNDS", 0);
    let mut managed_ok = 0u32;
    if managed_rounds > 0 {
        match tokio::time::timeout(
            Duration::from_secs(1200),
            managed_product_rounds(&app, &sid, managed_rounds),
        )
        .await
        {
            Ok(Ok(n)) => {
                managed_ok = n;
                push(
                    &mut scenarios,
                    "managed_product_rounds",
                    n >= managed_rounds,
                    format!("{n}/{managed_rounds}"),
                );
                if n < managed_rounds {
                    errors.push(format!("managed product rounds {n}/{managed_rounds}"));
                }
            }
            Ok(Err(e)) => {
                errors.push(format!("managed product: {e}"));
                push(&mut scenarios, "managed_product_rounds", false, e);
            }
            Err(_) => {
                errors.push("managed product timed out".into());
                push(
                    &mut scenarios,
                    "managed_product_rounds",
                    false,
                    "timeout 1200s".into(),
                );
            }
        }
    }

    let inner = env_u32("GROK_CU_APP_HARNESS_ROUNDS", 10).max(1);
    let mut inner_ok = 0u32;
    #[cfg(target_os = "windows")]
    let desktop_fx = match spawn_isolated_desktop_fixture() {
        Ok(pair) => Some(pair),
        Err(e) => {
            errors.push(format!("desktop fixture spawn: {e}"));
            None
        }
    };
    #[cfg(not(target_os = "windows"))]
    let desktop_fx: Option<(String, Child)> = None;
    let fixture_title_owned = desktop_fx.as_ref().map(|(t, _)| t.clone());
    for i in 1..=inner {
        match tokio::time::timeout(
            Duration::from_secs(45),
            one_round(&app, &sid, &label, i, fixture_title_owned.as_deref()),
        )
        .await
        {
            Ok(Ok(detail)) => {
                inner_ok += 1;
                push(&mut scenarios, &format!("round_{i:02}"), true, detail);
            }
            Ok(Err(e)) => {
                errors.push(format!("round {i}: {e}"));
                push(&mut scenarios, &format!("round_{i:02}"), false, e);
                break;
            }
            Err(_) => {
                errors.push(format!("round {i}: timeout"));
                push(
                    &mut scenarios,
                    &format!("round_{i:02}"),
                    false,
                    "timeout 45s".into(),
                );
                break;
            }
        }
    }
    #[cfg(target_os = "windows")]
    if let Some((_, mut child)) = desktop_fx {
        let _ = child.kill();
        let _ = child.wait();
    }

    let normal_stop =
        verify_normal_stop_catalog_cleanup(&app, &sid, &label, &acp_stub_ledger).await;
    match normal_stop {
        Ok(detail) => push(
            &mut scenarios,
            "normal_chat_stop_catalog_cleanup",
            true,
            detail,
        ),
        Err(error) => {
            errors.push(format!("normal chat Stop cleanup: {error}"));
            push(
                &mut scenarios,
                "normal_chat_stop_catalog_cleanup",
                false,
                error,
            );
        }
    }

    revoke_session(&sid).await;
    let slash_ok = slash_open(&app, "computer-use").await;
    let slash_browser_ok = slash_open(&app, "computer-use-browser").await;
    if let Err(e) = &slash_ok {
        errors.push(format!("slash_desktop: {e}"));
    }
    if let Err(e) = &slash_browser_ok {
        errors.push(format!("slash_managed_browser: {e}"));
    }
    push(
        &mut scenarios,
        "slash_desktop",
        slash_ok.is_ok(),
        slash_ok.err().unwrap_or_else(|| "opened".into()),
    );
    push(
        &mut scenarios,
        "slash_managed_browser",
        slash_browser_ok.is_ok(),
        slash_browser_ok.err().unwrap_or_else(|| "opened".into()),
    );

    let pair = crate::commands::computer_use_begin_pairing().await;
    match pair {
        Ok(ch) => {
            let endpoint_ok = url::Url::parse(&ch.endpoint).is_ok_and(|url| {
                url.scheme() == "http"
                    && url.host_str() == Some("127.0.0.1")
                    && url.port().is_some()
                    && url.path() == "/"
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none()
            });
            let groups: Vec<_> = ch.verification_code.split('-').collect();
            let code_ok = groups.len() == 4
                && groups.iter().all(|part| {
                    part.len() == 5 && part.bytes().all(|byte| byte.is_ascii_hexdigit())
                });
            let identity_ok = uuid::Uuid::parse_str(&ch.nonce).is_ok()
                && uuid::Uuid::parse_str(&ch.instance_id).is_ok()
                && ch.installed_extension_id == grok_computer_use_core::pairing::EXTENSION_ID;
            let confirm_ok = crate::commands::computer_use_confirm_pairing_app(ch.nonce.clone())
                .await
                .is_ok();
            let revoke_ok = crate::commands::computer_use_revoke_pairing().await.is_ok();
            let revoked_rejected = crate::commands::computer_use_confirm_pairing_app(ch.nonce)
                .await
                .is_err();
            let passed = endpoint_ok
                && code_ok
                && identity_ok
                && confirm_ok
                && revoke_ok
                && revoked_rejected;
            if !passed {
                errors.push("pairing App command contract failed".into());
            }
            push(
                &mut scenarios,
                "existing_tabs_pairing_app_commands",
                passed,
                format!("endpoint={endpoint_ok} code_format={code_ok} identity={identity_ok} confirm={confirm_ok} revoke={revoke_ok} revoked_rejected={revoked_rejected}; App commands only, not extension transport"),
            );
        }
        Err(e) => {
            errors.push(format!("pairing: {e}"));
            push(&mut scenarios, "existing_tabs_pairing", false, e);
        }
    }

    let shutdown = crate::computer_use::shutdown::shutdown_with_catalog_barrier(
        mgr.inner(),
        Duration::from_millis(2_500),
    )
    .await;
    let shutdown_ok = shutdown.is_clean();
    if !shutdown_ok {
        errors.push(format!(
            "app close cleanup: settled {}/{}: {}",
            shutdown.settled,
            shutdown.attempted,
            shutdown.errors.join("; ")
        ));
    }
    push(
        &mut scenarios,
        "app_close_cleanup",
        shutdown_ok,
        format!(
            "catalogs={}/{} errors={}",
            shutdown.settled,
            shutdown.attempted,
            shutdown.errors.len()
        ),
    );

    let blocking: Vec<&String> = errors
        .iter()
        .filter(|e| !e.contains("missing-hook") && !e.starts_with("slash_"))
        .collect();
    let ok = blocking.is_empty()
        && inner_ok == inner
        && (managed_rounds == 0 || managed_ok >= managed_rounds);
    finish(
        ok,
        &home,
        identity.as_deref(),
        AppShellRoundCounts {
            webview: wv_rounds,
            inner: inner_ok,
            managed: managed_ok,
        },
        scenarios,
        errors,
    )
}

fn last_stub_catalog(path: &Path) -> Result<(usize, usize), String> {
    let body =
        std::fs::read_to_string(path).map_err(|error| format!("read ACP stub ledger: {error}"))?;
    let row = body
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<Value>(line).ok())
        .ok_or("ACP stub ledger has no catalog update")?;
    let total = row
        .get("totalCount")
        .and_then(Value::as_u64)
        .ok_or("ACP stub ledger totalCount missing")? as usize;
    let computer_use = row
        .get("cuCount")
        .and_then(Value::as_u64)
        .ok_or("ACP stub ledger cuCount missing")? as usize;
    Ok((total, computer_use))
}

async fn verify_normal_stop_catalog_cleanup(
    app: &tauri::AppHandle,
    session_id: &str,
    webview_label: &str,
    ledger: &Path,
) -> Result<String, String> {
    let bound = production_bind_webview(app, session_id, webview_label).await?;
    if bound.target_id.is_empty() {
        return Err("normal Stop precondition did not bind a target".into());
    }
    let (attached_total, attached_cu) = last_stub_catalog(ledger)?;
    if attached_cu != 1 {
        return Err(format!(
            "normal Stop precondition expected one CU catalog entry, got {attached_cu}"
        ));
    }

    let manager = app.state::<Arc<SessionManager>>();
    manager
        .inner()
        .stop(app.clone(), Some(session_id.to_string()))
        .await?;
    let (detached_total, detached_cu) = last_stub_catalog(ledger)?;
    let status = manager
        .inner()
        .mcp_catalog_status(session_id)
        .ok_or("normal Stop did not publish MCP catalog status")?;
    if detached_cu != 0 || status.pending || status.desired_present {
        return Err(format!(
            "normal Stop did not settle absent: cu={detached_cu} pending={} desired_present={}",
            status.pending, status.desired_present
        ));
    }
    Ok(format!(
        "catalog={attached_total}/1 -> {detached_total}/0; generation={}/{}",
        status.desired_generation,
        status.applied_generation.unwrap_or_default()
    ))
}

#[derive(Default)]
struct AppShellRoundCounts {
    webview: u32,
    inner: u32,
    managed: u32,
}

fn finish(
    ok: bool,
    home: &Path,
    identity: Option<&str>,
    rounds: AppShellRoundCounts,
    scenarios: Vec<Value>,
    errors: Vec<String>,
) -> Value {
    json!({
        "ok": ok,
        "pid": std::process::id(),
        "identity": identity,
        "home": home.display().to_string(),
        "lease": home.join("computer-use").join("desktop.lease").display().to_string(),
        "webviewRounds": rounds.webview,
        "innerRoundsOk": rounds.inner,
        "managedRoundsOk": rounds.managed,
        "scenarios": scenarios,
        "errors": errors,
        "grade": "scripted App-shell E3",
    })
}

fn configure_isolated_settings() -> Result<(), String> {
    let mut s = store::load_settings();
    s.session_data_mode = "independent".into();
    s.computer_use_enabled = true;
    if let Some(stub) = acp_stub_path() {
        s.manual_cli_path = Some(stub.display().to_string());
    }
    store::save_settings(&s)
}

fn acp_stub_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("GROK_CU_ACP_STUB") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let sibling = exe.parent()?.join("cu-acp-stub.exe");
    sibling.is_file().then_some(sibling)
}

fn repair_runtime(app: &tauri::AppHandle) -> Result<(), String> {
    let resource = std::env::var("GROK_CU_RESOURCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources"));
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| resource.clone());
    let pack = crate::computer_use::runtime::repair_from_install(&resource, &exe_dir)?;
    tracing::info!(pack = %pack, "cu harness repaired runtime");
    let _ = app;
    Ok(())
}

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

async fn slash_open(app: &tauri::AppHandle, action: &str) -> Result<(), String> {
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

async fn bind_side_on_main(
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

async fn webview_rounds(
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

async fn production_bind_webview(
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

async fn webview_fail_reclaim(
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

async fn managed_product_rounds(
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

#[cfg(target_os = "windows")]
fn spawn_isolated_desktop_fixture() -> Result<(String, Child), String> {
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
    let adapter = super::windows_adapter::WindowsAdapter::new();
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

async fn one_round(
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

struct McpClient {
    child: Option<Child>,
    stdin: Option<std::process::ChildStdin>,
    rx: mpsc::Receiver<String>,
    stdout_reader: Option<std::thread::JoinHandle<()>>,
    stderr_reader: Option<std::thread::JoinHandle<()>>,
    next_id: i64,
}

impl McpClient {
    fn spawn(entry: &Value) -> Result<Self, String> {
        let command = entry
            .get("command")
            .and_then(Value::as_str)
            .ok_or("MCP command missing")?;
        let args = entry
            .get("args")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut cmd = Command::new(command);
        for arg in &args {
            if let Some(s) = arg.as_str() {
                cmd.arg(s);
            }
        }
        if let Some(rows) = entry.get("env").and_then(Value::as_array) {
            for row in rows {
                if let (Some(name), Some(value)) = (
                    row.get("name").and_then(Value::as_str),
                    row.get("value").and_then(Value::as_str),
                ) {
                    cmd.env(name, value);
                }
            }
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn().map_err(|e| format!("spawn MCP: {e}"))?;
        let stdin = child.stdin.take().ok_or("MCP stdin missing")?;
        let stdout = child.stdout.take().ok_or("MCP stdout missing")?;
        let stderr_reader = child.stderr.take().map(|stderr| {
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut BufReader::new(stderr), &mut std::io::sink());
            })
        });
        let (tx, rx) = mpsc::channel();
        let stdout_reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            rx,
            stdout_reader: Some(stdout_reader),
            stderr_reader,
            next_id: 1,
        })
    }

    fn initialize(&mut self) -> Result<(), String> {
        let _ = self.call(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "cu-app-shell", "version": "1"}
            }),
        )?;
        self.notify("notifications/initialized", json!({}))
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let msg = json!({"jsonrpc":"2.0","method":method,"params":params});
        self.write(&msg)
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        self.write(&msg)?;
        let line = self
            .rx
            .recv_timeout(Duration::from_secs(20))
            .map_err(|_| format!("MCP {method} timed out"))?;
        let value: Value =
            serde_json::from_str(line.trim()).map_err(|e| format!("MCP {method} json: {e}"))?;
        if let Some(err) = value.get("error") {
            return Err(format!("MCP {method} error: {err}"));
        }
        value
            .get("result")
            .cloned()
            .ok_or_else(|| format!("MCP {method} missing result"))
    }

    fn tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let raw = self.call("tools/call", json!({"name": name, "arguments": args}))?;
        if raw.get("isError") == Some(&json!(true)) {
            let text = raw
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find_map(|part| part.get("text").and_then(Value::as_str))
                .unwrap_or("");
            return Err(format!("{name} failed: {text}"));
        }
        if let Some(text) = raw
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find_map(|part| part.get("text").and_then(Value::as_str))
        {
            if let Ok(value) = serde_json::from_str::<Value>(text) {
                return Ok(value);
            }
        }
        Ok(raw)
    }

    fn write(&mut self, msg: &Value) -> Result<(), String> {
        let line = format!("{msg}\n");
        self.stdin
            .as_mut()
            .ok_or_else(|| "MCP stdin closed".to_string())?
            .write_all(line.as_bytes())
            .map_err(|e| e.to_string())?;
        self.stdin
            .as_mut()
            .ok_or_else(|| "MCP stdin closed".to_string())?
            .flush()
            .map_err(|e| e.to_string())
    }

    fn kill(&mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stdin.take();
        if let Some(mut child) = self.child.take() {
            if !matches!(child.try_wait(), Ok(Some(_))) {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        if let Some(reader) = self.stdout_reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}
