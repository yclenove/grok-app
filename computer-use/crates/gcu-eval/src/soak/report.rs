//! Strict report parsing and fail-closed validation.

use super::model::*;
use crate::Error;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

/// Resolves native raw artifact references; a report string alone is insufficient.
/// Methods may do bounded IO and must propagate failures, without synthesizing data.
pub trait ArtifactSource {
    /// True only for available raw evidence from the attested matching desktop.
    fn available(&self, evidence: &PlatformEvidence) -> Result<bool, Error>;
}

/// Read-only artifact lookup rooted at a trusted report bundle directory.
/// Symlinks, empty files, absolute paths and traversal are not accepted.
pub struct FileArtifacts {
    root: PathBuf,
}

impl FileArtifacts {
    /// Bind the bundle directory. Pure construction, no IO/cancellation.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl ArtifactSource for FileArtifacts {
    fn available(&self, evidence: &PlatformEvidence) -> Result<bool, Error> {
        if evidence.artifact.as_os_str().is_empty() {
            return Ok(false);
        }
        let mut full = self.root.clone();
        for component in evidence.artifact.components() {
            let Component::Normal(part) = component else {
                return Ok(false);
            };
            full.push(part);
            let metadata = match std::fs::symlink_metadata(&full) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(_) => return Err(Error::msg("could not inspect native soak artifact")),
            };
            if metadata.file_type().is_symlink() {
                return Ok(false);
            }
        }
        let metadata = std::fs::metadata(full)
            .map_err(|_| Error::msg("could not inspect native soak artifact"))?;
        Ok(metadata.is_file() && metadata.len() > 0)
    }
}

/// Recomputed gates. `resource_gate=pass` on fake input only proves harness logic.
/// Only `status=pass` could qualify a native soak; missing work is never green.
#[derive(Debug, Clone)]
pub struct Validation {
    /// Overall native qualification, including all scenario evidence.
    pub status: Status,
    /// Pure duration/sampling/resource invariant gate, regardless of provenance.
    pub resource_gate: Status,
    /// Observed violations, short durations, gaps and collection errors.
    pub issues: Vec<String>,
    /// Missing native evidence or unavailable scenarios, always explicit.
    pub not_run: Vec<String>,
    /// Recomputed invocation/restart counters.
    pub counts: Counts,
}

/// Parse strict versioned JSON. Unknown/duplicate fields and unsupported
/// versions/configurations are errors. Pure CPU work; no IO/cancellation.
pub fn parse_report(bytes: &[u8]) -> Result<SoakReport, Error> {
    let report: SoakReport = serde_json::from_slice(bytes)
        .map_err(|error| Error::msg(format!("soak report schema: {error}")))?;
    check_schema(&report)?;
    Ok(report)
}

fn check_schema(report: &SoakReport) -> Result<(), Error> {
    if report.schema_version != SCHEMA_VERSION {
        return Err(Error::msg(format!(
            "unsupported soak schema_version {}",
            report.schema_version
        )));
    }
    report.config.duration_ms()?;
    Ok(())
}

/// Validate raw observations against fixed gates and an independent required
/// duration. The required hours cannot be weakened by editing report config.
///
/// All CPU work is bounded by input; artifact lookup may do read-only IO.
/// No timeout/cancellation internally. Missing native artifacts or fake adapters
/// force overall not_run even when deterministic resource logic passes. Any
/// observed violation on a qualified native run fails; unavailable script
/// scenarios remain not_run and prevent a fully green L7 claim.
pub fn validate_report<A: ArtifactSource>(
    report: &SoakReport,
    required_hours: u64,
    artifacts: &A,
) -> Result<Validation, Error> {
    check_schema(report)?;
    let required_ms = required_hours
        .checked_mul(HOUR_MS)
        .ok_or_else(|| Error::msg("required soak duration overflow"))?
        .max(report.config.duration_ms()?);
    let mut not_run = Vec::new();
    let native = native_evidence(report, artifacts, &mut not_run)?;
    let mut issues = Vec::new();
    let resource_gate = if required_ms == 0 {
        not_run.push("soak disabled: no positive duration requested".into());
        Status::NotRun
    } else {
        resource_issues(report, required_ms, &mut issues);
        if issues.is_empty() {
            Status::Pass
        } else {
            Status::Fail
        }
    };
    if required_ms > 0 {
        workload_issues(report, &mut issues, &mut not_run);
    }
    let status = if !native || required_ms == 0 {
        Status::NotRun
    } else if !issues.is_empty() {
        Status::Fail
    } else if !not_run.is_empty() {
        Status::NotRun
    } else {
        Status::Pass
    };
    Ok(Validation {
        status,
        resource_gate,
        issues,
        not_run,
        counts: report.counts(),
    })
}

