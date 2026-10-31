//! Negative checks for filename exemptions, external fake backends, and cleanup.
//! These tests use local scripts or the built-in fake agent. They do not call a model.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_eval::{
    block_on, bundled_tasks_dir, host_platform, load_tasks, run_tasks, RunConfig, RunStatus, Task,
};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "gcu-ev-r3-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn bin(name: &str) -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.file_name().and_then(|part| part.to_str()) == Some("deps") {
        path.pop();
    }
    path.push(name);
    path
}

fn agent() -> PathBuf {
    bin("eval-fake-agent")
}

fn gcu() -> PathBuf {
    bin("eval-fake-gcu")
}

fn named(id: &str) -> Task {
    load_tasks(&bundled_tasks_dir().unwrap())
        .unwrap()
        .into_iter()
        .find(|task| task.id == id)
        .unwrap()
}

fn executable(path: &Path) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn touch_script(path: &Path, marker: &Path, log: Option<&Path>) {
    let log_line = match log {
        Some(log) => format!("printf '%s\\n' \"$*\" >> '{}'\n", log.display()),
        None => String::new(),
    };
    let body = format!(
        "#!/bin/sh\n{log_line}touch '{}'\nexit 0\n",
        marker.display()
    );
    std::fs::write(path, body).unwrap();
    executable(path);
}

fn external_cfg(path: PathBuf) -> RunConfig {
    RunConfig {
        grok_bin: Some(path),
        gcu_bin: Some(gcu()),
        platform: host_platform().into(),
        include_desktop: false,
        timeout: Some(Duration::from_secs(3)),
        builtin_fake: false,
        ..RunConfig::default()
    }
}

