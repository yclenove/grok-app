//! Ignored live table. Grant Accessibility and Screen Recording, start gcu-fixture,
//! then: `cargo test -p gcu-driver-macos --test contract -- --ignored --test-threads=1`

use std::time::Duration;

use gcu_core::driver::{
    AppQuery, Button, Cancel, CaptureOpts, Dir, Driver, DriverAction, KeyChord, KeyRoute,
    PointerRoute, TreeOpts, WindowRef,
};
use gcu_core::model::{RawNode, Role, WindowPoint};
use gcu_driver_macos::MacosDriver;

use super::oracle;
use super::report::{self, NOT_RUN};
use super::specials;
use super::table::{self, CaseResult, Facts};

#[tokio::test]
#[ignore = "macOS GUI session with TCC; not_run on GitHub macos-14"]
async fn live_contract_table_suite() {
    let driver = MacosDriver::new().await.expect("unelevated macOS driver");
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
    for row in table::ROWS {
        if let Some(reason) = table::skip_reason(row, &caps, &facts) {
            println!("  [{:^12}] {} -> SKIP ({reason})", row.id, row.description);
            results.push(CaseResult::skipped(row.id, row.description, reason));
            continue;
        }
        let ran = dispatch(&driver, row.id).await;
        match ran {
            Ok(()) => {
                println!("  [{:^12}] {} -> PASS", row.id, row.description);
                results.push(CaseResult::passed(row.id, row.description));
            }
            Err(error) => {
                println!("  [{:^12}] {} -> FAIL: {error}", row.id, row.description);
                results.push(CaseResult::failed(row.id, row.description, error));
                failed += 1;
            }
        }
    }
    let not_run = [NOT_RUN[1]];
    report::archive(&results, &not_run).expect("contract archive");
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

async fn fixture_window(driver: &MacosDriver) -> Result<WindowRef, String> {
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

async fn tree(driver: &MacosDriver, win: &WindowRef) -> Result<RawNode, String> {
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
    raw.root.ok_or_else(|| "fixture root missing".into())
}

fn find<'a>(node: &'a RawNode, pred: &impl Fn(&RawNode) -> bool) -> Option<&'a RawNode> {
    if pred(node) {
        return Some(node);
    }
    node.children.iter().find_map(|child| find(child, pred))
}

fn named(node: &RawNode, keys: &[&str]) -> bool {
    let id = node.identifier.as_deref().unwrap_or("");
    let name = node.name.as_deref().unwrap_or("");
    keys.iter()
        .any(|key| id.contains(key) || name.contains(key))
}

fn center(node: &RawNode) -> Result<WindowPoint, String> {
    let frame = node.frame.ok_or("node frame missing")?;
    Ok(WindowPoint {
        x: frame.x + frame.w / 2.0,
        y: frame.y + frame.h / 2.0,
    })
}

fn last_state() -> String {
    oracle::read_state(&oracle::fixture_state_path())
        .map(|state| state.to_string())
        .unwrap_or_else(|| "<no state read>".into())
}

async fn number(
    path: &str,
    want: impl Fn(serde_json::Value) -> Option<bool>,
) -> Result<(), String> {
    oracle::poll_until(
        &oracle::fixture_state_path(),
        Duration::from_secs(5),
        |state| state.pointer(path).cloned().and_then(&want),
    )
    .await
    .map(|_| ())
}

