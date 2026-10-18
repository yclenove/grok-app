#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use gcu_core::driver::Driver;
use gcu_driver_windows::gate::{check_target, IntegrityLevel};
use gcu_driver_windows::input::fake::FakeInput;
use gcu_driver_windows::input::hooks::HookEvent;
use gcu_driver_windows::input::hotkey::{FakeSafetyApi, HotkeyConfig};
use gcu_driver_windows::input::InputBackend;
use gcu_driver_windows::testing::{medium_security, FakeWinApi};
use gcu_driver_windows::worker::ExecMode;
use gcu_host_api::UserActivity;
use gcu_protocol::capabilities::{ForegroundCapability, Platform};
use std::time::Duration;

fn security(
    integrity: IntegrityLevel,
    ui_access: bool,
    secure_desktop: bool,
    interactive: bool,
) -> ProcessSecurity {
    ProcessSecurity {
        integrity,
        ui_access,
        session_id: 1,
        interactive,
        secure_desktop,
        created: 1,
    }
}

async fn refused(security: ProcessSecurity) -> DriverError {
    let mut calls = 0;
    let error = match open_monitored(&security, || {
        calls += 1;
        async { Err(DriverError::CapabilityUnavailable) }
    })
    .await
    {
        Err(error) => error,
        Ok(_) => panic!("precheck must refuse before launch"),
    };
    assert_eq!(calls, 0, "refused security must not start a worker");
    error
}

#[test]
fn handshake_withholds_input_until_the_host_latch_is_bound() {
    let caps = conservative_handshake();
    assert_eq!(caps.platform, Platform::Windows);
    assert!(!caps.user_activity_guard);
    assert!(!caps.observe.tree);
    assert!(!caps.observe.screenshot);
    assert!(!caps.delivery.accessibility);
    assert!(!caps.delivery.targeted_pointer);
    assert!(!caps.delivery.targeted_keys);
    assert_eq!(caps.delivery.foreground, ForegroundCapability::Unavailable);
}

#[test]
fn production_launch_is_the_worker_not_in_process() {
    assert_eq!(ExecMode::default(), ExecMode::Worker);
    assert_ne!(ExecMode::default(), ExecMode::InProcess);
}

#[tokio::test]
async fn elevated_uiaccess_and_secure_desktop_refuse_before_launch() {
    let cases = [
        (
            security(IntegrityLevel::High, false, false, true),
            DriverError::ElevationRequired,
        ),
        (
            security(IntegrityLevel::System, false, false, true),
            DriverError::ElevationRequired,
        ),
        (
            security(IntegrityLevel::Medium, true, false, true),
            DriverError::ElevationRequired,
        ),
        (
            security(IntegrityLevel::Medium, false, true, true),
            DriverError::SecureDesktop,
        ),
        (
            security(IntegrityLevel::Medium, false, false, false),
            DriverError::SecureDesktop,
        ),
    ];
    for (security, expected) in cases {
        assert_eq!(refused(security).await, expected);
    }
}

#[test]
fn admin_target_and_secure_desktop_refuse_every_route() {
    let engine = medium_security();
    for foreground in [false, true] {
        let mut admin = medium_security();
        admin.integrity = IntegrityLevel::High;
        assert_eq!(
            check_target(&engine, &admin, foreground, Some(true)).unwrap_err(),
            DriverError::ElevationRequired
        );
        let mut ui_access = medium_security();
        ui_access.ui_access = true;
        assert_eq!(
            check_target(&engine, &ui_access, foreground, Some(true)).unwrap_err(),
            DriverError::ElevationRequired
        );
        let mut secure = medium_security();
        secure.secure_desktop = true;
        assert_eq!(
            check_target(&engine, &secure, foreground, Some(true)).unwrap_err(),
            DriverError::SecureDesktop
        );
        let mut other_session = medium_security();
        other_session.interactive = false;
        assert_eq!(
            check_target(&engine, &other_session, foreground, Some(true)).unwrap_err(),
            DriverError::SecureDesktop
        );
    }
}

