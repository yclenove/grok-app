//! Readiness only at setup and explicit recovery after a failed case.

use gcu_core::driver::{Cancel, Driver, DriverAction, TreeOpts, WindowRef};
use gcu_driver_macos::MacosDriver;
use serde_json::Value;

use super::super::{fixture_tree, oracle, readiness};
use super::support::*;

pub(super) async fn initialize(driver: &MacosDriver) -> Result<Vec<Value>, String> {
    let state = oracle::read_state(&oracle::fixture_state_path()).ok_or("fixture state missing")?;
    let mut evidence = Vec::new();
    if state["tab"] != 0 || state["modal_open"] != false {
        let win = fixture_window(driver).await?;
        let ready = readiness::wait_for(
            "setup_before_reset",
            &["reset_button"],
            |timeout, cancel| read(driver, &win, timeout, cancel),
        )
        .await?;
        evidence.push(ready.evidence);
        let root = ready.tree.root.ok_or("fixture root missing before reset")?;
        reset(driver, &root).await?;
    }
    evidence.push(prepare(driver, "setup").await?);
    Ok(evidence)
}

async fn read(
    driver: &MacosDriver,
    win: &WindowRef,
    timeout: std::time::Duration,
    cancel: Cancel,
) -> Result<gcu_core::model::RawTree, String> {
    driver
        .snapshot_tree(
            win,
            &TreeOpts {
                timeout,
                max_tree_nodes: 5000,
                ..Default::default()
            },
            &cancel,
        )
        .await
        .map_err(|error| error.to_string())
}

pub(super) async fn prepare(driver: &MacosDriver, phase: &str) -> Result<Value, String> {
    let state = oracle::read_state(&oracle::fixture_state_path()).ok_or("fixture state missing")?;
    if state["tab"] != 0 || state["modal_open"] != false {
        return Err(format!(
            "reset fixture to tab=0, modal_open=false before setup; last state: {state}"
        ));
    }
    let win = fixture_window(driver).await?;
    readiness::wait(phase, |timeout, cancel| read(driver, &win, timeout, cancel))
        .await
        .map(|ready| ready.evidence)
}

pub(super) async fn recover_if_needed(
    driver: &MacosDriver,
    id: &str,
) -> Result<Option<Value>, String> {
    let state = oracle::read_state(&oracle::fixture_state_path()).ok_or("fixture state missing")?;
    if state["tab"] == 0 && state["modal_open"] == false {
        return Ok(None);
    }
    // Preserve the case failure, then reset the disposable fixture once.
    // Never retry the failed action, including uncertain deliveries.
    println!(
        "fixture recovery after {id}: tab={}, modal_open={}",
        state["tab"], state["modal_open"]
    );
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    reset(driver, &root).await?;
    prepare(driver, &format!("reset_after_{id}"))
        .await
        .map(Some)
}

async fn reset(driver: &MacosDriver, root: &gcu_core::model::RawNode) -> Result<(), String> {
    let reset = find(root, &|node| fixture_tree::exact(node, "reset_button"))
        .ok_or("fixture reset button missing")?;
    driver
        .perform(
            &DriverAction::Press {
                el: reset.handle.clone(),
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| error.to_string())?;
    oracle::poll_until(&oracle::fixture_state_path(), readiness::BUDGET, |state| {
        (state["tab"] == 0 && state["modal_open"] == false).then_some(())
    })
    .await?;
    Ok(())
}
