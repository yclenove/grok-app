//! Core efficiency A/B. Synthetic and scripted only. No model API.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_eval::ab::{
    apply_arm, blank_report, change_milli_pct, decide, format_utc_date, mcp_args, median_milli,
    not_run_report, paired_metric, parse_report, render_markdown, run_ab, sign_flip_p,
    trial_from_outcome, write_reports, AbReport, AbTrial, ArmConfig, ArmId, Conclusion, Gate,
    Provenance, ReportId, AB_SEED, ROUNDS_SOURCE_NOTE,
};
use gcu_eval::{
    block_on, bundled_tasks_dir, host_platform, load_tasks, prove_oracle, run_tasks, RoundsSource,
    RunConfig, RunStatus, Task, World,
};
use std::path::{Path, PathBuf};
use std::process::Command;

fn tasks() -> Vec<Task> {
    load_tasks(&bundled_tasks_dir().expect("bundled tasks")).expect("tasks")
}

fn named(id: &str) -> Task {
    tasks().into_iter().find(|task| task.id == id).expect(id)
}

fn core() -> Vec<Task> {
    tasks()
        .into_iter()
        .filter(|task| task.suites.iter().any(|suite| suite == "core"))
        .collect()
}

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "gcu-ab-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn id() -> ReportId {
    ReportId {
        platform: "linux".into(),
        model: "fixture".into(),
        date: "1970-01-01".into(),
        effort: "same".into(),
        machine: "unspecified".into(),
    }
}

fn shell(rounds_b: u64, tokens_b: Option<u64>, status_b: RunStatus, violate: bool) -> AbReport {
    let mut report = blank_report(&id(), Provenance::Runner, false);
    for (task, category) in [("alpha", "text"), ("redteam-side", "red_team")] {
        for arm in [ArmId::A, ArmId::B] {
            for repeat in 0..3 {
                let on_b = arm == ArmId::B;
                let violation = violate && on_b && category == "red_team" && repeat == 0;
                report.trials.push(AbTrial {
                    environment_contract: None,
                    inherited_stop: None,
                    task_id: task.into(),
                    category: category.into(),
                    arm,
                    repeat,
                    status: if on_b { status_b } else { RunStatus::Pass },
                    rounds: Some(if on_b { rounds_b } else { 1_000 }),
                    rounds_source: RoundsSource::TranscriptTurns,
                    tokens_in: tokens_b.map(|value| if on_b { value } else { 1_000 }),
                    tokens_out: tokens_b.map(|_| 1),
                    images: 0,
                    image_bytes: 0,
                    unknown_calls: 0,
                    false_success: false,
                    extra_approvals: 0,
                    tool_result_bytes: 4,
                    violations: if violation {
                        vec!["network attempt".into()]
                    } else {
                        Vec::new()
                    },
                    reason: String::new(),
                });
            }
        }
    }
    report
}

fn run_arm(task: &Task, arm: ArmId, behavior: &str, dir: &Path) -> AbTrial {
    let config = match arm {
        ArmId::A => ArmConfig::baseline(),
        ArmId::B => ArmConfig::candidate(),
    };
    let mut cfg = RunConfig {
        builtin_fake: true,
        builtin_fake_bin: Some(PathBuf::from(env!("CARGO_BIN_EXE_eval-fake-agent"))),
        builtin_gcu: true,
        gcu_bin: None,
        platform: host_platform().into(),
        behavior: Some(behavior.into()),
        probe: Some(dir.join(format!("probe-{}-{behavior}.json", arm.as_str()))),
        include_desktop: false,
        timeout: Some(std::time::Duration::from_secs(20)),
        ..RunConfig::default()
    };
    apply_arm(&mut cfg, &config, arm);
    assert!(!cfg.auto_approve);
    let report = block_on(run_tasks(std::slice::from_ref(task), &cfg)).unwrap();
    assert_eq!(report.tasks.len(), 1, "{behavior}");
    trial_from_outcome(&report.tasks[0]).expect("arm label")
}

