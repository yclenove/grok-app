//! Opt-in native smoke tests. Run only on a disposable interactive macOS desktop.
#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_core::driver::{AppQuery, Driver, DriverError};
use gcu_driver_macos::api::MacApi;
use gcu_driver_macos::{MacosDriver, RealMacApi};
use std::time::{Duration, Instant};

fn fixture_path() -> String {
    std::env::var("GCU_FIXTURE_APP").expect("Set GCU_FIXTURE_APP to the built fixture .app")
}

#[tokio::test]
#[ignore = "requires macOS, Accessibility, Screen Recording, and GCU_FIXTURE_APP"]
async fn fixture_appears_in_list_apps() {
    let driver = MacosDriver::new()
        .await
        .expect("start an unelevated session");
    let path = fixture_path();
    let app = driver
        .resolve_app(&AppQuery::from(path.as_str()))
        .await
        .unwrap();
    if app.info.pid.is_none() {
        match driver.launch_app(&app).await {
            Ok(_) => {}
            Err(DriverError::Native {
                maybe_executed: true,
                ..
            }) => {}
            Err(error) => panic!("{error}"),
        }
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let apps = driver.list_apps().await.unwrap();
        if apps.iter().any(|row| row.id == app.info.id) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "fixture did not appear in list_apps"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test]
#[ignore = "requires two apps that share the display name in GCU_AMBIGUOUS_NAME"]
async fn resolve_reports_ambiguous_name() {
    let driver = MacosDriver::new()
        .await
        .expect("start an unelevated session");
    let name = std::env::var("GCU_AMBIGUOUS_NAME").expect("Set GCU_AMBIGUOUS_NAME");
    let error = driver
        .resolve_app(&AppQuery::from(name.as_str()))
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::AmbiguousApp);
}

#[tokio::test]
#[ignore = "launches GCU_FIXTURE_APP; leave another app frontmost and the fixture quit"]
async fn launch_shows_a_window_without_activation() {
    let api = RealMacApi::new();
    let before = api.frontmost_pid().unwrap();
    let driver = MacosDriver::new()
        .await
        .expect("start an unelevated session");
    let path = fixture_path();
    let app = driver
        .resolve_app(&AppQuery::from(path.as_str()))
        .await
        .unwrap();
    if app.info.pid.is_none() {
        driver.launch_app(&app).await.unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let resolved = driver
            .resolve_app(&AppQuery::from(path.as_str()))
            .await
            .unwrap();
        if resolved.info.pid.is_some() {
            let windows = driver.list_windows(Some(&resolved)).await.unwrap();
            if !windows.is_empty() {
                assert_eq!(api.frontmost_pid().unwrap(), before);
                assert!(windows
                    .iter()
                    .all(|window| window.on_current_desktop.is_none()));
                return;
            }
        }
        assert!(Instant::now() < deadline, "fixture window did not appear");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}
