//! Portable safety contracts. No SendInput pretends to be physical hardware.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
#[path = "stop/timing.rs"]
mod timing;
use gcu_core::{driver::*, model::WindowPoint};
use gcu_driver_windows::{
    input::{
        fake::FakeInput,
        hooks::{HookEvent, PhysicalClock},
        hotkey::{FakeSafetyApi, HotkeyConfig, SafetyGuard, StopLatch},
        watchdog::HEARTBEAT_TIMEOUT,
        InputBackend,
    },
    overlay::{FakeOverlayApi, OverlayExclusion},
};
use gcu_protocol::capabilities::ForegroundCapability;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Default)]
struct Latch(AtomicBool);
impl StopLatch for Latch {
    fn is_stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    fn request_stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[test]
fn injected_keyboard_mouse_events_do_not_refresh_physical_clock() {
    let at = Instant::now();
    let clock = PhysicalClock::default();
    clock.confirm(at);
    for event in [
        HookEvent::Keyboard(0x10),
        HookEvent::Mouse(1),
        HookEvent::Keyboard(2),
        HookEvent::Mouse(2),
    ] {
        assert!(!clock.event(event, at + Duration::from_millis(20)));
    }
    assert_eq!(clock.ms_since_at(at + Duration::from_millis(50)), Some(50));
    for event in [HookEvent::Keyboard(0), HookEvent::Mouse(0)] {
        assert!(clock.event(event, at + Duration::from_millis(60)));
        assert_eq!(clock.ms_since_at(at + Duration::from_millis(70)), Some(10));
    }
}

#[test]
fn hook_loss_or_stale_heartbeat_returns_none_and_restarts_idle_interval() {
    let at = Instant::now();
    let clock = PhysicalClock::default();
    assert_eq!(clock.ms_since_at(at), None);
    clock.confirm(at);
    assert_eq!(
        clock.ms_since_at(at + HEARTBEAT_TIMEOUT + Duration::from_millis(1)),
        None
    );
    clock.invalidate();
    assert_eq!(clock.ms_since_at(at), None);
    clock.confirm(at + Duration::from_secs(10));
    assert_eq!(clock.ms_since_at(at + Duration::from_secs(10)), Some(0));
}

#[test]
fn doctor_discloses_absolute_mouse_probe_idle_gate_and_hover_effect() {
    let guard = FakeSafetyApi::default().start(Default::default(), Arc::new(Latch::default()));
    let mut issues = Vec::new();
    guard.doctor_issues(&mut issues);
    assert!(issues
        .iter()
        .any(|issue| issue.starts_with("mouse_hook_probe:")
            && issue.contains("absolute_same_position")
            && issue.contains("hover")
            && issue.contains("2000ms")
            && issue.contains("single system idle sample before all probes")
            && issue.contains("real physical callbacks clear it permanently")
            && issue.contains("without retry or compensation")));
}

fn click(win: &WindowRef, route: PointerRoute) -> DriverAction {
    DriverAction::PointerClick {
        win: win.clone(),
        at: WindowPoint { x: 20., y: 30. },
        button: Button::Left,
        count: 1,
        route,
    }
}

#[tokio::test]
async fn registration_conflict_disables_capability_and_perform_but_keeps_background() {
    let (_, driver, _, win) = common::fixture().await;
    let api = Arc::new(FakeSafetyApi::default());
    let reason = "RegisterHotKey: HRESULT=0x80070581, GetLastError=Some(1409); conflict";
    api.fail_open(reason);
    let guard = api.start(HotkeyConfig::default(), Arc::new(Latch::default()));
    let driver = driver
        .with_input(Arc::new(InputBackend::new(Arc::new(FakeInput::default()))))
        .with_safety(guard);
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Unavailable
    );
    assert_eq!(
        driver
            .perform(
                &click(&win, PointerRoute::AllowForeground),
                &Cancel::default()
            )
            .await,
        Err(DriverError::CapabilityUnavailable)
    );
    assert_eq!(
        driver
            .perform(&click(&win, PointerRoute::TargetedOnly), &Cancel::default())
            .await,
        Ok(Delivery::TargetedEvent)
    );
    let report = driver.doctor().await;
    assert!(report
        .issues
        .iter()
        .any(|i| i.contains("stop_hotkey: unavailable") && i.contains(reason)));
}

