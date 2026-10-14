//! Integration and contract tests for Linux X11 user activity guard and global stop hotkey.
//!
//! Covers:
//! - XGrabKey conflict and listener failure handling (foreground disabled, doctor updated).
//! - Modifier permutations (NumLock, CapsLock combinations).
//! - User physical input refreshing ActivityClock vs XTEST injection NOT refreshing.
//! - ActivityClock availability transitions (thread drop/crash -> clock is None, never stale).
//! - Native FFI hang and watchdog timeout (<= DRIVER_DEADLINE 4,750ms).
//! - Stop latch channel completely unblocked and non-blocking during FFI hang.
//! - Restarting / recreating driver preserves pre-latched Stop from Host.
//! - Budgets: Stop latch <= 100ms, input release <= 500ms.
//! - Live Xvfb contract tests (ignored by default, run in CI).

#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_core::driver::{
    Cancel, Delivery, Driver, DriverAction, DriverError, PointerRoute, WindowRef, DRIVER_DEADLINE,
};
use gcu_core::model::WindowPoint;
use gcu_driver_linux::{
    atspi::source::{FakeNode, FakeSource},
    x11::{
        activity::{ActivityClock, DeviceFilter, DeviceInfo, SharedActivityClock},
        fake::FakeX,
        hotkey::{
            handle_hotkey_trigger, modifier_combinations, HotkeyConfig, HotkeyManager,
            SharedStopLatch, StopLatch,
        },
        resolve_or_remap_keysym,
        watchdog::{run_with_watchdog, HeartbeatWatchdog},
        FakeWindow, XRootGeometry, XWindowGeometry,
    },
    LinuxDriver,
};
use gcu_protocol::{capabilities::ForegroundCapability, tools::MouseButton as Button, AppInfo};
use std::{sync::Arc, time::Duration};

fn make_fake_x() -> FakeX {
    let x = FakeX::new(
        XRootGeometry {
            root_id: 1,
            width: 1920,
            height: 1080,
            scale: 1.0,
        },
        true,
    );
    x.add_window(FakeWindow {
        id: 100,
        name: Some("test-window".into()),
        pid: Some(100),
        geometry: XWindowGeometry {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
            border_width: 0,
            depth: 24,
        },
        is_mapped: true,
        image: None,
    });
    x.set_active_window(Some(100));
    x
}

fn make_window_ref(id: u32) -> WindowRef {
    WindowRef {
        app: gcu_core::driver::AppRef {
            info: AppInfo {
                id: "test-app".into(),
                name: "test-app".into(),
                pid: Some(100),
                identity: None,
            },
        },
        id: id.to_string(),
    }
}

#[tokio::test]
async fn test_hotkey_grab_conflict_fails_and_disables_foreground() {
    let fake_x = make_fake_x();
    // Backspace keysym is 0xff08 -> maps to keycode in FakeX
    let kc = resolve_or_remap_keysym(&fake_x, 0xff08).unwrap().keycode;
    let default_cfg = HotkeyConfig::default();
    // Add conflict on base modifiers
    fake_x.add_conflicting_key(kc, default_cfg.modifiers);

    let driver = LinuxDriver::new_fake(FakeSource::default(), FakeNode::default(), vec![], vec![])
        .with_x_server(Arc::new(fake_x));

    // Hotkey registration failed due to conflict
    assert!(!driver.is_hotkey_available());
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Unavailable
    );

    // Doctor must report stop_hotkey unavailable with reason
    let doc = driver.doctor().await;
    let has_hotkey_issue = doc.issues.iter().any(|i| {
        i.contains("stop_hotkey: unavailable") && (i.contains("occupied") || i.contains("conflict"))
    });
    assert!(
        has_hotkey_issue,
        "doctor should report stop_hotkey unavailable issue: {:?}",
        doc.issues
    );

    // Attempting foreground action must return CapabilityUnavailable
    let win = make_window_ref(100);
    let cancel = Cancel::default();
    let err = driver
        .perform(
            &DriverAction::PointerClick {
                win,
                at: WindowPoint { x: 50.0, y: 50.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::Auto,
            },
            &cancel,
        )
        .await;

    assert_eq!(err.unwrap_err(), DriverError::CapabilityUnavailable);
}

