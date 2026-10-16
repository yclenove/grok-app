//! Live pointer and semantic controls, each checked by the external oracle.
use super::super::{fixture_tree, oracle};
use super::support::*;
use gcu_core::driver::{Button, Cancel, CaptureOpts, Driver, DriverAction, PointerRoute};
use gcu_core::model::{Rect, WindowPoint};
use gcu_driver_macos::MacosDriver;
use std::time::Duration;

pub(super) async fn press_counter(driver: &MacosDriver) -> Result<(), String> {
    let win = fixture_window(driver).await?;
    let before = counter_now();
    let root = tree(driver, &win).await?;
    let button = find(&root, &|node| named(node, &["counter_button", "Increment"]))
        .ok_or("counter button missing")?;
    driver
        .perform(
            &DriverAction::Press {
                el: button.handle.clone(),
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    oracle::poll_until(
        &oracle::fixture_state_path(),
        Duration::from_secs(5),
        |state| {
            state
                .get("counter")
                .and_then(|value| value.as_u64())
                .filter(|value| *value == before + 1)
        },
    )
    .await
    .map(|_| ())
}

pub(super) async fn click_grid(driver: &MacosDriver) -> Result<(), String> {
    let win = fixture_window(driver).await?;
    let before =
        oracle::read_state(&oracle::fixture_state_path()).ok_or("fixture state missing")?;
    if before["tab"] != 0 {
        return Err(format!(
            "C06 requires All Controls tab, got {}",
            before["tab"]
        ));
    }
    let root = tree(driver, &win).await?;
    let canvas = fixture_tree::canvas(&root).ok_or("grid canvas missing")?;
    let frame = canvas.frame.ok_or("grid canvas frame missing")?;
    let capture = driver
        .capture(&win, &CaptureOpts::default(), &Cancel::default())
        .await
        .map_err(|error| error.to_string())?;
    let (width, height, rgba) = decode_png(&capture.bytes)?;
    if width != capture.width
        || height != capture.height
        || (f64::from(width) - capture.geometry.frame.w).abs() >= 1.0
        || (f64::from(height) - capture.geometry.frame.h).abs() >= 1.0
    {
        return Err("C06 screenshot dimensions differ from physical window geometry".into());
    }
    let cell = if before["grid_hit"] == serde_json::json!([3, 2]) {
        [2, 1]
    } else {
        [3, 2]
    };
    let at = fixture_tree::cell_pixel(&rgba, width, height, frame, cell)?;
    println!(
        "{}",
        serde_json::json!({
            "kind": "GRID_CLICK", "canvas_name": canvas.name, "canvas_role": canvas.role,
            "canvas_frame": frame, "screenshot": [width, height], "geometry": capture.geometry,
            "at": at, "expected_grid_hit": cell, "tab_before": before["tab"],
        })
    );
    driver
        .perform(
            &DriverAction::PointerClick {
                win: win.clone(),
                at,
                button: Button::Left,
                count: 1,
                route: PointerRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    oracle::poll_until(
        &oracle::fixture_state_path(),
        Duration::from_secs(5),
        |state| {
            (state["grid_hit"] == serde_json::json!(cell) && state["tab"] == before["tab"])
                .then_some(())
        },
    )
    .await
    .map(|_| ())
}

pub(super) async fn toggle_checkbox(driver: &MacosDriver) -> Result<(), String> {
    let before = flag("checked").or_else(|_| flag("checkbox"))?;
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    let box_node =
        find(&root, &|node| named(node, &["agree_checkbox", "Agree"])).ok_or("checkbox missing")?;
    driver
        .perform(
            &DriverAction::Press {
                el: box_node.handle.clone(),
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    oracle::poll_until(
        &oracle::fixture_state_path(),
        Duration::from_secs(5),
        |state| {
            let now = state
                .get("checked")
                .or_else(|| state.get("checkbox"))
                .and_then(|value| value.as_bool())?;
            (now != before).then_some(())
        },
    )
    .await
    .map(|_| ())
}

pub(super) async fn select_third_combo(driver: &MacosDriver) -> Result<(), String> {
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    let combo = fixture_tree::combo_popup(&root).ok_or("combo popup missing")?;
    let frame = combo.frame.ok_or("combo frame missing")?;
    driver
        .perform(
            &DriverAction::Press {
                el: combo.handle.clone(),
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    tokio::time::sleep(Duration::from_millis(80)).await;
    let root = tree(driver, &win).await?;
    if let Some(item) = find(&root, &|node| named(node, &["Item 3", "第三项"])) {
        driver
            .perform(
                &DriverAction::Press {
                    el: item.handle.clone(),
                },
                &Cancel::default(),
            )
            .await
            .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    } else {
        click_at(driver, &win, third_item_point(frame)).await?;
        if combo_is_third_within(Duration::from_millis(400))
            .await
            .is_err()
        {
            // AX press did not open the menu. Open it with the pointer, then
            // the third row. Do not press the popup a second time.
            click_at(driver, &win, center_of(frame)).await?;
            tokio::time::sleep(Duration::from_millis(80)).await;
            click_at(driver, &win, third_item_point(frame)).await?;
        }
    }
    combo_is_third_within(Duration::from_secs(5))
        .await
        .map(|_| ())
}

pub(super) async fn set_slider(driver: &MacosDriver) -> Result<(), String> {
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    let slider =
        find(&root, &|node| named(node, &["slider_field", "Volume"])).ok_or("slider missing")?;
    driver
        .perform(
            &DriverAction::SetValue {
                el: slider.handle.clone(),
                value: "80".into(),
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    oracle::poll_until(
        &oracle::fixture_state_path(),
        Duration::from_secs(5),
        |state| {
            let value = state
                .get("slider")
                .or_else(|| state.get("value"))
                .and_then(|item| item.as_f64())?;
            ((value - 80.0).abs() < 0.5).then_some(())
        },
    )
    .await
    .map(|_| ())
}

pub(super) async fn scroll_list(driver: &MacosDriver) -> Result<(), String> {
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    let at = fixture_tree::virtual_list_point(&root).ok_or("virtual list rows missing")?;
    // Apple's positive wheel is up. Try one sign, then the other if the list
    // did not move. The oracle still requires visible_range start > 0.
    scroll_at(driver, &win, at, -480).await?;
    if list_moved_within(Duration::from_millis(800)).await.is_err() {
        scroll_at(driver, &win, at, 480).await?;
    }
    list_moved_within(Duration::from_secs(5)).await.map(|_| ())
}

pub(super) async fn modal_round_trip(driver: &MacosDriver) -> Result<(), String> {
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    let open = find(&root, &|node| {
        named(node, &["open_modal_button", "Open Modal", "打开弹窗"])
    })
    .ok_or("open modal missing")?;
    driver
        .perform(
            &DriverAction::Press {
                el: open.handle.clone(),
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    oracle::poll_until(
        &oracle::fixture_state_path(),
        Duration::from_secs(5),
        |state| {
            (state.get("modal_open").and_then(|value| value.as_bool()) == Some(true)).then_some(())
        },
    )
    .await?;
    let root = tree(driver, &win).await?;
    let confirm = find(&root, &|node| {
        named(node, &["modal_confirm_button", "确定"])
    })
    .ok_or_else(|| format!("modal confirm missing; last state: {}", last_state()))?;
    driver
        .perform(
            &DriverAction::Press {
                el: confirm.handle.clone(),
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    oracle::poll_until(
        &oracle::fixture_state_path(),
        Duration::from_secs(5),
        |state| {
            let open = state.get("modal_open").and_then(|value| value.as_bool())?;
            let closed = state
                .get("modal_closed")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let count = state
                .get("modal_closed_count")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            (!open && (closed || count > 0)).then_some(())
        },
    )
    .await
    .map(|_| ())
}

pub(super) async fn drag_source(driver: &MacosDriver) -> Result<(), String> {
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    let source = find(&root, &|node| named(node, &["drag_source", "Drag Me"]))
        .ok_or("drag source missing")?;
    let target = find(&root, &|node| named(node, &["drop_target", "Target"]))
        .ok_or("drop target missing")?;
    driver
        .perform(
            &DriverAction::PointerDrag {
                win,
                path: vec![center(source)?, center(target)?],
                route: PointerRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    oracle::poll_until(
        &oracle::fixture_state_path(),
        Duration::from_secs(5),
        |state| {
            let ok = state
                .get("drop_ok")
                .or_else(|| state.get("drag_drop_ok"))
                .and_then(|value| value.as_bool())?;
            ok.then_some(())
        },
    )
    .await
    .map(|_| ())
}

pub(super) async fn background_press(driver: &MacosDriver) -> Result<(), String> {
    let launched = !process_running("Calculator");
    let status = std::process::Command::new("open")
        .args(["-a", "Calculator"])
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err("open Calculator failed".into());
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        if is_calculator(&frontmost()?) {
            break;
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "Calculator did not become frontmost ({})",
                frontmost()?
            ));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let before = counter_now();
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    let button = find(&root, &|node| named(node, &["counter_button", "Increment"]))
        .ok_or("counter button missing")?;
    let clicked = driver
        .perform(
            &DriverAction::PointerClick {
                win,
                at: center(button)?,
                button: Button::Left,
                count: 1,
                route: PointerRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await;
    let arrived = oracle::poll_until(
        &oracle::fixture_state_path(),
        Duration::from_secs(5),
        |state| {
            state
                .get("counter")
                .and_then(|value| value.as_u64())
                .filter(|value| *value == before + 1)
        },
    )
    .await;
    let front = frontmost()?;
    if launched {
        let _ = std::process::Command::new("osascript")
            .args(["-e", "tell application \"Calculator\" to quit"])
            .status();
    }
    clicked.map_err(|error| format!("{error}; last state: {}", last_state()))?;
    arrived.map_err(|error| format!("field 51 click did not arrive: {error}"))?;
    if !is_calculator(&front) {
        return Err(format!("background press left {front} in front"));
    }
    Ok(())
}

async fn click_at(
    driver: &MacosDriver,
    win: &gcu_core::driver::WindowRef,
    at: WindowPoint,
) -> Result<(), String> {
    driver
        .perform(
            &DriverAction::PointerClick {
                win: win.clone(),
                at,
                button: Button::Left,
                count: 1,
                route: PointerRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))
        .map(|_| ())
}

fn center_of(frame: Rect) -> WindowPoint {
    WindowPoint {
        x: frame.x + frame.w / 2.0,
        y: frame.y + frame.h / 2.0,
    }
}

fn third_item_point(frame: Rect) -> WindowPoint {
    WindowPoint {
        x: frame.x + frame.w / 2.0,
        y: frame.y + frame.h * 3.5,
    }
}

async fn combo_is_third_within(timeout: Duration) -> Result<u64, String> {
    oracle::poll_until(&oracle::fixture_state_path(), timeout, |state| {
        state
            .get("combo_selected")
            .or_else(|| state.get("selected"))
            .and_then(|value| value.as_u64())
            .filter(|value| *value == 2)
    })
    .await
}

async fn scroll_at(
    driver: &MacosDriver,
    win: &gcu_core::driver::WindowRef,
    at: WindowPoint,
    dy: i32,
) -> Result<(), String> {
    driver
        .perform(
            &DriverAction::PointerScroll {
                win: win.clone(),
                at,
                dx: 0,
                dy,
                route: PointerRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))
        .map(|_| ())
}

async fn list_moved_within(timeout: Duration) -> Result<(), String> {
    oracle::poll_until(&oracle::fixture_state_path(), timeout, |state| {
        let range = state
            .get("visible_range")
            .or_else(|| state.get("list_visible_range"))?
            .as_array()?;
        let start = range.first()?.as_u64()?;
        (start > 0).then_some(())
    })
    .await
    .map(|_| ())
}
