//! Spawn, attach, and stop the Host-owned Playwright worker.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use grok_computer_use_core::browser::{csprng_bearer_token, keep_worker_env_key};
use grok_computer_use_core::runtime::JS_RUNTIME;

use crate::computer_use::playwright_worker;
use crate::computer_use::ComputerUseBroker;

#[cfg(windows)]
use super::process_tree::{assign_job, terminate_owned_job};

#[cfg(all(test, windows))]
#[path = "process_tests.rs"]
mod tests;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn apply_worker_env(command: &mut Command) {
    command.env_clear();
    for (key, value) in std::env::vars_os() {
        if keep_worker_env_key(&key.to_string_lossy()) {
            command.env(key, value);
        }
    }
}

pub struct BrowserSupervisor {
    child: Mutex<Child>,
    // Hold through physical teardown; simultaneous callers must not report
    // success merely because another caller has started shutting down.
    closed: Mutex<bool>,
    // A failed teardown still fences this worker. Retaining ownership is not
    // permission to reuse an instance that may have partially shut down.
    shutdown_requested: AtomicBool,
    #[cfg(any(all(test, windows), feature = "computer-use-probe"))]
    pid: u32,
    base: String,
    token: String,
    profile_root: PathBuf,
    #[cfg(windows)]
    job: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
unsafe impl Send for BrowserSupervisor {}
#[cfg(windows)]
unsafe impl Sync for BrowserSupervisor {}

pub struct SpawnRequest {
    pub node: PathBuf,
    pub script: PathBuf,
    pub profile_root: PathBuf,
    pub browser: Option<PathBuf>,
}

impl BrowserSupervisor {
    #[cfg(feature = "computer-use-probe")]
    pub fn base_url(&self) -> &str {
        &self.base
    }

    #[cfg(feature = "computer-use-probe")]
    pub fn pid(&self) -> u32 {
        self.pid
    }

    #[cfg(feature = "computer-use-probe")]
    pub fn token(&self) -> &str {
        &self.token
    }

    #[cfg(feature = "computer-use-probe")]
    pub fn profile_root(&self) -> &Path {
        &self.profile_root
    }