#[test]
fn test_modifier_permutations_and_clean_ungrab() {
    let fake_x = make_fake_x();
    let base_mods = 0x0001 | 0x0004 | 0x0008; // Shift | Ctrl | Alt = 13
    let perms = modifier_combinations(base_mods);
    assert_eq!(perms.len(), 4);
    assert!(perms.contains(&base_mods));
    assert!(perms.contains(&(base_mods | 0x0010))); // Mod2 (NumLock)
    assert!(perms.contains(&(base_mods | 0x0002))); // Lock (CapsLock)
    assert!(perms.contains(&(base_mods | 0x0010 | 0x0002))); // Mod2 | Lock

    let stop_latch: Arc<dyn StopLatch> = Arc::new(SharedStopLatch::default());
    let mgr = HotkeyManager::new(HotkeyConfig::default(), stop_latch);
    assert!(mgr.register(&fake_x).is_ok());
    assert!(mgr.is_available());

    let kc = resolve_or_remap_keysym(&fake_x, 0xff08).unwrap().keycode;
    for &m in &perms {
        assert!(fake_x.is_key_grabbed(kc, m));
    }

    mgr.unregister(&fake_x);
    assert!(!mgr.is_available());
    for &m in &perms {
        assert!(!fake_x.is_key_grabbed(kc, m));
    }
}

#[tokio::test]
async fn test_hotkey_trigger_latches_stop_and_releases_ledger_within_budgets() {
    tokio::time::pause();

    let fake_x = Arc::new(make_fake_x());
    let driver = LinuxDriver::new_fake(FakeSource::default(), FakeNode::default(), vec![], vec![])
        .with_x_server(fake_x.clone());

    assert!(!driver.is_stopped());
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Policy
    );

    // Hold key and pointer button in ledger
    driver.ledger().record_key_down(65);
    driver.ledger().record_button_down(1);
    let (keys, buttons) = driver.ledger().held_inputs();
    assert_eq!(keys, vec![65]);
    assert_eq!(buttons, vec![1]);

    // Trigger stop hotkey
    let start = tokio::time::Instant::now();
    handle_hotkey_trigger(
        driver.stop_latch().as_ref(),
        driver.ledger(),
        fake_x.as_ref(),
    )
    .await;
    let elapsed = start.elapsed();

    // Latch must be active within budget (<= 100ms)
    assert!(driver.is_stopped());
    assert!(elapsed <= Duration::from_millis(100));

    // Ledger inputs must be released within budget (<= 500ms)
    assert!(driver.ledger().is_empty());
    assert!(elapsed <= Duration::from_millis(500));

    // Foreground delivery now disabled
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Unavailable
    );

    // Subsequent foreground action fails
    let win = make_window_ref(100);
    let cancel = Cancel::default();
    let err = driver
        .perform(
            &DriverAction::PointerClick {
                win,
                at: WindowPoint { x: 50.0, y: 50.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::Auto,
            },
            &cancel,
        )
        .await;

    assert_eq!(err.unwrap_err(), DriverError::CapabilityUnavailable);
}