#[tokio::test]
async fn monitored_failure_is_returned_once_and_is_not_replayed() {
    for error in [
        DriverError::Crashed {
            maybe_executed: true,
        },
        DriverError::Crashed {
            maybe_executed: false,
        },
        DriverError::Timeout {
            maybe_executed: true,
        },
    ] {
        let mut calls = 0;
        let got = match open_monitored(&medium_security(), || {
            calls += 1;
            let error = error.clone();
            async move { Err(error) }
        })
        .await
        {
            Err(error) => error,
            Ok(_) => panic!("a failed worker must not become a driver"),
        };
        assert_eq!(calls, 1);
        assert_eq!(got.maybe_executed(), error.maybe_executed());
        assert_eq!(got, error);
    }
}

#[tokio::test]
async fn open_monitored_returns_the_windows_driver_without_a_second_launch() {
    let mut calls = 0;
    let driver = open_monitored(&medium_security(), || {
        calls += 1;
        async { WindowsDriver::with_api(Arc::new(FakeWinApi::default())).await }
    })
    .await
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(driver.platform(), Platform::Windows);
    assert_ne!(driver.platform(), Platform::Macos);
    assert_ne!(driver.platform(), Platform::Linux);
}

#[tokio::test]
async fn stop_through_host_latch_persists_and_shares_the_clock() {
    let directory = tempfile::TempDir::new().unwrap();
    let host = LocalHost::fixture(directory.path());
    let safety = FakeSafetyApi::default();
    let guard = safety.start(HotkeyConfig::default(), local_latch(&host));
    let registered = safety.registered_config().unwrap();
    assert_eq!(registered, HotkeyConfig::default());
    assert_eq!((registered.modifiers, registered.virtual_key), (7, 8));
    let driver = Arc::new(
        WindowsDriver::with_api(Arc::new(FakeWinApi::default()))
            .await
            .unwrap()
            .with_input(Arc::new(InputBackend::new(Arc::new(FakeInput::default()))))
            .with_safety(guard.clone()),
    );
    bind_activity(&driver, &host);
    assert_eq!(driver.platform(), Platform::Windows);
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Policy
    );
    assert!(!driver.capabilities().user_activity_guard);
    let report = driver.doctor().await;
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.starts_with("user_activity_guard:")));
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.starts_with("injection_lease:")));
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.starts_with("stop_hotkey:")));
    assert_ne!(
        report
            .issues
            .iter()
            .find(|issue| issue.starts_with("stop_hotkey:")),
        report
            .issues
            .iter()
            .find(|issue| issue.starts_with("injection_lease:"))
    );
    guard
        .activity_clock()
        .event(HookEvent::Mouse(0), std::time::Instant::now());
    assert_eq!(host.activity.ms_since_physical_input(), Some(0));
    assert!(driver.capabilities().user_activity_guard);
    assert!(!host.activity.latched_stop());
    safety.trigger();
    assert!(host.activity.latched_stop());
    assert!(guard.is_stopped());
    let mut persisted = false;
    for _ in 0..50 {
        if directory.path().join("stopped").exists() {
            persisted = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(persisted, "Host Stop file is the latch side effect");
    guard.disable("test transport failure");
    assert_eq!(host.activity.ms_since_physical_input(), None);
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Unavailable
    );
    assert!(!driver.capabilities().user_activity_guard);
}

#[tokio::test]
async fn engine_stop_latch_latches_the_mirror_and_queues_hotkey() {
    let stopped = StopMirror::new(true);
    let (notify, mut inbox) = tokio::sync::mpsc::channel(1);
    let latched = EngineStopLatch {
        stop: stopped.clone(),
        notify: notify.clone(),
    };
    assert!(latched.is_stopped());
    let open = StopMirror::new(false);
    let fresh = EngineStopLatch {
        stop: open.clone(),
        notify,
    };
    assert!(!fresh.is_stopped());
    fresh.request_stop();
    assert!(open.stopped(""));
    assert!(fresh.is_stopped());
    assert_eq!(inbox.try_recv().unwrap(), StopReason::Hotkey);
}
