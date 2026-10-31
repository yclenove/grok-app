//! A valid tool listing cannot bypass the disposable environment gate.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/client_tools.rs"]
mod client_tools;

#[test]
fn valid_tool_listing_cannot_bypass_missing_environment_provider() {
    // Tripwire set: discovery entry list_apps/list_windows plus the three client built-ins.
    // click and get_app_state are real gcu tools and are not part of this set.
    let listing = r#"{"tools":["list_apps","list_windows","search_tool","todo_write","use_tool"]}"#;
    let drive = client_tools::drive("pass", listing);
    assert_eq!(drive.rows.len(), 2, "both tasks are stopped");
    for row in &drive.rows {
        assert_eq!(row.status, gcu_eval::RunStatus::NotRun);
        assert!(row.reason.contains("provider_unavailable"));
    }
    assert!(
        !drive.marker.exists(),
        "task was dispatched without a provider"
    );
    assert!(
        client_tools::log_text(&drive).is_empty(),
        "tool preflight was started"
    );
    let _ = std::fs::remove_dir_all(&drive.dir);
}
