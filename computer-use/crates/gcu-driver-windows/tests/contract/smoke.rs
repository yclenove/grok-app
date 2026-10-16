//! Actions only; smoke-windows.ps1 supplies independent file/Explorer/UIA oracles.
use super::harness::{poll_async, DesktopClock};
use super::live::{self, unsafe_desktop};
use gcu_core::{
    driver::*,
    model::{RawNode, Role, WindowPoint},
};
use gcu_driver_windows::WindowsDriver;
use std::time::Duration;

async fn target(driver: &WindowsDriver, pid: u32) -> Result<WindowRef, String> {
    use gcu_driver_windows::{api::WinApi, RealWinApi};
    let api = RealWinApi::new().map_err(|e| e.to_string())?;
    let path = api.process_path(pid).map_err(|e| e.to_string())?;
    let app = driver
        .resolve_app(&AppQuery(path))
        .await
        .map_err(|e| e.to_string())?;
    let hwnd = std::env::var("GCU_SMOKE_HWND")
        .ok()
        .and_then(|n| n.parse::<u64>().ok());
    let windows = driver
        .list_windows(Some(&app))
        .await
        .map_err(|e| e.to_string())?;
    let window = windows
        .into_iter()
        .find(|w| {
            if let Some(hwnd) = hwnd {
                u64::from_str_radix(w.id.split(':').next().unwrap_or(""), 16).ok() == Some(hwnd)
            } else {
                unsafe_desktop::pid(&w.id) == Some(pid)
            }
        })
        .ok_or("exact smoke HWND missing")?;
    Ok(WindowRef { app, id: window.id })
}
fn pid() -> u32 {
    std::env::var("GCU_SMOKE_PID").unwrap().parse().unwrap()
}
async fn activate(driver: &WindowsDriver, win: &WindowRef) {
    live::perform(driver, DriverAction::Activate { win: win.clone() })
        .await
        .unwrap();
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
async fn ready_editor(driver: &WindowsDriver, win: &WindowRef) -> RawNode {
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
    .unwrap()
}

#[tokio::test]
#[ignore = "not_run: smoke-windows.ps1 opens a disposable Notepad file and checks saved bytes"]
async fn live_notepad_write_save() {
    let driver = live::driver().await.unwrap();
    let win = target(&driver, pid()).await.unwrap();
    activate(&driver, &win).await;
    let field = ready_editor(&driver, &win).await;
    live::click(&driver, &win, live::center(&field).unwrap())
        .await
        .unwrap();
    live::text(&driver, &win, "GCU 中文 👩‍💻").await.unwrap();
    live::key(&driver, &win, "ctrl+s").await.unwrap();
    driver.release_all_input().await;
}

#[tokio::test]
#[ignore = "not_run: smoke-windows.ps1 opens Explorer and checks its path through Shell.Application"]
async fn live_explorer_navigate() {
    let driver = live::driver().await.unwrap();
    let win = target(&driver, pid()).await.unwrap();
    activate(&driver, &win).await;
    live::key(&driver, &win, "ctrl+l").await.unwrap();
    live::text(&driver, &win, &std::env::var("GCU_SMOKE_DEST").unwrap())
        .await
        .unwrap();
    live::key(&driver, &win, "Return").await.unwrap();
    driver.release_all_input().await;
}

#[tokio::test]
#[ignore = "not_run: smoke-windows.ps1 opens Settings and checks query/results with independent UIA"]
async fn live_settings_search() {
    let driver = live::driver().await.unwrap();
    let win = target(&driver, pid()).await.unwrap();
    activate(&driver, &win).await;
    let field = ready_editor(&driver, &win).await;
    let frame = field.frame.unwrap();
    live::click(
        &driver,
        &win,
        WindowPoint {
            x: frame.x + frame.w / 2.,
            y: frame.y + frame.h / 2.,
        },
    )
    .await
    .unwrap();
    live::key(&driver, &win, "ctrl+a").await.unwrap();
    live::text(&driver, &win, "display").await.unwrap();
    driver.release_all_input().await;
}