async fn c01(driver: &MacosDriver) -> Result<(), String> {
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

async fn c02(driver: &MacosDriver) -> Result<(), String> {
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

async fn c03(driver: &MacosDriver) -> Result<(), String> {
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

async fn c04(driver: &MacosDriver) -> Result<(), String> {
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

async fn press_counter(driver: &MacosDriver) -> Result<(), String> {
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

async fn click_grid(driver: &MacosDriver) -> Result<(), String> {
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    let canvas = find(&root, &|node| named(node, &["grid_canvas", "Canvas Grid"]))
        .ok_or("grid canvas missing")?;
    let at = center(canvas)?;
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
    number("/grid_hit", |value| value.as_array().map(|_| true)).await
}

async fn set_text(driver: &MacosDriver, text: &str) -> Result<(), String> {
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    let field =
        find(&root, &|node| named(node, &["name_field", "Name"])).ok_or("name field missing")?;
    driver
        .perform(
            &DriverAction::SetValue {
                el: field.handle.clone(),
                value: text.into(),
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    expect_text(text).await
}

async fn type_sample(driver: &MacosDriver) -> Result<(), String> {
    let sample = "中文\u{200D}🚀\n";
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    let field =
        find(&root, &|node| named(node, &["name_field", "Name"])).ok_or("name field missing")?;
    driver
        .perform(
            &DriverAction::TypeText {
                win,
                focus: Some(field.handle.clone()),
                text: sample.into(),
                route: KeyRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    oracle::poll_until(
        &oracle::fixture_state_path(),
        Duration::from_secs(5),
        |state| {
            let text = state.get("text").and_then(|value| value.as_str())?;
            (text.contains('中') && text.contains('\u{200D}')).then_some(())
        },
    )
    .await
    .map(|_| ())
}

async fn clear_with_chord(driver: &MacosDriver) -> Result<(), String> {
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    let field =
        find(&root, &|node| named(node, &["name_field", "Name"])).ok_or("name field missing")?;
    driver
        .perform(
            &DriverAction::TypeText {
                win: win.clone(),
                focus: Some(field.handle.clone()),
                text: "clear_target".into(),
                route: KeyRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    expect_text("clear_target").await?;
    for chord in ["cmd+a", "BackSpace"] {
        driver
            .perform(
                &DriverAction::PressKeys {
                    win: win.clone(),
                    chord: KeyChord::parse(chord).map_err(|error| error.to_string())?,
                    route: KeyRoute::TargetedOnly,
                },
                &Cancel::default(),
            )
            .await
            .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    }
    oracle::poll_until(
        &oracle::fixture_state_path(),
        Duration::from_secs(5),
        |state| {
            state
                .get("text")
                .and_then(|value| value.as_str())
                .filter(|text| text.is_empty())
                .map(|_| ())
        },
    )
    .await
    .map(|_| ())
}

async fn toggle_checkbox(driver: &MacosDriver) -> Result<(), String> {
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

async fn select_third_combo(driver: &MacosDriver) -> Result<(), String> {
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    let combo = find(&root, &|node| named(node, &["combo_box"])).ok_or("combo box missing")?;
    driver
        .perform(
            &DriverAction::Press {
                el: combo.handle.clone(),
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    let root = tree(driver, &win).await?;
    let item = find(&root, &|node| named(node, &["Item 3", "第三项"]))
        .ok_or_else(|| format!("third combo item missing; last state: {}", last_state()))?;
    driver
        .perform(
            &DriverAction::Press {
                el: item.handle.clone(),
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
                .get("combo_selected")
                .or_else(|| state.get("selected"))
                .and_then(|value| value.as_u64())
                .filter(|value| *value == 2)
        },
    )
    .await
    .map(|_| ())
}

async fn set_slider(driver: &MacosDriver) -> Result<(), String> {
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

async fn scroll_list(driver: &MacosDriver) -> Result<(), String> {
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    let list = find(&root, &|node| {
        named(node, &["virtual_list_scroll", "Item 0000"])
    })
    .ok_or("virtual list missing")?;
    driver
        .perform(
            &DriverAction::ScrollElement {
                el: list.handle.clone(),
                dir: Dir::Down,
                pages: 1.0,
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    oracle::poll_until(
        &oracle::fixture_state_path(),
        Duration::from_secs(5),
        |state| {
            let range = state
                .get("visible_range")
                .or_else(|| state.get("list_visible_range"))?
                .as_array()?;
            let start = range.first()?.as_u64()?;
            (start > 0).then_some(())
        },
    )
    .await
    .map(|_| ())
}

async fn modal_round_trip(driver: &MacosDriver) -> Result<(), String> {
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

async fn drag_source(driver: &MacosDriver) -> Result<(), String> {
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

async fn background_press(driver: &MacosDriver) -> Result<(), String> {
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

async fn lowercase_after_release(driver: &MacosDriver) -> Result<(), String> {
    driver.release_all_input().await;
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    let field =
        find(&root, &|node| named(node, &["name_field", "Name"])).ok_or("name field missing")?;
    driver
        .perform(
            &DriverAction::TypeText {
                win,
                focus: Some(field.handle.clone()),
                text: "xyz".into(),
                route: KeyRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    oracle::poll_until(
        &oracle::fixture_state_path(),
        Duration::from_secs(5),
        |state| {
            let text = state.get("text").and_then(|value| value.as_str())?;
            (text.to_lowercase() == text && text.contains("xyz") && !text.contains('X'))
                .then_some(())
        },
    )
    .await
    .map(|_| ())
}

async fn expect_text(text: &str) -> Result<(), String> {
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

fn counter_now() -> u64 {
    oracle::read_state(&oracle::fixture_state_path())
        .and_then(|state| state.get("counter").and_then(|value| value.as_u64()))
        .unwrap_or(0)
}

fn flag(name: &str) -> Result<bool, String> {
    oracle::read_state(&oracle::fixture_state_path())
        .and_then(|state| state.get(name).and_then(|value| value.as_bool()))
        .ok_or_else(|| format!("{name} missing; last state: {}", last_state()))
}

fn frontmost() -> Result<String, String> {
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

fn is_calculator(name: &str) -> bool {
    name == "Calculator" || name.contains("计算器")
}

fn process_running(name: &str) -> bool {
    std::process::Command::new("pgrep")
        .args(["-x", name])
        .status()
        .is_ok_and(|status| status.success())
}

fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(4096)];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|error| error.to_string())?;
    buf.truncate(info.buffer_size());
    Ok((info.width, info.height, buf))
}
