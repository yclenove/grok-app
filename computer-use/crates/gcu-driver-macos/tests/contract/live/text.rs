//! Live text contracts.
use super::super::oracle;
use super::support::*;
use gcu_core::driver::{
    Button, Cancel, Driver, DriverAction, DriverError, KeyChord, KeyRoute, PointerRoute, WindowRef,
};
use gcu_core::model::{NativeHandle, WindowPoint};
use gcu_driver_macos::input::text::TextOptions;
use gcu_driver_macos::MacosDriver;
use std::time::Duration;

async fn focus_name_field(driver: &MacosDriver) -> Result<(WindowRef, NativeHandle), String> {
    let win = fixture_window(driver).await?;
    let at = name_point(driver, &win).await?;
    click_name(driver, &win, at).await?;
    if !name_focused().await {
        // Down and up in one sample can miss a background egui field. A same-point
        // drag waits one frame between them.
        driver
            .perform(
                &DriverAction::PointerDrag {
                    win: win.clone(),
                    path: vec![at, at],
                    route: PointerRoute::TargetedOnly,
                },
                &Cancel::default(),
            )
            .await
            .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    }
    if !name_focused().await {
        // Foreground Activate is fail-closed when idle is unknown. Keep the
        // error; do not treat a swallowed CapabilityUnavailable as success.
        let activate = driver
            .perform(
                &DriverAction::Activate { win: win.clone() },
                &Cancel::default(),
            )
            .await;
        click_name(driver, &win, at).await?;
        if !name_focused().await {
            let note = match activate {
                Ok(_) => "driver Activate returned without an error".to_string(),
                Err(error) => format!("driver Activate: {error}"),
            };
            return Err(format!(
                "name field did not take keyboard focus ({note}); last state: {}",
                last_state()
            ));
        }
    }
    let root = tree(driver, &win).await?;
    let field =
        find(&root, &|node| named(node, &["name_field", "Name"])).ok_or("name field missing")?;
    Ok((win, field.handle.clone()))
}

async fn name_point(driver: &MacosDriver, win: &WindowRef) -> Result<WindowPoint, String> {
    let root = tree(driver, win).await?;
    let field =
        find(&root, &|node| named(node, &["name_field", "Name"])).ok_or("name field missing")?;
    center(field)
}

async fn click_name(driver: &MacosDriver, win: &WindowRef, at: WindowPoint) -> Result<(), String> {
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

async fn name_focused() -> bool {
    oracle::poll_until(
        &oracle::fixture_state_path(),
        Duration::from_millis(300),
        |state| {
            state
                .get("focused_widget")
                .and_then(|value| value.as_str())
                .filter(|name| *name == "name_field")
                .map(|_| ())
        },
    )
    .await
    .is_ok()
}

pub(super) async fn set_text(driver: &MacosDriver, text: &str) -> Result<(), String> {
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    let field =
        find(&root, &|node| named(node, &["name_field", "Name"])).ok_or("name field missing")?;
    match driver
        .perform(
            &DriverAction::SetValue {
                el: field.handle.clone(),
                value: text.into(),
            },
            &Cancel::default(),
        )
        .await
    {
        Ok(_) => {}
        Err(DriverError::NotSettable { .. }) => {
            let (win, handle) = focus_name_field(driver).await?;
            driver
                .perform(
                    &DriverAction::TypeText {
                        win,
                        focus: Some(handle),
                        text: text.into(),
                        route: KeyRoute::TargetedOnly,
                    },
                    &Cancel::default(),
                )
                .await
                .map_err(|error| format!("{error}; last state: {}", last_state()))?;
        }
        Err(error) => return Err(format!("{error}; last state: {}", last_state())),
    }
    expect_text(text).await
}

pub(super) async fn type_sample(driver: &MacosDriver) -> Result<(), String> {
    let sample = "中文\u{200D}🚀\n";
    let (win, handle) = focus_name_field(driver).await?;
    // Unicode CGEvent chunks do not become egui text for this field. Targeted
    // cmd+v is the path that covers the same characters.
    driver
        .perform_text(
            &DriverAction::TypeText {
                win,
                focus: Some(handle),
                text: sample.into(),
                route: KeyRoute::TargetedOnly,
            },
            TextOptions {
                allow_clipboard: true,
                sensitive_text: false,
                high_risk_target: false,
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

pub(super) async fn clear_with_chord(driver: &MacosDriver) -> Result<(), String> {
    let (win, handle) = focus_name_field(driver).await?;
    driver
        .perform(
            &DriverAction::TypeText {
                win: win.clone(),
                focus: Some(handle),
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

pub(super) async fn lowercase_after_release(driver: &MacosDriver) -> Result<(), String> {
    // C16 leaves Calculator in front when that app was already running, and
    // nothing between C16 and C23 restores the fixture. C23 checks lowercase
    // typing after release_all_input, not background delivery.
    activate_fixture_harness().await?;
    let (win, handle) = focus_name_field(driver).await?;
    driver.release_all_input().await;
    if !name_focused().await {
        return Err(format!(
            "release_all_input dropped name field focus; last state: {}",
            last_state()
        ));
    }
    driver
        .perform(
            &DriverAction::TypeText {
                win,
                focus: Some(handle),
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
