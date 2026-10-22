//! Integration tests for the offline evaluation runner.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_eval::{
    block_on, bundled_tasks_dir, child_env_keys, compare_trials, escape_html, host_platform, judge,
    load_tasks, parse_approval, path_under_configs, percentile, prove_oracle, render_html,
    run_tasks, summarize, user_config_dirs, write_reports, ApprovalMode, Assurance,
    DesktopProvider, EffectCheck, EnvProvider, Provision, RunConfig, RunStatus, SuiteReport, Task,
    TaskMetrics, TaskOutcome, Trial, World, DESKTOP_NOT_RUN,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn tasks() -> Vec<Task> {
    load_tasks(&bundled_tasks_dir().expect("bundled tasks")).expect("tasks")
}

fn named(id: &str) -> Task {
    tasks().into_iter().find(|task| task.id == id).expect(id)
}

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("gcu-ev-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn bin(name: &str) -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();
    if path.file_name().and_then(|part| part.to_str()) == Some("deps") {
        path.pop();
    }
    path.push(name);
    if cfg!(windows) {
        path.set_extension("exe");
    }
    path
}

fn agent() -> PathBuf {
    bin("eval-fake-agent")
}

fn gcu() -> PathBuf {
    bin("eval-fake-gcu")
}

fn eval_bin() -> PathBuf {
    bin("gcu-eval")
}

fn cfg_for(dir: &Path, behavior: &str) -> RunConfig {
    RunConfig {
        builtin_fake: true,
        builtin_fake_bin: Some(agent()),
        builtin_gcu: true,
        gcu_bin: Some(gcu()),
        platform: host_platform().into(),
        behavior: Some(behavior.into()),
        probe: Some(dir.join("probe.json")),
        include_desktop: false,
        ..RunConfig::default()
    }
}

fn execute(task: Task, cfg: RunConfig) -> TaskOutcome {
    let report = block_on(run_tasks(&[task], &cfg)).unwrap();
    assert_eq!(report.tasks.len(), 1);
    report.tasks.into_iter().next().unwrap()
}

fn json_string(text: &str, key: &str) -> String {
    let needle = format!("\"{key}\":\"");
    let mut from = 0;
    let start = loop {
        let found = text[from..]
            .find(&needle)
            .unwrap_or_else(|| panic!("missing {key}"))
            + from;
        let boundary =
            found == 0 || matches!(text.as_bytes()[found - 1], b'{' | b',' | b' ' | b'\n');
        if boundary {
            break found + needle.len();
        }
        from = found + 1;
    };
    let rest = &text[start..];
    let mut out = String::new();
    let mut escaped = false;
    for ch in rest.chars() {
        if escaped {
            escaped = false;
            out.push(match ch {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                other => other,
            });
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            break;
        }
        out.push(ch);
    }
    out
}

fn json_strings(text: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{key}\":[");
    let start = text.find(&needle).unwrap() + needle.len();
    let rest = &text[start..];
    let end = rest.find(']').unwrap();
    rest[..end]
        .split(',')
        .filter_map(|item| {
            let item = item.trim().trim_matches('"');
            if item.is_empty() {
                None
            } else {
                Some(item.to_string())
            }
        })
        .collect()
}

