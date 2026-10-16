//! Opt-in native diagnostics for the disposable fixture probe.
//!
//! Disabled by default. Records contain API/attribute/action names, return
//! codes and timings, never attribute values or text being written. The probe
//! itself explicitly prints fixture node metadata. No permission is requested.

use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{Mutex, OnceLock},
    time::Instant,
};

static ENABLED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static TIMEOUTS: OnceLock<Mutex<HashMap<usize, f32>>> = OnceLock::new();

/// Enable JSON-lines native-call diagnostics on stdout for this process.
/// Synchronous output adds overhead; use only for diagnosis, not benchmarks.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

pub(crate) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Emit a metadata-only diagnostic record when the probe opted in.
pub(crate) fn note(kind: &str, detail: &str) {
    if enabled() {
        println!("{}", serde_json::json!({"kind": kind, "detail": detail}));
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn ax_call<T: Debug>(
    operation: &str,
    element: usize,
    detail: &str,
    call: impl FnOnce() -> T,
) -> T {
    if !enabled() {
        return call();
    }
    println!(
        "{}",
        serde_json::json!({"kind": "AX_START", "api": operation,
        "element": element, "detail": detail})
    );
    let started = Instant::now();
    let result = call();
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    let raw = format!("{result:?}");
    let timeouts = TIMEOUTS.get_or_init(Mutex::default);
    let timeout = timeouts.lock().ok().and_then(|mut map| {
        if operation == "AXUIElementSetMessagingTimeout" && raw == "0" {
            if let Ok(seconds) = detail.parse::<f32>() {
                map.insert(element, seconds);
            }
        }
        map.get(&element).copied()
    });
    println!(
        "{}",
        serde_json::json!({
            "kind": "AX_CALL", "api": operation, "element": element,
            "detail": detail, "native_result": raw, "elapsed_ms": elapsed,
            "messaging_timeout_secs": timeout,
            "timeout_source": if timeout.is_some() { "successful_set; no AX getter exists" }
                else { "native_default; not configured on this reference" },
        })
    );
    result
}

#[cfg(target_os = "macos")]
pub(crate) fn forget_reference(element: usize) {
    if let Some(timeouts) = TIMEOUTS.get() {
        if let Ok(mut map) = timeouts.lock() {
            map.remove(&element);
        }
    }
}

#[cfg(target_os = "macos")]
#[path = "diagnostics/ffi.rs"]
mod native;
#[cfg(target_os = "macos")]
pub use native::{runtime_report, FixtureToggles};

#[cfg(target_os = "macos")]
#[path = "diagnostics/windows_ffi.rs"]
mod windows_native;
#[cfg(target_os = "macos")]
pub use windows_native::fixture_window_report;
