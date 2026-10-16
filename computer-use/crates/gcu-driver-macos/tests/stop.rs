//! Deterministic Stop/foreground tests. Native desktop/TCC validation: not_run.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "support/stop_text.rs"]
mod text;

use gcu_core::{
    driver::{Button, Cancel, Delivery, DriverAction, DriverError, PointerRoute, DRIVER_DEADLINE},
    model::WindowPoint,
};
use gcu_driver_macos::input::{
    self,
    activity::{ActivityClock, ActivityMonitor, OWN_EVENT_TAG},
    clock::{Clock, FakeClock},
    hotkey::{
        register_hotkey, Guards, HotkeyConfig, HotkeyRegistrar, Registration, SharedStopLatch,
        StopLatch,
    },
    Call, FakeEvents, InputLedger, InputTarget, PostPath,
};
use std::{sync::Arc, time::Duration};

fn target(chromium: bool) -> InputTarget {
    InputTarget {
        pid: 4,
        window_id: 7,
        bundle_id: if chromium {
            "com.google.Chrome"
        } else {
            "com.example.Editor"
        }
        .into(),
        app_name: "Editor".into(),
        origin_x: 0.0,
        origin_y: 0.0,
        width_px: 200.0,
        height_px: 100.0,
        scale: 1.0,
    }
}

fn click() -> DriverAction {
    DriverAction::PointerClick {
        win: gcu_core::driver::WindowRef {
            app: gcu_core::driver::AppRef {
                info: gcu_protocol::AppInfo {
                    id: "editor".into(),
                    name: "Editor".into(),
                    pid: Some(4),
                    identity: None,
                },
            },
            id: "cg:7:4".into(),
        },
        at: WindowPoint { x: 5.0, y: 5.0 },
        button: Button::Left,
        count: 1,
        route: PointerRoute::AllowForeground,
    }
}

#[derive(Clone)]
struct Fixture {
    clock: Arc<FakeClock>,
    monitor: Arc<ActivityMonitor>,
    latch: Arc<SharedStopLatch>,
    guards: Arc<Guards>,
    events: Arc<FakeEvents>,
    ledger: Arc<InputLedger>,
}

impl Fixture {
    fn new() -> Self {
        let clock = Arc::new(FakeClock::new());
        let monitor = Arc::new(ActivityMonitor::new(clock.clone()));
        monitor.connected(true);
        monitor.hotkey_registered(true);
        monitor.observe(0);
        clock.advance(Duration::from_millis(3000));
        monitor.heartbeat();
        let latch = Arc::new(SharedStopLatch::new(false));
        let guards = Arc::new(Guards::new(
            clock.clone(),
            monitor.clone(),
            latch.clone(),
            3000,
        ));
        let events = Arc::new(FakeEvents::ready());
        events.set_guards(guards.clone());
        Self {
            clock,
            monitor,
            latch,
            guards,
            events,
            ledger: Arc::new(InputLedger::new()),
        }
    }
    async fn run(&self, chromium: bool) -> Result<Delivery, DriverError> {
        input::deliver(
            self.events.clone(),
            self.ledger.clone(),
            &target(chromium),
            &click(),
            &Cancel::default(),
            DRIVER_DEADLINE,
        )
        .await
    }
}

#[tokio::test]
async fn physical_input_recent_blocks_foreground_but_allows_targeted() {
    let f = Fixture::new();
    f.monitor.observe(0);
    assert_eq!(f.monitor.ms_since_physical_input(), Some(0));
    assert_eq!(f.run(true).await, Err(DriverError::CapabilityUnavailable));
    assert!(f.events.calls().is_empty());
    assert_eq!(f.run(false).await.unwrap(), Delivery::TargetedEvent);
}

#[test]
fn activity_own_marker_does_not_refresh_and_failure_returns_none() {
    let f = Fixture::new();
    f.monitor.observe(OWN_EVENT_TAG);
    assert_eq!(f.monitor.ms_since_physical_input(), Some(3000));
    f.monitor.connected(false);
    assert_eq!(f.monitor.ms_since_physical_input(), None);
    f.monitor.connected(true);
    assert_eq!(f.monitor.ms_since_physical_input(), None);
    assert!(!f.monitor.stop_available());
}

#[tokio::test]
async fn hotkey_conflict_or_tap_revoked_disables_foreground() {
    struct Registrar(Registration);
    impl HotkeyRegistrar for Registrar {
        fn register(&mut self, _: &HotkeyConfig) -> Registration {
            self.0
        }
    }
    for result in [Registration::Conflict, Registration::Unavailable] {
        let f = Fixture::new();
        assert_eq!(
            register_hotkey(&mut Registrar(result), &f.monitor, &HotkeyConfig::default()),
            result
        );
        assert!(!f.guards.stop_available());
        assert_eq!(f.run(true).await, Err(DriverError::CapabilityUnavailable));
        assert!(f.events.calls().is_empty());
        assert_eq!(f.run(false).await.unwrap(), Delivery::TargetedEvent);
    }
    let f = Fixture::new();
    f.monitor.connected(false);
    assert_eq!(f.run(true).await, Err(DriverError::CapabilityUnavailable));
    assert!(f.events.calls().is_empty());
}

