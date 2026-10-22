//! Opt-in, content-free native soak instrumentation. No timers or files in
//! normal mode. Observation never changes import lock outcomes or timeouts.

mod lock_metrics;
mod sample;
mod writer;

#[cfg(unix)]
mod unix_io;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

pub(crate) use lock_metrics::ImportOsLockKind;
pub use sample::FrontendSample;

static HOME: OnceLock<Option<PathBuf>> = OnceLock::new();
static LOCKS: LazyLock<lock_metrics::LockMetrics> = LazyLock::new(Default::default);
static STATE: OnceLock<TelemetryState> = OnceLock::new();
static WRITING: AtomicBool = AtomicBool::new(false);

fn configured_home(
    enabled: Option<&str>,
    acp: Option<&str>,
    roots_disabled: Option<&str>,
    home: Option<&str>,
) -> Option<PathBuf> {
    if enabled != Some("1") || acp != Some("mock") || roots_disabled != Some("1") {
        return None;
    }
    home.map(PathBuf::from).filter(|path| path.is_absolute())
}

fn home() -> Option<&'static PathBuf> {
    HOME.get_or_init(|| {
        configured_home(
            std::env::var("GROK_APP_SOAK_TELEMETRY").ok().as_deref(),
            std::env::var("GROK_APP_ACP").ok().as_deref(),
            std::env::var("GROK_DISABLE_DEFAULT_ROOTS").ok().as_deref(),
            std::env::var("GROK_APP_HOME").ok().as_deref(),
        )
    })
    .as_ref()
}

#[tauri::command]
pub fn native_soak_telemetry_enabled() -> bool {
    home().is_some()
}

/// The closure is the existing single blocking fs2 acquisition, with its
/// original Result preserved. It excludes path setup, hold time and unlock.
pub(crate) fn observe_import_os_lock<T, E>(
    kind: ImportOsLockKind,
    acquire: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    if home().is_some() {
        LOCKS.observe(kind, acquire)
    } else {
        acquire()
    }
}

struct TelemetryState {
    started: Instant,
    writer: Mutex<Option<writer::TelemetryWriter>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    version: u32,
    process_id: u32,
    sampled_at_unix_ms: u64,
    native_elapsed_ms: u64,
    frontend: FrontendSample,
    import_os_locks: lock_metrics::ImportOsLocksSnapshot,
}

struct WritePermit;

impl Drop for WritePermit {
    fn drop(&mut self) {
        WRITING.store(false, Ordering::Release);
    }
}

#[tauri::command]
pub async fn native_soak_telemetry_sample(sample: FrontendSample) -> Result<(), String> {
    write_sample(home().cloned(), sample).await
}

async fn write_sample(enabled_home: Option<PathBuf>, sample: FrontendSample) -> Result<(), String> {
    let home = enabled_home.ok_or("telemetry_disabled")?;
    sample.validate()?;
    WRITING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "telemetry_write_busy".to_string())?;
    let permit = WritePermit;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let state = STATE.get_or_init(|| TelemetryState {
            started: Instant::now(),
            writer: Mutex::new(None),
        });
        let record = Record {
            version: 1,
            process_id: std::process::id(),
            sampled_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "telemetry_clock_before_epoch")?
                .as_millis()
                .min(u64::MAX as u128) as u64,
            native_elapsed_ms: state.started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            frontend: sample,
            import_os_locks: LOCKS.snapshot(),
        };
        let mut bytes = serde_json::to_vec(&record).map_err(|_| "telemetry_serialize_failed")?;
        bytes.push(b'\n');
        let mut writer = state
            .writer
            .lock()
            .map_err(|_| "telemetry_writer_poisoned")?;
        if writer.is_none() {
            *writer = Some(writer::TelemetryWriter::open(&home, std::process::id())?);
        }
        writer
            .as_mut()
            .ok_or("telemetry_writer_missing")?
            .append(&bytes)
    })
    .await
    .map_err(|_| "telemetry_write_task_failed".to_string())?
}
