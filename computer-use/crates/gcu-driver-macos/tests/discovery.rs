//! Portable discovery, alignment, cache, and gate regressions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_core::driver::{
    AppQuery, Cancel, CaptureOpts, Driver, DriverAction, DriverError, TreeOpts, WindowRef,
};
use gcu_driver_macos::{
    api::{BundleFacts, RunningApp},
    apps::{list_catalog, resolve_loaded, ResolveOutcome},
    gate::security_prompt,
    identity::identity_from_facts,
    spotlight::{cache_decision, within_recent, CacheUse, CACHE_TTL, RECENT_WINDOW},
    testing::{ax_window, cg_window, recent_hit, rect, running_app, signed_facts, FakeMacApi},
    windows::align_pid,
    MacosDriver,
};
use gcu_protocol::capabilities::{BackgroundOperation, ForegroundCapability, Platform};
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

async fn start(api: Arc<FakeMacApi>) -> MacosDriver {
    MacosDriver::with_api(api).await.unwrap()
}

fn pair(pid: u32, facts: BundleFacts) -> (RunningApp, BundleFacts) {
    (running_app(pid, &facts), facts)
}

#[test]
fn resolve_ambiguous_name_keeps_sorted_candidates() {
    let left = signed_facts("com.example.B", "/Other/Editor.app");
    let right = signed_facts("com.example.A", "/Applications/Editor.app");
    let outcome = resolve_loaded(
        &[pair(2, left), pair(1, right)],
        &[],
        None,
        "Editor",
        SystemTime::now(),
    )
    .unwrap();
    match outcome {
        ResolveOutcome::Ambiguous(apps) => {
            assert_eq!(apps.len(), 2);
            assert_eq!(apps[0].identity.bundle_id.as_deref(), Some("com.example.A"));
            assert_eq!(apps[1].identity.bundle_id.as_deref(), Some("com.example.B"));
        }
        other => panic!("expected ambiguity, got {other:?}"),
    }
}

#[tokio::test]
async fn resolve_app_reports_ambiguous_without_choosing() {
    let api = Arc::new(FakeMacApi::default());
    api.add_running(
        1,
        &signed_facts("com.example.A", "/Applications/Editor.app"),
    );
    api.add_running(2, &signed_facts("com.example.B", "/Other/Editor.app"));
    let driver = start(api).await;
    assert_eq!(
        driver
            .resolve_app(&AppQuery::from("Editor"))
            .await
            .unwrap_err(),
        DriverError::AmbiguousApp
    );
}

#[tokio::test]
async fn resolve_matches_localized_name_bundle_id_and_path() {
    let api = Arc::new(FakeMacApi::default());
    let mut facts = signed_facts(
        "com.apple.finder",
        "/System/Library/CoreServices/Finder.app",
    );
    facts.localized_name = "访达".into();
    facts.bundle_name = "Finder".into();
    api.add_running(10, &facts);
    let driver = start(api).await;
    for query in [
        "访达",
        "Finder",
        "com.apple.finder",
        "/system/library/coreservices/finder.app",
    ] {
        let app = driver.resolve_app(&AppQuery::from(query)).await.unwrap();
        assert_eq!(app.info.id, "com.apple.finder");
        assert_eq!(app.info.name, "访达");
    }
}

#[tokio::test]
async fn resolve_absolute_path_outside_spotlight() {
    let api = Arc::new(FakeMacApi::default());
    let facts = signed_facts("com.example.Fixture", "/Applications/Fixture.app");
    api.update(|state| {
        state.facts.insert(facts.bundle_path.clone(), facts);
    });
    let driver = start(api).await;
    let app = driver
        .resolve_app(&AppQuery::from("/applications/fixture.app/"))
        .await
        .unwrap();
    assert_eq!(app.info.id, "com.example.Fixture");
    assert_eq!(app.info.pid, None);
}

