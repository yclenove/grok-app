//! Original App UI/native round trips, executed by the harness worker.

use super::*;
use tauri::Manager;

pub(in crate::computer_use::webview) fn main<T: Send + 'static>(
    app: &tauri::AppHandle,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(work());
    })
    .map_err(|_| "App main-thread dispatch unavailable")?;
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|_| "App main-thread round trip timed out")?
}

pub(in crate::computer_use::webview) fn eval(
    app: &tauri::AppHandle,
    label: &str,
    script: &str,
) -> Result<serde_json::Value, String> {
    let webview = app
        .get_webview(label)
        .ok_or("original App WebView missing")?;
    let (tx, rx) = mpsc::channel();
    webview
        .eval_with_callback(script.to_string(), move |result| {
            let _ = tx.send(result);
        })
        .map_err(|_| "App fixture script dispatch failed")?;
    let raw = rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "App fixture script callback timed out")?;
    serde_json::from_str(&raw).map_err(|_| "App fixture script result invalid".into())
}

pub(in crate::computer_use::webview) fn ready(
    app: &tauri::AppHandle,
    label: &str,
    product_main: bool,
) -> Result<(), String> {
    let script = if product_main {
        "document.readyState === 'complete' && typeof window.__TAURI_INTERNALS__?.invoke === 'function' && document.getElementById('root')?.children.length > 0"
    } else {
        "document.readyState === 'complete' && document.querySelectorAll('a[download]').length === 2"
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if eval(app, label, script)?.as_bool() == Some(true) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            let diagnostic = eval(
                app,
                label,
                "({readyState:document.readyState,url:location.href,downloadLinks:document.querySelectorAll('a[download]').length,rootChildren:document.getElementById('root')?.children.length??0,hasInvoke:typeof window.__TAURI_INTERNALS__?.invoke==='function'})",
            );
            return Err(format!(
                "App fixture document did not become ready; label={label}; product_main={product_main}; state={diagnostic:?}"
            ));
        }
    }
}

pub(in crate::computer_use::webview) fn create(
    app: &tauri::AppHandle,
    label: &str,
    url: &str,
    x: f64,
) -> Result<(), String> {
    let handle = app.clone();
    let label = label.to_string();
    let url = url.to_string();
    main(app, move || {
        crate::side_browser_host::create(&handle, label, url, "main".into(), x, 64.0, 350.0, 220.0)
    })
}

pub(super) fn native<T: Send + 'static>(
    view: &tauri::Webview,
    work: impl FnOnce(tauri::webview::PlatformWebview) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::channel();
    view.with_webview(move |platform| {
        let _ = tx.send(work(platform));
    })
    .map_err(|_| "original App native dispatch unavailable")?;
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|_| "original App native callback timed out")?
}

pub(super) fn pid(view: &tauri::Webview) -> Result<u32, String> {
    native(view, |platform| {
        let core = unsafe { platform.controller().CoreWebView2() }
            .map_err(|_| "App native core unavailable")?;
        let mut pid = 0;
        unsafe { core.BrowserProcessId(&mut pid) }.map_err(|_| "App browser PID unavailable")?;
        if pid == 0 {
            return Err("App browser PID is zero".into());
        }
        Ok(pid)
    })
}

pub(super) fn visible(view: &tauri::Webview) -> Result<bool, String> {
    native(view, |platform| {
        let mut visible = windows::core::BOOL::default();
        unsafe { platform.controller().IsVisible(&mut visible) }
            .map_err(|_| "App native visibility unavailable")?;
        Ok(visible.as_bool())
    })
}

pub(super) fn start_close(app: &tauri::AppHandle, label: &str, count: u32) -> Result<(), String> {
    let label = serde_json::to_string(label).map_err(|_| "fixture label invalid")?;
    let script = format!(
        r#"(() => {{
        const state = window.__cuDownloadClose = {{requested:{count},completed:0,failed:0,heartbeat:0}};
        for (let i=0; i<{count}; i++) {{
            window.__TAURI_INTERNALS__.invoke('side_browser_close', {{label:{label}}})
                .then(() => {{state.completed++}}, () => {{state.failed++}});
        }}
        return true;
    }})()"#
    );
    if eval(app, "main", &script)?.as_bool() != Some(true) {
        return Err("App close IPC did not start".into());
    }
    Ok(())
}

pub(super) fn heartbeat(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    eval(
        app,
        "main",
        "(() => { window.__cuDownloadClose.heartbeat++; return {...window.__cuDownloadClose} })()",
    )
}

pub(super) fn close_result(
    app: &tauri::AppHandle,
    expected: u64,
) -> Result<serde_json::Value, String> {
    close_result_with_observation_budget(app, expected, Duration::from_secs(5))
}

pub(super) fn close_result_with_observation_budget(
    app: &tauri::AppHandle,
    expected: u64,
    observation_budget: Duration,
) -> Result<serde_json::Value, String> {
    let deadline = Instant::now() + observation_budget;
    loop {
        let state = heartbeat(app)?;
        if state["completed"].as_u64().unwrap_or(0) + state["failed"].as_u64().unwrap_or(0)
            == expected
        {
            return Ok(state);
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "App close IPC observation deadline expired; state={state}"
            ));
        }
    }
}