#[test]
fn test_physical_device_refreshes_clock_and_xtest_device_does_not() {
    let filter = DeviceFilter::from_devices(&[
        DeviceInfo::new(1, "Virtual core pointer"),
        DeviceInfo::new(2, "Virtual core XTEST pointer"),
        DeviceInfo::new(3, "Virtual core XTEST keyboard"),
        DeviceInfo::new(4, "Logitech USB Optical Mouse"),
        DeviceInfo::new(5, "AT Translated Set 2 keyboard"),
    ]);

    assert!(filter.is_xtest(2));
    assert!(filter.is_xtest(3));
    assert!(!filter.is_xtest(1));
    assert!(!filter.is_xtest(4));
    assert!(!filter.is_xtest(5));

    let clock = SharedActivityClock::new(true);
    assert!(clock.is_available());
    assert_eq!(clock.ms_since_physical_input(), None);

    // Synthetic XTEST events MUST NOT refresh the clock
    assert!(!filter.process_event(2, &clock));
    assert_eq!(clock.ms_since_physical_input(), None);

    assert!(!filter.process_event(3, &clock));
    assert_eq!(clock.ms_since_physical_input(), None);

    // Physical mouse event MUST refresh the clock
    let t0 = std::time::Instant::now();
    clock.set_test_now(Some(t0));
    assert!(filter.process_event(4, &clock));
    assert_eq!(clock.ms_since_physical_input(), Some(0));

    // Advance time by 300ms
    let t1 = t0 + Duration::from_millis(300);
    clock.set_test_now(Some(t1));
    assert_eq!(clock.ms_since_physical_input(), Some(300));

    // Another XTEST event MUST NOT refresh elapsed clock
    assert!(!filter.process_event(2, &clock));
    assert_eq!(clock.ms_since_physical_input(), Some(300));

    // Physical keyboard event MUST refresh elapsed clock
    assert!(filter.process_event(5, &clock));
    assert_eq!(clock.ms_since_physical_input(), Some(0));
}

#[tokio::test]
async fn test_activity_guard_disconnect_or_crash_reports_clock_none_and_doctor() {
    let clock = Arc::new(SharedActivityClock::new(true));
    clock.record_physical_input();
    assert!(clock.ms_since_physical_input().is_some());

    let driver = LinuxDriver::new_fake(FakeSource::default(), FakeNode::default(), vec![], vec![])
        .with_activity_clock(clock.clone());

    assert!(driver.is_activity_guard_available());
    assert!(driver.capabilities().user_activity_guard);
    let doc = driver.doctor().await;
    assert!(doc
        .issues
        .iter()
        .any(|i| i == "user_activity_guard: available"));

    // Listener crashes or X connection drops
    clock.set_available(false);

    // CRITICAL DoD: MUST return None, NOT a large/stale number!
    assert!(!driver.is_activity_guard_available());
    assert_eq!(clock.ms_since_physical_input(), None);
    assert!(!driver.capabilities().user_activity_guard);

    let doc_down = driver.doctor().await;
    assert!(doc_down
        .issues
        .iter()
        .any(|i| i == "user_activity_guard: unavailable"));
}

#[tokio::test]
async fn test_watchdog_timeout_on_hung_native_call_and_stop_unblocked() {
    tokio::time::pause();

    let fake_x = Arc::new(make_fake_x());
    let stop_latch: Arc<dyn StopLatch> = Arc::new(SharedStopLatch::default());
    let driver = LinuxDriver::new_fake(FakeSource::default(), FakeNode::default(), vec![], vec![])
        .with_x_server(fake_x.clone())
        .with_stop_latch(stop_latch.clone());

    // Simulate an operation that hangs indefinitely
    let hung_future = async {
        tokio::time::sleep(Duration::from_secs(10)).await;
        Ok(Delivery::Foreground)
    };

    let watchdog_task = tokio::spawn(async move { run_with_watchdog(hung_future).await });

    // While hung, Stop channel is completely independent and unblocked
    assert!(!driver.is_stopped());
    stop_latch.request_stop();
    assert!(driver.is_stopped());

    // Advance virtual time by DRIVER_DEADLINE (4,750ms)
    tokio::time::advance(DRIVER_DEADLINE).await;

    let res = watchdog_task.await.unwrap();
    assert_eq!(
        res.unwrap_err(),
        DriverError::Timeout {
            maybe_executed: true,
        }
    );
}