fn native_evidence<A: ArtifactSource>(
    report: &SoakReport,
    artifacts: &A,
    not_run: &mut Vec<String>,
) -> Result<bool, Error> {
    let mut valid = true;
    if report.clock_kind != ClockKind::NativeMonotonic {
        not_run.push(
            "native monotonic wall clock not_run (fake/unavailable time is not evidence)".into(),
        );
        valid = false;
    }
    for (name, kind) in [
        ("target", report.target_kind),
        ("resource sampler", report.resource_kind),
        ("scenario checks", report.check_kind),
    ] {
        if kind != TargetKind::Native {
            not_run.push(format!("native {name} not_run"));
            valid = false;
        }
    }
    let Some(evidence) = &report.evidence else {
        not_run.push(format!(
            "{} platform artifact not_run",
            report.config.platform.as_str()
        ));
        return Ok(false);
    };
    if evidence.platform != report.config.platform || !evidence.disposable_desktop {
        not_run.push("matching disposable desktop artifact not_run".into());
        valid = false;
    }
    for (name, value) in [
        ("machine", &evidence.machine),
        ("os_version", &evidence.os_version),
        ("engine_revision", &evidence.engine_revision),
        ("fixture_revision", &evidence.fixture_revision),
        ("captured_at_utc", &evidence.captured_at_utc),
    ] {
        if value.trim().is_empty() {
            not_run.push(format!("platform artifact metadata {name} not_run"));
            valid = false;
        }
    }
    if !artifacts.available(evidence)? {
        not_run.push("native raw platform artifact missing: not_run".into());
        valid = false;
    }
    Ok(valid)
}