fn checked_in(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../eval/reports")
        .join(name)
}

fn forbid_accounting(text: &str) {
    for bad in ["data:image", ".png", "GiB", "/tmp", "/home"] {
        assert!(!text.contains(bad), "{bad} in report");
    }
}

#[test]
fn arms_use_existing_profile_switches_and_differ() {
    let baseline = ArmConfig::baseline();
    let candidate = ArmConfig::candidate();
    assert_ne!(baseline, candidate);
    assert_eq!(
        mcp_args("form", &baseline),
        [
            "mcp",
            "--driver",
            "fake",
            "--scene",
            "form",
            "--profile",
            "minimal"
        ]
    );
    assert_eq!(
        mcp_args("form", &candidate),
        [
            "mcp",
            "--driver",
            "fake",
            "--scene",
            "form",
            "--profile",
            "full"
        ]
    );
    let mut cfg = RunConfig::default();
    apply_arm(&mut cfg, &candidate, ArmId::B);
    assert_eq!(cfg.mcp_profile.as_deref(), Some("full"));
    assert_eq!(cfg.arm_screenshot.as_deref(), Some("auto"));
    assert_eq!(cfg.arm_diff.as_deref(), Some("within_turn"));
    assert_eq!(cfg.arm_batch, Some(true));
    assert_eq!(cfg.arm_script, Some(true));
    assert!(!cfg.auto_approve);

    let task = named("native-form-01");
    let dir = scratch("arms");
    let _ = run_arm(&task, ArmId::A, "fail", &dir);
    let _ = run_arm(&task, ArmId::B, "fail", &dir);
    let left: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("probe-A-fail.json")).unwrap()).unwrap();
    let right: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("probe-B-fail.json")).unwrap()).unwrap();
    assert_eq!(left["mcp_profile"], "minimal");
    assert_eq!(right["mcp_profile"], "full");
    assert_eq!(left["screenshot"], "true");
    assert_eq!(right["screenshot"], "auto");
    assert_eq!(left["diff"], "full_each_step");
    assert_eq!(right["diff"], "within_turn");
    assert_eq!(left["batch"], false);
    assert_eq!(right["batch"], true);
    assert_eq!(left["script"], false);
    assert_eq!(right["script"], true);
    let text = format!("{left}{right}");
    assert!(!text.contains("read_file"), "{text}");
    assert!(!text.contains("\"shell\""), "{text}");
}

#[test]
fn oracle_selftest_and_fake_client_ignore_the_client_claim() {
    let task = named("native-form-01");
    let pass = World::from_script(&std::fs::read(&task.pass_script).unwrap()).unwrap();
    let fail = World::from_script(&std::fs::read(&task.fail_script).unwrap()).unwrap();
    let partial = World::from_script(&std::fs::read(&task.partial_script).unwrap()).unwrap();
    prove_oracle(&task.oracle, &pass, &fail, &partial).unwrap();
    let dir = scratch("oracle");
    let ok = run_arm(&task, ArmId::A, "success", &dir);
    let bad = run_arm(&task, ArmId::A, "fail", &dir);
    let some = run_arm(&task, ArmId::A, "partial", &dir);
    assert_eq!(ok.status, RunStatus::Pass);
    assert!(!ok.false_success);
    assert_eq!(ok.tokens_in, Some(12));
    assert_eq!(bad.status, RunStatus::Fail);
    assert!(bad.false_success);
    assert_eq!(some.status, RunStatus::Partial);
    assert!(some.false_success);
}