#[tokio::test]
async fn activation_accepted_without_frontmost_never_posts() {
    let f = Fixture::new();
    f.events.accept_without_activation();
    let task = tokio::spawn({
        let f = f.clone();
        async move { f.run(true).await }
    });
    while f.events.calls().is_empty() {
        tokio::task::yield_now().await;
    }
    f.clock.advance(Duration::from_millis(250));
    f.monitor.heartbeat();
    assert!(task.await.unwrap().unwrap_err().maybe_executed());
    assert!(!f.events.calls().iter().any(|c| matches!(c, Call::Post(_))));
}

#[tokio::test]
async fn foreground_stolen_between_posts_stops_sequence_and_releases() {
    let f = Fixture::new();
    f.events.steal_focus_after_post(99);
    assert!(f.run(true).await.unwrap_err().maybe_executed());
    let posts: Vec<_> = f
        .events
        .calls()
        .into_iter()
        .filter_map(|call| {
            if let Call::Post(event) = call {
                Some(event)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(posts.len(), 2);
    assert!(posts[0].down);
    assert!(!posts[1].down);
    assert!(f.ledger.is_empty());
}

#[tokio::test]
async fn unknown_activity_never_guesses_idle_or_activates() {
    let f = Fixture::new();
    f.monitor.connected(true);
    assert_eq!(f.monitor.ms_since_physical_input(), None);
    assert_eq!(f.run(true).await, Err(DriverError::CapabilityUnavailable));
    assert!(f.events.calls().is_empty());
}

#[tokio::test]
async fn independent_watchdog_listener_stall_latches_and_releases() {
    let f = Fixture::new();
    let press = input::plan_click(
        &target(false),
        WindowPoint { x: 5.0, y: 5.0 },
        Button::Left,
        1,
        PostPath::ToPid,
    )
    .unwrap()
    .remove(0);
    f.ledger.record_press(press.release_event().unwrap());
    let quit = Cancel::default();
    let service = tokio::spawn({
        let (api, ledger, guards, quit) = (
            f.events.clone(),
            f.ledger.clone(),
            f.guards.clone(),
            quit.clone(),
        );
        async move { input::watchdog::supervise(api, ledger, guards, &quit).await }
    });
    tokio::task::yield_now().await;
    f.clock.advance(Duration::from_millis(101));
    while f.events.calls().is_empty() {
        tokio::task::yield_now().await;
    }
    assert!(f.latch.is_stopped());
    assert!(f.ledger.is_empty());
    assert_eq!(f.monitor.ms_since_physical_input(), None);
    quit.cancel();
    service.await.unwrap();
}

#[tokio::test]
async fn stop_blocked_release_dispatches_remaining_ups_before_deadline() {
    let f = Fixture::new();
    f.events.hold_ups();
    let press = input::plan_click(
        &target(false),
        WindowPoint { x: 5.0, y: 5.0 },
        Button::Left,
        1,
        PostPath::ToPid,
    )
    .unwrap()
    .remove(0);
    f.ledger.record_press(press.release_event().unwrap());
    let mut other = press.release_event().unwrap();
    other.button = Some(1);
    f.ledger.record_press(other);
    f.guards.request_stop();
    let task = tokio::spawn({
        let f = f.clone();
        async move { input::watchdog::release_stopped(f.events.as_ref(), &f.ledger, &f.guards).await }
    });
    while f.events.calls().len() < 2 {
        tokio::task::yield_now().await;
    }
    assert!(f.ledger.is_empty());
    assert!(!task.is_finished());
    f.clock.advance(Duration::from_millis(500));
    task.await.unwrap();
    assert_eq!(f.events.calls().len(), 2);
}

#[tokio::test]
async fn driver_capabilities_and_doctor_follow_listener_health() {
    use gcu_core::driver::Driver;
    use gcu_protocol::capabilities::ForegroundCapability;
    let f = Fixture::new();
    let mut driver = gcu_driver_macos::MacosDriver::with_api(Arc::new(
        gcu_driver_macos::testing::FakeMacApi::default(),
    ))
    .await
    .unwrap();
    driver.inject_events(f.events.clone());
    assert!(driver.capabilities().user_activity_guard);
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Policy
    );
    f.monitor.hotkey_registered(false);
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Unavailable
    );
    f.monitor.connected(false);
    assert!(!driver.capabilities().user_activity_guard);
    let report = driver.doctor().await;
    assert_eq!(report.capabilities, driver.capabilities().clone());
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.contains("stop_hotkey: false")));
}