    pub fn spawn(req: SpawnRequest) -> Result<Self, String> {
        if !req.node.is_file() {
            return Err(format!("js-runtime missing: {}", req.node.display()));
        }
        if !req.script.is_file() {
            return Err(format!(
                "browser worker script missing: {}",
                req.script.display()
            ));
        }
        if looks_like_path_lookup(&req.node) {
            return Err("managed browser refuses PATH node".into());
        }
        std::fs::create_dir_all(&req.profile_root).map_err(|e| e.to_string())?;
        let staging_root = req.profile_root.join(".staging");
        std::fs::create_dir_all(&staging_root).map_err(|e| e.to_string())?;
        let token = csprng_bearer_token();
        let mut command = Command::new(&req.node);
        apply_worker_env(&mut command);
        command
            .arg(&req.script)
            .env("GROK_CU_BROWSER_PORT", "0")
            .env("GROK_CU_BROWSER_TOKEN", &token)
            .env("GROK_CU_BROWSER_PROFILE_ROOT", &req.profile_root)
            .env("GROK_CU_BROWSER_STAGING_ROOT", &staging_root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(browser) = &req.browser {
            command.env("GROK_CU_CHROME", browser);
        } else {
            command.env("GROK_CU_ALLOW_SYSTEM_CHROME", "1");
        }
        if let Ok(manifest) = crate::computer_use::runtime::active_manifest() {
            if !manifest.compatibility.worker_source_sha256.is_empty() {
                command.env(
                    "GROK_CU_WORKER_SOURCE_SHA256",
                    &manifest.compatibility.worker_source_sha256,
                );
            }
            if !manifest.compatibility.playwright_core_version.is_empty() {
                command.env(
                    "GROK_CU_PLAYWRIGHT_VERSION",
                    &manifest.compatibility.playwright_core_version,
                );
            }
            if let Some(chrome) = manifest
                .components
                .get(crate::computer_use::runtime::CHROMIUM)
            {
                command.env("GROK_CU_BROWSER_VERSION", &chrome.version);
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().map_err(|e| format!("spawn worker: {e}"))?;
        #[cfg(any(all(test, windows), feature = "computer-use-probe"))]
        let pid = child.id();
        #[cfg(windows)]
        let job = assign_job(&child).inspect_err(|_| {
            let _ = child.kill();
            let _ = child.wait();
        })?;
        let stdout = child.stdout.take().ok_or("worker stdout missing")?;
        let port = read_port(stdout, Duration::from_secs(8)).inspect_err(|_| {
            let _ = child.kill();
            let _ = child.wait();
            #[cfg(windows)]
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(job);
            }
        })?;
        // The worker must never be able to block on a full stderr pipe. The
        // supervisor currently exposes only structured health errors, so
        // drain diagnostic text without retaining unbounded process output.
        if let Some(stderr) = child.stderr.take() {
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut buf = [0_u8; 4096];
                loop {
                    match std::io::Read::read(&mut reader, &mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                }
            });
        }
        Ok(Self {
            child: Mutex::new(child),
            closed: Mutex::new(false),
            shutdown_requested: AtomicBool::new(false),
            #[cfg(any(all(test, windows), feature = "computer-use-probe"))]
            pid,
            base: format!("http://127.0.0.1:{port}"),
            token,
            profile_root: req.profile_root,
            #[cfg(windows)]
            job,
        })
    }

    pub fn attach(&self, broker: &ComputerUseBroker) -> Result<(), String> {
        playwright_worker::bind_loopback(broker, &self.base, &self.token, &self.profile_root)
    }

    #[cfg(feature = "computer-use-probe")]
    pub fn health(&self) -> Result<serde_json::Value, String> {
        let response = reqwest::blocking::Client::new()
            .post(format!("{}/health", self.base))
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&serde_json::json!({}))
            .send()
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("health {}", response.status()));
        }
        response.json().map_err(|e| e.to_string())
    }

    #[cfg(feature = "computer-use-probe")]
    pub fn browser_pids(&self) -> Result<Vec<u32>, String> {
        let response = reqwest::blocking::Client::new()
            .post(format!("{}/pids", self.base))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("x-grok-cu-host", "1")
            .timeout(Duration::from_secs(3))
            .json(&serde_json::json!({}))
            .send()
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("pids {}", response.status()));
        }
        let body: serde_json::Value = response.json().map_err(|e| e.to_string())?;
        let mut pids = Vec::new();
        if body.get("workerPid").and_then(|v| v.as_u64()) != Some(u64::from(self.pid)) {
            return Err("worker PID diagnostic identity mismatch".into());
        }
        pids.push(self.pid);
        if let Some(rows) = body.get("browserPids").and_then(|v| v.as_array()) {
            for row in rows {
                if let Some(pid) = row
                    .as_u64()
                    .and_then(|pid| u32::try_from(pid).ok())
                    .filter(|pid| *pid > 0)
                {
                    pids.push(pid);
                }
            }
        }
        pids.sort_unstable();
        pids.dedup();
        Ok(pids)
    }

    pub fn shutdown(&self) -> Result<(), String> {
        self.shutdown_requested.store(true, Ordering::Release);
        let mut closed = self
            .closed
            .lock()
            .map_err(|_| "worker shutdown lock poisoned")?;
        if *closed {
            return Ok(());
        }
        let graceful = reqwest::blocking::Client::new()
            .post(format!("{}/shutdown", self.base))
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&serde_json::json!({}))
            .timeout(Duration::from_secs(2))
            .send();
        #[cfg(windows)]
        {
            // The kernel Job remains the ownership authority even after its
            // worker exits. Never reopen a process using diagnostic PID lists.
            terminate_owned_job(self.job, Duration::from_secs(5))?;
            let _ = graceful;
        }
        #[cfg(not(windows))]
        {
            let mut child = self
                .child
                .lock()
                .map_err(|_| "worker child lock poisoned")?;
            if child.try_wait().map_err(|e| e.to_string())?.is_none() {
                child
                    .kill()
                    .map_err(|e| format!("stop owned worker: {e}"))?;
            }
        }
        let mut child = self
            .child
            .lock()
            .map_err(|_| "worker child lock poisoned")?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if child
                .try_wait()
                .map_err(|e| format!("reap owned worker: {e}"))?
                .is_some()
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err("owned worker exit remains unconfirmed".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        #[cfg(not(windows))]
        if !graceful.is_ok_and(|response| response.status().is_success()) {
            // Killing a Unix worker alone does not prove its browsers exited.
            // A native process-lifetime authority is still required here.
            return Err("worker exited without confirmed browser cleanup".into());
        }
        *closed = true;
        Ok(())
    }

    #[cfg(any(all(test, windows), feature = "computer-use-probe"))]
    pub fn shutdown_with(&self, _diagnostic_pids: &[u32]) -> Result<(), String> {
        // Kept for probe callers; numeric observations never grant kill authority.
        self.shutdown()
    }
}

