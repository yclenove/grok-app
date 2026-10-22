//! Real-mode preflight success. The allow-list is a JSON tools array.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/client_tools.rs"]
mod client_tools;

#[test]
fn preflight_success_dispatches_the_task() {
    // Tripwire set: discovery entry list_apps/list_windows plus the three client built-ins.
    // click and get_app_state are real gcu tools and are not part of this set.
    let listing = r#"{"tools":["list_apps","list_windows","search_tool","todo_write","use_tool"]}"#;
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
    assert!(lines[0].contains("--model"), "{}", lines[0]);
    assert!(lines[0].contains("--no-plan"), "{}", lines[0]);
    assert!(lines[0].contains("--verbatim"), "{}", lines[0]);
    assert!(lines[1].contains("--tools todo_write"), "{}", lines[1]);
    assert!(lines[1].contains("Zhang San"), "{}", lines[1]);
    assert!(lines[2].contains("--tools todo_write"), "{}", lines[2]);
    assert!(lines[2].contains("Zhang San"), "{}", lines[2]);
    let _ = std::fs::remove_dir_all(&drive.dir);
}