#[tokio::test]
async fn listener_failures_disable_foreground_and_doctor_reports_unavailable() {
    for failure in [
        "hooks removed",
        "secure desktop",
        "pump panic",
        "FFI stalled",
    ] {
        let (_, driver, _, win) = common::fixture().await;
        let api = Arc::new(FakeSafetyApi::default());
        let latch = Arc::new(Latch::default());
        let guard = api.start(HotkeyConfig::default(), latch.clone());
        let driver = driver
            .with_input(Arc::new(InputBackend::new(Arc::new(FakeInput::default()))))
            .with_safety(guard.clone());
        assert!(driver.capabilities().user_activity_guard);
        api.fail_poll(failure);
        api.poll();
        assert_eq!(guard.activity_clock().ms_since_physical_input(), None);
        assert_eq!(
            driver.capabilities().delivery.foreground,
            ForegroundCapability::Unavailable
        );
        assert!(driver
            .perform(
                &click(&win, PointerRoute::AllowForeground),
                &Cancel::default()
            )
            .await
            .is_err());
        let report = driver.doctor().await;
        assert!(report
            .issues
            .iter()
            .any(|i| i == "user_activity_guard: unavailable"));
    }
}

#[tokio::test]
async fn stop_bypasses_stalled_ffi_and_reconnected_guard_keeps_host_latch() {
    let latch = Arc::new(Latch::default());
    let api = Arc::new(FakeSafetyApi::default());
    let guard = api.start(HotkeyConfig::default(), latch.clone());
    api.stall_poll();
    let at = Instant::now();
    guard.request_stop();
    assert!(latch.is_stopped());
    assert!(at.elapsed() <= Duration::from_millis(100));
    let restarted = FakeSafetyApi::default().start(HotkeyConfig::default(), latch);
    assert!(restarted.is_stopped());
    api.unstall();
}

#[tokio::test]
async fn silent_hook_removal_returns_none_even_with_responsive_message_pump() {
    let api = Arc::new(FakeSafetyApi::default());
    let latch = Arc::new(Latch::default());
    let guard = api.start(Default::default(), latch.clone());
    api.remove_hooks();
    for _ in 0..5 {
        api.advance(Duration::from_millis(20));
        api.poll(); // Both pumps still respond, but the hooks cannot prove health.
    }
    api.advance(Duration::from_millis(1));
    assert!(latch.is_stopped());
    assert_eq!(guard.activity_clock().ms_since_physical_input(), None);
    assert!(!guard.available());
}

