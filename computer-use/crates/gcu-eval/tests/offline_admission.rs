//! Relocated production runner: neither executable slot can acquire trust from placement.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_eval::{
    ab::{self, Provenance, ReportId},
    bundled_tasks_dir,
    disposable::Refusal,
    host_platform, load_tasks, run_tasks, write_reports, RunConfig, RunStatus, Task,
    TelemetryState,
};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, ExitCode, Stdio},
    time::{Duration, Instant},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        // Unique within the process even when the clock repeats (coarse on macOS).
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "gcu-s6-r3-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = self.0.kill();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if !matches!(self.0.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        eprintln!("test child did not exit within cleanup budget");
    }
}

fn bounded_success(command: &mut Command, root: &Path) {
    let stderr = root.join("child-stderr");
    let mut child = OwnedChild(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&stderr).unwrap())
            .spawn()
            .unwrap(),
    );
    command.stderr(Stdio::null()); // Close the parent's Windows file handle.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(
                status.success(),
                "{}",
                std::fs::read_to_string(stderr).unwrap()
            );
            break;
        }
        assert!(Instant::now() < deadline, "relocated caller did not exit");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn executable_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

fn sentinel(path: &Path, marker: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Absolute path and shell built-ins reproduce the review's no-PATH schedule.
        let quoted = marker.to_str().unwrap().replace('\'', "'\\''");
        std::fs::write(
            path,
            format!("#!/bin/sh\nprintf started > '{quoted}'\nexit 0\n"),
        )
        .unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(not(unix))]
    {
        let _ = marker;
        // The same harness has a harmless executable-sentinel entry on Windows.
        std::fs::copy(std::env::current_exe().unwrap(), path).unwrap();
    }
}

fn tasks() -> [Task; 2] {
    let first = load_tasks(&bundled_tasks_dir().unwrap())
        .unwrap()
        .into_iter()
        .find(|task| task.id == "native-form-01")
        .unwrap();
    let mut later = first.clone();
    later.id = "must-halt-before-later-oracle".into();
    later.pass_script = PathBuf::from("missing-later-oracle");
    [first, later]
}

fn config(root: &Path) -> RunConfig {
    RunConfig {
        builtin_fake: true,
        platform: host_platform().into(),
        behavior: Some("scripted".into()),
        include_desktop: false,
        probe: Some(root.join("client-probe")),
        timeout: Some(Duration::from_secs(1)),
        ..RunConfig::default()
    }
}

async fn relocated_checks(root: &Path) {
    let tasks = tasks();
    let marker = root.join("external-started");
    let id = ReportId {
        platform: host_platform().into(),
        model: "fake".into(),
        date: "1970-01-01".into(),
        effort: "same".into(),
        machine: "offline".into(),
    };
    let positive = run_tasks(std::slice::from_ref(&tasks[0]), &config(root)).await;
    let row = &positive.tasks[0];
    assert_eq!(row.status, RunStatus::Pass, "{}", row.reason);
    assert!(row.assurance.client_started && row.env_destroyed);
    assert_eq!(row.assurance.tool_telemetry, TelemetryState::Available);
    assert!(row.assurance.environment.is_none());
    assert!(!row.env_root.as_ref().unwrap().exists());
    assert!(!marker.exists());
    std::fs::remove_file(root.join("client-probe")).unwrap();

    // An invocation-local working directory must preserve the old child cwd
    // without changing this process's cwd or touching a relative host sentinel.
    let relative = PathBuf::from("relative-watch");
    std::fs::write(root.join(&relative), b"original").unwrap();
    let cfg = RunConfig {
        behavior: Some("side_effect".into()),
        watch_files: vec![relative.clone()],
        ..config(root)
    };
    let side_effect = run_tasks(std::slice::from_ref(&tasks[0]), &cfg).await;
    assert_eq!(side_effect.tasks[0].status, RunStatus::SideEffect);
    assert_eq!(std::fs::read(root.join(relative)).unwrap(), b"original");
    std::fs::remove_file(root.join("client-probe")).unwrap();

    for (client, stem) in [
        (true, "eval-fake-agent"),
        (false, "eval-fake-gcu"),
        (false, "gcu-eval"),
    ] {
        let external = root.join(executable_name(stem));
        assert!(external.is_file());
        for (declared, builtin_fake) in [(false, true), (true, true), (true, false)] {
            let cfg = RunConfig {
                builtin_fake,
                grok_bin: client.then(|| external.clone()),
                gcu_bin: (!client).then(|| external.clone()),
                builtin_gcu: declared,
                attest_disposable_desktop: declared,
                // No credential or fixture may be consulted before refusal.
                auth_seed: Some(root.join("missing-auth")),
                fixture_bin: Some(root.join("missing-fixture")),
                ..config(root)
            };
            let suite = run_tasks(&tasks, &cfg).await;
            assert_eq!(suite.tasks.len(), 2);
            assert_eq!(suite.summary.scored, 0);
            for (index, row) in suite.tasks.iter().enumerate() {
                assert_eq!(row.status, RunStatus::NotRun, "{}", row.reason);
                assert!(row.reason.contains("provider_unavailable"));
                assert!(!row.assurance.client_started && !row.assurance.cleanup_failed);
                assert!(row.env_root.is_none() && row.env_destroyed);
                assert_eq!(row.assurance.tool_telemetry, TelemetryState::Unknown);
                assert_eq!(row.assurance.side_telemetry, TelemetryState::Unknown);
                assert_eq!(row.assurance.state_telemetry, TelemetryState::Unknown);
                assert_eq!(row.assurance.transcript_telemetry, TelemetryState::Unknown);
                if index == 0 {
                    let gate = row.assurance.environment.as_ref().unwrap();
                    assert_eq!(gate.refusal, Some(Refusal::ProviderUnavailable));
                    assert!(!gate.complete());
                    assert!(row.assurance.inherited_stop.is_none());
                } else {
                    assert!(row.assurance.environment.is_none());
                    let stop = row.assurance.inherited_stop.as_ref().unwrap();
                    assert_eq!(stop.task_id, tasks[0].id);
                    assert_eq!(stop.environment_refusal, Some(Refusal::ProviderUnavailable));
                }
            }
            write_reports(root, &suite).unwrap();
            let json: serde_json::Value =
                serde_json::from_slice(&std::fs::read(root.join("summary.json")).unwrap()).unwrap();
            for row in json["tasks"].as_array().unwrap() {
                assert_eq!(row["status"], "not_run");
                assert_eq!(row["client_started"], false);
                assert!(row["tool_calls"].is_null() && row["unknown_calls"].is_null());
            }
            let report = ab::run_ab(&tasks, &cfg, &id).await;
            assert_eq!(report.provenance, Provenance::NotRun);
            assert_eq!(report.trials.len(), tasks.len() * 2 * ab::REPEATS as usize);
            for (index, trial) in report.trials.iter().enumerate() {
                assert_eq!(trial.status, RunStatus::NotRun);
                assert!(trial.rounds.is_none() && trial.tokens_in.is_none());
                if index == 0 {
                    assert!(trial.environment_contract.is_some());
                } else {
                    assert!(trial.environment_contract.is_none());
                    let stop = trial.inherited_stop.as_ref().unwrap();
                    assert_eq!(stop.task_id, tasks[0].id);
                    assert_eq!(stop.group, "A");
                    assert_eq!(stop.repeat, 0);
                    assert_eq!(stop.environment_refusal, Some(Refusal::ProviderUnavailable));
                }
            }
            let paths = ab::write_reports(root, &report).unwrap();
            assert_eq!(
                ab::parse_report(&std::fs::read(paths.json).unwrap()).unwrap(),
                report
            );
            assert!(!marker.exists(), "forged {stem} started");
            assert!(!root.join("client-probe").exists());
        }
    }
}