fn resource_issues(report: &SoakReport, required_ms: u64, issues: &mut Vec<String>) {
    if report.elapsed_ms < required_ms {
        issues.push(format!(
            "duration insufficient: {} ms < {required_ms} ms",
            report.elapsed_ms
        ));
    }
    for error in &report.errors {
        issues.push(format!("collection error: {error}"));
    }
    let Some(first) = report.samples.first() else {
        issues.push("resource samples missing".into());
        return;
    };
    let interval = report.config.sample_interval_ms;
    let minimum = required_ms
        .div_ceil(report.config.cadence_ms())
        .saturating_add(1);
    // Same-time active -> quiescent transitions are useful observations, but
    // never count as another interval of duration coverage.
    let distinct = report
        .samples
        .iter()
        .map(|s| s.at_ms)
        .collect::<BTreeSet<_>>()
        .len() as u64;
    if distinct < minimum {
        issues.push(format!(
            "sampling insufficient: {distinct} distinct timestamps < {minimum}"
        ));
    }
    if first.at_ms != 0 || !first.quiescent {
        issues.push("missing quiescent baseline sample at 0 ms".into());
    }
    if !report
        .samples
        .last()
        .is_some_and(|last| last.at_ms == report.elapsed_ms && last.quiescent)
    {
        issues.push("missing final quiescent resource sample".into());
    }
    let baseline = &first.resources;
    if baseline.rss_bytes == 0 {
        issues.push("RSS baseline must be a measured positive byte count".into());
    }
    for (i, sample) in report.samples.iter().enumerate() {
        let resources = &sample.resources;
        if sample.sequence != i as u64 {
            issues.push("resource sample sequence has missing/duplicate records".into());
        }
        if sample.at_ms > report.elapsed_ms {
            issues.push("resource timestamp exceeds elapsed duration".into());
        }
        if let Some(previous) = i.checked_sub(1).and_then(|i| report.samples.get(i)) {
            let final_cleanup_resample = i == report.samples.len() - 1
                && sample.at_ms == report.elapsed_ms
                && sample.quiescent;
            if sample.at_ms < previous.at_ms
                || (sample.at_ms == previous.at_ms
                    && (previous.quiescent || !sample.quiescent)
                    && !final_cleanup_resample)
            {
                issues.push("resource timestamps not strictly ordered (except active to quiescent transition)".into());
            }
            if sample.at_ms.saturating_sub(previous.at_ms) > interval {
                issues.push("resource sampling gap exceeds configured interval".into());
            }
            if resources.workers_started < previous.resources.workers_started
                || resources.workers_exited < previous.resources.workers_exited
            {
                issues.push("worker lifecycle counters reset across restart".into());
            }
        }
        if resources.rss_bytes == 0 {
            issues.push("RSS sample missing (zero)".into());
        }
        if resources.rss_bytes.saturating_sub(baseline.rss_bytes) > MAX_RSS_GROWTH_BYTES {
            issues.push(format!(
                "RSS growth exceeds {MAX_RSS_GROWTH_BYTES} bytes at {} ms",
                sample.at_ms
            ));
        }
        if resources
            .workers_started
            .checked_sub(resources.workers_exited)
            != Some(resources.workers)
        {
            issues.push("worker lifecycle counters do not match live workers".into());
        }
        if sample.quiescent {
            if resources.workers > baseline.workers {
                issues.push("worker leak at quiescent boundary".into());
            }
            if resources.input_ledger_entries != 0 || resources.pressed_keys != 0 {
                issues
                    .push("stuck key/button or input ledger residue at quiescent boundary".into());
            }
        }
    }
}

