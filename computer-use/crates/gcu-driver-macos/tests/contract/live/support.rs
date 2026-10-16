//! Shared live fixture reads and oracle helpers; no implicit readiness retries.
pub(super) use super::super::fixture_tree::find;
use super::super::oracle;
use gcu_core::driver::{AppQuery, Cancel, Driver, TreeOpts, WindowRef};
use gcu_core::model::{RawNode, WindowPoint};
use gcu_driver_macos::MacosDriver;
use std::time::Duration;

pub(super) async fn fixture_window(driver: &MacosDriver) -> Result<WindowRef, String> {
    let query = std::env::var("GCU_FIXTURE_APP").unwrap_or_else(|_| "fixture".into());
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(app) = driver.resolve_app(&AppQuery::from(query.as_str())).await {
            if app.info.pid.is_some() {
                if let Ok(info) = driver.key_window(&app).await {
                    return Ok(WindowRef { app, id: info.id });
                }
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "gcu-fixture window did not appear; last state: {}",
                last_state()
            ));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

pub(super) async fn tree(driver: &MacosDriver, win: &WindowRef) -> Result<RawNode, String> {
    let raw = driver
        .snapshot_tree(
            win,
            &TreeOpts {
                timeout: Duration::from_secs(5),
                max_tree_nodes: 5000,
                ..TreeOpts::default()
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    if let Some(fact) = raw.incomplete {
        return Err(format!(
            "fixture tree incomplete: {fact:?}; last state: {}",
            last_state()
        ));
    }
    raw.root.ok_or_else(|| "fixture root missing".into())
}

pub(super) fn named(node: &RawNode, keys: &[&str]) -> bool {
    let id = node.identifier.as_deref().unwrap_or("");
    let name = node.name.as_deref().unwrap_or("");
    keys.iter()
        .any(|key| id.contains(key) || name.contains(key))
}

pub(super) fn center(node: &RawNode) -> Result<WindowPoint, String> {
    let frame = node.frame.ok_or("node frame missing")?;
    Ok(WindowPoint {
        x: frame.x + frame.w / 2.0,
        y: frame.y + frame.h / 2.0,
    })
}

pub(super) fn last_state() -> String {
    oracle::read_state(&oracle::fixture_state_path())
        .map(|state| state.to_string())
        .unwrap_or_else(|| "<no state read>".into())
}

pub(super) async fn expect_text(text: &str) -> Result<(), String> {
    let want = text.to_string();
    oracle::poll_until(
        &oracle::fixture_state_path(),
        Duration::from_secs(5),
        move |state| {
            state
                .get("text")
                .and_then(|value| value.as_str())
                .filter(|value| value.contains(&want))
                .map(|_| ())
        },
    )
    .await
    .map(|_| ())
}

pub(super) fn counter_now() -> u64 {
    oracle::read_state(&oracle::fixture_state_path())
        .and_then(|state| state.get("counter").and_then(|value| value.as_u64()))
        .unwrap_or(0)
}

pub(super) fn flag(name: &str) -> Result<bool, String> {
    oracle::read_state(&oracle::fixture_state_path())
        .and_then(|state| state.get(name).and_then(|value| value.as_bool()))
        .ok_or_else(|| format!("{name} missing; last state: {}", last_state()))
}

pub(super) fn frontmost() -> Result<String, String> {
    let output = std::process::Command::new("osascript")
        .args([
            "-e",
            "tell application \"System Events\" to get name of first application process whose frontmost is true",
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "frontmost query failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(super) fn is_calculator(name: &str) -> bool {
    name == "Calculator" || name.contains("计算器")
}

pub(super) fn process_running(name: &str) -> bool {
    std::process::Command::new("pgrep")
        .args(["-x", name])
        .status()
        .is_ok_and(|status| status.success())
}

pub(super) fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(4096)];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|error| error.to_string())?;
    buf.truncate(info.buffer_size());
    Ok((info.width, info.height, buf))
}
