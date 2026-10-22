//! Real-mode preflight success. This binary is separate from the failure test
//! so the process-wide `OnceLock` starts empty.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/client_tools.rs"]
mod client_tools;

#[test]
fn preflight_success_dispatches_the_task() {
    let listing = "todo_write, search_tool, use_tool, click, get_app_state";
    let drive = client_tools::drive("pass", listing);
    assert_eq!(drive.rows.len(), 2, "both tasks are dispatched");
    for row in &drive.rows {
        assert!(row.scored(), "{}", row.reason);
        assert!(!row.reason.contains("预检"), "{}", row.reason);
    }
    assert!(
        drive.marker.exists(),
        "the fake client was dispatched the task"
    );
    let log = client_tools::log_text(&drive);
    let lines: Vec<_> = log.lines().collect();
    assert_eq!(lines.len(), 3, "{log}");
    assert!(lines[0].contains("--tools todo_write"), "{}", lines[0]);
    assert!(lines[0].contains("--max-turns 1"), "{}", lines[0]);
    assert!(lines[1].contains("--tools todo_write"), "{}", lines[1]);
    assert!(lines[1].contains("Zhang San"), "{}", lines[1]);
    assert!(lines[2].contains("--tools todo_write"), "{}", lines[2]);
    assert!(lines[2].contains("Zhang San"), "{}", lines[2]);
    let _ = std::fs::remove_dir_all(&drive.dir);
}
