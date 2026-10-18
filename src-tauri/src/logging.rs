//! Process diagnostics logging: stderr + retained file under app data `logs/`.
//!
//! File logs enable post-mortem of mid-turn agent exits and host timeouts
//! when the GUI process is launched from Finder (no terminal).
//!
//! Panic records are category and location only. They are written synchronously
//! so `abort()` after a Rust panic still leaves a diagnostic trail. This module
//! does not read `GCU_LOG_PAYLOADS`; payload policy lives in `gcu-host-protocol`.

use std::path::PathBuf;
use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Kept alive for the process lifetime so the non-blocking writer drains.
static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// Default filter when `RUST_LOG` is unset.
pub const DEFAULT_ENV_FILTER: &str = "info";

/// Initialize dual-sink tracing (stderr + retained app log). Safe to call once.
pub fn init() {
    let _ = crate::paths::ensure_app_dirs();
    let log_dir = crate::paths::app_data_root().join("logs");

    install_panic_hook(log_dir.clone());

    let file = match crate::log_retention::CuLogFile::open(&log_dir) {
        Ok(file) => Some(file),
        Err(_) => {
            crate::log_retention::warn_once("log_open", &log_dir.display().to_string());
            None
        }
    };

    let started = if let Some(file) = file {
        let (non_blocking, guard) = tracing_appender::non_blocking(file);
        let _ = LOG_GUARD.set(guard);
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(DEFAULT_ENV_FILTER));
        tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer())
            .with(
                fmt::layer()
                    .with_writer(non_blocking)
                    .with_target(true)
                    .with_ansi(false),
            )
            .try_init()
            .is_ok()
    } else {
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(DEFAULT_ENV_FILTER));
        tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer())
            .try_init()
            .is_ok()
    };
    if !started {
        return;
    }

    tracing::info!(
        target: "grok_app::logging",
        path = %log_dir.display(),
        "diagnostic file logging enabled (7 days, 5x5MB)"
    );
}

fn stderr_layer<S>() -> impl tracing_subscriber::Layer<S>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(true)
        .with_ansi(true)
}

/// Absolute path of the logs directory (may not exist until first write).
pub fn logs_dir() -> PathBuf {
    crate::paths::app_data_root().join("logs")
}

/// Append one scrubbed line to today's app log and flush.
///
/// Survives a wedged `tracing-appender` worker. IO failure is reported once as
/// a metadata warning and is not silent.
pub fn sync_diag(message: &str) {
    let log_dir = logs_dir();
    let day = chrono::Local::now().format("%Y-%m-%d");
    let name = format!("app.log.{day}");
    let body = gcu_host_protocol::inject::scrub(message);
    let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    let line = format!("{ts}  WARN grok_app::logging: {body}\n");
    let limits = crate::log_retention::production_limits();
    if crate::log_retention::try_append(&log_dir, &name, &line, &limits).is_err() {
        crate::log_retention::warn_once("sync_diag", &log_dir.display().to_string());
    }
}

/// Category and source location for a panic. The panic message is not read.
fn panic_record(location: &str, payload: &(dyn std::any::Any + Send)) -> String {
    let category = if payload.is::<&str>() {
        "panic_str"
    } else if payload.is::<String>() {
        "panic_string"
    } else {
        "panic_other"
    };
    gcu_host_protocol::log_policy::error_record(location, category)
}

/// Install a panic hook that sync-writes category and location only.
///
/// The previous hook is not called: the default hook prints the panic message.
/// Non-blocking `tracing-appender` can lose the final lines on `abort()`.
fn install_panic_hook(log_dir: PathBuf) {
    std::panic::set_hook(Box::new(move |info| {
        let location = match info.location() {
            Some(loc) => format!("{}:{}:{}", loc.file(), loc.line(), loc.column()),
            None => "unknown".to_string(),
        };
        let line = format!("{}\n", panic_record(&location, info.payload()));
        let limits = crate::log_retention::production_limits();
        if crate::log_retention::try_append(&log_dir, "panic.log", &line, &limits).is_err() {
            crate::log_retention::warn_once("panic_log", &log_dir.display().to_string());
        }
        let day = chrono::Local::now().format("%Y-%m-%d");
        let daily = format!("app.log.{day}");
        if crate::log_retention::try_append(&log_dir, &daily, &line, &limits).is_err() {
            crate::log_retention::warn_once("panic_log", &log_dir.display().to_string());
        }
        eprint!("{line}");
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_dir_under_app_data() {
        let dir = logs_dir();
        assert!(dir.ends_with("logs"), "{dir:?}");
    }

    #[test]
    fn panic_record_omits_payload_and_keeps_location() {
        let secret = "SUPER-SECRET-PANIC-99";
        let location = "src/logging.rs:12:3";
        let record = panic_record(location, &secret);
        assert!(record.contains("category=panic_str"), "{record}");
        assert!(record.contains("location=src/logging.rs:12:3"), "{record}");
        assert!(!record.contains(secret), "{record}");
        let owned = secret.to_string();
        let record = panic_record(location, &owned);
        assert!(record.contains("category=panic_string"), "{record}");
        assert!(!record.contains(secret), "{record}");
        let other = 7u8;
        let record = panic_record(location, &other);
        assert!(record.contains("category=panic_other"), "{record}");
    }
}
