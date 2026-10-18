//! Opt-in long-soak harness. Test clocks/targets never qualify as native evidence.
//!
//! Native adapters are deliberately absent. A matching one-shot desktop must
//! supply a wall clock, process-tree sampler, external task oracle, lifecycle
//! checks and real artifacts. There is no resident service or implicit long run.

mod checks;
mod model;
mod report;

pub use checks::{scan_outside_effects, scan_session_residue};
pub use model::*;
pub use report::{
    parse_report, validate_report, write_report, ArtifactSource, FileArtifacts, Validation,
};

use crate::{Error, Task};
use std::future::Future;

/// Monotonic clock, injectable for second-long deterministic tests.
///
/// `wait_until` must yield until the absolute deadline and cancel on drop.
/// Native implementations must use actual monotonic wall time. No method may
/// block an executor thread or move the clock backwards.
pub trait SoakClock {
    /// Clock provenance, recorded independently of other adapters.
    fn kind(&self) -> ClockKind;
    /// Absolute monotonic milliseconds; only differences enter reports.
    fn now_ms(&self) -> u64;
    /// Wait to an absolute deadline. Errors terminate collection.
    fn wait_until(&self, deadline_ms: u64) -> SoakFuture<'_, ()>;
}

/// Task execution against a fake or matching disposable desktop target.
///
/// Every task must use an independently self-tested oracle, with no user
/// credentials/homes. Complete/close its sessions before returning. Bound OS
/// work off the executor thread; dropping `run` must cancel without replaying
/// unknown input. A native adapter may never label a fake execution native.
pub trait SoakTarget {
    /// Actual target provenance.
    fn kind(&self) -> TargetKind;
    /// Execute this task once, within the harness deadline; no automatic retry.
    fn run<'a>(&'a mut self, task: &'a Task) -> SoakFuture<'a, Observation>;
}

/// Periodic resource sampler, including old/restarted process generations.
///
/// Sampling must be bounded and nonblocking (native OS reads belong on a
/// dedicated worker). Errors are retained as failures, never replaced by zero.
pub trait ResourceSource {
    /// Explicit native opt-in; test implementations default to fake.
    fn kind(&self) -> TargetKind {
        TargetKind::Fake
    }
    /// Sample engine/helpers/workers, OS input state and the input ledger.
    fn sample(&mut self) -> Result<Resources, Error>;
}

/// Injectable fault/lifecycle/security checks, independent of task self-report.
///
/// `SessionCleanup` must close remaining sessions before scanning all declared
/// data/image roots. `OutsideCuSideEffects` must compare file/process/network
/// observations with a baseline. Native callbacks must be bounded, cancel on
/// drop, and never clear Stop except through an actual trusted user decision.
pub trait CheckSource {
    /// Explicit native opt-in; test implementations default to fake.
    fn kind(&self) -> TargetKind {
        TargetKind::Fake
    }
    /// Inject/observe one scenario. Missing implementations return not_run.
    fn run(&mut self, kind: CheckKind) -> SoakFuture<'_, Observation>;
}

/// Construct a disabled/unavailable report without scheduling or waiting.
/// Pure construction; no IO, timeout or cancellation.
pub fn not_run_report(config: SoakConfig, reason: impl Into<String>) -> SoakReport {
    let reason = reason.into();
    let mut checks: Vec<_> = CheckKind::SCHEDULE
        .into_iter()
        .map(|kind| CheckRecord {
            kind,
            at_ms: 0,
            observation: Observation::not_run(reason.clone()),
        })
        .collect();
    for kind in [CheckKind::ScriptLimits, CheckKind::SlowFfi] {
        checks.push(CheckRecord {
            kind,
            at_ms: 0,
            observation: Observation::not_run("computer_script is not implemented (P6-05)"),
        });
    }
    SoakReport {
        schema_version: SCHEMA_VERSION,
        config,
        clock_kind: ClockKind::Unavailable,
        target_kind: TargetKind::Unavailable,
        resource_kind: TargetKind::Unavailable,
        check_kind: TargetKind::Unavailable,
        evidence: None,
        elapsed_ms: 0,
        task_ids: Vec::new(),
        samples: Vec::new(),
        tasks: Vec::new(),
        checks,
        errors: Vec::new(),
    }
}