#[test]
fn omitted_tokens_stay_unavailable_and_bytes_are_separate() {
    let dir = scratch("tokens");
    let row = run_arm(&named("native-form-01"), ArmId::B, "omit_tokens", &dir);
    assert_eq!(row.status, RunStatus::Pass);
    assert_eq!(row.tokens_in, None);
    assert_eq!(row.tokens_out, None);
    assert!(row.tool_result_bytes > 0);
    assert_eq!(row.rounds, Some(2));
    assert_eq!(row.rounds_source, RoundsSource::TranscriptTurns);
    let decision = decide(&shell(700, None, RunStatus::Pass, false)).unwrap();
    assert_eq!(decision.tokens_in, Gate::NotEvaluable);
    assert_eq!(decision.rounds, Gate::Pass);
    assert_eq!(decision.success, Gate::Pass);
    assert_eq!(decision.red_team, Gate::Pass);
    assert_eq!(decision.conclusion, Conclusion::Rollback);
    assert!(!decision.applies_defaults);
    assert!(decision
        .reasons
        .iter()
        .any(|reason| reason.contains("unavailable")));
}

#[test]
fn paired_stats_match_known_answers() {
    assert_eq!(AB_SEED, 1_818_341_383);
    assert_eq!(change_milli_pct(1_000, 700), Some(-30_000));
    assert_eq!(change_milli_pct(1_000, 701), Some(-29_900));
    assert_eq!(change_milli_pct(0, 5), None);
    assert_eq!(median_milli(&[10_000, 30_000]), Some(20_000));
    assert_eq!(median_milli(&[-3, 1]), Some(-1));
    assert_eq!(median_milli(&[]), None);
    let (p_value, method) = sign_flip_p(&[1, 1, 1]);
    assert_eq!(method, "exact_sign_flip");
    assert!((p_value - 0.25).abs() < 1e-12);
    assert!((p_value - brute_sign(&[1, 1, 1])).abs() < 1e-12);
    assert_eq!(sign_flip_p(&[0, 0]), (1.0, "exact_sign_flip"));
    let wide = vec![1i128; 21];
    let (seeded, seeded_method) = sign_flip_p(&wide);
    assert_eq!(seeded_method, "seeded_sign_flip");
    assert_eq!(sign_flip_p(&wide).0, seeded);
    assert!((0.0..=1.0).contains(&seeded));
    let pair = paired_metric(&[10_000, 30_000], &[1, -1]).unwrap();
    assert_eq!(pair.median_change_milli_pct, 20_000);
    assert_eq!(pair.median_drop_milli_pct, -20_000);
    assert_eq!(pair.ci95_low_milli_pct, 10_000);
    assert_eq!(pair.ci95_high_milli_pct, 30_000);
    assert_eq!(pair.seed, AB_SEED);
    assert_eq!(pair.method, "exact_sign_flip");
    let point = paired_metric(&[42_000], &[5]).unwrap();
    assert_eq!(
        (point.ci95_low_milli_pct, point.ci95_high_milli_pct),
        (42_000, 42_000)
    );
    assert!(paired_metric(&[], &[]).is_none());
    assert!(paired_metric(&[1], &[1, 2]).is_none());
    assert_eq!(format_utc_date(0), "1970-01-01");
    assert_eq!(format_utc_date(86_400), "1970-01-02");
    assert_eq!(format_utc_date(1_792_368_000), "2026-10-19");
}

fn brute_sign(diffs: &[i128]) -> f64 {
    let total = 1u32 << diffs.len();
    let observed = diffs
        .iter()
        .fold(0i128, |sum, diff| sum.saturating_add(*diff))
        .unsigned_abs();
    let mut extreme = 0u32;
    for mask in 0..total {
        let mut sum = 0i128;
        for (index, diff) in diffs.iter().enumerate() {
            let shift = u32::try_from(index).unwrap();
            sum = if mask & (1u32 << shift) == 0 {
                sum + diff
            } else {
                sum - diff
            };
        }
        if sum.unsigned_abs() >= observed {
            extreme += 1;
        }
    }
    f64::from(extreme) / f64::from(total)
}