fn relocated_cli_checks(root: &Path) {
    let cli = root.join(executable_name("relocated-cli"));
    std::fs::copy(env!("CARGO_BIN_EXE_gcu-eval"), &cli).unwrap();
    let output = root.join("cli-reports");
    for (slot, stem) in [("--grok", "eval-fake-agent"), ("--gcu", "eval-fake-gcu")] {
        let mut command = Command::new(&cli);
        command
            .args([
                "--suite",
                "core",
                "--env",
                "local",
                "--only",
                "native-form-01,text-note-01",
            ])
            // `--builtin-fake` conflicts with `--grok`; the client slot is external on its own.
            .args((slot == "--gcu").then_some("--builtin-fake"))
            .arg(slot)
            .arg(root.join(executable_name(stem)))
            .arg("--out")
            .arg(&output)
            .env_remove("GCU_APPROVAL");
        bounded_success(&mut command, root);
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output.join("summary.json")).unwrap()).unwrap();
        let rows: Vec<_> = report["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["id"] != "desktop-oneshot")
            .collect();
        assert_eq!(rows.len(), 2);
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(row["status"], "not_run");
            assert_eq!(row["client_started"], false);
            assert!(row["tool_calls"].is_null());
            if index == 0 {
                assert_eq!(
                    row["environment_contract"]["refusal"],
                    "provider_unavailable"
                );
            } else {
                assert!(row["environment_contract"].is_null());
                assert_eq!(
                    row["inherited_stop"]["environment_refusal"],
                    "provider_unavailable"
                );
            }
        }
        assert!(!root.join("external-started").exists());
    }
}

fn main() -> ExitCode {
    let exe = std::env::current_exe().unwrap();
    let root = exe.parent().unwrap();
    if ["eval-fake-agent", "eval-fake-gcu", "gcu-eval"]
        .iter()
        .any(|stem| exe.file_name().unwrap() == std::ffi::OsStr::new(&executable_name(stem)))
    {
        // Test-only sentinel entry, used instead of a shell wrapper on Windows.
        std::fs::write(root.join("external-started"), b"started").unwrap();
    } else if std::env::args().nth(1).as_deref() == Some("--relocated") {
        gcu_eval::block_on(async {
            tokio::time::timeout(Duration::from_secs(10), relocated_checks(root))
                .await
                .unwrap();
        })
        .unwrap();
    } else {
        let scratch = Scratch::new();
        let relocated = scratch.0.join(executable_name("relocated-runner"));
        std::fs::copy(exe, &relocated).unwrap();
        let marker = scratch.0.join("external-started");
        for stem in ["eval-fake-agent", "eval-fake-gcu", "gcu-eval"] {
            let path = scratch.0.join(executable_name(stem));
            sentinel(&path, &marker);
            // Each sentinel must be executable and observable before testing refusal.
            bounded_success(&mut Command::new(path), &scratch.0);
            assert!(marker.exists());
            std::fs::remove_file(&marker).unwrap();
        }
        bounded_success(
            Command::new(relocated)
                .arg("--relocated")
                .current_dir(&scratch.0),
            &scratch.0,
        );
        assert!(!marker.exists());
        relocated_cli_checks(&scratch.0);
    }
    ExitCode::SUCCESS
}
