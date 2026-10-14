//! Opt-in native smoke tests. Run only on a disposable interactive Windows desktop.
#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_core::driver::{AppQuery, AppRef, Driver};
use gcu_driver_windows::WindowsDriver;
use std::time::{Duration, Instant};

async fn driver() -> WindowsDriver {
    let driver = WindowsDriver::new_in_process()
        .await
        .expect("Use an unelevated interactive Windows session");
    let deadline = Instant::now() + Duration::from_secs(30);
    while driver.discovery_status().indexing {
        assert!(Instant::now() < deadline, "Start Menu index did not finish");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(driver.doctor().await.ready);
    driver
}
async fn launch_and_list(driver: &WindowsDriver, query: &str, running_query: &str) -> AppRef {
    let app = driver.resolve_app(&AppQuery::from(query)).await.unwrap();
    let launched = driver.launch_app(&app).await.unwrap();
    assert_eq!(launched.info.identity, app.info.identity);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(app) = driver.resolve_app(&AppQuery::from(running_query)).await {
            let windows = driver.list_windows(Some(&app)).await.unwrap();
            if !windows.is_empty() {
                assert!(windows.iter().all(|w| w.id.contains(':')));
                assert!(driver.key_window(&app).await.is_ok());
                assert!(driver
                    .list_apps()
                    .await
                    .unwrap()
                    .iter()
                    .any(|a| a.id == app.info.id));
                return app;
            }
        }
        assert!(Instant::now() < deadline, "No app window after launch");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
#[tokio::test]
#[ignore = "requires Windows interactive desktop and a built gcu-fixture"]
async fn fixture_list_resolve_launch_native_windows() {
    let driver = driver().await;
    let path =
        std::env::var("GCU_FIXTURE_EXE").expect("Set GCU_FIXTURE_EXE to the built fixture exe");
    launch_and_list(&driver, &path, &path).await;
}
#[tokio::test]
#[ignore = "launches system Notepad on a disposable Windows desktop"]
async fn notepad_list_resolve_launch_native_windows() {
    let driver = driver().await;
    let apps = driver.list_apps().await.unwrap();
    let packaged = apps
        .iter()
        .find(|a| a.id.to_lowercase().starts_with("microsoft.windowsnotepad_"));
    let path = format!(
        "{}\\System32\\notepad.exe",
        std::env::var("WINDIR").unwrap()
    );
    let query = packaged.map(|a| a.id.as_str()).unwrap_or(&path);
    let app = driver.resolve_app(&AppQuery::from(query)).await.unwrap();
    let running = app.info.id.clone();
    launch_and_list(&driver, query, &running).await;
}
#[tokio::test]
#[ignore = "launches UWP Settings on a disposable Windows desktop"]
async fn settings_list_resolve_launch_native_windows() {
    let driver = driver().await;
    let apps = driver.list_apps().await.unwrap();
    let app = apps
        .iter()
        .find(|a| {
            a.id.to_lowercase()
                .starts_with("windows.immersivecontrolpanel_")
        })
        .expect("Settings package is registered");
    let resolved = driver
        .resolve_app(&AppQuery::from(app.id.as_str()))
        .await
        .unwrap();
    let running = resolved.info.id.clone();
    let app = launch_and_list(&driver, &app.id, &running).await;
    assert!(!app
        .info
        .identity
        .unwrap()
        .executable
        .unwrap()
        .to_lowercase()
        .ends_with("applicationframehost.exe"));
}
