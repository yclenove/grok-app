//! Portable window/registry/cache discovery regressions.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_core::driver::{
    AppQuery, Cancel, CaptureOpts, Driver, DriverAction, DriverError, TreeOpts, WindowRef,
};
use gcu_driver_windows::{
    apps::{decode_cache, encode_cache},
    desktop::{visible_windows, window_process, HwndStamp},
    testing::{medium_security, signed_image, window, FakeWinApi},
    WindowsDriver,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
mod common;
use common::{fixture, indexed, shortcut};

#[tokio::test]
async fn elevated_visible_process_refused_before_unreadable_image_verification() {
    let (api, driver, _, _) = fixture().await;
    api.update(|s| {
        s.paths.insert(9, "C:\\High\\hi.exe".into());
        let mut high = medium_security();
        high.integrity = gcu_driver_windows::gate::IntegrityLevel::High;
        s.processes.insert(9, high);
        s.windows.push(window(10, 9));
        // Medium cannot read this High-labelled file; image() would be NotFound.
        s.image_reads.clear();
    });
    assert_eq!(
        driver
            .resolve_app(&AppQuery::from("C:\\High\\hi.exe"))
            .await,
        Err(DriverError::ElevationRequired)
    );
    assert!(!api
        .state()
        .unwrap()
        .image_reads
        .iter()
        .any(|p| p.ends_with("hi.exe")));
}

#[tokio::test]
async fn disappearing_unrelated_windows_do_not_accumulate_app_errors_in_doctor() {
    let (api, driver, app, _) = fixture().await;
    api.update(|s| {
        for pid in 100..110 {
            s.windows.push(window(pid as u64, pid));
        }
    });
    for _ in 0..3 {
        driver.list_windows(Some(&app)).await.unwrap();
        driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    }
    assert!(!driver
        .doctor()
        .await
        .issues
        .iter()
        .any(|i| i == "app not found"));
}

#[test]
fn filtering_keeps_owned_dialogs_and_excludes_hidden_tool_windows() {
    let root = window(1, 7);
    let mut dialog = window(2, 7);
    dialog.owner = Some(1);
    let mut hidden = window(3, 7);
    hidden.visible = false;
    let mut tool = window(4, 7);
    tool.tool_window = true;
    let mut forced = tool.clone();
    forced.hwnd = 5;
    forced.app_window = true;
    let windows = visible_windows(vec![root, dialog, hidden, tool, forced]);
    assert_eq!(
        windows.iter().map(|w| w.hwnd).collect::<Vec<_>>(),
        vec![1, 2, 5]
    );
    assert_eq!(windows[1].owner, Some(1));
}

#[test]
fn hwnd_reuse_same_pid_class_different_creation_invalidates_stamp() {
    let old = window(8, 7);
    let stamp = HwndStamp::from(&old);
    for changed in ["window", "process", "class", "pid"] {
        let mut new = old.clone();
        match changed {
            "window" => new.created += 1,
            "process" => new.process_created += 1,
            "class" => new.class += "2",
            _ => new.pid += 1,
        };
        assert_eq!(stamp.validate(&new), Err(DriverError::WindowGone));
        assert_ne!(stamp.id(), HwndStamp::from(&new).id());
    }
}

#[tokio::test]
async fn hwnd_reuse_returns_window_gone_without_substitution() {
    let (api, driver, _, win) = fixture().await;
    api.update(|s| s.windows[0].created += 1);
    assert_eq!(
        driver
            .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::WindowGone
    );
    assert_eq!(
        driver
            .perform(&DriverAction::Activate { win }, &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::WindowGone
    );
}

#[test]
fn uwp_unwrap_selects_real_process_and_rejects_ambiguous_frames() {
    let api = FakeWinApi::default();
    let mut frame = window(8, 10);
    frame.child_pids = vec![10, 7, 7];
    api.update(|s| {
        s.paths
            .insert(10, "C:\\Windows\\System32\\ApplicationFrameHost.exe".into());
        s.paths.insert(7, "C:\\Settings.exe".into());
    });
    assert_eq!(window_process(&api, &frame), Ok(7));
    frame.child_pids.clear();
    assert_eq!(window_process(&api, &frame), Err(DriverError::AppNotFound));
    frame.child_pids = vec![7, 9];
    api.update(|s| {
        s.paths.insert(9, "C:\\Other.exe".into());
    });
    assert_eq!(window_process(&api, &frame), Err(DriverError::AmbiguousApp));
}

#[tokio::test]
async fn uwp_identity_binds_child_process_and_rechecks_child_instance() {
    let (api, driver, _, _) = fixture().await;
    api.update(|s| {
        s.paths
            .insert(10, "C:\\Windows\\System32\\ApplicationFrameHost.exe".into());
        s.processes.insert(10, medium_security());
        s.windows[0].pid = 10;
        s.windows[0].child_pids = vec![7];
    });
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    assert_eq!(app.info.pid, Some(7));
    let win = WindowRef {
        app: app.clone(),
        id: driver.key_window(&app).await.unwrap().id,
    };
    api.update(|s| s.processes.get_mut(&7).unwrap().created += 1);
    assert_eq!(
        driver
            .capture(&win, &CaptureOpts::default(), &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::WindowGone
    );
}

#[tokio::test]
async fn off_desktop_window_is_marked_and_foreground_restore_refused() {
    let (api, driver, app, win) = fixture().await;
    api.update(|s| {
        s.desktops.insert(8, Some(false));
        s.windows[0].minimized = true;
    });
    assert_eq!(
        driver.list_windows(Some(&app)).await.unwrap()[0].on_current_desktop,
        Some(false)
    );
    assert_eq!(
        driver.key_window(&app).await.unwrap_err(),
        DriverError::CapabilityUnavailable
    );
    assert_eq!(
        driver
            .perform(&DriverAction::Activate { win }, &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::CapabilityUnavailable
    );
    assert!(api.state().unwrap().restores.is_empty());
}

#[tokio::test]
async fn minimized_window_restores_exact_hwnd_without_activation() {
    let (api, driver, app, _) = fixture().await;
    api.update(|s| s.windows[0].minimized = true);
    assert!(driver.key_window(&app).await.is_ok());
    assert_eq!(api.state().unwrap().restores, vec![8]);
    assert!(!api.state().unwrap().windows[0].foreground);
}

#[tokio::test]
async fn first_call_blocked_index_returns_cached_hints() {
    let api = Arc::new(FakeWinApi::default());
    api.update(|s| {
        s.cache = Some(encode_cache(&[shortcut()]).unwrap());
        s.links.push(shortcut());
    });
    let blocked = api.block_index();
    let driver = WindowsDriver::with_api(api).await.unwrap();
    blocked.wait_entered().await; // Cache has been published; scan cannot finish.
    let apps = driver.list_apps().await.unwrap();
    assert_eq!(apps[0].name, "Editor");
    assert!(driver.discovery_status().indexing);
    blocked.release();
}

#[tokio::test]
async fn corrupt_cache_recovers_and_file_notification_refreshes_index() {
    let api = Arc::new(FakeWinApi::default());
    api.update(|s| {
        s.cache = Some(b"broken JSON".to_vec());
        s.links.push(shortcut());
    });
    let driver = WindowsDriver::with_api(api.clone()).await.unwrap();
    indexed(&api, &driver).await;
    assert_eq!(driver.list_apps().await.unwrap().len(), 1);
    assert!(decode_cache(api.state().unwrap().cache.as_ref().unwrap()).is_ok());
    assert!(driver
        .doctor()
        .await
        .issues
        .iter()
        .any(|i| i.contains("cache invalid")));
    api.update(|s| s.links.clear());
    api.notify_change();
    for _ in 0..100 {
        if driver.list_apps().await.unwrap().is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("notification did not update index");
}

#[tokio::test]
async fn cached_identity_is_reverified_before_launch_and_replaced_link_refused() {
    let api = Arc::new(FakeWinApi::default());
    api.update(|s| {
        s.links.push(shortcut());
        s.images
            .insert("C:\\Editor.exe".into(), signed_image("C:\\Editor.exe"));
    });
    let driver = WindowsDriver::with_api(api.clone()).await.unwrap();
    indexed(&api, &driver).await;
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    assert!(driver.launch_app(&app).await.is_ok());
    assert_eq!(api.state().unwrap().launches.len(), 1);
    api.update(|s| s.images.get_mut("C:\\Editor.exe").unwrap().sha256 = "f".repeat(64));
    assert_eq!(
        driver.launch_app(&app).await.unwrap_err(),
        DriverError::AppNotFound
    );
    api.update(|s| {
        s.images.get_mut("C:\\Editor.exe").unwrap().sha256 = "a".repeat(64);
        s.links[0].target = "C:\\Impostor.exe".into();
    });
    assert_eq!(
        driver.launch_app(&app).await.unwrap_err(),
        DriverError::AppNotFound
    );
    assert_eq!(api.state().unwrap().launches.len(), 1);
}

#[test]
fn cache_bad_json_version_and_oversized_input_rejected() {
    for bytes in [
        b"{}".as_slice(),
        br#"{"version":2,"shortcuts":[]}"#,
        br#"{"version":1,"shortcuts":[],"extra":true}"#,
    ] {
        assert!(decode_cache(bytes).is_err());
    }
    assert!(decode_cache(&vec![b' '; 4 * 1024 * 1024 + 1]).is_err());
    assert_eq!(
        decode_cache(&encode_cache(&[shortcut()]).unwrap()).unwrap(),
        vec![shortcut()]
    );
}

#[tokio::test]
async fn first_call_blocked_running_enumeration_returns_cached_hints() {
    let api = Arc::new(FakeWinApi::default());
    api.update(|s| {
        s.cache = Some(encode_cache(&[shortcut()]).unwrap());
        s.links.push(shortcut());
    });
    let scan = api.block_index();
    let enumeration = api.block_enumeration();
    let driver = WindowsDriver::with_api(api).await.unwrap();
    scan.wait_entered().await;
    enumeration.wait_entered().await;
    assert_eq!(driver.list_apps().await.unwrap()[0].name, "Editor");
    assert!(driver.discovery_status().indexing);
    scan.release();
    enumeration.release();
}

#[tokio::test]
async fn direct_executable_path_resolves_and_launches_without_shell_arguments() {
    let api = Arc::new(FakeWinApi::default());
    api.image("C:\\Editor.exe", signed_image("C:\\Editor.exe"));
    let driver = WindowsDriver::with_api(api.clone()).await.unwrap();
    let app = driver
        .resolve_app(&AppQuery::from("C:\\Editor.exe"))
        .await
        .unwrap();
    driver.launch_app(&app).await.unwrap();
    let launched = &api.state().unwrap().launches[0];
    assert_eq!(launched.target, "c:\\editor.exe");
    assert!(launched.link.is_empty());
    assert_eq!(
        driver
            .resolve_app(&AppQuery::from("C:\\Editor.exe --execute"))
            .await
            .unwrap_err(),
        DriverError::AppNotFound
    );
}

#[tokio::test]
async fn damaged_shortcut_does_not_hide_valid_index_entries() {
    let api = Arc::new(FakeWinApi::default());
    api.update(|s| {
        s.links.push(shortcut());
        s.broken_links.push("C:\\broken.lnk".into());
    });
    let driver = WindowsDriver::with_api(api.clone()).await.unwrap();
    indexed(&api, &driver).await;
    assert_eq!(driver.list_apps().await.unwrap().len(), 1);
    assert!(driver
        .doctor()
        .await
        .issues
        .iter()
        .any(|i| i.contains("app not found")));
}

#[tokio::test]
async fn restore_refused_by_app_returns_window_minimized() {
    let (api, driver, app, _) = fixture().await;
    api.update(|s| {
        s.windows[0].minimized = true;
        s.refuse_restore = true;
    });
    assert_eq!(
        driver.key_window(&app).await.unwrap_err(),
        DriverError::WindowMinimized
    );
}

#[tokio::test]
async fn unresolved_query_is_not_found_and_same_name_apps_are_ambiguous() {
    let (api, driver, _, _) = fixture().await;
    assert_eq!(
        driver
            .resolve_app(&AppQuery::from("Missing"))
            .await
            .unwrap_err(),
        DriverError::AppNotFound
    );
    api.update(|s| {
        s.paths.insert(9, "C:\\Other\\Editor.exe".into());
        s.images.insert(
            "C:\\Other\\Editor.exe".into(),
            signed_image("C:\\Other\\Editor.exe"),
        );
        s.processes.insert(9, medium_security());
        s.windows.push(window(10, 9));
    });
    assert_eq!(
        driver
            .resolve_app(&AppQuery::from("Editor"))
            .await
            .unwrap_err(),
        DriverError::AmbiguousApp
    );
}

#[tokio::test]
async fn uwp_elevated_frame_owner_is_refused_even_with_medium_child() {
    let (api, driver, _, _) = fixture().await;
    api.update(|s| {
        s.paths
            .insert(10, "C:\\Windows\\System32\\ApplicationFrameHost.exe".into());
        let mut owner = medium_security();
        owner.integrity = gcu_driver_windows::gate::IntegrityLevel::High;
        s.processes.insert(10, owner);
        s.windows[0].pid = 10;
        s.windows[0].child_pids = vec![7];
    });
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let win = WindowRef {
        app,
        id: driver.list_windows(None).await.unwrap()[0].id.clone(),
    };
    assert_eq!(
        driver
            .capture(&win, &CaptureOpts::default(), &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::ElevationRequired
    );
}
#[tokio::test]
async fn dropped_queued_restore_never_executes_after_caller_timeout() {
    let (api, driver, app, win) = fixture().await;
    let driver = Arc::new(driver);
    api.update(|s| s.windows[0].minimized = true);
    let blocked = api.block_enumeration();
    let read_driver = driver.clone();
    let read = tokio::spawn(async move { read_driver.window_geometry(&win).await });
    blocked.wait_entered().await;
    tokio::time::pause();
    assert!(
        tokio::time::timeout(Duration::from_millis(20), driver.key_window(&app))
            .await
            .is_err()
    );
    tokio::time::resume();
    blocked.release();
    let _ = read.await.unwrap();
    driver.doctor().await; // Barrier: queued restoration has been skipped.
    assert!(api.state().unwrap().restores.is_empty());
}
#[tokio::test]
async fn stalled_native_observation_returns_bounded_zero_effect_timeout() {
    let (api, driver, _, win) = fixture().await;
    api.update(|s| s.enum_delay = Duration::from_millis(200));
    let opts = CaptureOpts {
        timeout: Duration::from_millis(10),
        include_cursor: false,
    };
    let start = Instant::now();
    assert_eq!(
        driver
            .capture(&win, &opts, &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::Timeout {
            maybe_executed: false
        }
    );
    assert!(start.elapsed() < Duration::from_millis(100));
}
