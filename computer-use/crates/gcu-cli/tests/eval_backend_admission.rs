//! Round-2 S6: actual run_tasks/run_ab with the genuine S3 executable available.
//! Fake client and backend use the production in-process implementations.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_eval::{
    ab::{self, Provenance, ReportId},
    bundled_tasks_dir,
    disposable::Refusal,
    host_platform, load_tasks, run_tasks, write_reports, RunConfig, RunStatus, Task,
    TelemetryState,
};
use gcu_host_protocol::scope::Scope;
use std::{path::PathBuf, process::ExitCode, time::Duration};

fn empty_scope() -> Scope {
    Scope {
        engine_instance: "offline-engine".into(),
        desktop_session: "offline-desktop".into(),
        control_lease: "private".into(),
        targets: vec![],
    }
}

fn tasks() -> [Task; 2] {
    let first = load_tasks(&bundled_tasks_dir().unwrap())
        .unwrap()
        .into_iter()
        .find(|task| task.id == "native-form-01")
        .unwrap();
    let mut later = first.clone();
    later.id = "must-not-start-or-validate-later-task".into();
    // Proves halt occurs before even the next task's self-test.
    later.pass_script = PathBuf::from("missing-later-oracle");
    [first, later]
}

fn config() -> RunConfig {
    RunConfig {
        builtin_fake: true,
        builtin_gcu: true,
        platform: host_platform().into(),
        behavior: Some("scripted".into()),
        timeout: Some(Duration::from_secs(3)),
        include_desktop: false,
        run_local: true,
        ..RunConfig::default()
    }
}

async fn check() {
    let engine = PathBuf::from(env!("CARGO_BIN_EXE_gcu"));
    assert!(engine.is_file(), "test must have the actual S3 executable");
    let tasks = tasks();
    let root = tempfile::tempdir().unwrap();
    let id = ReportId {
        platform: host_platform().into(),
        model: "fake".into(),
        date: "1970-01-01".into(),
        effort: "same".into(),
        machine: "offline".into(),
    };

    // Positive production control: no backend path means the in-process fake.
    let positive = run_tasks(std::slice::from_ref(&tasks[0]), &config()).await;
    let row = &positive.tasks[0];
    assert_eq!(row.status, RunStatus::Pass, "{}", row.reason);
    assert!(row.assurance.client_started && row.env_destroyed);
    assert_eq!(row.assurance.tool_telemetry, TelemetryState::Available);
    assert!(row.assurance.environment.is_none());
    assert!(!row.env_root.as_ref().unwrap().exists());

    for behavior in ["crash", "scripted"] {
        for (backend, scoped) in [
            (Some(engine.clone()), false),
            (Some(engine.clone()), true),
            (None, true),
        ] {
            for declared in [false, true] {
                let probe = root.path().join("client-started.json");
                let cfg = RunConfig {
                    gcu_bin: backend.clone(),
                    fake_scope: scoped.then(empty_scope),
                    // The original schedule uses true, though it is no evidence.
                    attest_disposable_desktop: declared,
                    builtin_gcu: declared,
                    behavior: Some(behavior.into()),
                    probe: Some(probe.clone()),
                    ..config()
                };
                let report = run_tasks(&tasks, &cfg).await;
                assert_eq!(report.summary.scored, 0);
                for (index, row) in report.tasks.iter().enumerate() {
                    assert_eq!(row.status, RunStatus::NotRun, "{}", row.reason);
                    assert!(!row.assurance.client_started);
                    assert!(!row.assurance.cleanup_failed);
                    // No allocation means start_scope never had a run root or ACK.
                    assert!(row.env_root.is_none());
                    assert!(row.env_destroyed);
                    assert_eq!(row.assurance.tool_telemetry, TelemetryState::Unknown);
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
                assert!(!probe.exists());
                write_reports(root.path(), &report).unwrap();
                let json: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(root.path().join("summary.json")).unwrap(),
                )
                .unwrap();
                for row in json["tasks"].as_array().unwrap() {
                    assert_eq!(row["status"], "not_run");
                    assert_eq!(row["client_started"], false);
                    assert!(row["tool_calls"].is_null() && row["unknown_calls"].is_null());
                }
                let ab = ab::run_ab(&tasks, &cfg, &id).await;
                assert_eq!(ab.provenance, Provenance::NotRun);
                assert_eq!(ab.trials.len(), tasks.len() * 2 * ab::REPEATS as usize);
                for (index, trial) in ab.trials.iter().enumerate() {
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
                let paths = ab::write_reports(root.path(), &ab).unwrap();
                let written = ab::parse_report(&std::fs::read(paths.json).unwrap()).unwrap();
                assert_eq!(written, ab);
                assert!(!probe.exists());
            }
        }
    }
}

fn main() -> ExitCode {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(30), check())
                .await
                .unwrap();
        });
    ExitCode::SUCCESS
}
