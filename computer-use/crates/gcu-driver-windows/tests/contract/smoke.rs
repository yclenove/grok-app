//! Native discovery/actions; smoke-windows.ps1 supplies independent app oracles.
use super::harness::{
    attempt, foreground_unavailable, poll_async, progress, DesktopClock, Outcome,
};
use super::live::{self, unsafe_desktop};
use gcu_core::{
    driver::*,
    model::{RawNode, Role},
};
use gcu_driver_windows::WindowsDriver;
use std::time::Duration;

#[path = "../smoke/integrity.rs"]
mod integrity;
#[path = "../smoke/unsafe_token.rs"]
mod unsafe_token;

async fn target(driver: &WindowsDriver, pid: u32) -> Result<WindowRef, integrity::DiscoveryError> {
    use gcu_driver_windows::{api::WinApi, RealWinApi};
    let api = RealWinApi::new()?;
    let path = api.process_path(pid)?;
    let app = driver.resolve_app(&AppQuery(path)).await?;
    let hwnd = std::env::var("GCU_SMOKE_HWND")
        .ok()
        .map(|n| n.parse::<u64>().map_err(|e| format!("GCU_SMOKE_HWND: {e}")))
        .transpose()?;
    let windows = driver.list_windows(Some(&app)).await?;
    let window = windows
        .into_iter()
        .find(|w| {
            if let Some(hwnd) = hwnd {
                u64::from_str_radix(w.id.split(':').next().unwrap_or(""), 16).ok() == Some(hwnd)
            } else {
                unsafe_desktop::pid(&w.id) == Some(pid)
            }
        })
        .ok_or_else(|| "exact smoke HWND missing".to_owned())?;
    Ok(WindowRef { app, id: window.id })
}
fn pid() -> Result<u32, String> {
    std::env::var("GCU_SMOKE_PID")
        .map_err(|e| format!("GCU_SMOKE_PID: {e}"))?
        .parse()
        .map_err(|e| format!("GCU_SMOKE_PID: {e}"))
}
async fn activate(driver: &WindowsDriver, win: &WindowRef) -> Result<(), String> {
    live::perform(driver, DriverAction::Activate { win: win.clone() })
        .await
        .map_err(|e| format!("activate exact smoke window: {e}"))?;
    Ok(())
}
fn editor(node: &RawNode) -> Option<&RawNode> {
    if matches!(
        node.role,
        Role::TextField | Role::TextArea | Role::SearchField | Role::Document
    ) && node.frame.is_some()
    {
        Some(node)
    } else {
        node.children.iter().find_map(editor)
    }
}
async fn ready_editor(driver: &WindowsDriver, win: &WindowRef) -> Result<RawNode, String> {
    poll_async(
        &mut DesktopClock::default(),
        Duration::from_secs(5),
        "system-app UIA editor",
        async || {
            let raw = live::tree(driver, win).await?;
            raw.root
                .as_ref()
                .and_then(editor)
                .cloned()
                .ok_or_else(|| format!("editor missing; last tree={raw:?}"))
        },
        |_| true,
    )
    .await
}

#[derive(Clone, Copy)]
enum Scenario {
    Notepad,
    Explorer,
    Settings,
}

