//! Master contract test suite for Linux AT-SPI and X11 driver against gcu-fixture.
//! Covers C01–C27 from 07-quality-gates.md and P3 special security requirements.

#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::chunks_exact_to_as_chunks
)]

#[path = "contract/support.rs"]
mod support;

#[path = "contract/cases_input.rs"]
mod cases_input;

#[path = "contract/cases_safety.rs"]
mod cases_safety;

use cases_input::*;
use cases_safety::*;
use gcu_core::driver::Driver;
use gcu_driver_linux::LinuxDriver;
use support::*;

#[test]
fn test_p3_identity_reauth_offline_unit() {
    assert_eq!(test_p3_identity_reauth(), Ok(ContractOutcome::Passed));
}

#[test]
fn test_c26_execution_surfaces_denied_offline_unit() {
    assert_eq!(
        test_c26_execution_surfaces_denied(),
        Ok(ContractOutcome::Passed)
    );
}

#[tokio::test]
async fn test_p3_hotkey_failure_disables_foreground_offline_unit() {
    assert_eq!(
        test_p3_hotkey_failure_disables_foreground().await,
        Ok(ContractOutcome::Passed)
    );
}

#[tokio::test]
async fn test_c27_stop_hotkey_and_watchdog_budgets_offline_unit() {
    assert_eq!(
        test_c27_stop_hotkey_and_watchdog_budgets().await,
        Ok(ContractOutcome::Passed)
    );
}

#[tokio::test]
async fn test_c22_timeout_no_replay_offline_unit() {
    assert_eq!(
        test_c22_timeout_no_replay().await,
        Ok(ContractOutcome::Passed)
    );
}

/// Master table-driven contract test running live against gcu-fixture in Xvfb.
/// Marked `#[ignore]` so it only runs in the live CI / Xvfb session.
#[tokio::test]
#[ignore]
async fn live_contract_table_suite_c01_to_c27() {
    let driver = LinuxDriver::connect()
        .await
        .expect("must connect to live AT-SPI and X11");

    let (app, win_ref) = wait_for_fixture(&driver)
        .await
        .expect("gcu-fixture application must be discoverable");

    ensure_modal_closed(&driver, &win_ref).await;

    let doc = driver.doctor().await;
    println!(
        "  [Doctor] Ready: {}, Hotkey available: {}, Stopped: {}",
        doc.ready,
        driver.is_hotkey_available(),
        driver.is_stopped()
    );
    for issue in &doc.issues {
        println!("  [Doctor] Issue: {issue}");
    }

    println!("\n======================================================================");
    println!("               LINUX DRIVER CONTRACT TEST SUITE (C01 - C27)          ");
    println!("======================================================================");

    let mut passed_count = 0;
    let mut skipped_count = 0;
    let mut failed_count = 0;

    macro_rules! run_step {
        ($id:literal, $desc:literal, $call:expr) => {
            print!("  [{:^4}] {:<65}", $id, $desc);
            match $call {
                Ok(ContractOutcome::Passed) => {
                    println!("-> PASS");
                    passed_count += 1;
                }
                Ok(ContractOutcome::Skipped(reason)) => {
                    println!("-> SKIP ({reason})");
                    skipped_count += 1;
                }
                Err(err) => {
                    println!("-> FAIL: {err}");
                    failed_count += 1;
                }
            }
        };
    }

    run_step!(
        "C01",
        "list_apps and resolve name/ID/path",
        test_c01_discovery(&driver).await
    );
    run_step!(
        "C02",
        "launch app not activating foreground",
        test_c02_launch(&driver, &app).await
    );
    run_step!(
        "C03",
        "tree roles conformance",
        test_c03_tree_roles(&driver, &win_ref).await
    );
    run_step!(
        "C04",
        "screenshot physical geometry and XComposite occlusion",
        test_c04_capture_and_occlusion(&driver, &win_ref).await
    );
    run_step!(
        "C05",
        "button click counter increment exactly once",
        test_c05_press_counter(&driver, &win_ref).await
    );
    run_step!(
        "C06",
        "screenshot pixel click canvas grid cell hit",
        test_c06_click_grid_cell(&driver, &win_ref).await
    );
    run_step!(
        "C07",
        "set_value textfield",
        test_c07_set_value_textfield(&driver, &win_ref).await
    );
    run_step!(
        "C08",
        "type_text Unicode, emoji ZWJ, and newline",
        test_c08_type_text_unicode_zwj(&driver, &win_ref).await
    );
    run_step!(
        "C09",
        "press_keys ctrl+a then BackSpace clears field",
        test_c09_press_keys_ctrl_a_backspace(&driver, &win_ref).await
    );
    run_step!(
        "C10",
        "checkbox toggle state",
        test_c10_toggle_checkbox(&driver, &win_ref).await
    );
    run_step!(
        "C11",
        "combobox select 3rd item",
        test_c11_combobox_select().await
    );
    run_step!(
        "C12",
        "slider set_value via AT-SPI Value",
        test_c12_slider_set_value(&driver, &win_ref).await
    );
    run_step!(
        "C13",
        "virtual list pointer scroll updates visible range",
        test_c13_virtual_list_scroll(&driver, &win_ref).await
    );
    run_step!(
        "C14",
        "modal dialog lifecycle and prioritize modal",
        test_c14_modal_lifecycle(&driver, &win_ref).await
    );
    run_step!(
        "C15",
        "drag and drop box to target area",
        test_c15_drag_and_drop(&driver, &win_ref).await
    );
    run_step!(
        "C16",
        "background press while another window focused",
        test_c16_background_press_with_other_window(&driver, &win_ref).await
    );
    run_step!(
        "C17",
        "stale geometry on old snapshot click",
        test_c17_stale_geometry().await
    );
    run_step!(
        "C18",
        "stale element handle fails without effect",
        test_c18_stale_element_handle(&driver).await
    );
    run_step!(
        "C19",
        "closed window ID fails with window_gone",
        test_c19_closed_window(&driver).await
    );
    run_step!(
        "C20",
        "password field masking and screenshot redaction",
        test_c20_password_redaction(&driver, &win_ref).await
    );
    run_step!(
        "C21",
        "overlay exclusion from capture",
        test_c21_overlay_exclusion().await
    );
    run_step!(
        "C22",
        "uncertain timeout does not replay",
        test_c22_timeout_no_replay().await
    );
    run_step!(
        "C23",
        "release_all_input clears held Shift keys",
        test_c23_release_all_input_no_stuck_keys(&driver, &win_ref).await
    );
    run_step!(
        "C24",
        "2000-row virtual list truncation under budget",
        test_c24_virtual_list_tree_budget(&driver, &win_ref).await
    );
    run_step!(
        "C25",
        "pixel hash stale screenshot gate",
        test_c25_pixel_hash_stale().await
    );
    run_step!(
        "C26",
        "IDE / terminal execution surface rejection",
        test_c26_execution_surfaces_denied()
    );
    run_step!(
        "C27",
        "global stop latch latency and watchdog timeout",
        test_c27_stop_hotkey_and_watchdog_budgets().await
    );
    run_step!(
        "P3-1",
        "Linux image identity verification and re-auth",
        test_p3_identity_reauth()
    );
    run_step!(
        "P3-2",
        "hotkey registration failure disables foreground",
        test_p3_hotkey_failure_disables_foreground().await
    );

    println!("======================================================================");
    println!(
        "SUMMARY: {} passed, {} skipped, 0 not_run, {} failed",
        passed_count, skipped_count, failed_count
    );
    println!("======================================================================\n");

    assert_eq!(
        failed_count, 0,
        "All contract test cases must pass or be explicitly skipped"
    );
}
