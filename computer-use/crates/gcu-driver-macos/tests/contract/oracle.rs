//! External fixture oracle. Polls a JSON file; failures include the last read.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Path from `GCU_FIXTURE_STATE`, otherwise the historical fixture default.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn fixture_state_path() -> PathBuf {
    std::env::var_os("GCU_FIXTURE_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/fixture_state.json"))
}

/// Read one JSON object. Missing or invalid files are `None`.
pub fn read_state(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Poll `path` until `pred` returns `Some` or `timeout` elapses.
///
/// The error names the timeout and the last JSON object that was parsed.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub async fn poll_until<T, F>(path: &Path, timeout: Duration, mut pred: F) -> Result<T, String>
where
    F: FnMut(&Value) -> Option<T>,
{
    let start = tokio::time::Instant::now();
    let mut last = None;
    while start.elapsed() < timeout {
        if let Some(state) = read_state(path) {
            if let Some(found) = pred(&state) {
                return Ok(found);
            }
            last = Some(state);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Err(timeout_message(timeout, last.as_ref()))
}

/// Blocking poll for unit tests that are not on the Tokio runtime.
pub fn poll_blocking<T, F>(path: &Path, timeout: Duration, mut pred: F) -> Result<T, String>
where
    F: FnMut(&Value) -> Option<T>,
{
    let start = std::time::Instant::now();
    let mut last = None;
    while start.elapsed() < timeout {
        if let Some(state) = read_state(path) {
            if let Some(found) = pred(&state) {
                return Ok(found);
            }
            last = Some(state);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err(timeout_message(timeout, last.as_ref()))
}

fn timeout_message(timeout: Duration, last: Option<&Value>) -> String {
    let dump = last
        .map(|state| state.to_string())
        .unwrap_or_else(|| "<no state read>".into());
    format!("oracle poll timed out after {timeout:?}; last fixture state: {dump}")
}