fn workload_issues(report: &SoakReport, issues: &mut Vec<String>, not_run: &mut Vec<String>) {
    let ids: BTreeSet<_> = report.task_ids.iter().collect();
    if ids.is_empty()
        || ids.len() != report.task_ids.len()
        || ids.iter().any(|id| id.trim().is_empty())
    {
        issues.push("task schedule missing/duplicate/empty task id".into());
    }
    let mut observed = BTreeSet::new();
    for (i, task) in report.tasks.iter().enumerate() {
        observed.insert(&task.id);
        if task.sequence != i as u64
            || report.task_ids.get(i % report.task_ids.len().max(1)) != Some(&task.id)
        {
            issues.push("task invocation count/order differs from round-robin schedule".into());
        }
        if task.started_ms > task.finished_ms
            || task.finished_ms > report.elapsed_ms
            || (i > 0 && task.started_ms < report.tasks[i - 1].finished_ms)
        {
            issues.push("task invocation timeline invalid".into());
        }
        observation_issues(&task.id, &task.observation, issues, not_run);
    }
    if report.tasks.is_empty() || ids != observed {
        issues.push("task schedule lacks a complete cycle".into());
    }
    let injected: Vec<_> = report
        .checks
        .iter()
        .filter(|c| !matches!(c.kind, CheckKind::ScriptLimits | CheckKind::SlowFfi))
        .collect();
    if injected.len() != report.tasks.len().saturating_add(2) {
        issues.push("scenario count differs from one per task plus two final scans".into());
    }
    for (i, task) in report.tasks.iter().enumerate() {
        let expected = CheckKind::SCHEDULE[i % CheckKind::SCHEDULE.len()];
        if !injected.get(i).is_some_and(|c| {
            c.kind == expected
                && c.at_ms >= task.finished_ms
                && report
                    .tasks
                    .get(i + 1)
                    .is_none_or(|next| c.at_ms <= next.started_ms)
        }) {
            issues.push("scenario schedule/timeline differs from task rotation".into());
        }
    }
    for (offset, kind) in [CheckKind::SessionCleanup, CheckKind::OutsideCuSideEffects]
        .into_iter()
        .enumerate()
    {
        if !injected
            .get(report.tasks.len() + offset)
            .is_some_and(|c| c.kind == kind)
        {
            issues.push(format!(
                "{} final scan missing from schedule",
                kind.as_str()
            ));
        }
    }
    for check in &report.checks {
        if check.at_ms > report.elapsed_ms {
            issues.push("scenario timestamp exceeds elapsed duration".into());
        }
        let label = check.kind.as_str();
        observation_issues(label, &check.observation, issues, not_run);
        if matches!(check.kind, CheckKind::ScriptLimits | CheckKind::SlowFfi) {
            if check.observation.status != Status::NotRun {
                issues.push(format!(
                    "{label}: computer_script unavailable; must remain not_run"
                ));
            }
        } else if matches!(check.kind, CheckKind::CrashRestart | CheckKind::StopRestart)
            && check.observation.status == Status::Pass
            && !valid_restart(check.kind, check.observation.restart.as_ref())
        {
            issues.push(format!("{label}: invalid restart observation"));
        }
    }
    for kind in CheckKind::SCHEDULE
        .into_iter()
        .chain([CheckKind::ScriptLimits, CheckKind::SlowFfi])
    {
        if !report.checks.iter().any(|c| c.kind == kind) {
            not_run.push(format!(
                "{} not_run: missing scenario observation",
                kind.as_str()
            ));
        }
    }
    // A later clean scan cannot erase a leak/side effect observed earlier.
    for kind in [CheckKind::SessionCleanup, CheckKind::OutsideCuSideEffects] {
        if !report
            .checks
            .iter()
            .any(|c| c.kind == kind && c.at_ms >= report.tasks.last().map_or(0, |t| t.finished_ms))
        {
            issues.push(format!("{} missing after final task", kind.as_str()));
        }
    }
}

fn observation_issues(
    label: &str,
    observation: &Observation,
    issues: &mut Vec<String>,
    not_run: &mut Vec<String>,
) {
    if observation.status == Status::Fail {
        issues.push(format!("{label}: check failed: {}", observation.reason));
    }
    for violation in &observation.violations {
        issues.push(format!("{label}: {violation}"));
    }
    if observation.status == Status::NotRun {
        not_run.push(format!("{label} not_run: {}", observation.reason));
        if observation.reason.trim().is_empty() {
            issues.push(format!("{label}: not_run reason missing"));
        }
    }
}

pub(super) fn valid_restart(kind: CheckKind, restart: Option<&RestartObservation>) -> bool {
    restart.is_some_and(|r| {
        r.generation_before.checked_add(1) == Some(r.generation_after)
            && r.old_workers_remaining == 0
            && r.actions_while_stopped == 0
            && (kind != CheckKind::StopRestart || r.stop_latched_after_restart)
    })
}

/// Write raw versioned JSON only, never a fabricated native measurement or a
/// trusted status summary. Blocking filesystem IO, no timeout/cancellation.
/// Caller selects an output directory; no report is written by default.
pub fn write_report(dir: &Path, report: &SoakReport) -> Result<PathBuf, Error> {
    check_schema(report)?;
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|error| Error::msg(format!("soak report json: {error}")))?;
    std::fs::create_dir_all(dir)
        .map_err(|_| Error::msg("could not create soak report directory"))?;
    let path = dir.join(format!("soak-{}.json", report.config.platform.as_str()));
    std::fs::write(&path, bytes).map_err(|_| Error::msg("could not write soak report"))?;
    Ok(path)
}
