//! Ignored live table. Grant Accessibility and Screen Recording, start gcu-fixture,
//! then: `cargo test -p gcu-driver-macos --test contract -- --ignored --test-threads=1`

use super::report::{self, NOT_RUN};
use super::specials;
use super::table::{self, CaseKind, CaseResult, Facts};
use gcu_core::driver::Driver;
use gcu_driver_macos::input::hotkey::{HotkeyConfig, SharedStopLatch, StopLatch};
use gcu_driver_macos::MacosDriver;
use std::sync::Arc;
#[path = "live/actions.rs"]
mod actions;
#[path = "live/observe.rs"]
mod observe;
#[path = "live/setup.rs"]
mod setup;
#[path = "live/support.rs"]
mod support;
#[path = "live/text.rs"]
mod text;
use actions::*;
use observe::*;
use support::*;
use text::*;

pub async fn live_contract_table_suite() {
    // The disposable test Host starts resumed, matching Linux's contract.
    // C27 creates its own latch; no production/persistent Stop is cleared.
    let latch = Arc::new(SharedStopLatch::new(false));
    let driver = MacosDriver::with_stop_latch(latch.clone(), HotkeyConfig::default(), 3000)
        .await
        .expect("unelevated native macOS driver with RealEvents");
    assert!(!latch.is_stopped());
    let guards = driver
        .input_guards()
        .expect("native RealEvents guards installed");
    assert!(!guards.is_stopped());
    let runtime = gcu_driver_macos::diagnostics::runtime_report(&driver)
        .await
        .expect("native backend facts");
    assert_eq!(runtime["installed_probe"]["appkit_ready"], true);
    assert_eq!(
        runtime["capabilities"]["delivery"]["targeted_pointer"],
        runtime["installed_probe"]["background_ready"]
    );
    assert_eq!(
        runtime["capabilities"]["delivery"]["targeted_keys"],
        runtime["installed_probe"]["background_ready"]
    );
    println!("{runtime}");
    let doc = driver.doctor().await;
    let caps = driver.capabilities().clone();
    let fixture_running = fixture_window(&driver).await.is_ok();
    let facts = Facts {
        accessibility: !doc
            .issues
            .iter()
            .any(|issue| issue.contains("permission missing: accessibility")),
        screen_recording: !doc
            .issues
            .iter()
            .any(|issue| issue.contains("permission missing: screen_recording")),
        field_51_ready: caps.delivery.targeted_pointer,
        fixture_running,
    };
    let mut results = Vec::with_capacity(table::ROWS.len());
    let mut failed = 0;
    let mut readiness = Vec::new();
    let mut setup_error = None;
    if facts.fixture_running && facts.accessibility {
        match setup::initialize(&driver).await {
            Ok(evidence) => {
                readiness.extend(evidence);
                results.push(CaseResult::passed(
                    "SETUP-AX",
                    "fixture ready and ADR 0004 first-read evidence",
                ));
            }
            Err(error) => {
                println!("  [  SETUP-AX  ] -> FAIL: {error}");
                results.push(CaseResult::failed(
                    "SETUP-AX",
                    "fixture ready and ADR 0004 first-read evidence",
                    error.clone(),
                ));
                setup_error = Some(error);
                failed += 1;
            }
        }
    }
    for row in table::ROWS {
        if let Some(reason) = table::skip_reason(row, &caps, &facts) {
            println!("  [{:^12}] {} -> SKIP ({reason})", row.id, row.description);
            results.push(CaseResult::skipped(row.id, row.description, reason));
            continue;
        }
        let fixture_case = matches!(
            row.kind,
            CaseKind::Live {
                needs_fixture: true,
                ..
            }
        );
        let ran = if fixture_case && setup_error.is_some() {
            Err(format!(
                "fixture setup/recovery failed: {}",
                setup_error.as_deref().unwrap_or("")
            ))
        } else {
            dispatch(&driver, row.id).await
        };
        match ran {
            Ok(()) => {
                println!("  [{:^12}] {} -> PASS", row.id, row.description);
                results.push(CaseResult::passed(row.id, row.description));
            }
            Err(error) => {
                println!("  [{:^12}] {} -> FAIL: {error}", row.id, row.description);
                results.push(CaseResult::failed(row.id, row.description, error));
                failed += 1;
                if fixture_case && setup_error.is_none() {
                    match setup::recover_if_needed(&driver, row.id).await {
                        Ok(Some(evidence)) => readiness.push(evidence),
                        Ok(None) => {}
                        Err(error) => {
                            println!("fixture recovery after {} failed: {error}", row.id);
                            setup_error = Some(error);
                        }
                    }
                }
            }
        }
    }
    let not_run = [NOT_RUN[1]];
    report::archive_with_readiness(&results, &not_run, &readiness).expect("contract archive");
    assert_eq!(failed, 0, "contract failures: {failed}");
}

async fn dispatch(driver: &MacosDriver, id: &str) -> Result<(), String> {
    match id {
        "C01" => c01(driver).await,
        "C02" => c02(driver).await,
        "C03" => c03(driver).await,
        "C04" => c04(driver).await,
        "C05" => press_counter(driver).await,
        "C06" => click_grid(driver).await,
        "C07" => set_text(driver, "fixture-value").await,
        "C08" => type_sample(driver).await,
        "C09" => clear_with_chord(driver).await,
        "C10" => toggle_checkbox(driver).await,
        "C11" => select_third_combo(driver).await,
        "C12" => set_slider(driver).await,
        "C13" => scroll_list(driver).await,
        "C14" => modal_round_trip(driver).await,
        "C15" => drag_source(driver).await,
        "C16" => background_press(driver).await,
        "C21" => Err("overlay exclusion has no live window in this driver".into()),
        "C23" => lowercase_after_release(driver).await,
        "C18" | "C19" | "C20" | "C22" | "C26" | "C27" | "M-TCC" | "M-CHROMIUM" | "M-FIELD51" => {
            portable_one(id).await
        }
        other => Err(format!("no live body for {other}")),
    }
}

async fn portable_one(id: &str) -> Result<(), String> {
    let results = specials::run_portable_matrix().await;
    let row = results
        .iter()
        .find(|row| row.id == id)
        .ok_or_else(|| format!("{id} missing from portable matrix"))?;
    match row.outcome {
        "passed" => Ok(()),
        "skipped" => Err(format!(
            "portable row skipped: {}",
            row.reason.as_deref().unwrap_or("")
        )),
        _ => Err(row.reason.clone().unwrap_or_else(|| "failed".into())),
    }
}
