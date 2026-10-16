//! Master contract test suite for Linux Wayland driver.
//! Covers C01–C27 and Wayland specials (token lifecycle, source scope, stop guard, redaction, surfaces).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

#[path = "wayland_contract/support.rs"]
mod support;

#[path = "wayland_contract/cases_portable.rs"]
mod cases_portable;

#[path = "wayland_contract/cases_live.rs"]
mod cases_live;

use cases_portable::*;
use support::*;

#[test]
fn test_w01_token_permissions_and_expiry_unit() {
    assert_eq!(
        test_w_token_permissions_and_expiry(),
        Ok(ContractOutcome::Passed)
    );
}

#[test]
fn test_w02_token_origin_binding_unit() {
    assert_eq!(test_w_token_origin_binding(), Ok(ContractOutcome::Passed));
}

#[test]
fn test_w03_token_revocation_and_cleanup_unit() {
    assert_eq!(
        test_w_token_revocation_and_cleanup(),
        Ok(ContractOutcome::Passed)
    );
}

#[tokio::test]
async fn test_w04_source_scope_window_only_unit() {
    assert_eq!(
        test_w_source_scope_window_only().await,
        Ok(ContractOutcome::Passed)
    );
}

#[tokio::test]
async fn test_w05_unknown_capture_bounds_rejected_adr0004_unit() {
    assert_eq!(
        test_w_unknown_capture_bounds_rejected_adr0004().await,
        Ok(ContractOutcome::Passed)
    );
}

#[tokio::test]
async fn test_w06_tree_incomplete_handoff_adr0004_unit() {
    assert_eq!(
        test_w_tree_incomplete_handoff_adr0004().await,
        Ok(ContractOutcome::Passed)
    );
}

#[test]
fn test_w07_stop_failure_disables_input_unit() {
    assert_eq!(
        test_w_stop_failure_disables_input(),
        Ok(ContractOutcome::Passed)
    );
}

#[tokio::test]
async fn test_w08_stop_budgets_and_release_all_input_unit() {
    assert_eq!(
        test_w_stop_budgets_and_release_all_input().await,
        Ok(ContractOutcome::Passed)
    );
}

#[test]
fn test_w09_stream_coordinate_mapping_unit() {
    assert_eq!(
        test_w_stream_coordinate_mapping(),
        Ok(ContractOutcome::Passed)
    );
}

#[test]
fn test_w10_type_text_unicode_zwj_and_newline_unit() {
    assert_eq!(
        test_w_type_text_unicode_zwj_and_newline(),
        Ok(ContractOutcome::Passed)
    );
}

#[test]
fn test_w11_execution_surfaces_denied_unit() {
    assert_eq!(
        test_w_execution_surfaces_denied(),
        Ok(ContractOutcome::Passed)
    );
}

#[test]
fn test_w12_capabilities_matrix_and_honest_degradation_unit() {
    assert_eq!(
        test_w_capabilities_matrix_and_honest_degradation(),
        Ok(ContractOutcome::Passed)
    );
}

#[test]
fn test_w13_platform_skips_match_capabilities_unit() {
    assert_eq!(
        test_w_platform_skips_match_capabilities(),
        Ok(ContractOutcome::Passed)
    );
}

#[test]
fn test_w14_window_identity_and_reauth_unit() {
    assert_eq!(
        test_w_window_identity_and_reauth(),
        Ok(ContractOutcome::Passed)
    );
}

#[tokio::test]
async fn test_w15_stale_element_and_closed_window_unit() {
    assert_eq!(
        test_w_stale_element_and_closed_window().await,
        Ok(ContractOutcome::Passed)
    );
}

#[test]
fn test_w16_password_redaction_unit() {
    assert_eq!(test_w_password_redaction(), Ok(ContractOutcome::Passed));
}

/// Master table-driven portable contract test suite running across all platforms.
#[tokio::test]
async fn test_wayland_contract_portable_table_suite() {
    println!("\n======================================================================");
    println!("             WAYLAND DRIVER CONTRACT TEST SUITE (PORTABLE)            ");
    println!("======================================================================");

    let mut summary = SummaryReport::default();

    summary.record(
        "W-SPEC-01",
        "restore token permissions (0600/0700) and 24h/auth lifetime expiry",
        test_w_token_permissions_and_expiry(),
    );
    summary.record(
        "W-SPEC-02",
        "restore token origin binding mismatch causes eviction",
        test_w_token_origin_binding(),
    );
    summary.record(
        "W-SPEC-03",
        "restore token revocation and session cleanup",
        test_w_token_revocation_and_cleanup(),
    );
    summary.record(
        "W-SPEC-04",
        "source scope confinement and cross-origin frame rejection",
        test_w_source_scope_window_only().await,
    );
    summary.record(
        "W-SPEC-05",
        "unknown capture bounds rejected (ADR 0004)",
        test_w_unknown_capture_bounds_rejected_adr0004().await,
    );
    summary.record(
        "W-SPEC-06",
        "incomplete tree traversal forces sensitive localization handoff",
        test_w_tree_incomplete_handoff_adr0004().await,
    );
    summary.record(
        "W-SPEC-07",
        "Stop listener failure disables foreground input routes before portal",
        test_w_stop_failure_disables_input(),
    );
    summary.record(
        "W-SPEC-08",
        "Stop latch budget (<=100ms) and input release budget (<=500ms)",
        test_w_stop_budgets_and_release_all_input().await,
    );
    summary.record(
        "W-SPEC-09",
        "stream coordinate mapping within window bounds and NaN/inf rejection",
        test_w_stream_coordinate_mapping(),
    );
    summary.record(
        "W-SPEC-10",
        "type_text Unicode, emoji ZWJ, and newline fidelity",
        test_w_type_text_unicode_zwj_and_newline(),
    );
    summary.record(
        "W-SPEC-11",
        "rejection of IDE, Web SSH, cloud terminal, and debug consoles",
        test_w_execution_surfaces_denied(),
    );
    summary.record(
        "W-SPEC-12",
        "capability table honest degradation and doctor alignment",
        test_w_capabilities_matrix_and_honest_degradation(),
    );
    summary.record(
        "W-SPEC-13",
        "platform skip rationales strictly match capability declarations",
        test_w_platform_skips_match_capabilities(),
    );
    summary.record(
        "W-SPEC-14",
        "window identity image provenance and re-authorization requirements",
        test_w_window_identity_and_reauth(),
    );
    summary.record(
        "W-SPEC-15",
        "closed window ID returns WindowGone without fallback",
        test_w_stale_element_and_closed_window().await,
    );
    summary.record(
        "W-SPEC-16",
        "password field masking and screenshot redaction",
        test_w_password_redaction(),
    );

    summary.print_summary("PORTABLE");

    assert_eq!(
        summary.failed, 0,
        "All portable contract test cases must pass or be explicitly skipped"
    );
}

/// Master table-driven live contract test suite running in headless GNOME Wayland.
/// Marked `#[ignore]` so it is only executed in environments with live mutter/portal.
#[tokio::test]
#[ignore]
async fn live_contract_table_suite_wayland() {
    let summary = cases_live::run_live_wayland_suite().await;
    assert_eq!(
        summary.failed, 0,
        "All live contract test cases must pass, skip, or truthfully report not_run"
    );
}
