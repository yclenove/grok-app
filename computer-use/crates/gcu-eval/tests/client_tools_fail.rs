//! The environment gate precedes even a tool preflight that would refuse.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/client_tools.rs"]
mod client_tools;

use gcu_eval::RunStatus;

#[test]
fn environment_gate_precedes_a_forbidden_tool_listing() {
    let drive = client_tools::drive("fail", r#"{"tools":["SECRET","run_terminal_command"]}"#);
    assert_eq!(drive.rows.len(), 2, "both the task and the one after it");
    for row in &drive.rows {
        assert_eq!(row.status, RunStatus::NotRun, "{}", row.reason);
        assert!(row.reason.contains("provider_unavailable"));
        assert!(!row.reason.contains("SECRET"), "{}", row.reason);
    }
    assert!(!drive.marker.exists(), "the task prompt was not dispatched");
    assert!(
        client_tools::log_text(&drive).is_empty(),
        "tool preflight was started"
    );
    let _ = std::fs::remove_dir_all(&drive.dir);
}