/// Rotate core tasks and lifecycle scenarios for the explicit duration.
///
/// Async, bounded by configured hours plus a five-second final cleanup window.
/// Resource sampling continues while task/check futures are pending. Each tick
/// runs one task and one scenario; unknown/failure is recorded without replay.
/// Cancellation by dropping this future relies on adapter drop cleanup; native
/// adapters must independently preserve Stop and release inputs. Zero hours
/// invokes no adapter. Fake clocks/data always retain their provenance.
pub async fn run_soak<C: SoakClock, T: SoakTarget, R: ResourceSource, K: CheckSource>(
    tasks: &[Task],
    config: SoakConfig,
    clock: &C,
    target: &mut T,
    resources: &mut R,
    checks: &mut K,
    evidence: Option<PlatformEvidence>,
) -> Result<SoakReport, Error> {
    let duration = config.duration_ms()?;
    if duration == 0 {
        return Ok(not_run_report(
            config,
            "soak disabled: explicit positive --soak-hours required",
        ));
    }
    let platform = match config.platform {
        SoakPlatform::LinuxX11 => "linux",
        SoakPlatform::Macos => "macos",
        SoakPlatform::Windows => "windows",
    };
    let selected: Vec<_> = tasks
        .iter()
        .filter(|t| {
            t.suites.iter().any(|s| s == "core") && t.platforms.iter().any(|p| p == platform)
        })
        .collect();
    if selected.is_empty() {
        return Ok(not_run_report(config, "no matching core tasks"));
    }
    // The measurement horizon begins at the baseline observation. Taking a
    // second clock reading later must not invent a nonzero baseline timestamp.
    let baseline = resources.sample();
    let start = clock.now_ms();
    let deadline = start
        .checked_add(duration)
        .ok_or_else(|| Error::msg("soak deadline overflow"))?;
    let mut report = not_run_report(config, "scenario not sampled");
    report
        .checks
        .retain(|c| matches!(c.kind, CheckKind::ScriptLimits | CheckKind::SlowFfi));
    report.clock_kind = clock.kind();
    report.target_kind = target.kind();
    report.resource_kind = resources.kind();
    report.check_kind = checks.kind();
    report.evidence = evidence;
    report.task_ids = selected.iter().map(|t| t.id.clone()).collect();
    let baseline_error = match baseline {
        Ok(resources) => {
            report.samples.push(ResourceSample {
                sequence: 0,
                at_ms: 0,
                quiescent: true,
                resources,
            });
            None
        }
        Err(error) => Some(error),
    };
    let mut collection = Collection {
        clock,
        resources,
        start,
        deadline,
        last_now: start,
        next_sample: start
            .checked_add(report.config.cadence_ms())
            .ok_or_else(|| Error::msg("soak sample deadline overflow"))?,
        report: &mut report,
    };
    let result = match baseline_error {
        Some(error) => Err(error),
        None => collect(&selected, target, checks, &mut collection).await,
    };
    if let Err(error) = result {
        collection.report.errors.push(error.to_string());
    }
    // Close/scan even after an execution or sampling failure. Never resume Stop.
    let cleanup_start = collection.now()?;
    collection.deadline = cleanup_start
        .checked_add(5_000)
        .ok_or_else(|| Error::msg("soak cleanup deadline overflow"))?;
    for kind in [CheckKind::SessionCleanup, CheckKind::OutsideCuSideEffects] {
        let observation = match collection.drive(checks.run(kind)).await {
            Ok(value) => value,
            Err(error) => failed(error.to_string()),
        };
        let at_ms = collection.elapsed()?;
        collection.report.checks.push(CheckRecord {
            kind,
            at_ms,
            observation,
        });
    }
    match collection.sample(true) {
        Ok(()) => {
            // Use the actual final observation timestamp, not another reading
            // that can advance after collection has already finished.
            collection.report.elapsed_ms = collection
                .report
                .samples
                .last()
                .map_or(0, |sample| sample.at_ms);
        }
        Err(error) => {
            collection.report.errors.push(error.to_string());
            collection.report.elapsed_ms = collection.elapsed()?;
        }
    }
    Ok(report)
}

