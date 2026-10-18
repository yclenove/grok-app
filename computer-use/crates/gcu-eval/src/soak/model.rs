//! Versioned raw observations. No caller-supplied passing summary is trusted.

use crate::Error;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

/// Soak report contract version, independent of the bench report contract.
pub const SCHEMA_VERSION: u32 = 1;
/// The fixed memory growth ceiling: 50 MB in decimal bytes.
pub const MAX_RSS_GROWTH_BYTES: u64 = 50_000_000;
/// Maximum permitted gap between resource samples: one minute.
pub const MAX_SAMPLE_INTERVAL_MS: u64 = 60_000;
/// Milliseconds per requested hour.
pub const HOUR_MS: u64 = 3_600_000;
/// Adapter future. Dropping it must cancel work and retain Stop/cleanup safety.
pub type SoakFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, Error>> + 'a>>;

/// Outcome of an observation or a recomputed gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Complete evidence satisfies this specific check.
    Pass,
    /// Observed violation or insufficient sampling/duration.
    Fail,
    /// Missing native evidence, disabled run or unavailable capability.
    NotRun,
}

impl Status {
    /// Stable CLI/report spelling. Pure, without IO/timeout/cancellation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::NotRun => "not_run",
        }
    }
}

/// Source of elapsed time; deterministic time is never native evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockKind {
    /// Matching platform's monotonic wall clock.
    NativeMonotonic,
    /// Injected logical test clock.
    Deterministic,
    /// No collection took place.
    Unavailable,
}

/// Provenance of an adapter, independently recorded for target/resources/checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    /// A matching one-shot desktop adapter.
    Native,
    /// Deterministic fixture/test implementation.
    Fake,
    /// Adapter not installed.
    Unavailable,
}

/// Matching desktop; Wayland requires its own future acceptance contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SoakPlatform {
    /// macOS desktop.
    Macos,
    /// Windows interactive desktop.
    Windows,
    /// Linux X11 desktop.
    LinuxX11,
}

impl SoakPlatform {
    /// Stable platform filename component. Pure, without IO/cancellation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::LinuxX11 => "linux-x11",
        }
    }
}

/// Explicit duration and sampling cadence; default duration is zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoakConfig {
    /// Only the core task suite is supported by this contract.
    pub suite: String,
    /// Requested matching platform.
    pub platform: SoakPlatform,
    /// Required hours; zero disables scheduling.
    pub hours: u64,
    /// Maximum sampling gap in milliseconds, at most one minute.
    pub sample_interval_ms: u64,
}

impl Default for SoakConfig {
    fn default() -> Self {
        Self::core(SoakPlatform::LinuxX11, 0)
    }
}

impl SoakConfig {
    /// Construct the core workload. Pure, without IO/timeout/cancellation.
    pub fn core(platform: SoakPlatform, hours: u64) -> Self {
        Self {
            suite: "core".into(),
            platform,
            hours,
            sample_interval_ms: MAX_SAMPLE_INTERVAL_MS,
        }
    }

    /// Validate fixed contract and duration arithmetic. Pure, no cancellation.
    pub fn duration_ms(&self) -> Result<u64, Error> {
        if self.suite != "core"
            || self.sample_interval_ms == 0
            || self.sample_interval_ms > MAX_SAMPLE_INTERVAL_MS
        {
            return Err(Error::msg(
                "soak requires suite core and sampling interval 1..=60000 ms",
            ));
        }
        self.hours
            .checked_mul(HOUR_MS)
            .ok_or_else(|| Error::msg("soak duration overflow"))
    }

    /// Schedule at half the maximum gap to leave wall-clock scheduling headroom.
    /// Pure, without IO/timeout/cancellation. Never relaxes the maximum gap.
    pub fn cadence_ms(&self) -> u64 {
        (self.sample_interval_ms / 2).max(1)
    }
}

/// Metadata linking raw native logs to the matching disposable desktop.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformEvidence {
    /// Actual desktop platform, must match the requested platform.
    pub platform: SoakPlatform,
    /// Nonpersonal machine/CPU/memory description.
    pub machine: String,
    /// OS and desktop versions.
    pub os_version: String,
    /// Engine build/commit identifier.
    pub engine_revision: String,
    /// Fixture build/commit identifier.
    pub fixture_revision: String,
    /// UTC collection timestamp.
    pub captured_at_utc: String,
    /// Attestation that no user homes/credentials were mounted.
    pub disposable_desktop: bool,
    /// Reference to real raw platform logs, resolved by the artifact source.
    pub artifact: PathBuf,
}

/// Engine plus helper/worker process-tree totals, sampled without payloads.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    /// Aggregate RSS in bytes, including retired-but-live workers.
    pub rss_bytes: u64,
    /// Open descriptors/OS handles, recorded without an invented budget.
    pub handles: u64,
    /// All live workers, including workers from previous engine generations.
    pub workers: u64,
    /// Cumulative worker starts across every generation; never reset on restart.
    pub workers_started: u64,
    /// Cumulative observed exits across every generation; never reset on restart.
    pub workers_exited: u64,
    /// Outstanding input ledger entries.
    pub input_ledger_entries: u64,
    /// OS-observed keys/buttons still held by CU.
    pub pressed_keys: u64,
}

/// One timestamped resource observation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceSample {
    /// Global sample sequence; missing/duplicated records fail validation.
    pub sequence: u64,
    /// Monotonic milliseconds since this run started.
    pub at_ms: u64,
    /// Task/check has completed and released inputs/session resources.
    pub quiescent: bool,
    /// Raw process-tree and input counters.
    pub resources: Resources,
}

