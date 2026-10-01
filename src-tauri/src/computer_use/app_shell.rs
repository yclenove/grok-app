//! Isolated debug App-shell E3 harness.
//!
//! Runs only when `GROK_APP_INSTANCE_ID` is a non-shipping suffix **and**
//! `GROK_CU_APP_HARNESS=1`. Not compiled into release binaries.

use std::path::{Path, PathBuf};
#[cfg(not(target_os = "windows"))]
use std::process::Child;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::Manager;

use crate::computer_use::ComputerUseAdapter;
use crate::session_manager::SessionManager;
use crate::store;

mod desktop;
mod managed;
mod mcp;
mod webview;

use desktop::one_round;
#[cfg(target_os = "windows")]
use desktop::spawn_isolated_desktop_fixture;
use managed::managed_product_rounds;
use webview::{production_bind_webview, slash_open, webview_fail_reclaim, webview_rounds};

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
