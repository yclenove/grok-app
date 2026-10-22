//! Real-mode preflight failure. This binary is separate from the pass test so
//! the process-wide `OnceLock` starts empty.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/client_tools.rs"]
mod client_tools;

use gcu_eval::RunStatus;

#[test]
fn preflight_failure_marks_the_task_not_run_and_does_not_dispatch() {
    let drive = client_tools::drive("fail", "SECRET run_terminal_command please");
    assert_eq!(drive.rows.len(), 2, "both the task and the one after it");
    for row in &drive.rows {
        assert_eq!(row.status, RunStatus::NotRun, "{}", row.reason);
        assert!(row.reason.contains("客户端工具预检失败"), "{}", row.reason);
        assert!(row.reason.contains("预检"), "{}", row.reason);
        assert!(
            row.reason.contains("run_terminal_command"),
            "{}",
            row.reason
        );
        assert!(!row.reason.contains("SECRET"), "{}", row.reason);
    }
    assert!(!drive.marker.exists(), "the task prompt was not dispatched");
    let log = client_tools::log_text(&drive);
    assert_eq!(log.lines().count(), 1, "{log}");
    assert!(log.contains("--tools todo_write"), "{log}");
    assert!(log.contains("--max-turns 1"), "{log}");
    assert!(!log.contains("Zhang San"), "{log}");
    let _ = std::fs::remove_dir_all(&drive.dir);
}
