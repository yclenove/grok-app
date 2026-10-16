//! Discovery, tree roles and screenshot contracts.
use super::support::*;
use gcu_core::driver::{AppQuery, Cancel, CaptureOpts, Driver};
use gcu_core::model::Role;
use gcu_driver_macos::MacosDriver;

pub(super) async fn c01(driver: &MacosDriver) -> Result<(), String> {
    let apps = driver
        .list_apps()
        .await
        .map_err(|error| error.to_string())?;
    let query = std::env::var("GCU_FIXTURE_APP").unwrap_or_else(|_| "fixture".into());
    let app = driver
        .resolve_app(&AppQuery::from(query.as_str()))
        .await
        .map_err(|error| error.to_string())?;
    if !apps.iter().any(|item| item.id == app.info.id) {
        return Err("fixture missing from list_apps".into());
    }
    let by_id = driver
        .resolve_app(&AppQuery::from(app.info.id.as_str()))
        .await
        .map_err(|error| error.to_string())?;
    if by_id.info.id != app.info.id {
        return Err("resolve by id changed the app".into());
    }
    let Some(path) = app
        .info
        .identity
        .as_ref()
        .and_then(|id| id.executable.clone())
    else {
        return Err("fixture executable missing".into());
    };
    let by_path = driver
        .resolve_app(&AppQuery::from(path.as_str()))
        .await
        .map_err(|error| error.to_string())?;
    if by_path.info.id != app.info.id {
        return Err("resolve by path changed the app".into());
    }
    Ok(())
}

pub(super) async fn c02(driver: &MacosDriver) -> Result<(), String> {
    let before = frontmost()?;
    let win = fixture_window(driver).await?;
    driver
        .launch_app(&win.app)
        .await
        .map_err(|error| error.to_string())?;
    let after = frontmost()?;
    if before != after {
        return Err(format!("launch changed frontmost from {before} to {after}"));
    }
    Ok(())
}

pub(super) async fn c03(driver: &MacosDriver) -> Result<(), String> {
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    if root.role != Role::Window {
        return Err(format!("root role {:?}", root.role));
    }
    for (keys, role) in [
        (&["counter_button", "Increment"][..], Role::Button),
        (&["name_field", "Name"][..], Role::TextField),
        (&["agree_checkbox", "Agree"][..], Role::CheckBox),
        (&["slider_field", "Volume"][..], Role::Slider),
    ] {
        let found = find(&root, &|node| named(node, keys));
        if found.is_none_or(|node| node.role != role) {
            return Err(format!(
                "missing {keys:?} as {role:?}; last state: {}",
                last_state()
            ));
        }
    }
    Ok(())
}

pub(super) async fn c04(driver: &MacosDriver) -> Result<(), String> {
    let win = fixture_window(driver).await?;
    let geometry = driver
        .window_geometry(&win)
        .await
        .map_err(|error| error.to_string())?;
    let capture = driver
        .capture(&win, &CaptureOpts::default(), &Cancel::default())
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    let width_ok = (f64::from(capture.width) - geometry.frame.w).abs() < 1.0;
    let height_ok = (f64::from(capture.height) - geometry.frame.h).abs() < 1.0;
    if capture.width == 0
        || !width_ok
        || !height_ok
        || !capture.bytes.starts_with(b"\x89PNG\r\n\x1a\n")
    {
        return Err(format!(
            "capture {}x{} != {}x{}; last state: {}",
            capture.width,
            capture.height,
            geometry.frame.w,
            geometry.frame.h,
            last_state()
        ));
    }
    let (_w, _h, pixels) = decode_png(&capture.bytes)?;
    let marker = [95u8, 115, 135];
    if !pixels
        .as_chunks::<4>()
        .0
        .iter()
        .any(|px| px[0] == marker[0] && px[1] == marker[1] && px[2] == marker[2])
    {
        return Err(format!(
            "capture missing fixture cell (0,0) color; last state: {}",
            last_state()
        ));
    }
    Ok(())
}
