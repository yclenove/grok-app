//! macOS driver contract suite (07 §2 C01–C27 plus ADR 0006).
//!
//! Portable rows run on every host. The live table is `#[ignore]` and compiles
//! only for `target_os = "macos"`. GitHub's macOS runner has no TCC grant, so
//! that table stays out of the default `cargo test` path.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "contract/oracle.rs"]
mod oracle;
#[path = "contract/report.rs"]
mod report;
#[path = "contract/specials.rs"]
mod specials;
#[path = "contract/table.rs"]
mod table;

#[cfg(target_os = "macos")]
#[path = "contract/live.rs"]
mod live;

use report::NOT_RUN;
use table::CaseResult;

#[tokio::test]
async fn portable_matrix_passes_or_skips_and_archives_json() {
    let results = specials::run_portable_matrix().await;
    let failed: Vec<_> = results
        .iter()
        .filter(|row| row.outcome == "failed")
        .map(|row| format!("{}: {}", row.id, row.reason.as_deref().unwrap_or("")))
        .collect();
    assert!(failed.is_empty(), "{failed:?}");

    let path =
        std::env::temp_dir().join(format!("gcu-contract-portable-{}.json", std::process::id()));
    report::write_report(&path, &results, NOT_RUN).unwrap();
    let body: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(body["failed"], 0);
    assert!(body["passed"].as_u64().unwrap() > 0);
    assert!(body["skipped"].as_u64().unwrap() > 0);
    assert!(body["not_run"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("smoke-macos.sh")));
    report::archive(&results, NOT_RUN).unwrap();
}

#[test]
fn skip_reasons_match_capabilities_and_the_shared_table() {
    table::assert_shared_scope_text();
    table::assert_background_skip_matches_probe();
    table::assert_permission_skip_matches_flags();
    table::assert_overlay_skip_matches_capability();
}

#[test]
fn oracle_timeout_reports_the_last_state() {
    let path = std::env::temp_dir().join(format!("gcu-oracle-{}.json", std::process::id()));
    std::fs::write(&path, r#"{"text":"kept","counter":1}"#).unwrap();
    let error = oracle::poll_blocking(&path, std::time::Duration::from_millis(80), |state| {
        state
            .get("counter")
            .and_then(|value| value.as_u64())
            .filter(|n| *n == 9)
    });
    let _ = std::fs::remove_file(&path);
    let error = error.expect_err("predicate never matches");
    assert!(error.contains("kept"), "{error}");
    assert!(error.contains("counter"), "{error}");
}

#[test]
fn oracle_poll_returns_when_the_predicate_matches() {
    let path = std::env::temp_dir().join(format!("gcu-oracle-ok-{}.json", std::process::id()));
    std::fs::write(&path, r#"{"counter":2}"#).unwrap();
    let got = oracle::poll_blocking(&path, std::time::Duration::from_millis(200), |state| {
        state.get("counter").and_then(|value| value.as_u64())
    })
    .unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(got, 2);
}

#[test]
fn case_result_round_trip_keeps_skip_reason() {
    let row = CaseResult::skipped(
        "C21",
        "overlay",
        "capability: overlay_excluded_from_capture=false",
    );
    assert_eq!(row.outcome, "skipped");
    assert!(row
        .reason
        .unwrap()
        .contains("overlay_excluded_from_capture=false"));
}