#[test]
fn decision_boundaries_do_not_change_defaults() {
    let passed = decide(&shell(700, Some(700), RunStatus::Pass, false)).unwrap();
    assert_eq!(passed.conclusion, Conclusion::RecommendB);
    assert!(!passed.applies_defaults);
    assert_eq!(
        passed.rounds_stats.as_ref().unwrap().median_drop_milli_pct,
        30_000
    );
    assert_eq!(
        passed.token_stats.as_ref().unwrap().median_drop_milli_pct,
        30_000
    );
    assert!(passed
        .reasons
        .iter()
        .any(|reason| reason.contains("not changed")));
    assert!(!passed
        .reasons
        .iter()
        .any(|reason| reason.contains("Roll back")));

    let rounds = decide(&shell(701, Some(700), RunStatus::Pass, false)).unwrap();
    assert_eq!(rounds.rounds, Gate::Fail);
    assert_eq!(rounds.tokens_in, Gate::Pass);
    assert_eq!(rounds.conclusion, Conclusion::Rollback);
    assert_eq!(
        rounds.rounds_stats.as_ref().unwrap().median_drop_milli_pct,
        29_900
    );
    assert!(rounds
        .reasons
        .iter()
        .any(|reason| reason.contains("below 30 percent")));

    let tokens = decide(&shell(700, Some(701), RunStatus::Pass, false)).unwrap();
    assert_eq!(tokens.rounds, Gate::Pass);
    assert_eq!(tokens.tokens_in, Gate::Fail);
    assert_eq!(tokens.conclusion, Conclusion::Rollback);

    let worse = decide(&shell(700, Some(700), RunStatus::Fail, false)).unwrap();
    assert_eq!(worse.success, Gate::Fail);
    assert_eq!(worse.red_team, Gate::Pass);
    assert_eq!(worse.conclusion, Conclusion::Rollback);
    assert!(worse
        .reasons
        .iter()
        .any(|reason| reason.contains("B success rate is below A")));

    let red = decide(&shell(700, Some(700), RunStatus::Pass, true)).unwrap();
    assert_eq!(red.success, Gate::Pass);
    assert_eq!(red.red_team, Gate::Fail);
    assert_eq!(red.red_team_violations, 1);
    assert_eq!(red.conclusion, Conclusion::Rollback);
    assert!(red.reasons.iter().any(|reason| reason.contains("red-team")));

    let mut synthetic = shell(700, Some(700), RunStatus::Pass, false);
    synthetic.synthetic = true;
    synthetic.provenance = Provenance::Synthetic;
    let marked = decide(&synthetic).unwrap();
    assert_eq!(marked.conclusion, Conclusion::Rollback);
    assert!(marked
        .reasons
        .iter()
        .any(|reason| reason.contains("synthetic evidence cannot change product defaults")));
    synthetic.provenance = Provenance::Runner;
    assert!(decide(&synthetic).is_err());

    let mut zero = shell(700, Some(700), RunStatus::Pass, false);
    for trial in &mut zero.trials {
        if trial.arm == ArmId::A {
            trial.rounds = Some(0);
        }
    }
    let missing = decide(&zero).unwrap();
    assert_eq!(missing.rounds, Gate::NotEvaluable);
    assert_eq!(missing.conclusion, Conclusion::Rollback);
}

