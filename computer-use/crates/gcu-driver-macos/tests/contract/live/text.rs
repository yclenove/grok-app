//! Live text contracts.
use super::super::oracle;
use super::support::*;
use gcu_core::driver::{
    Button, Cancel, Driver, DriverAction, DriverError, KeyChord, KeyRoute, PointerRoute, WindowRef,
};
use gcu_core::model::NativeHandle;
use gcu_driver_macos::MacosDriver;
use std::time::Duration;

async fn focus_name_field(driver: &MacosDriver) -> Result<(WindowRef, NativeHandle), String> {
    let win = fixture_window(driver).await?;
    let root = tree(driver, &win).await?;
    let field =
        find(&root, &|node| named(node, &["name_field", "Name"])).ok_or("name field missing")?;
    driver
        .perform(
            &DriverAction::PointerClick {
                win: win.clone(),
                at: center(field)?,
                button: Button::Left,
                count: 1,
                route: PointerRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await
        .map_err(|error| format!("{error}; last state: {}", last_state()))?;
    tokio::time::sleep(Duration::from_millis(80)).await;
    let root = tree(driver, &win).await?;
    let field =
        find(&root, &|node| named(node, &["name_field", "Name"])).ok_or("name field missing")?;
    Ok((win, field.handle.clone()))
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
        Err(DriverError::NotSettable) => {
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
    driver
        .perform(
            &DriverAction::TypeText {
                win,
                focus: Some(handle),
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
    driver.release_all_input().await;
    let (win, handle) = focus_name_field(driver).await?;
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
