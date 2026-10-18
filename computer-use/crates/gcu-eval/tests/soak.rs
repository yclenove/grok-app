//! Deterministic harness/validator tests, never native soak evidence.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_eval::soak::*;
use gcu_eval::{block_on, bundled_tasks_dir, load_tasks, Error, Task};
use serde_json::json;
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Default)]
struct FakeClock(Cell<u64>);

impl SoakClock for FakeClock {
    fn kind(&self) -> ClockKind {
        ClockKind::Deterministic
    }
    fn now_ms(&self) -> u64 {
        self.0.get()
    }
    fn wait_until(&self, deadline_ms: u64) -> SoakFuture<'_, ()> {
        Box::pin(async move {
            self.0.set(deadline_ms.max(self.0.get()));
            Ok(())
        })
    }
}

struct FakeTarget(Rc<Cell<u64>>);

impl SoakTarget for FakeTarget {
    fn kind(&self) -> TargetKind {
        TargetKind::Fake
    }
    fn run<'a>(&'a mut self, _task: &'a Task) -> SoakFuture<'a, Observation> {
        Box::pin(async move {
            self.0.set(self.0.get() + 1);
            Ok(Observation::pass())
        })
    }
}

#[derive(Default)]
struct FakeResources {
    calls: u64,
}

impl ResourceSource for FakeResources {
    fn sample(&mut self) -> Result<Resources, Error> {
        self.calls += 1;
        Ok(Resources {
            rss_bytes: 100_000_000,
            handles: 10,
            workers: 0,
            workers_started: self.calls,
            workers_exited: self.calls,
            input_ledger_entries: 0,
            pressed_keys: 0,
        })
    }
}

struct FakeChecks {
    tasks: Rc<Cell<u64>>,
    root: Option<PathBuf>,
    seen: Vec<(CheckKind, u64)>,
}

impl CheckSource for FakeChecks {
    fn run(&mut self, kind: CheckKind) -> SoakFuture<'_, Observation> {
        Box::pin(async move {
            self.seen.push((kind, self.tasks.get()));
            if kind == CheckKind::SessionCleanup {
                if let Some(root) = &self.root {
                    return scan_session_residue(root);
                }
            }
            let mut result = Observation::pass();
            if matches!(kind, CheckKind::CrashRestart | CheckKind::StopRestart) {
                result.restart = Some(RestartObservation {
                    generation_before: self.seen.len() as u64,
                    generation_after: self.seen.len() as u64 + 1,
                    old_workers_remaining: 0,
                    stop_latched_after_restart: kind == CheckKind::StopRestart,
                    actions_while_stopped: 0,
                });
            }
            Ok(result)
        })
    }
}

fn tasks() -> Vec<Task> {
    let mut tasks = load_tasks(&bundled_tasks_dir().unwrap()).unwrap();
    tasks.retain(|task| task.suites.iter().any(|suite| suite == "core"));
    tasks.truncate(3);
    assert_eq!(tasks.len(), 3);
    tasks
}

fn run(root: Option<PathBuf>) -> (SoakReport, Vec<(CheckKind, u64)>) {
    let counter = Rc::new(Cell::new(0));
    let mut checks = FakeChecks {
        tasks: counter.clone(),
        root,
        seen: Vec::new(),
    };
    let report = block_on(run_soak(
        &tasks(),
        SoakConfig::core(SoakPlatform::LinuxX11, 1),
        &FakeClock::default(),
        &mut FakeTarget(counter.clone()),
        &mut FakeResources::default(),
        &mut checks,
        None,
    ))
    .unwrap()
    .unwrap();
    assert_eq!(report.counts().completed_tasks, counter.get());
    (report, checks.seen)
}

struct NoArtifacts;
impl ArtifactSource for NoArtifacts {
    fn available(&self, _evidence: &PlatformEvidence) -> Result<bool, Error> {
        Ok(false)
    }
}

fn resources(report: &SoakReport) -> Validation {
    validate_report(report, 1, &NoArtifacts).unwrap()
}