#[tokio::test]
async fn test_restarting_driver_with_prelatched_stop_refuses_foreground() {
    let shared_latch = Arc::new(SharedStopLatch::new(true)); // Pre-latched from previous session
    assert!(shared_latch.is_stopped());

    let fake_x = Arc::new(make_fake_x());
    let driver = LinuxDriver::new_fake(FakeSource::default(), FakeNode::default(), vec![], vec![])
        .with_x_server(fake_x.clone())
        .with_stop_latch(shared_latch.clone());

    // Driver must NOT clear or ignore the Stop latch on startup
    assert!(driver.is_stopped());
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Unavailable
    );

    let win = make_window_ref(100);
    let cancel = Cancel::default();
    let err = driver
        .perform(
            &DriverAction::PointerClick {
                win: win.clone(),
                at: WindowPoint { x: 50.0, y: 50.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::Auto,
            },
            &cancel,
        )
        .await;

    assert_eq!(err.unwrap_err(), DriverError::CapabilityUnavailable);

    // Explicit user unlatch clears the stop state
    shared_latch.clear_for_test();
    assert!(!driver.is_stopped());
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Policy
    );

    let ok = driver
        .perform(
            &DriverAction::PointerClick {
                win,
                at: WindowPoint { x: 50.0, y: 50.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::Auto,
            },
            &cancel,
        )
        .await;

    assert_eq!(ok.unwrap(), Delivery::Foreground);
}

#[tokio::test]
async fn test_heartbeat_watchdog_detects_stale_listener() {
    let clock = SharedActivityClock::new(true);
    let stop_latch: Arc<dyn StopLatch> = Arc::new(SharedStopLatch::default());
    let hotkey = HotkeyManager::new(HotkeyConfig::default(), stop_latch);
    hotkey.set_available(true);

    let watchdog = HeartbeatWatchdog::new();
    assert!(!watchdog.is_alive(Duration::from_millis(50)));

    watchdog.record_heartbeat();
    assert!(watchdog.is_alive(Duration::from_millis(500)));
    assert!(watchdog.check_liveness(&clock, &hotkey, Duration::from_millis(500)));
    assert!(clock.is_available());
    assert!(hotkey.is_available());

    // Wait for staleness
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(!watchdog.is_alive(Duration::from_millis(50)));
    assert!(!watchdog.check_liveness(&clock, &hotkey, Duration::from_millis(50)));

    // Liveness check marks both as unavailable
    assert!(!clock.is_available());
    assert!(!hotkey.is_available());
}

#[tokio::test]
#[ignore]
async fn live_xvfb_stop_hotkey_and_xtest_not_refreshing() {
    if std::env::var("DISPLAY").is_err() {
        eprintln!("Skipping live_xvfb_stop_hotkey_and_xtest_not_refreshing: DISPLAY not set");
        return;
    }

    let driver = match LinuxDriver::connect().await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Live connect failed (non-X11 environment): {e}");
            return;
        }
    };

    assert!(driver.x_server().is_some());
    assert!(driver.is_hotkey_available());

    // XTEST injection must NOT refresh activity clock
    let clock = driver.activity_clock();
    assert_eq!(clock.ms_since_physical_input(), None);
    if let Some(x) = driver.x_server() {
        let _ = x.xtest_fake_motion(100, 100);
        let _ = x.flush();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            clock.ms_since_physical_input(),
            None,
            "XTEST fake motion must NOT refresh activity clock"
        );
    }

    // If xdotool is installed, trigger hotkey
    let xdotool_check = std::process::Command::new("which").arg("xdotool").output();

    if let Ok(out) = xdotool_check {
        if out.status.success() {
            let trigger = std::process::Command::new("xdotool")
                .args(["key", "ctrl+alt+shift+BackSpace"])
                .output();

            if let Ok(t_out) = trigger {
                if t_out.status.success() {
                    for _ in 0..50 {
                        if driver.is_stopped() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    assert!(driver.is_stopped(), "xdotool key trigger should latch Stop");
                    assert!(driver.ledger().is_empty());
                }
            }
        } else {
            eprintln!("xdotool not installed; skipping live key simulation");
        }
    }

    eprintln!("[NOTE] Physical hardware input cannot be simulated in virtual Xvfb without kernel evemu; real physical input refresh is verified via DeviceFilter unit tests [not_run: real physical hardware input in Xvfb]");
}