#[tokio::test]
async fn hook_thread_panic_and_guard_drop_revoke_retained_clock() {
    use gcu_driver_windows::input::hotkey::{GuardEvents, SafetyApi, SafetyPump};
    struct Crash(AtomicBool);
    struct Api(Arc<Crash>);
    struct Pump(Arc<Crash>, Arc<GuardEvents>);
    impl SafetyApi for Api {
        fn open(
            &self,
            _: HotkeyConfig,
            events: Arc<GuardEvents>,
        ) -> std::result::Result<Box<dyn SafetyPump>, String> {
            Ok(Box::new(Pump(self.0.clone(), events)))
        }
    }
    impl SafetyPump for Pump {
        fn poll(&mut self, _: bool) -> std::result::Result<(), String> {
            assert!(!self.0 .0.load(Ordering::Acquire), "fake hook panic");
            self.1.clock().confirm(Instant::now());
            self.1.hotkey_heartbeat();
            Ok(())
        }
    }
    let api = Arc::new(Crash(AtomicBool::new(false)));
    let guard = SafetyGuard::start(
        Arc::new(Api(api.clone())),
        Default::default(),
        Arc::new(Latch::default()),
    )
    .await;
    let clock = guard.activity_clock();
    api.0.store(true, Ordering::Release);
    tokio::time::timeout(Duration::from_millis(200), async {
        while clock.available() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(clock.ms_since_physical_input(), None);
    let next = FakeSafetyApi::default().start(Default::default(), Arc::new(Latch::default()));
    let retained = next.activity_clock();
    drop(next);
    assert_eq!(retained.ms_since_physical_input(), None);
}

#[tokio::test]
async fn custom_hotkey_registers_and_periodically_replaces_hooks_without_clearing_stop() {
    let api = Arc::new(FakeSafetyApi::default());
    let latch = Arc::new(Latch::default());
    let config = HotkeyConfig {
        modifiers: 3,
        virtual_key: 0x7b,
    };
    let guard = api.start(config, latch.clone());
    assert_eq!(api.registered_config(), Some(config));
    guard.request_stop();
    for _ in 0..50 {
        api.advance(Duration::from_millis(20));
        api.poll();
    }
    assert_eq!(api.replacements(), 1);
    assert!(guard.is_stopped());
    assert!(guard.activity_clock().available());
    latch.0.store(false, Ordering::SeqCst); // Simulated explicit trusted Host resume.
    assert!(!guard.is_stopped());
    for config in [
        HotkeyConfig {
            modifiers: 0,
            virtual_key: 8,
        },
        HotkeyConfig {
            modifiers: 16,
            virtual_key: 8,
        },
        HotkeyConfig {
            modifiers: 7,
            virtual_key: 0,
        },
    ] {
        let api = Arc::new(FakeSafetyApi::default());
        let guard = api.start(config, Arc::new(Latch::default()));
        assert!(api.registered_config().is_none());
        assert!(!guard.available());
    }
}

#[test]
fn exclusion_requires_windows_2004_build_even_if_old_os_accepts_affinity() {
    use gcu_driver_windows::overlay::supports_exclusion_build;
    assert!(!supports_exclusion_build(6, 19041));
    assert!(!supports_exclusion_build(10, 19040));
    assert!(supports_exclusion_build(10, 19041));
    assert!(supports_exclusion_build(10, 26100));
}

#[tokio::test]
async fn actual_overlay_exclusion_failure_revokes_cached_capability() {
    use gcu_driver_windows::overlay::OverlayApi;
    struct Failure;
    impl OverlayApi for Failure {
        fn probe(&self) -> bool {
            true
        }
        fn exclude(&self, _: u64) -> bool {
            false
        }
    }
    let exclusion = OverlayExclusion::probe(Arc::new(Failure)).await;
    let (_, driver, _, _) = common::fixture().await;
    let driver = driver.with_overlay_exclusion(exclusion.clone());
    assert!(driver.capabilities().overlay_excluded_from_capture);
    assert!(!exclusion.exclude(8).await);
    assert!(!driver.capabilities().overlay_excluded_from_capture);
}

#[tokio::test]
async fn exclusion_probe_requires_exact_exclude_affinity_and_failure_is_reported() {
    for (supported, readback) in [(false, 0), (true, 1), (true, 0x11)] {
        let exclusion = OverlayExclusion::probe(Arc::new(FakeOverlayApi {
            supported,
            readback,
        }))
        .await;
        assert_eq!(exclusion.available(), supported && readback == 0x11);
        assert_eq!(exclusion.exclude(8).await, supported && readback == 0x11);
        let (_, driver, _, _) = common::fixture().await;
        let driver = driver.with_overlay_exclusion(exclusion);
        assert_eq!(
            driver.capabilities().overlay_excluded_from_capture,
            supported && readback == 0x11
        );
        if readback != 0x11 {
            assert!(driver
                .doctor()
                .await
                .issues
                .iter()
                .any(|i| i.contains("redact")));
        }
    }
}

#[cfg(all(feature = "test-worker-faults", debug_assertions))]
#[path = "stop/worker.rs"]
mod worker;