#[tokio::test]
async fn restored_host_stop_requires_explicit_trusted_user_resume() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct HostLatch(AtomicBool);
    impl StopLatch for HostLatch {
        fn is_stopped(&self) -> bool {
            self.0.load(Ordering::Acquire)
        }
        fn request_stop(&self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let f = Fixture::new();
    let host = Arc::new(HostLatch(AtomicBool::new(true)));
    for _ in 0..2 {
        let guards = Arc::new(Guards::new(
            f.clock.clone(),
            f.monitor.clone(),
            host.clone(),
            3000,
        ));
        f.events.set_guards(guards);
        assert_eq!(
            f.run(false).await,
            Err(DriverError::Cancelled {
                maybe_executed: false
            })
        );
    }
    // Only the fixture's trusted Host UI owns this write; the driver trait has
    // no clearer, and recreating its Guards did not reset it.
    host.0.store(false, Ordering::Release);
    assert_eq!(f.run(true).await.unwrap(), Delivery::Foreground);
}

#[tokio::test]
async fn stopped_before_send_and_recreated_guards_preserve_host_latch() {
    let f = Fixture::new();
    f.latch.request_stop();
    assert_eq!(
        f.run(false).await,
        Err(DriverError::Cancelled {
            maybe_executed: false
        })
    );
    let restarted = Guards::new(f.clock.clone(), f.monitor.clone(), f.latch.clone(), 3000);
    assert!(restarted.is_stopped());
    assert!(f.events.calls().is_empty());
}

#[test]
fn hotkey_matches_default_ignores_own_events_and_extra_modifiers() {
    let f = Fixture::new();
    let config = HotkeyConfig::default();
    assert!(!f
        .guards
        .key_down(&config, config.keycode, config.modifiers, OWN_EVENT_TAG));
    assert!(!f
        .guards
        .key_down(&config, config.keycode, config.modifiers | 131072, 0));
    assert!(f
        .guards
        .key_down(&config, config.keycode, config.modifiers, 0));
    assert!(f.latch.is_stopped());
}

#[tokio::test]
async fn stop_during_hung_post_releases_independently_with_virtual_clock() {
    let f = Fixture::new();
    let quit = Cancel::default();
    let service = tokio::spawn({
        let (api, ledger, guards, quit) = (
            f.events.clone(),
            f.ledger.clone(),
            f.guards.clone(),
            quit.clone(),
        );
        async move { input::watchdog::supervise(api, ledger, guards, &quit).await }
    });
    f.events.hold_posts();
    let api = f.events.clone();
    let ledger = f.ledger.clone();
    let action = click();
    let task = tokio::spawn(async move {
        input::deliver(
            api,
            ledger,
            &target(false),
            &action,
            &Cancel::default(),
            DRIVER_DEADLINE,
        )
        .await
    });
    while f.events.calls().is_empty() {
        tokio::task::yield_now().await;
    }
    let at = f.clock.now();
    f.guards.request_stop();
    assert!(f.clock.now() - at <= Duration::from_millis(100));
    let error = task.await.unwrap().unwrap_err();
    assert_eq!(
        error,
        DriverError::Cancelled {
            maybe_executed: true
        }
    );
    assert!(f.clock.now() - at <= Duration::from_millis(500));
    assert!(f.ledger.is_empty());
    assert!(f
        .events
        .calls()
        .iter()
        .any(|call| matches!(call, Call::Post(e) if !e.down)));
    quit.cancel();
    service.await.unwrap();
}

#[tokio::test]
async fn native_hang_times_out_before_five_seconds_without_sleep() {
    let f = Fixture::new();
    f.events.hold_posts();
    let api = f.events.clone();
    let ledger = f.ledger.clone();
    let task = tokio::spawn(async move {
        input::deliver(
            api,
            ledger,
            &target(false),
            &click(),
            &Cancel::default(),
            DRIVER_DEADLINE,
        )
        .await
    });
    while f.events.calls().is_empty() {
        tokio::task::yield_now().await;
    }
    f.clock.advance(DRIVER_DEADLINE);
    assert_eq!(
        task.await.unwrap(),
        Err(DriverError::Timeout {
            maybe_executed: true
        })
    );
    assert!(f.ledger.is_empty());
    assert!(DRIVER_DEADLINE <= Duration::from_secs(5));
}

#[tokio::test]
async fn listener_stale_heartbeat_returns_none_and_refuses_foreground() {
    let f = Fixture::new();
    f.clock.advance(Duration::from_millis(101));
    assert_eq!(f.monitor.ms_since_physical_input(), None);
    assert!(!f.guards.foreground_ready());
    assert_eq!(f.run(true).await, Err(DriverError::CapabilityUnavailable));
    assert!(f.events.calls().is_empty());
}

#[test]
fn ledger_release_keeps_original_target_and_delivery_path() {
    let f = Fixture::new();
    for path in [PostPath::ToPid, PostPath::HidTap] {
        let press = input::plan_click(
            &target(false),
            WindowPoint { x: 5.0, y: 5.0 },
            Button::Left,
            1,
            path,
        )
        .unwrap()
        .remove(0);
        f.ledger.record_press(press.release_event().unwrap());
        let up = f.ledger.take_ups().remove(0);
        assert_eq!(
            (up.pid, up.window_id, up.path, up.down),
            (4, 7, path, false)
        );
    }
}