impl Drop for BrowserSupervisor {
    fn drop(&mut self) {
        let _ = self.shutdown();
        #[cfg(windows)]
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.job);
        }
    }
}

fn looks_like_path_lookup(node: &Path) -> bool {
    node.components().count() == 1
}

fn read_port<R: std::io::Read + Send + 'static>(
    stdout: R,
    timeout: Duration,
) -> Result<u16, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut lines = BufReader::new(stdout).lines();
        let line = lines.next().and_then(Result::ok);
        let _ = tx.send(line);
    });
    let line = rx
        .recv_timeout(timeout)
        .map_err(|_| "worker did not publish port".to_string())?
        .ok_or_else(|| "worker stdout closed before port".to_string())?;
    let v: serde_json::Value =
        serde_json::from_str(&line).map_err(|e| format!("worker port json: {e}"))?;
    v.get("port")
        .and_then(|p| p.as_u64())
        .and_then(|p| u16::try_from(p).ok())
        .ok_or_else(|| format!("worker port missing in {line}"))
}

pub fn product_spawn_request() -> Result<SpawnRequest, String> {
    let paths = crate::computer_use::runtime::product_paths()?;
    Ok(SpawnRequest {
        node: paths.node,
        script: paths.worker,
        profile_root: crate::paths::app_data_root()
            .join("computer-use")
            .join("browser-profiles"),
        browser: Some(paths.browser),
    })
}

pub fn attach_product(broker: &ComputerUseBroker) -> Result<(), String> {
    ensure_slot(
        product_supervisor_slot(),
        || BrowserSupervisor::spawn(product_spawn_request()?),
        |supervisor| supervisor.attach(broker),
    )
}

pub fn ensure_product_attached(broker: &ComputerUseBroker) -> Result<(), String> {
    attach_product(broker)
}

fn ensure_slot(
    slot: &Mutex<Option<BrowserSupervisor>>,
    spawn: impl FnOnce() -> Result<BrowserSupervisor, String>,
    attach: impl FnOnce(&BrowserSupervisor) -> Result<(), String>,
) -> Result<(), String> {
    // Serialize publication with startup and shutdown. A second initializer
    // cannot bind a worker and then have another caller silently replace it.
    let mut slot = slot.lock().map_err(|_| "worker slot lock poisoned")?;
    if let Some(supervisor) = slot.as_ref() {
        return if supervisor.shutdown_requested.load(Ordering::Acquire) {
            Err("managed browser cleanup is pending; worker cannot be reused".into())
        } else {
            Ok(())
        };
    }
    *slot = Some(spawn()?);
    let supervisor = slot.as_ref().expect("worker was just stored");
    if let Err(error) = attach(supervisor) {
        // Keep the exact child/Job if failed binding cannot be cleaned up.
        // An unconfirmed destructor must not discard the last recovery handle.
        if let Err(cleanup) = supervisor.shutdown() {
            return Err(format!(
                "{error}; owned worker cleanup also failed: {cleanup}"
            ));
        }
        slot.take();
        return Err(error);
    }
    Ok(())
}

static PRODUCT_SUPERVISOR: OnceLock<Mutex<Option<BrowserSupervisor>>> = OnceLock::new();

fn product_supervisor_slot() -> &'static Mutex<Option<BrowserSupervisor>> {
    PRODUCT_SUPERVISOR.get_or_init(|| Mutex::new(None))
}

/// Explicitly tear down the App-owned browser before the Tauri process exits.
/// Release the static slot only after physical teardown succeeds. Failed cleanup
/// retains the original lifetime authority for an explicit later retry.
pub fn shutdown_product() -> Result<(), String> {
    shutdown_slot(product_supervisor_slot())
}

fn shutdown_slot(slot: &Mutex<Option<BrowserSupervisor>>) -> Result<(), String> {
    let mut slot = slot.lock().map_err(|_| "worker slot lock poisoned")?;
    if let Some(supervisor) = slot.as_ref() {
        supervisor.shutdown()?;
    }
    slot.take();
    Ok(())
}

#[cfg(feature = "computer-use-probe")]
pub(crate) fn test_node_exe() -> Option<PathBuf> {
    if let Ok(node) = crate::computer_use::runtime::resolve(JS_RUNTIME) {
        if node.is_file() {
            return Some(node);
        }
    }
    // Probe-only absolute files. Product attach never searches PATH.
    for candidate in [
        r"C:\Program Files\nodejs\node.exe",
        r"C:\Program Files (x86)\nodejs\node.exe",
    ] {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}
