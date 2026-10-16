//! macOS driver contract suite (07 §2 C01–C27 plus ADR 0006).
//!
//! Portable rows run on every host. The live table is ignored and compiles
//! only for `target_os = "macos"`. GitHub's macOS runner has no TCC grant, so
//! that table stays out of the default `cargo test` path.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "contract/fixture_tree.rs"]
mod fixture_tree;
#[path = "contract/harness.rs"]
mod harness;
#[path = "contract/oracle.rs"]
mod oracle;
#[path = "contract/readiness.rs"]
mod readiness;
#[path = "contract/readiness_tests.rs"]
mod readiness_tests;
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

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
}

fn main() -> Result<(), String> {
    use harness::Case;
    let cases = [
        Case {
            name: "portable_matrix_passes_or_skips_and_archives_json",
            ignored: false,
            run: || runtime().block_on(portable_matrix_passes_or_skips_and_archives_json()),
        },
        Case {
            name: "skip_reasons_match_capabilities_and_the_shared_table",
            ignored: false,
            run: skip_reasons_match_capabilities_and_the_shared_table,
        },
        Case {
            name: "oracle_timeout_reports_the_last_state",
            ignored: false,
            run: oracle_timeout_reports_the_last_state,
        },
        Case {
            name: "oracle_poll_returns_when_the_predicate_matches",
            ignored: false,
            run: oracle_poll_returns_when_the_predicate_matches,
        },
        Case {
            name: "case_result_round_trip_keeps_skip_reason",
            ignored: false,
            run: case_result_round_trip_keeps_skip_reason,
        },
        Case {
            name: "readiness_partial_then_complete_reports_activation_time",
            ignored: false,
            run: || {
                runtime().block_on(
                    readiness_tests::readiness_partial_then_complete_reports_activation_time(),
                )
            },
        },
        Case {
            name: "readiness_unmarked_partial_first_read_fails_adr0004",
            ignored: false,
            run: || {
                runtime().block_on(
                    readiness_tests::readiness_unmarked_partial_first_read_fails_adr0004(),
                )
            },
        },
        Case {
            name: "readiness_complete_controls_with_incomplete_never_pass",
            ignored: false,
            run: || {
                runtime().block_on(
                    readiness_tests::readiness_complete_controls_with_incomplete_never_pass(),
                )
            },
        },
        Case {
            name: "readiness_hung_read_cancels_within_setup_budget",
            ignored: false,
            run: || {
                runtime()
                    .block_on(readiness_tests::readiness_hung_read_cancels_within_setup_budget())
            },
        },
        Case {
            name: "canvas_label_collision_selects_grid_image_at_both_scales",
            ignored: false,
            run: readiness_tests::canvas_label_collision_selects_grid_image_at_both_scales,
        },
        Case {
            name: "grid_pixel_targets_exact_cell_inside_canvas_at_both_scales",
            ignored: false,
            run: readiness_tests::grid_pixel_targets_exact_cell_inside_canvas_at_both_scales,
        },
        #[cfg(target_os = "macos")]
        Case {
            name: "live::live_contract_table_suite",
            ignored: true,
            run: || {
                assert!(
                    gcu_driver_macos::input::ensure_appkit(),
                    "AppKit on main before native probe"
                );
                runtime().block_on(live::live_contract_table_suite());
            },
        },
    ];
    harness::run(&cases)
}

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

fn skip_reasons_match_capabilities_and_the_shared_table() {
    table::assert_shared_scope_text();
    table::assert_background_skip_matches_probe();
    table::assert_permission_skip_matches_flags();
    table::assert_overlay_skip_matches_capability();
}

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