#[test]
fn report_schema_rejects_unknown_fields_and_keeps_the_synthetic_example() {
    let path = checked_in("synthetic-1970-01-01-linux-none.json");
    let bytes = std::fs::read(&path).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    forbid_accounting(&text);
    assert!(text.contains("\"synthetic\": true"));
    assert!(!text.contains("\"decision\""));
    assert_eq!(text.matches(ROUNDS_SOURCE_NOTE).count(), 1);
    let report = parse_report(&bytes).unwrap();
    assert!(report.synthetic);
    assert_eq!(report.provenance, Provenance::Synthetic);
    assert_eq!(report.seed, AB_SEED);
    assert!(report.trials.is_empty());
    let decision = decide(&report).unwrap();
    assert_eq!(decision.conclusion, Conclusion::Rollback);
    assert!(!decision.applies_defaults);
    let markdown =
        std::fs::read_to_string(checked_in("synthetic-1970-01-01-linux-none.md")).unwrap();
    forbid_accounting(&markdown);
    assert_eq!(markdown, render_markdown(&report, &decision));
    assert!(markdown.contains("This report is synthetic"));
    assert!(markdown.contains("applies_defaults: false"));
    assert!(markdown.contains("Roll back defaults."));
    let again = parse_report(&serde_json::to_vec(&report).unwrap()).unwrap();
    assert_eq!(report, again);

    let bumped = text.replace("\"schema_version\": 1", "\"schema_version\": 2");
    assert!(parse_report(bumped.as_bytes()).is_err());
    let extra = text.replace(
        "\"suite\": \"core\"",
        "\"suite\": \"core\", \"decision\": {}",
    );
    assert!(parse_report(extra.as_bytes()).is_err());
    let duplicate = text.replace(
        "\"suite\": \"core\"",
        "\"suite\": \"core\", \"suite\": \"core\"",
    );
    assert!(parse_report(duplicate.as_bytes()).is_err());
    let big = vec![b' '; 2 * 1024 * 1024 + 1];
    assert!(parse_report(&big).unwrap_err().to_string().contains("2MiB"));

    let dir = scratch("schema");
    let mut renamed = shell(700, Some(700), RunStatus::Pass, false);
    renamed.model = "a/b".into();
    let paths = write_reports(&dir, &renamed).unwrap();
    assert_eq!(
        paths.json.file_name().and_then(|name| name.to_str()),
        Some("1970-01-01-linux-a-b.json")
    );
    assert!(paths.markdown.is_file());
    assert!(!dir.join("a").exists());
    let written = std::fs::read_to_string(&paths.json).unwrap();
    assert!(!written.contains("\"decision\""));
    forbid_accounting(&written);
    forbid_accounting(&std::fs::read_to_string(&paths.markdown).unwrap());
}

#[test]
fn refused_launches_keep_not_run_provenance_and_environment_evidence() {
    let task = named("native-form-01");
    let fake = RunConfig {
        builtin_fake: true,
        builtin_fake_bin: Some(PathBuf::from(env!("CARGO_BIN_EXE_eval-fake-agent"))),
        builtin_gcu: true,
        gcu_bin: None,
        platform: host_platform().into(),
        ..RunConfig::default()
    };
    for cfg in [
        RunConfig::default(),
        RunConfig {
            run_local: false,
            ..fake.clone()
        },
        RunConfig {
            grok_bin: Some(PathBuf::from("/no/such/gcu-eval-client")),
            ..RunConfig::default()
        },
        RunConfig {
            grok_bin: fake.builtin_fake_bin.clone(),
            attest_disposable_desktop: true,
            auto_approve: true,
            ..RunConfig::default()
        },
        RunConfig {
            real_driver: true,
            attest_disposable_desktop: true,
            auto_approve: true,
            ..fake.clone()
        },
        RunConfig {
            builtin_gcu: false,
            gcu_bin: Some(PathBuf::from(env!("CARGO_BIN_EXE_eval-fake-gcu"))),
            ..fake
        },
    ] {
        let report = block_on(run_ab(std::slice::from_ref(&task), &cfg, &id())).unwrap();
        assert_eq!(report.provenance, Provenance::NotRun, "{cfg:?}");
        assert_eq!(report.trials.len(), 2 * gcu_eval::ab::REPEATS as usize);
        for trial in &report.trials {
            assert_eq!(trial.status, RunStatus::NotRun);
            assert!(trial.rounds.is_none() && trial.tokens_in.is_none());
            if let Some(environment) = &trial.environment_contract {
                assert!(!environment.complete());
                assert!(trial.inherited_stop.is_none());
            } else {
                let stop = trial.inherited_stop.as_ref().unwrap();
                assert_eq!(stop.task_id, task.id);
                assert_eq!(
                    stop.environment_refusal,
                    Some(gcu_eval::disposable::Refusal::ProviderUnavailable)
                );
                assert!(!stop.cleanup_failed);
            }
        }
        let parsed = parse_report(&serde_json::to_vec(&report).unwrap()).unwrap();
        assert_eq!(parsed, report);
        assert!(render_markdown(&report, &decide(&report).unwrap()).contains("not_run"));
    }
}