#[test]
fn memory_growth_at_threshold_passes_and_one_byte_over_fails() {
    let (mut report, _) = run(None);
    report.samples.last_mut().unwrap().resources.rss_bytes += MAX_RSS_GROWTH_BYTES;
    assert_eq!(resources(&report).resource_gate, Status::Pass);
    report.samples.last_mut().unwrap().resources.rss_bytes += 1;
    let checked = resources(&report);
    assert_eq!(checked.resource_gate, Status::Fail);
    assert!(checked
        .issues
        .iter()
        .any(|issue| issue.contains("RSS growth")));
}

#[test]
fn peak_memory_cannot_be_hidden_by_final_recovery() {
    let (mut report, _) = run(None);
    report.samples[2].resources.rss_bytes += MAX_RSS_GROWTH_BYTES + 1;
    assert_eq!(resources(&report).resource_gate, Status::Fail);
}

#[test]
fn worker_leak_or_unbalanced_lifecycle_fails() {
    for unbalanced in [false, true] {
        let (mut report, _) = run(None);
        let sample = &mut report.samples.last_mut().unwrap().resources;
        if unbalanced {
            sample.workers_started += 1;
        } else {
            sample.workers = 1;
        }
        assert_eq!(resources(&report).resource_gate, Status::Fail);
    }
}

#[test]
fn pressed_key_or_input_ledger_residue_fails() {
    for ledger in [false, true] {
        let (mut report, _) = run(None);
        let sample = &mut report.samples[1].resources;
        if ledger {
            sample.input_ledger_entries = 1;
        } else {
            sample.pressed_keys = 1;
        }
        assert_eq!(resources(&report).resource_gate, Status::Fail);
    }
}

#[test]
fn missing_samples_gap_or_short_duration_fails() {
    let (original, _) = run(None);
    for mode in 0..5 {
        let mut report = original.clone();
        match mode {
            0 => report.samples.clear(),
            1 => {
                report.samples.remove(1);
            }
            2 => {
                report
                    .samples
                    .retain(|sample| sample.at_ms < report.elapsed_ms);
            }
            3 => report.elapsed_ms -= 1,
            _ => {
                report.samples[1].at_ms = 0;
            }
        }
        assert_eq!(
            resources(&report).resource_gate,
            Status::Fail,
            "mode {mode}"
        );
    }
    assert_eq!(
        validate_report(&original, 2, &NoArtifacts)
            .unwrap()
            .resource_gate,
        Status::Fail
    );
}

#[test]
fn fake_time_fake_target_or_missing_platform_is_not_run() {
    let (mut report, _) = run(None);
    assert_eq!(resources(&report).status, Status::NotRun);
    assert_eq!(resources(&report).resource_gate, Status::Pass);
    report.clock_kind = ClockKind::NativeMonotonic;
    report.target_kind = TargetKind::Native;
    assert_eq!(resources(&report).status, Status::NotRun);
    assert!(resources(&report)
        .not_run
        .iter()
        .any(|item| item.contains("artifact")));
    assert!(report
        .checks
        .iter()
        .filter(|check| matches!(check.kind, CheckKind::ScriptLimits | CheckKind::SlowFfi))
        .all(|check| check.observation.status == Status::NotRun));
}

#[test]
fn crash_and_stop_restart_preserve_task_and_worker_counts() {
    let (report, seen) = run(None);
    let counts = report.counts();
    assert_eq!(counts.completed_tasks, 120);
    assert_eq!(counts.crash_restarts, 20);
    assert_eq!(counts.stop_restarts, 20);
    assert!(seen.windows(2).all(|pair| pair[1].1 >= pair[0].1));
    assert_eq!(resources(&report).resource_gate, Status::Pass);
    let selected = tasks();
    for (i, task) in report.tasks.iter().enumerate() {
        assert_eq!(task.id, selected[i % selected.len()].id);
        assert_eq!(task.sequence, i as u64);
    }
}

