//! Oracle, discovery, and helper utilities for the contract test suite.

use gcu_core::{
    driver::{AppQuery, AppRef, Cancel, Driver, DriverAction, DriverError, TreeOpts, WindowRef},
    model::{RawNode, Role},
};
use gcu_driver_linux::LinuxDriver;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractOutcome {
    Passed,
    Skipped(&'static str),
}

pub fn read_fixture_state() -> Option<serde_json::Value> {
    let path =
        std::env::var("GCU_FIXTURE_STATE").unwrap_or_else(|_| "/tmp/fixture_state.json".into());
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

pub async fn poll_oracle<T, F>(timeout: Duration, f: F) -> Result<T, String>
where
    F: Fn(&serde_json::Value) -> Option<T>,
{
    let start = tokio::time::Instant::now();
    while start.elapsed() < timeout {
        if let Some(state) = read_fixture_state() {
            if let Some(res) = f(&state) {
                return Ok(res);
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(format!("oracle poll timed out after {:?}", timeout))
}

pub fn find_raw_node<'a, F>(node: &'a RawNode, pred: &F) -> Option<&'a RawNode>
where
    F: Fn(&RawNode) -> bool,
{
    if pred(node) {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_raw_node(child, pred) {
            return Some(found);
        }
    }
    None
}

pub async fn wait_for_fixture(driver: &LinuxDriver) -> Result<(AppRef, WindowRef), DriverError> {
    let mut fixture_app = None;
    for _ in 0..40 {
        if let Ok(app) = driver.resolve_app(&AppQuery("fixture".into())).await {
            fixture_app = Some(app);
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let app = fixture_app.ok_or(DriverError::WindowGone)?;
    let win_info = driver.key_window(&app).await?;
    let win_ref = WindowRef {
        app: app.clone(),
        id: win_info.id.clone(),
    };
    Ok((app, win_ref))
}

pub async fn ensure_modal_closed(driver: &LinuxDriver, win_ref: &WindowRef) {
    let opts = TreeOpts {
        timeout: Duration::from_secs(5),
        max_tree_nodes: 5000,
        ..Default::default()
    };
    let cancel = Cancel::default();
    if let Ok(tree) = driver.snapshot_tree(win_ref, &opts, &cancel).await {
        if let Some(ref root) = tree.root {
            if let Some(confirm) = find_raw_node(root, &|n| {
                n.role == Role::Button
                    && (n.identifier.as_deref() == Some("modal_confirm_button")
                        || n.identifier.as_deref() == Some("modal_cancel_button")
                        || n.name.as_deref().is_some_and(|s| {
                            s.contains("modal_confirm_button")
                                || s.contains("modal_cancel_button")
                                || s.contains("OK")
                                || s.contains("确定")
                        }))
            }) {
                let _ = driver
                    .perform(
                        &DriverAction::Press {
                            el: confirm.handle.clone(),
                        },
                        &cancel,
                    )
                    .await;
                let _ = poll_oracle(Duration::from_secs(3), |v| {
                    if v.get("modal_open").and_then(|m| m.as_bool()) == Some(false) {
                        Some(())
                    } else {
                        None
                    }
                })
                .await;
            }
        }
    }
}

pub fn decode_png_to_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(4096)];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    Ok((info.width, info.height, buf))
}