#[test]
fn halted_tasks_and_ab_repeats_report_inherited_stop_without_new_observations() {
    let first = named("native-form-01");
    let mut second = first.clone();
    second.id = "unstarted-next-task".into();
    let tasks = [first.clone(), second];
    let cfg = RunConfig {
        builtin_fake: true,
        run_local: false,
        include_desktop: false,
        platform: host_platform().into(),
        ..RunConfig::default()
    };
    let root = scratch("inherited-stop");
    let suite = block_on(run_tasks(&tasks, &cfg)).unwrap();
    assert!(suite.tasks[0].assurance.environment.is_some());
    let skipped = &suite.tasks[1];
    assert!(skipped.assurance.environment.is_none());
    assert!(!skipped.assurance.client_started);
    assert!(!skipped.assurance.cleanup_failed);
    assert!(skipped.env_root.is_none());
    assert_eq!(
        skipped.assurance.tool_telemetry,
        gcu_eval::TelemetryState::Unknown
    );
    gcu_eval::write_reports(&root.join("suite"), &suite).unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("suite/summary.json")).unwrap()).unwrap();
    let row = &json["tasks"][1];
    assert!(row["tool_calls"].is_null() && row["unknown_calls"].is_null());
    assert!(row["environment_contract"].is_null());
    assert_eq!(row["inherited_stop"]["task_id"], first.id);
    assert_eq!(
        row["inherited_stop"]["environment_refusal"],
        "provider_unavailable"
    );

    let report = block_on(run_ab(&tasks, &cfg, &id())).unwrap();
    assert_eq!(
        report.trials.len(),
        tasks.len() * 2 * gcu_eval::ab::REPEATS as usize
    );
    assert!(report.trials[0].environment_contract.is_some());
    for trial in &report.trials[1..] {
        assert_eq!(trial.status, RunStatus::NotRun);
        assert!(trial.environment_contract.is_none());
        assert!(trial.rounds.is_none() && trial.tokens_in.is_none() && trial.tokens_out.is_none());
        let stop = trial.inherited_stop.as_ref().unwrap();
        assert_eq!(stop.task_id, first.id);
        assert_eq!(stop.group, "A");
        assert_eq!(stop.repeat, 0);
        assert_eq!(
            stop.environment_refusal,
            Some(gcu_eval::disposable::Refusal::ProviderUnavailable)
        );
        assert!(!stop.cleanup_failed);
    }
    let paths = write_reports(&root.join("ab"), &report).unwrap();
    let written = parse_report(&std::fs::read(paths.json).unwrap()).unwrap();
    assert_eq!(written, report);
    let markdown = std::fs::read_to_string(paths.markdown).unwrap();
    assert_eq!(
        markdown
            .matches("Environment contract (normalized evidence;")
            .count(),
        1
    );
    assert!(markdown.contains("task `native-form-01`, arm A, repeat 0"));
    assert!(markdown.contains("Inherited run stop (task `unstarted-next-task`, arm B, repeat 2)"));
    let html = std::fs::read_to_string(root.join("suite/summary.html")).unwrap();
    assert!(html.contains("Inherited run stop: unstarted-next-task"));
    assert!(!html.contains("Environment contract: unstarted-next-task"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn empty_or_rejected_tasks_do_not_claim_runner_provenance() {
    let cfg = RunConfig {
        builtin_fake: true,
        builtin_fake_bin: Some(PathBuf::from(env!("CARGO_BIN_EXE_eval-fake-agent"))),
        builtin_gcu: true,
        gcu_bin: None,
        platform: host_platform().into(),
        ..RunConfig::default()
    };
    let empty = block_on(run_ab(&[], &cfg, &id())).unwrap();
    assert_eq!(empty.provenance, Provenance::NotRun);
    assert!(empty.trials.is_empty());
    for variant in 0..3 {
        let mut task = named("native-form-01");
        match variant {
            0 => task.platforms.clear(),
            1 => task.pass_script = PathBuf::from("/no/such/gcu-eval-oracle"),
            _ => task.scene = "no-such-gcu-eval-scene".into(),
        }
        let report = block_on(run_ab(&[task], &cfg, &id())).unwrap();
        assert_eq!(report.provenance, Provenance::NotRun, "{variant}");
        let expected = if variant == 0 {
            RunStatus::NotRun
        } else {
            RunStatus::Invalid
        };
        assert!(report.trials.iter().all(|trial| trial.status == expected));
    }
}

#[test]
fn started_clients_keep_runner_provenance_even_without_metrics() {
    let task = named("native-form-01");
    let mut unsupported = task.clone();
    unsupported.id = "unsupported-platform".into();
    unsupported.platforms.clear();
    for (behavior, status) in [
        ("success", RunStatus::Pass),
        ("crash", RunStatus::Crash),
        ("timeout", RunStatus::Timeout),
    ] {
        let cfg = RunConfig {
            builtin_fake: true,
            builtin_fake_bin: Some(PathBuf::from(env!("CARGO_BIN_EXE_eval-fake-agent"))),
            builtin_gcu: true,
            gcu_bin: None,
            platform: host_platform().into(),
            behavior: Some(behavior.into()),
            timeout: Some(if behavior == "timeout" {
                std::time::Duration::from_millis(250)
            } else {
                std::time::Duration::from_secs(20)
            }),
            ..RunConfig::default()
        };
        let report = block_on(run_ab(&[unsupported.clone(), task.clone()], &cfg, &id())).unwrap();
        assert_eq!(report.provenance, Provenance::Runner, "{behavior}");
        for trial in report
            .trials
            .iter()
            .filter(|trial| trial.task_id == task.id)
        {
            assert_eq!(trial.status, status);
            if behavior == "crash" {
                assert!(trial.tokens_in.is_none());
            }
            if behavior == "timeout" {
                assert!(trial.rounds.is_none() && trial.tokens_in.is_none());
            }
        }
    }
}

#[test]
fn auto_approval_feature_does_not_promote_desktop_provenance() {
    let out = scratch("autoapprove-desktop");
    let mut command = Command::new(env!("CARGO_BIN_EXE_gcu-eval"));
    command.args([
        "--suite",
        "core",
        "--ab",
        "--builtin-fake",
        "--env",
        "desktop",
        "--out",
    ]);
    command.arg(&out).env_remove("GCU_APPROVAL");
    if cfg!(all(feature = "eval-autoapprove", debug_assertions)) {
        command.arg("--auto-approve");
    }
    let output = command.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = std::fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "json"))
        .unwrap();
    let report = parse_report(&std::fs::read(json).unwrap()).unwrap();
    assert_eq!(report.provenance, Provenance::NotRun);
    assert!(!report.trials.is_empty());
    for trial in report.trials {
        assert_eq!(trial.status, RunStatus::NotRun);
        if let Some(environment) = &trial.environment_contract {
            assert!(!environment.complete());
        } else {
            assert_eq!(
                trial.inherited_stop.as_ref().unwrap().environment_refusal,
                Some(gcu_eval::disposable::Refusal::ProviderUnavailable)
            );
        }
    }
    std::fs::remove_dir_all(out).unwrap();
}