async fn actions(driver: &WindowsDriver, scenario: Scenario) -> Result<Outcome, String> {
    let pid = pid()?;
    let tokens = if matches!(scenario, Scenario::Settings) {
        let tokens =
            integrity::TokenPair::measure(&unsafe_token::NativeTokens, pid, std::process::id())?;
        progress(format!("SMOKE PROBE Settings token integrity: {tokens}"));
        Some(tokens)
    } else {
        None
    };
    let win = match target(driver, pid).await {
        Ok(win) => win,
        Err(error) => {
            if let Some(tokens) = tokens {
                return tokens.discovery_refusal(error).map(Outcome::Skip);
            }
            return Err(format!("exact executable/window discovery: {error}"));
        }
    };
    progress(format!(
        "SMOKE ASSERT: exact process executable resolved; requested PID={pid}; window={}",
        win.id
    ));
    let has_editor = !matches!(scenario, Scenario::Explorer);
    if has_editor {
        let field = ready_editor(driver, &win).await?;
        live::center(&field)?;
        progress("SMOKE ASSERT: UIA editor and physical frame found without foreground input");
    }
    // Discovery is a real assertion on every desktop. Only the remaining
    // foreground actions may Skip, using precisely the contract's decision.
    if driver.capabilities().delivery.foreground
        == gcu_protocol::capabilities::ForegroundCapability::Unavailable
    {
        let doctor = driver.doctor().await;
        let discovery = if has_editor {
            "exact executable/window/UIA editor discovery passed"
        } else {
            "exact executable/window discovery passed"
        };
        return foreground_unavailable(&doctor.issues, super::unsafe_mouse_probe::decision()?).map(
            |outcome| match outcome {
                Outcome::Skip(reason) => Outcome::Skip(format!(
                    "{discovery}; foreground input only skipped; {reason}"
                )),
                other => other,
            },
        );
    }
    activate(driver, &win).await?;
    match scenario {
        Scenario::Notepad => {
            let field = ready_editor(driver, &win).await?;
            live::click(driver, &win, live::center(&field)?).await?;
            live::text(driver, &win, "GCU 中文 👩‍💻").await?;
            live::key(driver, &win, "ctrl+s").await?;
        }
        Scenario::Explorer => {
            let dest =
                std::env::var("GCU_SMOKE_DEST").map_err(|e| format!("GCU_SMOKE_DEST: {e}"))?;
            live::key(driver, &win, "ctrl+l").await?;
            live::text(driver, &win, &dest).await?;
            live::key(driver, &win, "Return").await?;
        }
        Scenario::Settings => {
            let field = ready_editor(driver, &win).await?;
            live::click(driver, &win, live::center(&field)?).await?;
            live::key(driver, &win, "ctrl+a").await?;
            live::text(driver, &win, "display").await?;
        }
    }
    Ok(Outcome::Passed)
}

async fn run(name: &str, scenario: Scenario) -> Result<(), String> {
    let result = match live::driver().await {
        Ok(driver) => {
            let result = attempt(actions(&driver, scenario)).await;
            driver.release_all_input().await;
            result
        }
        Err(error) => Err(error),
    };
    let (outcome, reason) = match result {
        Ok(Outcome::Passed) => (
            "passed",
            "discovery and foreground actions completed; external oracle still required".into(),
        ),
        Ok(Outcome::Skip(reason)) => ("skipped", reason),
        Ok(other) => ("failed", format!("unexpected smoke result: {other:?}")),
        Err(error) => ("failed", error),
    };
    let path = std::env::var_os("GCU_SMOKE_REPORT").ok_or("GCU_SMOKE_REPORT is required")?;
    let report = serde_json::json!({"test": name, "outcome": outcome, "reason": reason});
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("write explicit smoke result: {e}"))?;
    progress(format!("SMOKE NATIVE RESULT: {report}"));
    if outcome == "failed" {
        Err(reason)
    } else {
        Ok(())
    }
}

#[tokio::test]
#[ignore = "not_run: smoke-windows.ps1 opens a disposable Notepad file and checks saved bytes"]
async fn live_notepad_write_save() -> Result<(), String> {
    run("smoke::live_notepad_write_save", Scenario::Notepad).await
}

#[tokio::test]
#[ignore = "not_run: smoke-windows.ps1 opens Explorer and checks its path through Shell.Application"]
async fn live_explorer_navigate() -> Result<(), String> {
    run("smoke::live_explorer_navigate", Scenario::Explorer).await
}

#[tokio::test]
#[ignore = "not_run: smoke-windows.ps1 opens Settings and checks query/results with independent UIA"]
async fn live_settings_search() -> Result<(), String> {
    run("smoke::live_settings_search", Scenario::Settings).await
}