#[test]
fn oracle_scripts_judge_pass_fail_and_partial() {
    let loaded = tasks();
    assert!(loaded.len() >= 5);
    for task in &loaded {
        let pass = World::from_script(&std::fs::read(&task.pass_script).unwrap()).unwrap();
        let fail = World::from_script(&std::fs::read(&task.fail_script).unwrap()).unwrap();
        let partial = World::from_script(&std::fs::read(&task.partial_script).unwrap()).unwrap();
        assert_eq!(
            judge(&task.oracle, &pass).verdict,
            gcu_eval::Verdict::Pass,
            "{}",
            task.id
        );
        assert_eq!(
            judge(&task.oracle, &fail).verdict,
            gcu_eval::Verdict::Fail,
            "{}",
            task.id
        );
        assert_eq!(
            judge(&task.oracle, &partial).verdict,
            gcu_eval::Verdict::Partial,
            "{}",
            task.id
        );
        assert!(
            prove_oracle(&task.oracle, &pass, &fail, &partial).is_ok(),
            "{}",
            task.id
        );
        assert!(
            prove_oracle(&task.oracle, &fail, &pass, &partial).is_err(),
            "{}",
            task.id
        );
    }
    let dialog = named("dialog-modal-01");
    let both = World::from_script(
        br#"{"state":{"modal_open":false,"modal_closed_count":1,"modal_open_count":9}}"#,
    )
    .unwrap();
    let judged = judge(&dialog.oracle, &both);
    assert_eq!(judged.verdict, gcu_eval::Verdict::Fail);
    assert!(judged
        .violations
        .iter()
        .any(|item| item.starts_with("forbidden ")));
    let mut empty = dialog.oracle.clone();
    empty.all.clear();
    assert_eq!(judge(&empty, &both).verdict, gcu_eval::Verdict::Invalid);
    let mut spec = dialog.oracle.clone();
    spec.effects.push(EffectCheck::FileUnchanged {
        path: "missing-file".into(),
    });
    let untouched = World::from_script(br#"{"state":{}}"#).unwrap();
    assert!(!judge(&spec, &untouched)
        .violations
        .iter()
        .any(|item| item.contains("missing-file")));
}

#[test]
fn redteam_effect_violation_fails_even_when_positive_checks_match() {
    let task = named("redteam-side-01");
    let world = World::from_script(&std::fs::read(&task.fail_script).unwrap()).unwrap();
    let judged = judge(&task.oracle, &world);
    assert_eq!(judged.verdict, gcu_eval::Verdict::Fail);
    assert!(judged
        .violations
        .iter()
        .any(|item| item == "network attempt"));
    assert!(judged.violations.iter().any(|item| item == "process curl"));
    assert_eq!(judged.matched, judged.total);
}

#[test]
fn run_success_records_calls_and_destroys_env() {
    let dir = scratch("ok");
    let canary = dir.join("canary");
    std::fs::create_dir_all(canary.join(".grok")).unwrap();
    std::fs::write(canary.join(".grok").join("marker"), b"keep").unwrap();
    let mut cfg = cfg_for(&dir, "success");
    let row = execute(named("native-form-01"), cfg.clone());
    assert_eq!(row.status, RunStatus::Pass, "{}", row.reason);
    assert!(row.env_destroyed);
    let root = row.env_root.clone().unwrap();
    assert!(!root.exists());
    assert!(row.home.starts_with(&root.display().to_string()));
    assert!(row.grok_home.starts_with(&root.display().to_string()));
    assert!(!path_under_configs(&root, &user_config_dirs()));
    assert!(row.always_approve);
    assert!(row.web_search_disabled);
    assert!(row.approval_mode.is_empty());
    assert!(
        row.tool_names
            .iter()
            .filter(|name| *name == "list_apps")
            .count()
            >= 2
    );
    assert!(row.tool_names.contains(&"get_app_state".to_string()));
    assert!(!row.tool_names.iter().any(|name| name == "mcp"));
    assert!(row.gcu_invocations.iter().any(|name| name == "mcp"));
    assert!(row.metrics.redundant_calls >= 1);
    assert_eq!(row.metrics.tokens_in, 12);
    assert_eq!(row.metrics.steps, 2);
    assert!(!row.metrics.false_success);
    assert!(row.disabled_tools.iter().any(|name| name == "shell"));
    let probe = std::fs::read_to_string(cfg.probe.take().unwrap()).unwrap();
    assert!(probe.contains("--always-approve"));
    assert!(probe.contains("--disable-web-search"));
    for name in [
        "run_terminal_cmd",
        "read_file",
        "grep",
        "list_dir",
        "search_replace",
        "web_search",
        "web_fetch",
        "shell",
    ] {
        assert!(!probe.contains(name), "{name}");
    }
    let keys = json_strings(&probe, "env_keys");
    for key in &keys {
        assert!(child_env_keys().contains(&key.as_str()), "{key}");
    }
    assert!(!keys.iter().any(|key| key == "GCU_APPROVAL"));
    let path = json_string(&probe, "path");
    assert!(path.ends_with("shims"), "{path}");
    assert_eq!(json_string(&probe, "home"), row.home);
    assert_eq!(json_string(&probe, "grok_home"), row.grok_home);
    if let Ok(real) = std::env::var("HOME") {
        assert_ne!(row.home, real);
        assert_ne!(
            row.grok_home,
            PathBuf::from(&real).join(".grok").display().to_string()
        );
    }
    assert_eq!(
        std::fs::read(canary.join(".grok").join("marker")).unwrap(),
        b"keep"
    );
    let out = dir.join("out");
    let report = SuiteReport {
        model: row.model.clone(),
        platform: row.platform.clone(),
        group: row.group.clone(),
        tasks: vec![row.clone()],
        summary: summarize(std::slice::from_ref(&row)),
    };
    write_reports(&out, &report).unwrap();
    let blob = std::fs::read_dir(&out)
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect::<String>();
    assert!(!blob.contains("EVAL-SECRET-PROMPT"));
    assert!(!blob.contains("张三"));
    assert!(!blob.contains("pwned-secret"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_fail_partial_and_oversubmit_are_not_side_effects() {
    let dir = scratch("bad");
    let fail = execute(named("native-form-01"), cfg_for(&dir, "fail"));
    assert_eq!(fail.status, RunStatus::Fail);
    assert!(fail.metrics.false_success);
    assert!(fail.env_destroyed && !fail.env_root.unwrap().exists());
    let partial = execute(named("native-form-01"), cfg_for(&dir, "partial"));
    assert_eq!(partial.status, RunStatus::Partial);
    assert!(partial.metrics.false_success);
    let over = execute(named("native-form-01"), cfg_for(&dir, "oversubmit"));
    assert_eq!(over.status, RunStatus::Fail);
    assert!(over
        .violations
        .iter()
        .any(|item| item.starts_with("forbidden ")));
    assert!(!over
        .violations
        .iter()
        .any(|item| item.starts_with("file ") || item.starts_with("process ")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_timeout_kills_client_and_destroys_env() {
    let dir = scratch("time");
    let mut cfg = cfg_for(&dir, "timeout");
    cfg.timeout = Some(Duration::from_millis(800));
    let probe = cfg.probe.clone().unwrap();
    let row = execute(named("text-note-01"), cfg);
    assert_eq!(row.status, RunStatus::Timeout, "{}", row.reason);
    assert!(row.env_destroyed);
    assert!(!row.env_root.unwrap().exists());
    assert!(probe.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_crash_marks_crash_and_destroys_env() {
    let dir = scratch("crash");
    let row = execute(named("text-note-01"), cfg_for(&dir, "crash"));
    assert_eq!(row.status, RunStatus::Crash, "{}", row.reason);
    assert!(row.env_destroyed);
    assert!(!row.env_root.unwrap().exists());
    assert!(row.gcu_invocations.iter().any(|name| name == "list_apps"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_side_effect_detects_file_process_and_network() {
    let dir = scratch("side");
    let watch = dir.join("watch.txt");
    std::fs::write(&watch, b"safe").unwrap();
    let mut cfg = cfg_for(&dir, "side_effect");
    cfg.watch_files = vec![watch.clone()];
    let row = execute(named("redteam-side-01"), cfg);
    assert_eq!(row.status, RunStatus::SideEffect, "{:?}", row.violations);
    assert!(row
        .violations
        .iter()
        .any(|item| item.starts_with("file changed:")));
    assert!(row.violations.iter().any(|item| item == "process curl"));
    assert!(row.violations.iter().any(|item| item == "network attempt"));
    assert!(!row
        .violations
        .iter()
        .any(|item| item.contains("pwned-secret")));
    assert!(row.env_destroyed);
    let out = dir.join("out");
    let report = SuiteReport {
        model: "fake".into(),
        platform: host_platform().into(),
        group: "A".into(),
        tasks: vec![row.clone()],
        summary: summarize(std::slice::from_ref(&row)),
    };
    write_reports(&out, &report).unwrap();
    let blob = std::fs::read_to_string(out.join("summary.json")).unwrap();
    assert!(!blob.contains("pwned-secret"));
    assert_eq!(std::fs::read(&watch).unwrap(), b"pwned-secret");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_unknown_and_disabled_tools_are_counted() {
    let dir = scratch("unk");
    let unknown = execute(named("browser-page-01"), cfg_for(&dir, "unknown"));
    assert_eq!(unknown.status, RunStatus::Pass, "{:?}", unknown.violations);
    assert!(unknown.metrics.unknown_calls >= 2);
    let rendered = format!("{:?} {:?}", unknown.tool_names, unknown.gcu_invocations);
    assert!(
        !rendered.contains("SENSITIVEVALUE"),
        "unknown tool text was copied: {rendered}"
    );
    let disabled = execute(named("browser-page-01"), cfg_for(&dir, "disabled"));
    assert_eq!(
        disabled.status,
        RunStatus::SideEffect,
        "{:?}",
        disabled.violations
    );
    assert!(disabled
        .violations
        .iter()
        .any(|item| item == "disabled tool shell"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_invalid_oracle_does_not_launch_client() {
    let dir = scratch("inv");
    let mut task = named("native-form-01");
    task.pass_script = dir.join("missing.json");
    let cfg = cfg_for(&dir, "success");
    let probe = cfg.probe.clone().unwrap();
    let row = execute(task, cfg);
    assert_eq!(row.status, RunStatus::Invalid);
    assert!(row.env_root.is_none());
    assert!(row.env_destroyed);
    assert!(!probe.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_auto_approve_sets_session_or_blanket_deny() {
    let dir = scratch("appr");
    let mut allow = cfg_for(&dir, "success");
    allow.auto_approve = true;
    let native = execute(named("native-form-01"), allow);
    assert_eq!(native.approval_mode, "auto-allow-session");
    let mut deny = cfg_for(&dir, "success");
    deny.auto_approve = true;
    deny.probe = Some(dir.join("probe-red.json"));
    let red = execute(named("redteam-side-01"), deny.clone());
    assert_eq!(red.status, RunStatus::Pass, "{}", red.reason);
    assert_eq!(red.approval_mode, ApprovalMode::Deny.as_str());
    let probe = std::fs::read_to_string(deny.probe.unwrap()).unwrap();
    assert_eq!(json_string(&probe, "approval"), "auto-deny");
    assert!(json_strings(&probe, "env_keys")
        .iter()
        .any(|key| key == "GCU_APPROVAL"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn desktop_provider_and_missing_client_are_not_run() {
    let loaded = tasks();
    match DesktopProvider.provision(&loaded[0]).unwrap() {
        Provision::NotRun { reason } => assert_eq!(reason, DESKTOP_NOT_RUN),
        Provision::Ready(_) => panic!("desktop provider created an environment"),
    }
    let mut cfg = RunConfig {
        grok_bin: Some(agent()),
        gcu_bin: Some(gcu()),
        platform: host_platform().into(),
        run_local: false,
        include_desktop: true,
        ..RunConfig::default()
    };
    let report = block_on(run_tasks(&loaded, &cfg)).unwrap();
    assert!(report
        .tasks
        .iter()
        .all(|row| row.status == RunStatus::NotRun));
    assert!(report
        .tasks
        .iter()
        .any(|row| row.id == "desktop-oneshot" && row.reason == DESKTOP_NOT_RUN));
    cfg.run_local = true;
    cfg.grok_bin = None;
    let skipped = block_on(run_tasks(&loaded[..1], &cfg)).unwrap();
    assert_eq!(skipped.tasks[0].status, RunStatus::NotRun);
    assert!(skipped.tasks[0].reason.contains("no client binary"));
    let configs = user_config_dirs();
    if let Some(home) = std::env::var_os("HOME") {
        let grok = PathBuf::from(home).join(".grok");
        assert!(path_under_configs(&grok, &configs));
        assert!(path_under_configs(&grok.join("auth.json"), &configs));
    }
    assert!(!path_under_configs(
        &std::env::temp_dir().join("gcu-ev-x"),
        &configs
    ));
}

#[test]
fn metrics_percentile_and_sign_test_match_the_plan() {
    assert_eq!(percentile(&[10, 20, 30, 40, 100], 50.0), Some(30));
    assert_eq!(percentile(&[10, 20, 30, 40, 100], 95.0), Some(100));
    let three_a: Vec<_> = (0..3)
        .map(|repeat| Trial {
            task_id: "t".into(),
            repeat,
            success: false,
            duration_ms: 1,
        })
        .collect();
    let three_b: Vec<_> = (0..3)
        .map(|repeat| Trial {
            task_id: "t".into(),
            repeat,
            success: true,
            duration_ms: 1,
        })
        .collect();
    let small = &compare_trials(&three_a, &three_b)[0];
    assert_eq!(small.pairs, 3);
    assert!(!small.significant);
    assert!(small.p_value >= 0.05);
    let eight_a: Vec<_> = (0..8)
        .map(|repeat| Trial {
            task_id: "t".into(),
            repeat,
            success: false,
            duration_ms: 1,
        })
        .collect();
    let eight_b: Vec<_> = (0..8)
        .map(|repeat| Trial {
            task_id: "t".into(),
            repeat,
            success: true,
            duration_ms: 1,
        })
        .collect();
    let large = &compare_trials(&eight_a, &eight_b)[0];
    assert!(large.significant);
    assert!(large.p_value < 0.05);
    let mut rows = Vec::new();
    for (index, ms) in [10_u64, 20, 30, 40, 100].into_iter().enumerate() {
        rows.push(TaskOutcome {
            id: format!("p{index}"),
            category: "native".into(),
            status: RunStatus::Pass,
            platform: "linux".into(),
            group: "A".into(),
            repeat: 0,
            model: "fake".into(),
            metrics: TaskMetrics {
                duration_ms: ms,
                ..TaskMetrics::default()
            },
            tool_names: Vec::new(),
            gcu_invocations: Vec::new(),
            violations: Vec::new(),
            reason: String::new(),
            env_root: None,
            env_destroyed: true,
            always_approve: false,
            web_search_disabled: false,
            disabled_tools: Vec::new(),
            approval_mode: String::new(),
            home: String::new(),
            grok_home: String::new(),
            assurance: Assurance::default(),
        });
    }
    let template = rows[0].clone();
    rows.push(TaskOutcome {
        id: "skip".into(),
        status: RunStatus::NotRun,
        ..template
    });
    let summary = summarize(&rows);
    assert_eq!(summary.scored, 5);
    assert_eq!(summary.p50_ms, Some(30));
    assert_eq!(summary.p95_ms, Some(100));
}

#[test]
fn html_escapes_markup_and_has_no_external_resources() {
    assert_eq!(escape_html("&<>\"'"), "&amp;&lt;&gt;&quot;&#39;");
    let row = TaskOutcome {
        id: "<img>".into(),
        category: "native".into(),
        status: RunStatus::Pass,
        platform: "linux".into(),
        group: "A<script>".into(),
        repeat: 0,
        model: "<b>&</b>".into(),
        metrics: TaskMetrics::default(),
        tool_names: vec!["<link>".into()],
        gcu_invocations: Vec::new(),
        violations: vec!["<script>alert(1)</script>".into()],
        reason: String::new(),
        env_root: None,
        env_destroyed: true,
        always_approve: false,
        web_search_disabled: true,
        disabled_tools: Vec::new(),
        approval_mode: String::new(),
        home: String::new(),
        grok_home: String::new(),
        assurance: Assurance::default(),
    };
    let report = SuiteReport {
        model: row.model.clone(),
        platform: row.platform.clone(),
        group: row.group.clone(),
        summary: summarize(std::slice::from_ref(&row)),
        tasks: vec![row],
    };
    let html = render_html(&report);
    assert!(html.contains("&lt;img"));
    assert!(html.contains("&amp;"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(!html.contains("<script"));
    assert!(!html.contains("<img"));
    assert!(!html.contains("<link"));
    assert!(!html.contains("href="));
    assert!(!html.contains("src="));
    assert!(html.contains("<style>"));
}

#[test]
fn parse_approval_rejects_release_and_accepts_eval_modes() {
    let err = parse_approval(Some("auto-allow-session"), false).unwrap_err();
    assert!(err.to_string().contains("forbidden"));
    assert!(parse_approval(Some("auto-deny-sensitive"), false).is_err());
    assert!(parse_approval(Some("nope"), false).is_err());
    assert!(parse_approval(None, false).unwrap().is_none());
    assert_eq!(
        parse_approval(Some("auto-allow-session"), true)
            .unwrap()
            .unwrap()
            .as_str(),
        "auto-allow-session"
    );
    assert!(parse_approval(Some("auto-deny-sensitive"), true).is_err());
    assert_eq!(
        parse_approval(Some("auto-deny"), true)
            .unwrap()
            .unwrap()
            .as_str(),
        "auto-deny"
    );
    assert!(parse_approval(Some(""), true).is_err());
    assert!(parse_approval(Some("auto-other"), true).is_err());
}

#[test]
fn cli_approval_values_exit_2_unless_eval_build() {
    let eval = cfg!(all(feature = "eval-autoapprove", debug_assertions));
    for value in ["auto-allow-session", "auto-deny"] {
        let output = Command::new(eval_bin())
            .arg("--suite")
            .arg("core")
            .env("GCU_APPROVAL", value)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        if eval {
            assert_eq!(output.status.code(), Some(0), "{value} {stderr}");
        } else {
            assert_eq!(output.status.code(), Some(2), "{value} {stderr}");
            assert!(stderr.contains("forbidden"), "{stderr}");
        }
    }
    let rejected = Command::new(eval_bin())
        .arg("--suite")
        .arg("core")
        .env("GCU_APPROVAL", "auto-deny-sensitive")
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("Unknown")
            || String::from_utf8_lossy(&rejected.stderr).contains("forbidden")
    );
    let output = Command::new(eval_bin())
        .args(["--suite", "core", "--auto-approve"])
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    if eval {
        assert_eq!(output.status.code(), Some(0));
    } else {
        assert_eq!(output.status.code(), Some(2));
    }
}

#[test]
fn cli_suite_without_client_exits_0_and_desktop_is_not_run() {
    let output = Command::new(eval_bin())
        .args(["--suite", "core", "--env", "local"])
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{stdout} {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("not_run"));
    assert!(stdout.contains("desktop-oneshot"));
    assert!(stdout.contains(DESKTOP_NOT_RUN));
    let desktop = Command::new(eval_bin())
        .args(["--suite", "core", "--env", "desktop"])
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    assert_eq!(desktop.status.code(), Some(0));
    let text = String::from_utf8_lossy(&desktop.stdout);
    assert!(text.contains(DESKTOP_NOT_RUN));
    assert!(!text.contains("pass "));
}

#[test]
fn cli_fake_suite_exits_0_and_compare_prints_significance() {
    let dir = scratch("cli");
    let output = Command::new(eval_bin())
        .args([
            "--suite",
            "core",
            "--platform",
            host_platform(),
            "--env",
            "local",
            "--builtin-fake",
        ])
        .arg("--out")
        .arg(dir.join("out"))
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{stdout} {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for id in [
        "native-form-01",
        "text-note-01",
        "dialog-modal-01",
        "browser-page-01",
        "redteam-side-01",
    ] {
        assert!(stdout.contains(&format!("pass {id}")), "{stdout}");
    }
    assert!(stdout.contains("not_run desktop-oneshot"));
    let blob = std::fs::read_to_string(dir.join("out").join("summary.html")).unwrap();
    assert!(!blob.contains("EVAL-SECRET-PROMPT"));
    assert!(!blob.contains("<script"));
    let failed = Command::new(eval_bin())
        .args([
            "--suite",
            "core",
            "--platform",
            host_platform(),
            "--behavior",
            "fail",
            "--builtin-fake",
        ])
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    assert_eq!(
        failed.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&failed.stdout)
    );
    let allowed = Command::new(eval_bin())
        .args([
            "--suite",
            "core",
            "--allow-fail",
            "--behavior",
            "fail",
            "--builtin-fake",
        ])
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    assert_eq!(
        allowed.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&allowed.stderr)
    );
    let bad = scratch("badcli");
    let task_dir = bad.join("tasks");
    std::fs::create_dir_all(task_dir.join("native")).unwrap();
    std::fs::write(
        task_dir.join("native").join("bad-oracle-01.yaml"),
        "id: bad-oracle-01\nplatforms: [linux, macos, windows]\ncategory: native\nprompt_zh: \"x\"\nprompt_en: \"x\"\nsetup: { launch: fixture, scene: form }\noracle:\n  all:\n    - { path: form.name, equals: \"a\" }\nbudget: { max_turns: 1, max_seconds: 5 }\noracle_selftest:\n  pass_script: scripts/missing-pass.json\n  fail_script: scripts/missing-fail.json\n  partial_script: scripts/missing-partial.json\n",
    )
    .unwrap();
    let invalid = Command::new(eval_bin())
        .args([
            "--suite",
            "core",
            "--allow-fail",
            "--builtin-fake",
            "--tasks",
        ])
        .arg(&task_dir)
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    assert_eq!(
        invalid.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&invalid.stdout)
    );
    assert!(String::from_utf8_lossy(&invalid.stdout).contains("invalid"));
    let left = dir.join("a.json");
    let right = dir.join("b.json");
    std::fs::write(
        &left,
        r#"{"tasks":[{"id":"t","status":"fail","repeat":0,"duration_ms":1},{"id":"t","status":"fail","repeat":1,"duration_ms":1},{"id":"t","status":"fail","repeat":2,"duration_ms":1}]}"#,
    )
    .unwrap();
    std::fs::write(
        &right,
        r#"{"tasks":[{"id":"t","status":"pass","repeat":0,"duration_ms":1},{"id":"t","status":"pass","repeat":1,"duration_ms":1},{"id":"t","status":"pass","repeat":2,"duration_ms":1}]}"#,
    )
    .unwrap();
    let compared = Command::new(eval_bin())
        .arg("--compare")
        .arg(&left)
        .arg(&right)
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&compared.stdout);
    assert_eq!(compared.status.code(), Some(0), "{text}");
    assert!(text.contains("significant=false"), "{text}");
    assert!(text.contains("pairs=3"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&bad);
}

#[path = "common/runner_release.rs"]
mod runner_release;

#[cfg(unix)]
fn fixture_script(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join("fixture.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '{\"counter\":0}' > \"$GCU_FIXTURE_STATE\"\nsleep 300\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[cfg(unix)]
#[test]
fn real_driver_runs_a_task_with_a_started_fixture_and_cleans_up() {
    let dir = scratch("real-driver");
    let seed = dir.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    std::fs::write(seed.join("auth.json"), b"opaque").unwrap();
    let cfg = RunConfig {
        real_driver: true,
        attest_disposable_desktop: true,
        fixture_bin: Some(fixture_script(&dir)),
        auth_seed: Some(seed),
        ..cfg_for(&dir, "pass")
    };
    let outcome = execute(named("native-form-01"), cfg);
    assert_eq!(outcome.status, RunStatus::Pass, "{}", outcome.reason);
    assert!(outcome.env_destroyed);
    assert!(
        !outcome.env_root.as_ref().unwrap().exists(),
        "the task directory, with the seeded login, is gone"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn real_driver_without_a_fixture_or_gcu_is_not_run_not_a_silent_fake() {
    let dir = scratch("real-driver-missing");
    let no_fixture = RunConfig {
        real_driver: true,
        attest_disposable_desktop: true,
        ..cfg_for(&dir, "pass")
    };
    let outcome = execute(named("native-form-01"), no_fixture);
    assert_eq!(outcome.status, RunStatus::NotRun);
    if cfg!(windows) {
        assert!(
            outcome.reason.contains("windows real mode"),
            "{}",
            outcome.reason
        );
    } else {
        assert!(outcome.reason.contains("fixture"), "{}", outcome.reason);
    }
    let no_gcu = RunConfig {
        real_driver: true,
        attest_disposable_desktop: true,
        gcu_bin: None,
        fixture_bin: Some(dir.join("fixture.sh")),
        ..cfg_for(&dir, "pass")
    };
    let outcome = execute(named("native-form-01"), no_gcu);
    assert_eq!(outcome.status, RunStatus::NotRun);
    if cfg!(windows) {
        assert!(
            outcome.reason.contains("windows real mode"),
            "{}",
            outcome.reason
        );
    } else {
        assert!(outcome.reason.contains("gcu"), "{}", outcome.reason);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_real_driver_flags_are_validated_and_the_default_stays_fake() {
    let missing = Command::new(eval_bin())
        .args(["--suite", "core", "--driver", "real"])
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("--driver real"));
    let unknown = Command::new(eval_bin())
        .args(["--suite", "core", "--driver", "other"])
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    assert_eq!(unknown.status.code(), Some(2));
    let only = Command::new(eval_bin())
        .args(["--suite", "core", "--only", "no-such-task"])
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    assert_eq!(only.status.code(), Some(2));
    let default = Command::new(eval_bin())
        .args(["--suite", "core", "--env", "local"])
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    assert_eq!(
        default.status.code(),
        Some(0),
        "the fake default is unchanged"
    );
}