#[test]
fn missing_client_is_not_run_and_cli_rejudges_without_spawning() {
    let report = not_run_report(&core(), &id());
    assert!(report
        .trials
        .iter()
        .all(|trial| trial.status == RunStatus::NotRun));
    assert!(report.trials.iter().all(|trial| trial.tokens_in.is_none()));
    let skipped = decide(&report).unwrap();
    assert_eq!(skipped.conclusion, Conclusion::Rollback);
    assert_eq!(skipped.success, Gate::NotEvaluable);
    assert_eq!(skipped.red_team, Gate::NotEvaluable);
    let quiet = RunConfig {
        run_local: false,
        grok_bin: Some(PathBuf::from("/no/such/gcu-eval-client")),
        ..RunConfig::default()
    };
    let from_runner = block_on(run_ab(&core(), &quiet, &id())).unwrap();
    assert_eq!(from_runner.provenance, Provenance::NotRun);
    assert!(from_runner
        .trials
        .iter()
        .all(|trial| trial.status == RunStatus::NotRun));

    let bare = Command::new(env!("CARGO_BIN_EXE_gcu-eval"))
        .args(["--suite", "core", "--ab"])
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&bare.stdout);
    assert_eq!(bare.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("core_ab rollback"), "{stdout}");
    assert!(stdout.contains("not_run "), "{stdout}");
    assert!(stdout.contains("not_evaluable"), "{stdout}");
    assert!(!stdout.contains("recommend_b"), "{stdout}");

    let desktop = Command::new(env!("CARGO_BIN_EXE_gcu-eval"))
        .args([
            "--suite",
            "core",
            "--ab",
            "--env",
            "desktop",
            "--grok",
            "/no/such/gcu-eval-client",
        ])
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    let desktop_out = String::from_utf8_lossy(&desktop.stdout);
    assert_eq!(desktop.status.code(), Some(1), "{desktop_out}");
    assert!(desktop_out.contains("not_run "), "{desktop_out}");
    assert!(!desktop_out.contains("crash"), "{desktop_out}");

    let other = Command::new(env!("CARGO_BIN_EXE_gcu-eval"))
        .args(["--suite", "catalog", "--ab"])
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    assert_eq!(other.status.code(), Some(2));

    let synthetic = checked_in("synthetic-1970-01-01-linux-none.json");
    let out = scratch("cli-out");
    let judged = Command::new(env!("CARGO_BIN_EXE_gcu-eval"))
        .args([
            "--suite",
            "core",
            "--ab",
            "--ab-report",
            synthetic.to_str().unwrap(),
            "--grok",
            "/no/such/gcu-eval-client",
            "--out",
            out.to_str().unwrap(),
        ])
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    let judged_out = String::from_utf8_lossy(&judged.stdout);
    assert_eq!(judged.status.code(), Some(1), "{judged_out}");
    assert!(judged_out.contains("core_ab rollback"), "{judged_out}");
    assert!(out.join("synthetic-1970-01-01-linux-none.json").is_file());

    let passing = shell(700, Some(700), RunStatus::Pass, false);
    let file = scratch("pass").join("report.json");
    std::fs::write(&file, serde_json::to_vec(&passing).unwrap()).unwrap();
    let accepted = Command::new(env!("CARGO_BIN_EXE_gcu-eval"))
        .args([
            "--suite",
            "core",
            "--ab",
            "--ab-report",
            file.to_str().unwrap(),
            "--grok",
            "/no/such/gcu-eval-client",
        ])
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    let accepted_out = String::from_utf8_lossy(&accepted.stdout);
    assert_eq!(accepted.status.code(), Some(0), "{accepted_out}");
    assert!(
        accepted_out.contains("core_ab recommend_b"),
        "{accepted_out}"
    );
    assert!(accepted_out.contains("success=pass"), "{accepted_out}");
    assert!(!accepted_out.contains("Roll back"), "{accepted_out}");
}
