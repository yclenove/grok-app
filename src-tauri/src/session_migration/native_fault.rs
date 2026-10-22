//! Explicit, debug-only native recovery audit barriers. See the module's cfg.
//! No files, discovery, IPC, self-termination, retries or persistence changes.
//! The external owner verifies its process tree before any termination.

use std::io::{self, Write};
use std::path::{Component, Path};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use serde::Serialize;
use uuid::Uuid;

const PAUSE: Duration = Duration::from_secs(120);
const PREFIX: &str = "GROK_IMPORT_NATIVE_AUDIT ";
static GATE: OnceLock<Option<Gate>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Point {
    Queued,
    Running,
    AfterRename,
    AfterPublishedReceipt,
    AfterCatalogCommit,
    BeforeCompletionEvent,
}

impl Point {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "after_rename" => Self::AfterRename,
            "after_published_receipt" => Self::AfterPublishedReceipt,
            "after_catalog_commit" => Self::AfterCatalogCommit,
            "before_completion_event" => Self::BeforeCompletionEvent,
            _ => return None,
        })
    }
}

struct Config {
    point: Point,
    run: Uuid,
}

impl Config {
    fn parse(
        acp: Option<&str>,
        roots_disabled: Option<&str>,
        home: Option<&Path>,
        point: Option<&str>,
        run: Option<&str>,
    ) -> Option<Self> {
        if acp != Some("mock") || roots_disabled != Some("1") {
            return None;
        }
        let run = Uuid::parse_str(run?).ok().filter(|run| !run.is_nil())?;
        let home = home?;
        if !home.is_absolute()
            || home
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
            || home.file_name()?.to_str()? != format!("grok-import-audit-{run}")
        {
            return None;
        }
        Some(Self {
            point: Point::parse(point?)?,
            run,
        })
    }

    fn from_environment() -> Option<Self> {
        let home = std::env::var_os("GROK_APP_HOME");
        Self::parse(
            std::env::var("GROK_APP_ACP").ok().as_deref(),
            std::env::var("GROK_DISABLE_DEFAULT_ROOTS").ok().as_deref(),
            home.as_deref().map(Path::new),
            std::env::var("GROK_IMPORT_NATIVE_AUDIT_POINT")
                .ok()
                .as_deref(),
            std::env::var("GROK_IMPORT_NATIVE_AUDIT_RUN")
                .ok()
                .as_deref(),
        )
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Marker {
    version: u8,
    process_id: u32,
    run: Uuid,
    point: Point,
    job_sha256: String,
    pause_seconds: u64,
    status: &'static str,
}

struct Gate {
    config: Config,
    fired: AtomicBool,
}

impl Gate {
    fn new(config: Config) -> Self {
        Self {
            config,
            fired: AtomicBool::new(false),
        }
    }

    fn reach(
        &self,
        point: Point,
        job: &str,
        mut emit: impl FnMut(&Marker) -> io::Result<()>,
        wait: impl FnOnce(Duration),
    ) {
        if point != self.config.point
            || self
                .fired
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return;
        }
        let mut marker = Marker {
            version: 1,
            process_id: std::process::id(),
            run: self.config.run,
            point,
            job_sha256: super::hash::sha256_bytes(job.as_bytes()),
            pause_seconds: PAUSE.as_secs(),
            status: "paused",
        };
        // If no observable marker can be written, do not introduce a blind stall.
        if emit(&marker).is_err() {
            return;
        }
        wait(PAUSE);
        marker.status = "expired";
        let _ = emit(&marker);
    }
}

fn emit(marker: &Marker) -> io::Result<()> {
    let json = serde_json::to_string(marker).map_err(io::Error::other)?;
    let mut output = io::stderr().lock();
    writeln!(output, "{PREFIX}{json}")?;
    output.flush()
}

pub(super) fn pause(point: Point, job: &str) {
    if let Some(gate) = GATE.get_or_init(|| Config::from_environment().map(Gate::new)) {
        // stderr is locked only during each marker write, never during the wait.
        gate.reach(point, job, emit, std::thread::sleep);
    }
}

#[cfg(test)]
#[path = "native_fault_tests.rs"]
mod tests;
