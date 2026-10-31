#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::execute;
use crate::fake_worker::test_hook::{self, Pause};
use crate::{
    block_on, bundled_tasks_dir, load_tasks, run_tasks, RunConfig, RunStatus, Task, TelemetryState,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "gcu-unconfirmed-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn task(id: &str) -> Task {
    load_tasks(&bundled_tasks_dir().expect("bundled tasks"))
        .expect("load tasks")
        .into_iter()
        .find(|task| task.id == id)
        .unwrap_or_else(|| panic!("missing task {id}"))
}

fn within(limit: Duration, fut: impl std::future::Future<Output = ()>) {
    let result = block_on(async move {
        match tokio::time::timeout(limit, fut).await {
            Ok(()) => Ok(()),
            Err(_) => Err("timed out before the worker reached the probe pause"),
        }
    })
    .expect("runtime");
    result.expect("pause");
}

#[test]
fn execute_returns_unconfirmed_when_join_budget_expires_before_stop() {
    let _lock = test_hook::serialize();
    let mut pause = Pause::until_join_budget(Duration::ZERO);
    let dir = Scratch::new("execute");
    let probe = dir.path().join("probe.json");
    let state = dir.path().join("state.json");
    let pass = dir.path().join("pass.json");
    std::fs::write(&pass, br#"{"state":{"continued":true}}"#).unwrap();
    let mut env = BTreeMap::new();
    env.insert("GCU_EVAL_PROBE".into(), probe.display().to_string());
    env.insert("GCU_EVAL_BEHAVIOR".into(), "scripted".into());
    env.insert("GCU_FIXTURE_STATE".into(), state.display().to_string());
    env.insert("GCU_EVAL_PASS_SCRIPT".into(), pass.display().to_string());
    let args = vec!["--model".into(), "fake".into()];
    let work = dir.path().to_path_buf();
    let finished = block_on(async {
        match tokio::time::timeout(
            Duration::from_secs(10),
            execute(&args, &env, &work, Duration::from_secs(30)),
        )
        .await
        {
            Ok(finished) => finished,
            Err(_) => panic!("timed out before the worker reached the probe pause"),
        }
    })
    .expect("runtime");
    assert!(finished.worker_unconfirmed);
    assert!(finished.timed_out);
    assert!(finished.exit_code.is_none());
    assert!(!finished.wait_failed);
    assert!(finished.stdout.bytes.is_empty());
    assert_eq!(test_hook::parked_count(), 1);
    assert!(!probe.exists());
    assert!(test_hook::late_writes().is_empty());
    pause.release();
    test_hook::join_parked();
    assert_eq!(test_hook::parked_count(), 0);
    assert!(probe.exists(), "late probe write did not happen");
    assert!(test_hook::late_writes().iter().any(|path| path == &probe));
    assert!(
        !state.exists(),
        "worker continued past the next cancel check"
    );
    assert!(
        finished.worker_unconfirmed,
        "late write was adopted as a result"
    );
}

#[test]
fn unconfirmed_worker_is_not_run_and_halts_later_tasks() {
    let _lock = test_hook::serialize();
    let mut pause = Pause::until_join_budget(Duration::ZERO);
    let dir = Scratch::new("runner");
    let probe = dir.path().join("probe.json");
    let first = task("native-form-01");
    let second = task("text-note-01");
    let cfg = RunConfig {
        builtin_fake: true,
        builtin_gcu: true,
        gcu_bin: None,
        platform: crate::host_platform().into(),
        behavior: Some("scripted".into()),
        include_desktop: false,
        probe: Some(probe.clone()),
        timeout: Some(Duration::from_secs(30)),
        ..RunConfig::default()
    };
    let tasks = [first.clone(), second.clone()];
    let report = block_on(async {
        match tokio::time::timeout(Duration::from_secs(10), run_tasks(&tasks, &cfg)).await {
            Ok(report) => report,
            Err(_) => panic!("timed out before the worker reached the probe pause"),
        }
    })
    .expect("runtime");
    assert_eq!(report.tasks.len(), 2);
    let row = &report.tasks[0];
    assert_eq!(row.status, RunStatus::NotRun, "{}", row.reason);
    assert!(row.reason.contains("worker_unconfirmed"), "{}", row.reason);
    assert!(row.reason.contains("cleanup_failed"), "{}", row.reason);
    assert!(super::reason_is_unconfirmed(&row.reason), "{}", row.reason);
    assert!(row.assurance.cleanup_failed);
    assert!(!row.env_destroyed);
    assert!(row.assurance.client_started);
    assert!(!row.scored());
    assert_eq!(row.assurance.tool_telemetry, TelemetryState::Unknown);
    assert_eq!(row.assurance.state_telemetry, TelemetryState::Unknown);
    let root = row.env_root.clone().expect("root");
    assert!(root.exists(), "unconfirmed cleanup removed the root");
    assert_eq!(std::fs::read(root.join("state.json")).unwrap(), b"{}\n");
    let skipped = &report.tasks[1];
    assert_eq!(skipped.status, RunStatus::NotRun, "{}", skipped.reason);
    assert!(!skipped.assurance.client_started);
    assert!(skipped.env_root.is_none());
    let stop = skipped.assurance.inherited_stop.as_ref().unwrap();
    assert_eq!(stop.task_id, first.id);
    assert!(stop.cleanup_failed);
    assert!(
        skipped.reason.contains("remaining tasks"),
        "{}",
        skipped.reason
    );
    assert_eq!(report.summary.scored, 0);
    assert_eq!(report.summary.passed, 0);
    assert!(!probe.exists());
    assert!(test_hook::late_writes().is_empty());
    assert_eq!(test_hook::parked_count(), 1);
    pause.release();
    test_hook::join_parked();
    assert!(probe.exists(), "late probe write did not happen");
    let text = std::fs::read_to_string(&probe).unwrap();
    assert!(text.contains(&first.prompt_en), "{text}");
    assert!(
        !text.contains(&second.prompt_en),
        "the next client started: {text}"
    );
    assert!(test_hook::late_writes().iter().any(|path| path == &probe));
    assert_eq!(std::fs::read(root.join("state.json")).unwrap(), b"{}\n");
    assert_eq!(report.tasks[0].status, RunStatus::NotRun);
    assert!(!report.tasks[0].scored());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn dropping_execute_sets_cancel_flag() {
    let _lock = test_hook::serialize();
    let mut pause = Pause::until_dropped(Duration::ZERO);
    let dir = Scratch::new("drop");
    let probe = dir.path().join("probe.json");
    let state = dir.path().join("state.json");
    let pass = dir.path().join("pass.json");
    std::fs::write(&pass, br#"{"state":{"continued":true}}"#).unwrap();
    let mut env = BTreeMap::new();
    env.insert("GCU_EVAL_PROBE".into(), probe.display().to_string());
    env.insert("GCU_EVAL_BEHAVIOR".into(), "scripted".into());
    env.insert("GCU_FIXTURE_STATE".into(), state.display().to_string());
    env.insert("GCU_EVAL_PASS_SCRIPT".into(), pass.display().to_string());
    let args = vec!["--model".into(), "fake".into()];
    let work = dir.path().to_path_buf();
    within(Duration::from_secs(10), async move {
        let exec = execute(&args, &env, &work, Duration::from_secs(30));
        tokio::pin!(exec);
        tokio::select! {
            biased;
            result = exec.as_mut() => panic!(
                "execute finished before pause: unconfirmed={}",
                result.worker_unconfirmed
            ),
            _ = test_hook::paused() => {}
        }
    });
    assert!(
        test_hook::cancel_is_set(),
        "dropping execute did not set the cancel flag"
    );
    assert!(!probe.exists());
    assert_eq!(test_hook::parked_count(), 1);
    pause.release();
    test_hook::join_parked();
    assert!(probe.exists(), "the check-to-write window did not run");
    assert!(test_hook::late_writes().iter().any(|path| path == &probe));
    assert!(
        !state.exists(),
        "worker continued past the next cancel check"
    );
}
