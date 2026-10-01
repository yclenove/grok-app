//! Fixed Host scripts on the captured native view, with callback-owned occupancy.

use super::{QueuedScript, ScriptOperation, SideBrowserLiveView};

#[cfg(windows)]
pub(super) fn run(
    view: &SideBrowserLiveView,
    script: &str,
    operation: ScriptOperation,
) -> Result<String, String> {
    if let Err(error) = view.require_document().and_then(|()| operation.check()) {
        operation.finished();
        return Err(error);
    }
    let Some(world) = view.native.script_world() else {
        operation.finished();
        return Err("WebView native world unavailable".into());
    };
    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    let native = view.native.clone();
    let document = view.document;
    let queued = QueuedScript::new(operation, tx.clone());
    let script = script.to_string();
    let scheduled = view.webview.with_webview(move |platform| {
        if !native.matches(document) || queued.check().is_err() {
            queued.reject("WebView native dispatch retired before execution");
            return;
        }
        let core = match unsafe { platform.controller().CoreWebView2() } {
            Ok(core) => core,
            Err(_) => {
                queued.reject("WebView native controller unavailable");
                return;
            }
        };
        let observed_native = native.clone();
        if super::isolated_windows::process::install(&core, &world, move |failure| {
            observed_native.process_failed(failure);
        })
        .is_err()
        {
            queued.reject("WebView process observer unavailable");
            return;
        }
        super::isolated_windows::dispatch(
            core,
            world,
            document.generation,
            queued.handoff(),
            move || native.matches(document),
            script,
            tx,
        );
    });
    if scheduled.is_err() {
        return Err("WebView native dispatch unavailable".into());
    }
    let result = rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .map_err(|_| "WebView typed script outcome unknown")??;
    view.require_document()?;
    Ok(result)
}

// macOS/Linux native isolated worlds remain a separate unfinished W2 gate.
#[cfg(not(windows))]
pub(super) fn run(
    view: &SideBrowserLiveView,
    script: &str,
    operation: ScriptOperation,
) -> Result<String, String> {
    if let Err(error) = view.require_document().and_then(|()| operation.check()) {
        operation.finished();
        return Err(error);
    }
    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    let webview = view.webview.clone();
    let native = view.native.clone();
    let document = view.document;
    let queued = QueuedScript::new(operation, tx.clone());
    let script = script.to_string();
    // Tauri's dispatcher executes immediately on its native UI thread. Re-check
    // queued cancellation/document identity there, before EvaluateScript dispatch.
    let scheduled = view.app.run_on_main_thread(move || {
        if !native.matches(document) || queued.check().is_err() {
            queued.reject("WebView native dispatch retired before execution");
            return;
        }
        let queued = queued.handoff();
        let completed = queued.clone();
        let replied = tx.clone();
        let dispatched = webview.eval_with_callback(script, move |result| {
            completed.finished();
            let _ = replied.send(Ok(result));
        });
        if dispatched.is_err() {
            // Dispatcher enqueue failed: no native EvaluateScript was scheduled.
            queued.finished();
            let _ = tx.send(Err("WebView typed script unavailable".into()));
        }
        // Tauri may discard a callback on an underlying native error. A missing
        // callback is not physical completion; keep that ticket outstanding.
    });
    if scheduled.is_err() {
        return Err("WebView native dispatch unavailable".into());
    }
    let result = rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .map_err(|_| "WebView typed script outcome unknown")??;
    view.require_document()?;
    Ok(result)
}
