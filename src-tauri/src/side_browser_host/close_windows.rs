//! Off-UI native close. Failed cancellation retains the exact owner for explicit retry.

use super::*;
use crate::computer_use::webview;
use parking_lot::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

static CLOSING: LazyLock<Mutex<HashMap<String, Arc<CloseOwner>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct CloseOwner {
    webview: tauri::Webview,
    instance: Option<u64>,
    gate: Mutex<()>,
    closed: AtomicBool,
}

pub(super) fn ensure_openable(label: &str) -> Result<(), String> {
    if CLOSING.lock().contains_key(label) {
        Err("side-browser-close-pending: retry closing the retained view".into())
    } else {
        Ok(())
    }
}

fn replacement(label: &str, expected: Option<u64>) -> bool {
    webview::lifecycle::current(label)
        .and_then(|native| native.document())
        .is_some_and(|document| Some(document.instance) != expected)
}

pub(super) fn close(app: &AppHandle, label: String) -> Result<(), String> {
    let owner = {
        let mut closing = CLOSING.lock();
        if let Some(owner) = closing.get(&label) {
            if replacement(&label, owner.instance) {
                return Err("side browser close belongs to a retired native instance".into());
            }
            owner.clone()
        } else {
            let Some(webview) = app.get_webview(&label) else {
                return Ok(());
            };
            let native = webview::lifecycle::current(&label);
            let instance = native
                .as_ref()
                .and_then(|native| native.document())
                .map(|document| document.instance);
            if let Some(instance) = instance {
                // Fence download admission before any caller can observe close.
                webview::retire_side_browser_downloads(&webview, instance);
            }
            if let Some(native) = native {
                webview::lifecycle::closed(&label, &native);
            }
            let owner = Arc::new(CloseOwner {
                webview,
                instance,
                gate: Mutex::new(()),
                closed: AtomicBool::new(false),
            });
            closing.insert(label.clone(), owner.clone());
            owner
        }
    };
    // Concurrent close callers join this owner. They cannot close a replacement
    // or interpret an earlier caller's pending/failed attempt as completion.
    let _gate = owner.gate.lock();
    if owner.closed.load(Ordering::SeqCst) {
        return Ok(());
    }
    if replacement(&label, owner.instance) {
        return Err("side browser close belongs to a retired native instance".into());
    }
    // A removed tab must not leave its retained child painted over another
    // page while cancellation is pending. Hide preserves the original owner.
    owner
        .webview
        .hide()
        .map_err(|error| format!("side browser hide before close: {error}"))?;
    if let Some(instance) = owner.instance {
        webview::retire_side_browser_downloads(&owner.webview, instance);
        webview::wait_side_browser_downloads_retired(instance)?;
        webview::uninstall_side_browser_downloads(&owner.webview, instance)?;
    }
    if replacement(&label, owner.instance) {
        return Err("side browser changed during native close".into());
    }
    owner
        .webview
        .close()
        .map_err(|error| format!("side browser close: {error}"))?;
    owner.closed.store(true, Ordering::SeqCst);
    let mut closing = CLOSING.lock();
    if closing
        .get(&label)
        .is_some_and(|current| Arc::ptr_eq(current, &owner))
    {
        closing.remove(&label);
    }
    Ok(())
}