fn failed(reason: String) -> Observation {
    Observation {
        status: Status::Fail,
        reason,
        violations: Vec::new(),
        restart: None,
    }
}

async fn collect<C: SoakClock, T: SoakTarget, R: ResourceSource, K: CheckSource>(
    selected: &[&Task],
    target: &mut T,
    checks: &mut K,
    collection: &mut Collection<'_, C, R>,
) -> Result<(), Error> {
    let mut tick = 0usize;
    while collection.now()? < collection.deadline {
        let task = selected[tick % selected.len()];
        let started_ms = collection.elapsed()?;
        let result = collection.drive(target.run(task)).await;
        let finished_ms = collection.elapsed()?;
        let failed_run = result.is_err();
        collection.report.tasks.push(TaskRecord {
            sequence: collection.report.tasks.len() as u64,
            id: task.id.clone(),
            started_ms,
            finished_ms,
            observation: result.unwrap_or_else(|error| failed(error.to_string())),
        });
        if failed_run {
            return Err(Error::msg("soak task collection failed; no replay"));
        }
        if collection.now()? >= collection.deadline {
            break;
        }
        let kind = CheckKind::SCHEDULE[tick % CheckKind::SCHEDULE.len()];
        let result = collection.drive(checks.run(kind)).await;
        let at_ms = collection.elapsed()?;
        let failed_check = result.is_err();
        collection.report.checks.push(CheckRecord {
            kind,
            at_ms,
            observation: result.unwrap_or_else(|error| failed(error.to_string())),
        });
        if failed_check {
            return Err(Error::msg("soak scenario collection failed"));
        }
        let wake = collection.next_sample.min(collection.deadline);
        collection.clock.wait_until(wake).await?;
        if collection.now()? < wake {
            return Err(Error::msg("soak clock woke before its deadline"));
        }
        collection.sample(true)?;
        collection.advance_sample()?;
        tick = tick
            .checked_add(1)
            .ok_or_else(|| Error::msg("soak task count overflow"))?;
    }
    Ok(())
}

struct Collection<'a, C, R> {
    clock: &'a C,
    resources: &'a mut R,
    report: &'a mut SoakReport,
    start: u64,
    deadline: u64,
    last_now: u64,
    next_sample: u64,
}

impl<C: SoakClock, R: ResourceSource> Collection<'_, C, R> {
    fn now(&mut self) -> Result<u64, Error> {
        let now = self.clock.now_ms();
        if now < self.last_now {
            return Err(Error::msg("soak clock moved backwards"));
        }
        self.last_now = now;
        Ok(now)
    }

    fn elapsed(&mut self) -> Result<u64, Error> {
        Ok(self.now()? - self.start)
    }

    fn sample(&mut self, quiescent: bool) -> Result<(), Error> {
        let at_ms = self.elapsed()?;
        let resources = self.resources.sample()?;
        self.report.samples.push(ResourceSample {
            sequence: self.report.samples.len() as u64,
            at_ms,
            quiescent,
            resources,
        });
        Ok(())
    }

    fn advance_sample(&mut self) -> Result<(), Error> {
        self.next_sample = self
            .now()?
            .checked_add(self.report.config.cadence_ms())
            .ok_or_else(|| Error::msg("soak sample deadline overflow"))?;
        Ok(())
    }

    async fn drive<F: Future<Output = Result<Observation, Error>>>(
        &mut self,
        operation: F,
    ) -> Result<Observation, Error> {
        tokio::pin!(operation);
        loop {
            let wake = self.next_sample.min(self.deadline);
            let clock = self.clock;
            tokio::select! {
                biased;
                result = &mut operation => {
                    if self.now()? > self.deadline {
                        return Err(Error::msg("soak operation exceeded run deadline; no replay"));
                    }
                    return result;
                }
                result = clock.wait_until(wake) => {
                    result?;
                    if self.now()? < wake { return Err(Error::msg("soak clock woke before its deadline")); }
                    self.sample(false)?;
                    self.advance_sample()?;
                    if self.now()? >= self.deadline { return Err(Error::msg("soak operation exceeded run deadline; cancelled without replay")); }
                }
            }
        }
    }
}