#[test]
fn same_name_wrapper_copy_and_hard_link_still_need_attestation() {
    let dir = scratch("alias");
    let marker = dir.join("started");
    let wrapper = dir.join("eval-fake-agent");
    touch_script(&wrapper, &marker, None);
    let copy = dir.join("copy").join("eval-fake-agent");
    std::fs::create_dir(dir.join("copy")).unwrap();
    std::fs::copy(&wrapper, &copy).unwrap();
    executable(&copy);
    let hard = dir.join("link").join("eval-fake-agent");
    std::fs::create_dir(dir.join("link")).unwrap();
    std::fs::hard_link(&wrapper, &hard).unwrap();
    for path in [&wrapper, &copy, &hard] {
        let _ = std::fs::remove_file(&marker);
        let report = block_on(run_tasks(
            &[named("native-form-01")],
            &external_cfg(path.clone()),
        ))
        .unwrap();
        let row = &report.tasks[0];
        assert_eq!(
            row.status,
            RunStatus::NotRun,
            "{} {}",
            path.display(),
            row.reason
        );
        assert!(
            row.reason.contains("attest-disposable-desktop"),
            "{} {}",
            path.display(),
            row.reason
        );
        assert!(!marker.exists(), "started {}", path.display());
    }
    let _ = std::fs::remove_file(&marker);
    let output = Command::new(bin("gcu-eval"))
        .args([
            "--suite",
            "core",
            "--only",
            "native-form-01",
            "--env",
            "local",
        ])
        .arg("--grok")
        .arg(&wrapper)
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("attest-disposable-desktop"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!marker.exists(), "the CLI started the same-named wrapper");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn builtin_fake_does_not_start_an_external_gcu_without_attestation() {
    let dir = scratch("ext-gcu");
    let marker = dir.join("gcu-started");
    let wrap = dir.join("gcu.sh");
    touch_script(&wrap, &marker, None);
    let cfg = RunConfig {
        builtin_fake: true,
        builtin_fake_bin: Some(agent()),
        builtin_gcu: false,
        gcu_bin: Some(wrap),
        platform: host_platform().into(),
        behavior: Some("scripted".into()),
        include_desktop: false,
        timeout: Some(Duration::from_secs(3)),
        ..RunConfig::default()
    };
    let report = block_on(run_tasks(&[named("native-form-01")], &cfg)).unwrap();
    let row = &report.tasks[0];
    assert_eq!(row.status, RunStatus::NotRun, "{}", row.reason);
    assert!(
        row.reason.contains("provider_unavailable"),
        "{}",
        row.reason
    );
    assert!(!marker.exists(), "the external gcu was started");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn both_fake_agent_entries_refuse_external_backends_despite_forged_markers() {
    let dir = scratch("direct-gcu");
    let marker = dir.join("gcu-started");
    let wrap = dir.join("gcu.sh");
    touch_script(&wrap, &marker, None);
    for hidden in [false, true] {
        for flags in 0..8 {
            let mut command = Command::new(if hidden { bin("gcu-eval") } else { agent() });
            if hidden {
                command.arg(gcu_eval::INTERNAL_FAKE_AGENT);
            }
            let output = command
                .env_clear()
                .env("GCU_BIN", &wrap)
                .env("GCU_EVAL_ATTESTED", if flags & 1 != 0 { "1" } else { "0" })
                .env(
                    "GCU_EVAL_BUILTIN_GCU",
                    if flags & 2 != 0 { "1" } else { "0" },
                )
                .env(
                    "GCU_EVAL_GCU_REEXEC",
                    if flags & 4 != 0 { "1" } else { "0" },
                )
                // Crash reaches backend admission without fixture files.
                .env("GCU_EVAL_BEHAVIOR", "crash")
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(1),
                "hidden={hidden} flags={flags}"
            );
            assert!(String::from_utf8_lossy(&output.stderr).contains("provider_unavailable"));
            assert!(
                !marker.exists(),
                "external backend ran: hidden={hidden} flags={flags}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(debug_assertions)]
#[test]
fn cleanup_failure_does_not_start_the_next_client() {
    let dir = scratch("halt");
    gcu_eval::testing_fail_next_removal();
    let probe = dir.join("probe.json");
    let cfg = RunConfig {
        builtin_fake: true,
        builtin_fake_bin: Some(agent()),
        builtin_gcu: true,
        gcu_bin: Some(gcu()),
        platform: host_platform().into(),
        behavior: Some("scripted".into()),
        include_desktop: false,
        probe: Some(probe.clone()),
        timeout: Some(Duration::from_secs(8)),
        ..RunConfig::default()
    };
    let first = named("native-form-01");
    let second = named("text-note-01");
    let report = block_on(run_tasks(&[first.clone(), second.clone()], &cfg)).unwrap();
    assert_eq!(
        report.tasks[0].status,
        RunStatus::NotRun,
        "{}",
        report.tasks[0].reason
    );
    assert!(
        report.tasks[0].reason.contains("cleanup_failed"),
        "{}",
        report.tasks[0].reason
    );
    assert!(report.tasks[0].assurance.cleanup_failed);
    assert!(report.tasks[0].assurance.client_started);
    assert!(!report.tasks[0].env_destroyed);
    let root = report.tasks[0].env_root.as_ref().unwrap();
    assert!(root.exists(), "the failed cleanup removed the root");
    assert_eq!(report.tasks[1].status, RunStatus::NotRun);
    let skipped = &report.tasks[1];
    assert!(!skipped.assurance.client_started);
    assert!(!skipped.assurance.cleanup_failed);
    assert!(!skipped.assurance.capture_incomplete);
    assert!(skipped.assurance.environment.is_none());
    assert!(skipped.env_root.is_none());
    let stop = skipped.assurance.inherited_stop.as_ref().unwrap();
    assert_eq!(stop.task_id, first.id);
    assert!(stop.cleanup_failed);
    assert!(stop.environment_refusal.is_none());
    for state in [
        skipped.assurance.tool_telemetry,
        skipped.assurance.side_telemetry,
        skipped.assurance.state_telemetry,
        skipped.assurance.transcript_telemetry,
    ] {
        assert_eq!(state, gcu_eval::TelemetryState::Unknown);
    }
    // Positive control: the preceding task really read its own telemetry.
    assert_eq!(
        report.tasks[0].assurance.tool_telemetry,
        gcu_eval::TelemetryState::Available
    );
    assert_eq!(
        report.tasks[0].assurance.side_telemetry,
        gcu_eval::TelemetryState::Available
    );
    assert_eq!(
        report.tasks[0].assurance.state_telemetry,
        gcu_eval::TelemetryState::Available
    );
    assert_eq!(
        report.tasks[0].assurance.transcript_telemetry,
        gcu_eval::TelemetryState::Available
    );
    let reports = dir.join("reports");
    gcu_eval::write_reports(&reports, &report).unwrap();
    for path in [
        reports.join(format!("{}.json", second.id)),
        reports.join("summary.json"),
    ] {
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let row = if json["tasks"].is_array() {
            &json["tasks"][1]
        } else {
            &json
        };
        assert_eq!(row["status"], "not_run");
        assert_eq!(row["client_started"], false);
        assert!(row["tool_calls"].is_null());
        assert!(row["unknown_calls"].is_null());
        for key in [
            "tool_telemetry",
            "side_telemetry",
            "state_telemetry",
            "transcript_telemetry",
        ] {
            assert_eq!(row[key], "unknown");
        }
        assert!(row["environment_contract"].is_null());
        assert_eq!(row["cleanup_failed"], false);
        assert_eq!(row["inherited_stop"]["task_id"], first.id);
        assert_eq!(row["inherited_stop"]["cleanup_failed"], true);
    }
    assert!(
        report.tasks[1].reason.contains("remaining tasks"),
        "{}",
        report.tasks[1].reason
    );
    let text = std::fs::read_to_string(&probe).unwrap();
    assert!(text.contains(&first.prompt_en), "{text}");
    assert!(
        !text.contains(&second.prompt_en),
        "the next client started: {text}"
    );
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(debug_assertions)]
#[test]
fn populate_failure_reports_the_leftover_when_cleanup_also_fails() {
    let dir = scratch("popfail");
    gcu_eval::testing_fail_next_populate();
    gcu_eval::testing_fail_next_removal();
    let cfg = RunConfig {
        builtin_fake: true,
        builtin_fake_bin: Some(agent()),
        builtin_gcu: true,
        gcu_bin: Some(gcu()),
        platform: host_platform().into(),
        behavior: Some("scripted".into()),
        include_desktop: false,
        timeout: Some(Duration::from_secs(8)),
        ..RunConfig::default()
    };
    let report = block_on(run_tasks(&[named("native-form-01")], &cfg)).unwrap();
    let row = &report.tasks[0];
    assert_eq!(row.status, RunStatus::NotRun, "{}", row.reason);
    assert!(row.reason.contains("populate_failed"), "{}", row.reason);
    assert!(row.reason.contains("cleanup_failed"), "{}", row.reason);
    assert!(!row.env_destroyed);
    assert!(row.assurance.cleanup_failed);
    let root = row.env_root.as_ref().unwrap();
    assert!(root.join("leftover").is_file(), "{}", root.display());
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(&dir);
}
