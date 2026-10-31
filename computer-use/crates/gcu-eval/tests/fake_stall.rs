//! Deadline and cleanup for an in-process fake that would block on a FIFO.
//!
//! The parent is only a process watchdog. It never calls the runner, so a
//! broken implementation cannot hang the test executor. The child runs the
//! production `run_tasks` entry.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CHILD_FLAG: &str = "GCU_EVAL_STALL_CHILD";

#[test]
fn stalled_in_process_fake_honours_deadline_and_cleans_up() {
    if std::env::var_os(CHILD_FLAG).is_none() {
        watchdog("stalled_in_process_fake_honours_deadline_and_cleans_up");
        return;
    }
    run_cases();
}

fn watchdog(test_name: &str) {
    let root = std::env::temp_dir().join(format!(
        "gcu-stall-watch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let stdout_path = root.join("stdout");
    let stderr_path = root.join("stderr");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg(test_name)
        .env(CHILD_FLAG, "1")
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&stdout_path).unwrap())
        .stderr(std::fs::File::create(&stderr_path).unwrap())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let stdout = std::fs::read_to_string(&stdout_path).unwrap_or_default();
                let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
                panic!("stall child exceeded 20s\n{stdout}\n{stderr}");
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(err) => panic!("wait failed: {err}"),
        }
    };
    let stdout = std::fs::read_to_string(&stdout_path).unwrap_or_default();
    let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&root);
    assert!(status.success(), "stall child failed\n{stdout}\n{stderr}");
    assert!(
        stdout.contains("1 passed"),
        "watchdog did not run the child case\n{stdout}\n{stderr}"
    );
}

fn run_cases() {
    use gcu_eval::{bundled_tasks_dir, load_tasks};
    let task = load_tasks(&bundled_tasks_dir().unwrap())
        .unwrap()
        .into_iter()
        .find(|task| task.id == "native-form-01")
        .unwrap();
    fifo_probe_returns_and_cleans_up(&task);
    timeout_behavior_stays_timeout(&task);
    regular_probe_is_written_and_survives_cleanup(&task);
}

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "gcu-stall-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn cfg(behavior: &str, timeout: Duration) -> gcu_eval::RunConfig {
    gcu_eval::RunConfig {
        builtin_fake: true,
        builtin_gcu: true,
        gcu_bin: None,
        grok_bin: None,
        platform: gcu_eval::host_platform().into(),
        behavior: Some(behavior.into()),
        include_desktop: false,
        timeout: Some(timeout),
        ..gcu_eval::RunConfig::default()
    }
}

fn fifo_probe_returns_and_cleans_up(task: &gcu_eval::Task) {
    use gcu_eval::{block_on, run_tasks, write_reports, RunStatus};
    let dir = scratch("fifo");
    let fifo = dir.join("probe.fifo");
    let made = Command::new("mkfifo").arg(&fifo).status().unwrap();
    assert!(made.success(), "mkfifo failed");
    let mut cfg = cfg("scripted", Duration::from_millis(50));
    cfg.probe = Some(fifo.clone());
    let started = Instant::now();
    let report = block_on(run_tasks(std::slice::from_ref(task), &cfg)).unwrap();
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "fifo probe blocked the runner for {elapsed:?}"
    );
    assert_eq!(report.tasks.len(), 1);
    let row = &report.tasks[0];
    assert_eq!(row.status, RunStatus::Crash, "{}", row.reason);
    assert!(row.assurance.client_started);
    assert!(row.env_destroyed);
    assert!(!row.assurance.cleanup_failed);
    let root = row.env_root.as_ref().unwrap();
    assert!(!root.exists(), "cleanup left {}", root.display());
    let reports = dir.join("reports");
    write_reports(&reports, &report).unwrap();
    let body = std::fs::read_to_string(reports.join("native-form-01.json")).unwrap();
    assert!(body.contains("\"env_destroyed\": true"), "{body}");
    assert!(body.contains("\"cleanup_failed\": false"), "{body}");
    assert_no_late_writer(&fifo);
    let _ = std::fs::remove_dir_all(&dir);
}

fn timeout_behavior_stays_timeout(task: &gcu_eval::Task) {
    use gcu_eval::{block_on, run_tasks, RunStatus};
    let dir = scratch("timeout");
    let cfg = cfg("timeout", Duration::from_millis(50));
    let started = Instant::now();
    let report = block_on(run_tasks(std::slice::from_ref(task), &cfg)).unwrap();
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "timeout behavior blocked the runner for {elapsed:?}"
    );
    let row = &report.tasks[0];
    assert_eq!(row.status, RunStatus::Timeout, "{}", row.reason);
    assert!(row.assurance.client_started && row.env_destroyed);
    assert!(!row.assurance.cleanup_failed);
    assert!(!row.env_root.as_ref().unwrap().exists());
    let _ = std::fs::remove_dir_all(&dir);
}

fn regular_probe_is_written_and_survives_cleanup(task: &gcu_eval::Task) {
    use gcu_eval::{block_on, run_tasks, RunStatus};
    let dir = scratch("probe");
    let probe = dir.join("probe.json");
    let mut cfg = cfg("scripted", Duration::from_secs(3));
    cfg.probe = Some(probe.clone());
    let report = block_on(run_tasks(std::slice::from_ref(task), &cfg)).unwrap();
    let row = &report.tasks[0];
    assert_eq!(row.status, RunStatus::Pass, "{}", row.reason);
    assert!(row.env_destroyed && !row.assurance.cleanup_failed);
    assert!(!row.env_root.as_ref().unwrap().exists());
    let text = std::fs::read_to_string(&probe).unwrap();
    assert!(text.contains("argv"), "{text}");
    assert!(probe.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Open flags for a nonblocking read. Values are the platform ABI constants.
#[cfg(target_os = "linux")]
const O_NONBLOCK: i32 = 0o4000;
#[cfg(target_os = "linux")]
const O_CLOEXEC: i32 = 0o2000000;
#[cfg(target_os = "macos")]
const O_NONBLOCK: i32 = 0x0004;
#[cfg(target_os = "macos")]
const O_CLOEXEC: i32 = 0x0100_0000;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("nonblocking fifo read flags are defined for linux and macos only");

fn assert_no_late_writer(fifo: &std::path::Path) {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    options.custom_flags(O_NONBLOCK | O_CLOEXEC);
    let mut file = options.open(fifo).unwrap();
    // The runner joins this worker before returning, so a completed write is
    // already in the pipe. There is no grace period to sleep for.
    let mut buf = [0u8; 64];
    match file.read(&mut buf) {
        Ok(0) => {}
        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
        other => panic!("probe wrote after the runner returned: {other:?}"),
    }
}