/// Injectable lifecycle/security scenarios; script scenarios stay unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    /// Crash a worker/engine, observe unknown, restart without replay.
    CrashRestart,
    /// Revoke grants and verify delivery/token denial.
    Revocation,
    /// Exercise independent sessions and shared-window exclusion.
    MultiSession,
    /// Stop, restart, and verify the latch and absence of automatic delivery.
    StopRestart,
    /// End sessions and scan every declared data/temporary-image root.
    SessionCleanup,
    /// Scan files, processes and network effects outside CU.
    OutsideCuSideEffects,
    /// P6-05 script resource limits; not implemented.
    ScriptLimits,
    /// P6-05 slow native script callback; not implemented.
    SlowFfi,
}

impl CheckKind {
    /// Round-robin injection schedule, repeated throughout the soak.
    pub const SCHEDULE: [Self; 6] = [
        Self::CrashRestart,
        Self::Revocation,
        Self::MultiSession,
        Self::StopRestart,
        Self::SessionCleanup,
        Self::OutsideCuSideEffects,
    ];
    /// Report identity. Pure, without IO/cancellation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CrashRestart => "crash_restart",
            Self::Revocation => "revocation",
            Self::MultiSession => "multi_session",
            Self::StopRestart => "stop_restart",
            Self::SessionCleanup => "session_cleanup",
            Self::OutsideCuSideEffects => "outside_cu_side_effects",
            Self::ScriptLimits => "script_limits",
            Self::SlowFfi => "slow_ffi",
        }
    }
}

/// External observations required to count a successful restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestartObservation {
    /// Generation before the injected fault/Stop.
    pub generation_before: u64,
    /// Observed replacement generation.
    pub generation_after: u64,
    /// Retired workers still alive after restart.
    pub old_workers_remaining: u64,
    /// Stop latch observed in the replacement engine.
    pub stop_latched_after_restart: bool,
    /// Deliveries observed while Stop was latched; must be zero.
    pub actions_while_stopped: u64,
}

/// External oracle/check outcome; agent claims are not evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// Check outcome.
    pub status: Status,
    /// Safe diagnostic; mandatory for not_run.
    pub reason: String,
    /// Labels only, no file contents, secrets or user payloads.
    pub violations: Vec<String>,
    /// Required on successful crash/Stop restart checks.
    pub restart: Option<RestartObservation>,
}

impl Observation {
    /// Successful external observation. Pure, without IO/cancellation.
    pub fn pass() -> Self {
        Self {
            status: Status::Pass,
            reason: String::new(),
            violations: Vec::new(),
            restart: None,
        }
    }
    /// Missing capability; never a passing result. Pure, without IO/cancellation.
    pub fn not_run(reason: impl Into<String>) -> Self {
        Self {
            status: Status::NotRun,
            reason: reason.into(),
            violations: Vec::new(),
            restart: None,
        }
    }
}

/// Completed or cancelled scheduled task; records are never reset on restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRecord {
    /// Zero-based global invocation sequence.
    pub sequence: u64,
    /// Catalog task identifier.
    pub id: String,
    /// Invocation start, relative to run start.
    pub started_ms: u64,
    /// Completion/cancellation time.
    pub finished_ms: u64,
    /// External oracle outcome; unknown is never replayed automatically.
    pub observation: Observation,
}

/// A timestamped scenario observation, including unsupported scenarios.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRecord {
    /// Injected scenario.
    pub kind: CheckKind,
    /// Completion/cancellation time relative to run start.
    pub at_ms: u64,
    /// External observation.
    pub observation: Observation,
}

/// Raw versioned soak artifact. A validator derives all gates from these fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoakReport {
    /// Reader-supported contract version.
    pub schema_version: u32,
    /// Requested workload/duration/cadence.
    pub config: SoakConfig,
    /// Actual clock provenance.
    pub clock_kind: ClockKind,
    /// Actual target provenance.
    pub target_kind: TargetKind,
    /// Actual sampler provenance.
    pub resource_kind: TargetKind,
    /// Actual scenario adapter provenance.
    pub check_kind: TargetKind,
    /// Native platform artifact; absent means not_run.
    pub evidence: Option<PlatformEvidence>,
    /// Measured elapsed time, never assigned the requested duration.
    pub elapsed_ms: u64,
    /// Full task schedule in catalog order, repeated without replay on unknown.
    pub task_ids: Vec<String>,
    /// Raw samples including baseline and final quiescent sample.
    pub samples: Vec<ResourceSample>,
    /// Invocation history across every restart.
    pub tasks: Vec<TaskRecord>,
    /// Scenario history plus explicit unsupported items.
    pub checks: Vec<CheckRecord>,
    /// Collection failures and cancellation diagnostics.
    pub errors: Vec<String>,
}

/// Recomputed counters; never accepted from serialized report input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    /// Every scheduled task, including failures/cancellations.
    pub attempted_tasks: u64,
    /// Oracle-proven successful tasks.
    pub completed_tasks: u64,
    /// Successful observed crash restarts.
    pub crash_restarts: u64,
    /// Successful observed Stop restarts.
    pub stop_restarts: u64,
}

impl SoakReport {
    /// Derive counts from raw observations, no IO/timeout/cancellation.
    pub fn counts(&self) -> Counts {
        let successful = |o: &Observation| o.status == Status::Pass && o.violations.is_empty();
        let restarts = |kind| {
            self.checks
                .iter()
                .filter(|c| {
                    c.kind == kind
                        && successful(&c.observation)
                        && super::report::valid_restart(kind, c.observation.restart.as_ref())
                })
                .count() as u64
        };
        Counts {
            attempted_tasks: self.tasks.len() as u64,
            completed_tasks: self
                .tasks
                .iter()
                .filter(|t| successful(&t.observation))
                .count() as u64,
            crash_restarts: restarts(CheckKind::CrashRestart),
            stop_restarts: restarts(CheckKind::StopRestart),
        }
    }
}
