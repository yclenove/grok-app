//! Discovery must remain usable while unrelated native work is blocked.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_core::driver::{AppQuery, Driver, DriverError};
use gcu_driver_windows::{
    testing::{medium_security, signed_image, window, FakeWinApi},
    WindowsDriver,
};
use std::{sync::Arc, time::Duration};

fn fixture() -> Arc<FakeWinApi> {
    let api = Arc::new(FakeWinApi::default());
    api.update(|s| {
        for (pid, path) in [(7, "C:\\gcu-fixture.exe"), (9, "C:\\Slow.exe")] {
            s.paths.insert(pid, path.into());
            s.images.insert(path.into(), signed_image(path));
            s.processes.insert(pid, medium_security());
            s.windows.push(window(u64::from(pid), pid));
        }
        s.aumids.insert(7, "Example.Fixture!App".into());
        s.windows[0].title = "GCU Fixture".into();
    });
    api
}

// Only a deadlock watchdog uses time. Assertions depend on gates still being
// blocked when reads complete, not on elapsed time or process startup speed.
async fn bounded<T>(work: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), work)
        .await
        .expect("discovery waited for unrelated blocked native work")
}

#[tokio::test]
async fn list_apps_blocked_index_returns_hints_and_indexing() {
    let api = fixture();
    let blocked = api.block_index();
    let driver = WindowsDriver::with_api(api).await.unwrap();
    blocked.wait_entered().await;
    let apps = bounded(driver.list_apps()).await.unwrap();
    assert!(driver.discovery_status().indexing);
    // The initial catalog may still be empty; absence is not a discovery error.
    assert!(apps.iter().all(|app| app.pid.is_some()));
    blocked.release();
}

#[tokio::test]
async fn startup_blocked_cache_list_and_running_resolution_remain_available() {
    let api = fixture();
    let blocked = api.block_cache();
    let driver = bounded(WindowsDriver::with_api(api)).await.unwrap();
    blocked.wait_entered().await;
    assert!(bounded(driver.list_apps()).await.is_ok());
    assert!(driver.discovery_status().indexing);
    let app = bounded(driver.resolve_app(&AppQuery::from("fixture")))
        .await
        .unwrap();
    assert_eq!(app.info.pid, Some(7));
    blocked.release();
}

#[tokio::test]
async fn resolve_running_blocked_index_matches_fixture_name_path_and_aumid() {
    let api = fixture();
    let blocked = api.block_index();
    let driver = WindowsDriver::with_api(api).await.unwrap();
    blocked.wait_entered().await;
    for query in [
        "fixture",
        "gcu-fixture.exe",
        "C:\\gcu-fixture.exe",
        "Example.Fixture!App",
    ] {
        let app = bounded(driver.resolve_app(&AppQuery::from(query)))
            .await
            .unwrap();
        assert_eq!(app.info.pid, Some(7), "query={query}");
        assert!(
            app.info.identity.is_some(),
            "fast resolution must verify identity"
        );
    }
    blocked.release();
}

#[tokio::test]
async fn reads_blocked_catalog_signature_resolve_and_list_target_windows() {
    let api = fixture();
    let blocked = api.block_image("C:\\Slow.exe");
    let driver = WindowsDriver::with_api(api).await.unwrap();
    blocked.wait_entered().await;
    let apps = bounded(driver.list_apps()).await.unwrap();
    assert!(apps.iter().any(|app| app.pid == Some(7)));
    assert!(driver.discovery_status().indexing);
    let app = bounded(driver.resolve_app(&AppQuery::from("fixture")))
        .await
        .unwrap();
    assert_eq!(
        bounded(driver.list_windows(Some(&app)))
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(bounded(driver.list_windows(None)).await.unwrap().len(), 2);
    blocked.release();
}

#[tokio::test]
async fn reads_blocked_installed_resolution_keep_running_queue_available() {
    let api = fixture();
    let driver = Arc::new(WindowsDriver::with_api(api.clone()).await.unwrap());
    let blocked = api.block_image("C:\\Installed.exe");
    api.image("C:\\Installed.exe", signed_image("C:\\Installed.exe"));
    let installed_driver = driver.clone();
    let installed = tokio::spawn(async move {
        installed_driver
            .resolve_app(&AppQuery::from("C:\\Installed.exe"))
            .await
    });
    blocked.wait_entered().await;
    assert!(bounded(driver.list_apps()).await.is_ok());
    let app = bounded(driver.resolve_app(&AppQuery::from("fixture")))
        .await
        .unwrap();
    assert_eq!(
        bounded(driver.list_windows(Some(&app)))
            .await
            .unwrap()
            .len(),
        1
    );
    blocked.release();
    assert!(installed.await.unwrap().is_ok());
}

#[tokio::test]
async fn fast_resolution_updated_image_still_invalidates_old_authorization() {
    let api = fixture();
    let driver = WindowsDriver::with_api(api.clone()).await.unwrap();
    let app = driver
        .resolve_app(&AppQuery::from("fixture"))
        .await
        .unwrap();
    api.update(|s| s.images.get_mut("C:\\gcu-fixture.exe").unwrap().sha256 = "f".repeat(64));
    assert_eq!(
        driver.list_windows(Some(&app)).await.unwrap_err(),
        DriverError::AppNotFound
    );
    assert_ne!(
        driver
            .resolve_app(&AppQuery::from("fixture"))
            .await
            .unwrap()
            .info
            .identity,
        app.info.identity
    );
}