#[test]
fn align_matches_iou_and_keeps_unmatched_windows() {
    let mut matched = cg_window(7, 42, "Doc", rect(0.0, 0.0, 100.0, 100.0));
    matched.on_current_desktop = Some(true);
    let cg = [
        matched,
        cg_window(8, 42, "Other", rect(400.0, 400.0, 50.0, 40.0)),
    ];
    let ax = [ax_window(3, "Doc", rect(1.0, 1.0, 98.0, 99.0))];
    let aligned = align_pid(42, &cg, &ax);
    assert_eq!(aligned.len(), 2);
    assert_eq!(aligned[0].ax_index, Some(3));
    assert!(aligned[0].main);
    assert_eq!(aligned[0].on_current_desktop, Some(true));
    assert_eq!(aligned[1].ax_index, None);
    assert_eq!(aligned[1].on_current_desktop, None);
    assert_eq!(aligned[0].id, "cg:7:42");
}

#[test]
fn desktop_and_tiny_windows_are_dropped() {
    let mut desktop = cg_window(1, 1, "", rect(0.0, 0.0, 800.0, 600.0));
    desktop.owner_name = "Window Server".into();
    let mut tiny = cg_window(2, 9, "x", rect(0.0, 0.0, 1.0, 1.0));
    tiny.owner_name = "App".into();
    assert!(align_pid(1, &[desktop], &[]).is_empty());
    assert!(align_pid(9, &[tiny], &[]).is_empty());
}

#[test]
fn cache_decision_uses_injected_instants() {
    let now = Instant::now();
    assert_eq!(cache_decision(None, now, false), CacheUse::Miss);
    assert_eq!(
        cache_decision(Some(now), now, false),
        CacheUse::Fresh { indexing: false }
    );
    assert_eq!(
        cache_decision(Some(now), now, true),
        CacheUse::Fresh { indexing: true }
    );
    let stale = now.checked_sub(CACHE_TTL + Duration::from_secs(1)).unwrap();
    assert_eq!(cache_decision(Some(stale), now, false), CacheUse::Stale);
}

#[test]
fn recent_window_drops_missing_and_old_hits() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    assert!(!within_recent(None, now));
    assert!(within_recent(Some(now + Duration::from_secs(5)), now));
    let old = now
        .checked_sub(RECENT_WINDOW + Duration::from_secs(1))
        .unwrap();
    assert!(!within_recent(Some(old), now));
    let mut facts = signed_facts("com.example.Running", "/Applications/Running.app");
    facts.localized_name = "Running".into();
    let running = identity_from_facts(facts.clone()).unwrap();
    let resolved = gcu_driver_macos::apps::resolved_from(facts, Some(4), false).unwrap();
    assert_eq!(running.app_identity(), resolved.identity.app_identity());
    let mut hit = recent_hit("Old", "/Applications/Old.app", Some("com.example.Old"));
    hit.last_used = Some(old);
    let mut fresh = recent_hit(
        "Fresh",
        "/Applications/Fresh.app",
        Some("com.example.Fresh"),
    );
    fresh.last_used = Some(now);
    let listed = list_catalog(&[resolved], &[hit, fresh], now);
    assert!(listed.iter().any(|app| app.id == "com.example.Running"));
    assert!(listed
        .iter()
        .any(|app| app.name == "Fresh" && app.identity.is_none()));
    assert!(listed.iter().all(|app| app.name != "Old"));
}

#[tokio::test]
async fn list_apps_returns_before_spotlight_finishes() {
    let api = Arc::new(FakeMacApi::default());
    api.add_running(
        4,
        &signed_facts("com.example.Running", "/Applications/Running.app"),
    );
    api.update(|state| {
        state.spotlight_delay = Duration::from_millis(800);
        state.spotlight.push(recent_hit(
            "Recent",
            "/Applications/Recent.app",
            Some("com.example.Recent"),
        ));
    });
    let driver = start(api).await;
    let started = Instant::now();
    let apps = driver.list_apps().await.unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(400),
        "elapsed {:?}",
        started.elapsed()
    );
    assert!(driver.discovery_status().indexing);
    assert!(apps
        .iter()
        .any(|app| app.id == "com.example.Running" && app.pid == Some(4)));
    assert!(apps.iter().all(|app| app.name != "Recent"));
    let deadline = Instant::now() + Duration::from_secs(3);
    while driver.discovery_status().indexing {
        assert!(Instant::now() < deadline, "spotlight index did not finish");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let apps = driver.list_apps().await.unwrap();
    assert!(apps
        .iter()
        .any(|app| app.name == "Recent" && app.identity.is_none()));
}