#[test]
fn restart_without_stop_latch_or_with_old_worker_fails() {
    for old_worker in [false, true] {
        let (mut report, _) = run(None);
        let check = report
            .checks
            .iter_mut()
            .find(|check| check.kind == CheckKind::StopRestart)
            .unwrap();
        let restart = check.observation.restart.as_mut().unwrap();
        if old_worker {
            restart.old_workers_remaining = 1;
        } else {
            restart.stop_latched_after_restart = false;
        }
        assert!(resources(&report)
            .issues
            .iter()
            .any(|issue| issue.contains("restart")));
    }
}

#[test]
fn session_end_scans_nested_residual_file_and_temporary_image() {
    let root = std::env::temp_dir().join(format!("gcu-soak-residue-{}", std::process::id()));
    std::fs::create_dir_all(root.join("images")).unwrap();
    std::fs::write(root.join("images/temp.png"), b"test fixture only").unwrap();
    std::fs::write(root.join("payload.json"), b"{}").unwrap();
    let (report, _) = run(Some(root.clone()));
    let check = report
        .checks
        .iter()
        .find(|check| check.kind == CheckKind::SessionCleanup)
        .unwrap();
    assert_eq!(check.observation.status, Status::Fail);
    assert_eq!(check.observation.violations.len(), 2);
    assert!(resources(&report)
        .issues
        .iter()
        .any(|issue| issue.contains("session_cleanup")));
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(scan_session_residue(&root).unwrap().status, Status::Pass);
}

