//! Probe/R2.1H-c: real Rust client against the real Node worker without Chromium.

use std::path::PathBuf;
use std::time::Duration;

use grok_computer_use_core::browser::{ManagedBrowserWorker, WorkerCompletion};
use uuid::Uuid;

use crate::computer_use::playwright_worker;

use super::process::{BrowserSupervisor, SpawnRequest};
use super::process_tree::process_alive;

/// Probe/R2.1H-c: exercise the real Rust client against the real Node worker
/// without opening Chromium. `/open` rejects an invalid profile before launch.
#[cfg(feature = "computer-use-probe")]
pub fn run_browser_error_contract_gate() -> Result<(), String> {
    use grok_computer_use_core::browser::{ManagedBrowserWorker, WorkerCompletion};

    let node = std::env::var_os("GROK_CU_NODE_FILE")
        .map(PathBuf::from)
        .ok_or("GROK_CU_NODE_FILE must name an absolute Node executable")?;
    if !node.is_absolute() || !node.is_file() {
        return Err(format!(
            "GROK_CU_NODE_FILE must name an absolute Node executable: {}",
            node.display()
        ));
    }
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tools")
        .join("computer-use-browser")
        .join("server.mjs");
    if !script.is_file() {
        return Err(format!("missing {}", script.display()));
    }
    let profile_root = std::env::temp_dir().join(format!(
        "grok-cu-browser-error-contract-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    let supervisor = BrowserSupervisor::spawn(SpawnRequest {
        node,
        script,
        profile_root: profile_root.clone(),
        browser: None,
    })?;
    let pid = supervisor.pid();
    let worker =
        playwright_worker::LoopbackPlaywrightWorker::new(supervisor.base_url(), supervisor.token());
    let result = match worker.open_profile("contract-owner", "../invalid-profile") {
        Ok(_) => Err("invalid profile unexpectedly opened".to_string()),
        Err(error)
            if error.status == 400
                && error.code == "invalid_request"
                && error.completion == WorkerCompletion::NotStarted =>
        {
            println!(
                "gate: browser_error_contract status={} code={} completion=not_started pid={pid}",
                error.status, error.code
            );
            Ok(())
        }
        Err(error) => Err(format!(
            "typed error mismatch: status={} code={} completion={:?}",
            error.status, error.code, error.completion
        )),
    };
    let shutdown = supervisor.shutdown();
    std::thread::sleep(Duration::from_millis(200));
    let still_alive = process_alive(pid);
    let cleanup = std::fs::remove_dir_all(&profile_root);

    result?;
    shutdown?;
    if still_alive {
        return Err(format!("worker pid {pid} still alive after shutdown"));
    }
    if let Err(error) = cleanup {
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(format!("remove probe profile root: {error}"));
        }
    }
    Ok(())
}