#[tokio::test]
async fn missing_permissions_block_windows_not_app_list() {
    let api = Arc::new(FakeMacApi::default());
    api.add_running(
        4,
        &signed_facts("com.example.Editor", "/Applications/Editor.app"),
    );
    api.update(|state| state.accessibility = false);
    let driver = start(Arc::clone(&api)).await;
    assert!(driver
        .list_apps()
        .await
        .unwrap()
        .iter()
        .any(|app| app.pid == Some(4)));
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    assert_eq!(
        driver.list_windows(Some(&app)).await.unwrap_err(),
        DriverError::PermissionMissing {
            permission: "accessibility".into()
        }
    );
    api.update(|state| {
        state.accessibility = true;
        state.screen_recording = false;
    });
    assert_eq!(
        driver.list_windows(Some(&app)).await.unwrap_err(),
        DriverError::PermissionMissing {
            permission: "screen_recording".into()
        }
    );
    let report = driver.doctor().await;
    assert!(!report.ready);
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.contains("screen_recording")));
}

#[tokio::test]
async fn security_prompt_blocks_windows_launch_and_actions() {
    let api = Arc::new(FakeMacApi::default());
    let facts = signed_facts("com.example.Editor", "/Applications/Editor.app");
    api.add_running(4, &facts);
    api.update(|state| {
        state
            .cg_windows
            .push(cg_window(1, 4, "Editor", rect(0.0, 0.0, 200.0, 100.0)));
        state.ax_windows.insert(
            4,
            vec![ax_window(0, "Editor", rect(0.0, 0.0, 200.0, 100.0))],
        );
    });
    let driver = start(Arc::clone(&api)).await;
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let windows = driver.list_windows(Some(&app)).await.unwrap();
    assert_eq!(windows[0].on_current_desktop, None);
    api.update(|state| {
        state.running.push(running_app(9, &{
            let mut agent = signed_facts(
                "com.apple.SecurityAgent",
                "/System/Library/CoreServices/SecurityAgent.app",
            );
            agent.localized_name = "SecurityAgent".into();
            agent
        }));
        state.cg_windows.push(cg_window(
            2,
            9,
            "Authenticate",
            rect(20.0, 20.0, 300.0, 180.0),
        ));
    });
    assert_eq!(
        driver.list_windows(Some(&app)).await.unwrap_err(),
        DriverError::SecureDesktop
    );
    assert_eq!(
        driver.launch_app(&app).await.unwrap_err(),
        DriverError::SecureDesktop
    );
    assert!(api.state().unwrap().launches.is_empty());
    let win = WindowRef {
        app: app.clone(),
        id: windows[0].id.clone(),
    };
    assert_eq!(
        driver
            .perform(&DriverAction::Activate { win }, &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::SecureDesktop
    );
    let menu = signed_facts(
        "com.apple.systemuiserver",
        "/System/Library/CoreServices/SystemUIServer.app",
    );
    assert!(!security_prompt(
        &[running_app(3, &menu)],
        &[cg_window(5, 3, "Menu", rect(0.0, 0.0, 800.0, 24.0))]
    ));
}

#[tokio::test]
async fn root_engine_is_refused_at_startup() {
    let api = Arc::new(FakeMacApi::default());
    api.update(|state| state.euid = 0);
    let error = match MacosDriver::with_api(api).await {
        Err(error) => error,
        Ok(_) => panic!("root engine started"),
    };
    assert_eq!(error, DriverError::ElevationRequired);
}

#[tokio::test]
async fn launch_does_not_activate_or_change_frontmost() {
    let api = Arc::new(FakeMacApi::default());
    api.update(|state| {
        let facts = signed_facts("com.example.Editor", "/Applications/Editor.app");
        state.facts.insert(facts.bundle_path.clone(), facts.clone());
        state.spotlight.push(recent_hit(
            "Editor",
            &facts.bundle_path,
            Some("com.example.Editor"),
        ));
        state.frontmost = Some(11);
    });
    let driver = start(Arc::clone(&api)).await;
    let deadline = Instant::now() + Duration::from_secs(2);
    while driver.discovery_status().indexing {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    assert!(app.info.pid.is_none());
    let launched = driver.launch_app(&app).await.unwrap();
    assert_eq!(launched.info.id, app.info.id);
    let state = api.state().unwrap();
    assert_eq!(state.frontmost, Some(11));
    assert_eq!(state.launches.len(), 1);
    assert!(!state.launches[0].activates);
}

#[tokio::test]
async fn key_window_unhides_and_raises_without_activation() {
    let api = Arc::new(FakeMacApi::default());
    let facts = signed_facts("com.example.Editor", "/Applications/Editor.app");
    api.add_running(4, &facts);
    api.update(|state| {
        state.running[0].hidden = true;
        state.frontmost = Some(11);
        state
            .reveal_on_unhide
            .push(cg_window(7, 4, "Editor", rect(0.0, 0.0, 120.0, 80.0)));
        state.ax_windows.entry(4).or_default().push(ax_window(
            1,
            "Editor",
            rect(0.0, 0.0, 120.0, 80.0),
        ));
    });
    let driver = start(Arc::clone(&api)).await;
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let window = driver.key_window(&app).await.unwrap();
    assert_eq!(window.id, "cg:7:4");
    let state = api.state().unwrap();
    assert_eq!(state.unhides, vec![4]);
    assert_eq!(state.frontmost, Some(11));

    api.update(|state| {
        state.ax_windows.get_mut(&4).unwrap()[0].minimized = true;
        state.keep_minimized = false;
    });
    let window = driver.key_window(&app).await.unwrap();
    assert_eq!(window.id, "cg:7:4");
    assert_eq!(api.state().unwrap().raises, vec![(4, 1)]);
    assert_eq!(api.state().unwrap().frontmost, Some(11));

    api.update(|state| {
        state.ax_windows.get_mut(&4).unwrap()[0].minimized = true;
        state.keep_minimized = true;
    });
    assert_eq!(
        driver.key_window(&app).await.unwrap_err(),
        DriverError::WindowMinimized
    );
}

#[tokio::test]
async fn tree_capture_and_input_are_unavailable() {
    let api = Arc::new(FakeMacApi::default());
    let facts = signed_facts("com.example.Editor", "/Applications/Editor.app");
    api.add_running(4, &facts);
    api.update(|state| {
        state
            .cg_windows
            .push(cg_window(7, 4, "Editor", rect(0.0, 0.0, 80.0, 40.0)));
    });
    let driver = start(api).await;
    let caps = driver.capabilities();
    assert_eq!(caps.platform, Platform::Macos);
    assert!(caps.version.contains("P1-03"));
    assert!(caps.version.contains("P1-04"));
    assert!(caps.observe.tree && caps.observe.screenshot && caps.observe.events);
    assert_eq!(caps.delivery.foreground, ForegroundCapability::Unavailable);
    assert_eq!(caps.background_operation, BackgroundOperation::Partial);
    assert_eq!(caps.limits.max_tree_nodes, 5000);
    assert_eq!(caps.limits.max_image_edge, 2560);
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let windows = driver.list_windows(Some(&app)).await.unwrap();
    let win = WindowRef {
        app: app.clone(),
        id: windows[0].id.clone(),
    };
    let tree_error = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(
        tree_error,
        DriverError::Native {
            message: "accessibility tree source is not connected".into(),
            maybe_executed: false,
        }
    );
    assert_eq!(
        driver
            .capture(&win, &CaptureOpts::default(), &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::CapabilityUnavailable
    );
    assert_eq!(
        driver
            .perform(&DriverAction::Activate { win }, &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::CapabilityUnavailable
    );
    assert!(driver.focused_element(&app).await.unwrap_err() == DriverError::CapabilityUnavailable);
    driver.release_all_input().await;
}

#[tokio::test]
async fn changed_signature_invalidates_the_grant() {
    let api = Arc::new(FakeMacApi::default());
    let facts = signed_facts("com.example.Editor", "/Applications/Editor.app");
    api.add_running(4, &facts);
    let driver = start(Arc::clone(&api)).await;
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    api.update(|state| {
        let entry = state.facts.get_mut(&facts.bundle_path).unwrap();
        entry.signature.state = gcu_driver_macos::identity::SignatureState::Invalid;
    });
    assert_eq!(
        driver.launch_app(&app).await.unwrap_err(),
        DriverError::AppNotFound
    );
}