#[test]
fn report_schema_errors_are_rejected() {
    let (report, _) = run(None);
    let original = serde_json::to_value(report).unwrap();
    for mode in 0..7 {
        let mut value = original.clone();
        match mode {
            0 => value["schema_version"] = json!(SCHEMA_VERSION + 1),
            1 => {
                value.as_object_mut().unwrap().remove("samples");
            }
            2 => value["unknown"] = json!(true),
            3 => value["clock_kind"] = json!("pretend_native"),
            4 => value["samples"][0]["resources"]["rss_bytes"] = json!(-1),
            5 => value["samples"][0]["resources"]["unknown"] = json!(true),
            _ => value["config"]["sample_interval_ms"] = json!(MAX_SAMPLE_INTERVAL_MS + 1),
        }
        assert!(
            parse_report(&serde_json::to_vec(&value).unwrap()).is_err(),
            "mode {mode}"
        );
    }
    let raw = serde_json::to_string(&original).unwrap();
    let duplicate = raw.replacen('{', r#"{"schema_version":1,"#, 1);
    assert!(parse_report(duplicate.as_bytes()).is_err());
}

#[test]
fn zero_duration_is_disabled_without_running_work() {
    let counter = Rc::new(Cell::new(0));
    let mut source = FakeResources::default();
    let mut checks = FakeChecks {
        tasks: counter.clone(),
        root: None,
        seen: Vec::new(),
    };
    let report = block_on(run_soak(
        &tasks(),
        SoakConfig::default(),
        &FakeClock::default(),
        &mut FakeTarget(counter.clone()),
        &mut source,
        &mut checks,
        None,
    ))
    .unwrap()
    .unwrap();
    assert_eq!(counter.get(), 0);
    assert_eq!(source.calls, 0);
    assert!(checks.seen.is_empty());
    assert_eq!(
        validate_report(&report, 0, &NoArtifacts).unwrap().status,
        Status::NotRun
    );
}

#[test]
fn pending_task_is_sampled_and_cancelled_without_replay() {
    struct PendingTarget {
        calls: u64,
        cancelled: Rc<Cell<bool>>,
    }
    struct CancelOnDrop(Rc<Cell<bool>>);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    impl SoakTarget for PendingTarget {
        fn kind(&self) -> TargetKind {
            TargetKind::Fake
        }
        fn run<'a>(&'a mut self, _task: &'a Task) -> SoakFuture<'a, Observation> {
            Box::pin(async move {
                self.calls += 1;
                let _guard = CancelOnDrop(self.cancelled.clone());
                std::future::pending().await
            })
        }
    }
    let cancelled = Rc::new(Cell::new(false));
    let mut target = PendingTarget {
        calls: 0,
        cancelled: cancelled.clone(),
    };
    let mut checks = FakeChecks {
        tasks: Rc::new(Cell::new(0)),
        root: None,
        seen: Vec::new(),
    };
    let report = block_on(run_soak(
        &tasks(),
        SoakConfig::core(SoakPlatform::LinuxX11, 1),
        &FakeClock::default(),
        &mut target,
        &mut FakeResources::default(),
        &mut checks,
        None,
    ))
    .unwrap()
    .unwrap();
    assert_eq!(target.calls, 1);
    assert!(cancelled.get());
    assert_eq!(report.elapsed_ms, HOUR_MS);
    assert_eq!(
        report
            .samples
            .iter()
            .filter(|sample| !sample.quiescent)
            .count(),
        120
    );
    assert_eq!(report.tasks[0].observation.status, Status::Fail);
    assert_eq!(resources(&report).resource_gate, Status::Fail);
    assert!(report
        .checks
        .iter()
        .any(|check| check.kind == CheckKind::SessionCleanup));
    assert!(report
        .checks
        .iter()
        .any(|check| check.kind == CheckKind::OutsideCuSideEffects));
}

fn synthetic_evidence() -> PlatformEvidence {
    PlatformEvidence {
        platform: SoakPlatform::LinuxX11,
        machine: "SYNTHETIC VALIDATOR TEST ONLY".into(),
        os_version: "test-os".into(),
        engine_revision: "test-engine".into(),
        fixture_revision: "test-fixture".into(),
        captured_at_utc: "2026-10-17T00:00:00Z".into(),
        disposable_desktop: true,
        artifact: "raw/test-only.log".into(),
    }
}

struct SyntheticArtifacts;
impl ArtifactSource for SyntheticArtifacts {
    fn available(&self, _evidence: &PlatformEvidence) -> Result<bool, Error> {
        Ok(true)
    }
}

#[test]
fn unavailable_script_scenarios_cannot_be_forged_green() {
    let (mut report, _) = run(None);
    report.clock_kind = ClockKind::NativeMonotonic;
    report.target_kind = TargetKind::Native;
    report.resource_kind = TargetKind::Native;
    report.check_kind = TargetKind::Native;
    report.evidence = Some(synthetic_evidence());
    let checked = validate_report(&report, 1, &SyntheticArtifacts).unwrap();
    assert_eq!(checked.resource_gate, Status::Pass);
    assert_eq!(checked.status, Status::NotRun);
    assert!(checked
        .not_run
        .iter()
        .any(|item| item.contains("script_limits")));
    assert!(checked.not_run.iter().any(|item| item.contains("slow_ffi")));
    for check in &mut report.checks {
        if matches!(check.kind, CheckKind::ScriptLimits | CheckKind::SlowFfi) {
            check.observation = Observation::pass();
        }
    }
    assert_eq!(
        validate_report(&report, 1, &SyntheticArtifacts)
            .unwrap()
            .status,
        Status::Fail
    );
    report.clock_kind = ClockKind::Deterministic;
    assert_eq!(
        validate_report(&report, 1, &SyntheticArtifacts)
            .unwrap()
            .status,
        Status::NotRun
    );
}

#[test]
fn missing_platform_artifact_or_mismatched_metadata_is_not_run() {
    let (mut report, _) = run(None);
    report.clock_kind = ClockKind::NativeMonotonic;
    report.target_kind = TargetKind::Native;
    report.resource_kind = TargetKind::Native;
    report.check_kind = TargetKind::Native;
    report.evidence = Some(synthetic_evidence());
    assert!(validate_report(&report, 1, &NoArtifacts)
        .unwrap()
        .not_run
        .iter()
        .any(|issue| issue.contains("artifact missing")));
    for mode in 0..3 {
        let mut invalid = report.clone();
        let evidence = invalid.evidence.as_mut().unwrap();
        match mode {
            0 => evidence.platform = SoakPlatform::Windows,
            1 => evidence.disposable_desktop = false,
            _ => evidence.engine_revision.clear(),
        }
        assert_eq!(
            validate_report(&invalid, 1, &SyntheticArtifacts)
                .unwrap()
                .status,
            Status::NotRun
        );
    }
}

#[test]
fn artifact_lookup_rejects_empty_missing_or_traversing_paths() {
    let root = std::env::temp_dir().join(format!("gcu-soak-artifacts-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("empty.log"), b"").unwrap();
    std::fs::write(root.join("test-only.log"), b"SYNTHETIC TEST ONLY").unwrap();
    let source = FileArtifacts::new(&root);
    let mut evidence = synthetic_evidence();
    for path in [
        PathBuf::from("missing.log"),
        PathBuf::from("empty.log"),
        PathBuf::from("../test-only.log"),
        root.join("test-only.log"),
    ] {
        evidence.artifact = path;
        assert!(!source.available(&evidence).unwrap());
    }
    evidence.artifact = "test-only.log".into();
    assert!(source.available(&evidence).unwrap());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn external_file_process_and_network_effects_fail_oracle_scan() {
    use gcu_eval::{EffectCheck, World};
    let task = tasks().remove(0);
    let mut spec = task.oracle;
    spec.effects = vec![
        EffectCheck::FileAbsent {
            path: "external-file".into(),
        },
        EffectCheck::ProcessAbsent {
            name: "external-process".into(),
        },
        EffectCheck::NoNetwork,
    ];
    let world = World::from_script(&std::fs::read(task.pass_script).unwrap()).unwrap();
    assert_eq!(scan_outside_effects(&spec, &world).status, Status::Pass);
    for mode in 0..3 {
        let mut changed = world.clone();
        match mode {
            0 => {
                changed
                    .files
                    .insert("external-file".into(), "fingerprint-only".into());
            }
            1 => changed.processes.push("external-process".into()),
            _ => changed.network.push("fixture.invalid".into()),
        }
        assert_eq!(scan_outside_effects(&spec, &changed).status, Status::Fail);
    }
    spec.effects.pop();
    assert_eq!(scan_outside_effects(&spec, &world).status, Status::NotRun);
}

#[test]
fn cli_without_native_adapters_exits_not_run_and_writes_only_raw_report() {
    let root = std::env::temp_dir().join(format!("gcu-soak-cli-{}", std::process::id()));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_gcu-eval"))
        .env_remove("GCU_APPROVAL")
        .args([
            "--suite",
            "core",
            "--soak-hours",
            "4",
            "--platform",
            "linux-x11",
            "--out",
        ])
        .arg(&root)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("not_run"));
    let report = parse_report(&std::fs::read(root.join("soak-linux-x11.json")).unwrap()).unwrap();
    assert_eq!(report.elapsed_ms, 0);
    assert!(report.samples.is_empty());
    assert!(report.tasks.is_empty());
    assert!(report.evidence.is_none());
    assert_eq!(report.checks.len(), 8);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn worker_counters_cannot_reset_on_restart() {
    let (mut report, _) = run(None);
    report.samples[20].resources.workers_started = 0;
    report.samples[20].resources.workers_exited = 0;
    let checked = resources(&report);
    assert_eq!(checked.resource_gate, Status::Fail);
    assert!(checked
        .issues
        .iter()
        .any(|issue| issue.contains("counters reset")));
}

#[test]
fn missing_or_reordered_scenario_records_fail_validation() {
    let (original, _) = run(None);
    for mode in 0..2 {
        let mut report = original.clone();
        if mode == 0 {
            report.checks.remove(2);
        } else {
            report.checks.swap(2, 3);
        }
        assert!(resources(&report)
            .issues
            .iter()
            .any(|issue| issue.contains("scenario")));
    }
}

#[test]
fn final_cleanup_is_resampled_even_when_clock_does_not_advance() {
    struct CleanupCheck {
        scans: u64,
        dirty: Rc<Cell<bool>>,
    }
    impl CheckSource for CleanupCheck {
        fn run(&mut self, kind: CheckKind) -> SoakFuture<'_, Observation> {
            Box::pin(async move {
                if kind == CheckKind::SessionCleanup {
                    self.scans += 1;
                    // Twenty scheduled cleanup checks, then the final cleanup.
                    if self.scans > 20 {
                        self.dirty.set(true);
                    }
                }
                Ok(Observation::pass())
            })
        }
    }
    struct CleanupResources(Rc<Cell<bool>>);
    impl ResourceSource for CleanupResources {
        fn sample(&mut self) -> Result<Resources, Error> {
            Ok(Resources {
                rss_bytes: 100_000_000
                    + if self.0.get() {
                        MAX_RSS_GROWTH_BYTES + 1
                    } else {
                        0
                    },
                ..Resources::default()
            })
        }
    }
    let dirty = Rc::new(Cell::new(false));
    let mut checks = CleanupCheck {
        scans: 0,
        dirty: dirty.clone(),
    };
    let report = block_on(run_soak(
        &tasks(),
        SoakConfig::core(SoakPlatform::LinuxX11, 1),
        &FakeClock::default(),
        &mut FakeTarget(Rc::new(Cell::new(0))),
        &mut CleanupResources(dirty),
        &mut checks,
        None,
    ))
    .unwrap()
    .unwrap();
    assert_eq!(report.samples.last().unwrap().at_ms, HOUR_MS);
    assert_eq!(report.samples[report.samples.len() - 2].at_ms, HOUR_MS);
    assert_eq!(resources(&report).resource_gate, Status::Fail);
    assert!(resources(&report)
        .issues
        .iter()
        .any(|issue| issue.contains("RSS growth")));
}

#[test]
fn clock_read_overhead_keeps_baseline_and_final_timestamps_consistent() {
    struct TickClock(Cell<u64>);
    impl SoakClock for TickClock {
        fn kind(&self) -> ClockKind {
            ClockKind::Deterministic
        }
        fn now_ms(&self) -> u64 {
            let now = self.0.get();
            self.0.set(now + 1);
            now
        }
        fn wait_until(&self, deadline_ms: u64) -> SoakFuture<'_, ()> {
            Box::pin(async move {
                self.0.set(self.0.get().max(deadline_ms));
                Ok(())
            })
        }
    }
    let counter = Rc::new(Cell::new(0));
    let mut checks = FakeChecks {
        tasks: counter.clone(),
        root: None,
        seen: Vec::new(),
    };
    let report = block_on(run_soak(
        &tasks(),
        SoakConfig::core(SoakPlatform::LinuxX11, 1),
        &TickClock(Cell::new(1_000)),
        &mut FakeTarget(counter),
        &mut FakeResources::default(),
        &mut checks,
        None,
    ))
    .unwrap()
    .unwrap();
    assert_eq!(report.samples[0].at_ms, 0);
    assert_eq!(report.samples.last().unwrap().at_ms, report.elapsed_ms);
    assert!(report.elapsed_ms > HOUR_MS);
    assert_eq!(resources(&report).resource_gate, Status::Pass);
}

#[test]
fn sampler_error_is_retained_and_final_cleanup_still_runs() {
    struct FailingResources(u64);
    impl ResourceSource for FailingResources {
        fn sample(&mut self) -> Result<Resources, Error> {
            self.0 += 1;
            if self.0 == 3 {
                return Err(Error::msg("test sampler unavailable"));
            }
            Ok(Resources {
                rss_bytes: 100_000_000,
                ..Resources::default()
            })
        }
    }
    let counter = Rc::new(Cell::new(0));
    let mut checks = FakeChecks {
        tasks: counter.clone(),
        root: None,
        seen: Vec::new(),
    };
    let report = block_on(run_soak(
        &tasks(),
        SoakConfig::core(SoakPlatform::LinuxX11, 1),
        &FakeClock::default(),
        &mut FakeTarget(counter),
        &mut FailingResources(0),
        &mut checks,
        None,
    ))
    .unwrap()
    .unwrap();
    assert!(report
        .errors
        .iter()
        .any(|error| error.contains("test sampler unavailable")));
    assert_eq!(
        report.checks.last().unwrap().kind,
        CheckKind::OutsideCuSideEffects
    );
    assert_eq!(resources(&report).resource_gate, Status::Fail);
}
