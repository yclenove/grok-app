//! Every-route token/session/desktop safety regressions.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_core::driver::*;
use gcu_driver_windows::{
    gate::{check_engine, check_target, IntegrityLevel},
    testing::{medium_security, FakeWinApi},
    WindowsDriver,
};
use std::sync::Arc;
mod common;
use common::{actions, fixture};

#[test]
fn engine_token_medium_low_allowed_high_system_uiaccess_refused() {
    for level in [
        IntegrityLevel::Untrusted,
        IntegrityLevel::Low,
        IntegrityLevel::Medium,
        IntegrityLevel::High,
        IntegrityLevel::System,
    ] {
        for ui_access in [false, true] {
            let mut security = medium_security();
            security.integrity = level;
            security.ui_access = ui_access;
            if level >= IntegrityLevel::High || ui_access {
                assert_eq!(check_engine(&security), Err(DriverError::ElevationRequired));
            } else {
                assert!(check_engine(&security).is_ok());
            }
        }
    }
}

#[tokio::test]
async fn startup_elevated_engine_returns_typed_zero_effect_refusal() {
    for (level, ui_access) in [
        (IntegrityLevel::High, false),
        (IntegrityLevel::System, false),
        (IntegrityLevel::Medium, true),
    ] {
        let api = Arc::new(FakeWinApi::default());
        api.update(|s| {
            s.engine.integrity = level;
            s.engine.ui_access = ui_access;
        });
        let error = WindowsDriver::with_api(api.clone()).await.err().unwrap();
        assert_eq!(error, DriverError::ElevationRequired);
        assert!(!error.maybe_executed());
        assert_eq!(
            error.error_code(),
            gcu_protocol::ErrorCode::ElevationRequired
        );
        assert!(api.state().unwrap().launches.is_empty());
        assert!(api.state().unwrap().cache.is_none());
    }
}

#[test]
fn target_admin_noninteractive_secure_and_other_session_refused() {
    let engine = medium_security();
    let mut scenarios = Vec::new();
    for level in [IntegrityLevel::High, IntegrityLevel::System] {
        let mut s = engine.clone();
        s.integrity = level;
        scenarios.push((s, DriverError::ElevationRequired));
    }
    let mut s = engine.clone();
    s.interactive = false;
    scenarios.push((s, DriverError::SecureDesktop));
    let mut s = engine.clone();
    s.secure_desktop = true;
    scenarios.push((s, DriverError::SecureDesktop));
    let mut s = engine.clone();
    s.session_id = 2;
    scenarios.push((s, DriverError::SecureDesktop));
    let mut s = engine.clone();
    s.ui_access = true;
    scenarios.push((s, DriverError::ElevationRequired));
    for (target, error) in scenarios {
        for foreground in [false, true] {
            assert_eq!(
                check_target(&engine, &target, foreground, Some(true)),
                Err(error.clone())
            );
        }
    }
    assert_eq!(
        check_target(&engine, &engine, true, Some(false)),
        Err(DriverError::CapabilityUnavailable)
    );
    assert!(check_target(&engine, &engine, false, Some(false)).is_ok());
    assert_eq!(
        check_target(&engine, &engine, true, None),
        Err(DriverError::CapabilityUnavailable)
    );
}

#[tokio::test]
async fn every_input_and_observation_route_enforces_live_target_gate() {
    let (api, driver, app, win) = fixture().await;
    for error in [DriverError::ElevationRequired, DriverError::SecureDesktop] {
        api.update(|s| {
            let p = s.processes.get_mut(&7).unwrap();
            p.integrity = if error == DriverError::ElevationRequired {
                IntegrityLevel::High
            } else {
                IntegrityLevel::Medium
            };
            p.secure_desktop = error == DriverError::SecureDesktop;
        });
        for action in actions(&win) {
            let actual = driver
                .perform(&action, &Cancel::default())
                .await
                .unwrap_err();
            assert_eq!(actual, error);
            assert!(!actual.maybe_executed());
        }
        assert_eq!(
            driver
                .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
                .await
                .unwrap_err(),
            error
        );
        assert_eq!(
            driver
                .capture(&win, &CaptureOpts::default(), &Cancel::default())
                .await
                .unwrap_err(),
            error
        );
        assert_eq!(driver.window_geometry(&win).await.unwrap_err(), error);
        assert_eq!(driver.focused_element(&app).await.unwrap_err(), error);
        assert_eq!(driver.key_window(&app).await.unwrap_err(), error);
        assert_eq!(driver.launch_app(&app).await.unwrap_err(), error);
    }
    assert!(api.state().unwrap().launches.is_empty());
    assert!(api.state().unwrap().restores.is_empty());
}

#[tokio::test]
async fn elevated_engine_after_startup_refuses_discovery_and_input() {
    let (api, driver, _, win) = fixture().await;
    api.update(|s| s.engine.ui_access = true);
    assert_eq!(
        driver.list_apps().await.unwrap_err(),
        DriverError::ElevationRequired
    );
    assert_eq!(
        driver.list_windows(None).await.unwrap_err(),
        DriverError::ElevationRequired
    );
    for action in actions(&win) {
        assert_eq!(
            driver
                .perform(&action, &Cancel::default())
                .await
                .unwrap_err(),
            DriverError::ElevationRequired
        );
    }
    assert!(!driver.doctor().await.ready);
}

#[tokio::test]
async fn unsupported_capabilities_are_honest_and_cancelled_calls_have_no_effects() {
    let (_, driver, app, win) = fixture().await;
    let caps = driver.capabilities();
    assert!(!caps.observe.tree && !caps.observe.screenshot && !caps.observe.events);
    assert!(
        !caps.delivery.accessibility
            && !caps.delivery.targeted_keys
            && !caps.delivery.targeted_pointer
    );
    assert_eq!(
        caps.delivery.foreground,
        gcu_protocol::capabilities::ForegroundCapability::Unavailable
    );
    assert!(driver.subscribe(&win).is_none());
    driver.release_all_input().await;
    assert_eq!(
        driver.focused_element(&app).await.unwrap_err(),
        DriverError::CapabilityUnavailable
    );
    for action in actions(&win) {
        assert_eq!(
            driver
                .perform(&action, &Cancel::default())
                .await
                .unwrap_err(),
            DriverError::CapabilityUnavailable
        );
    }
    let cancel = Cancel::default();
    cancel.cancel();
    assert_eq!(
        driver
            .capture(&win, &CaptureOpts::default(), &cancel)
            .await
            .unwrap_err(),
        DriverError::Cancelled {
            maybe_executed: false
        }
    );
    assert!(driver
        .doctor()
        .await
        .issues
        .iter()
        .any(|i| i.contains("P2-02+")));
}

#[tokio::test]
async fn missing_permission_fails_closed_without_native_input() {
    let (api, driver, _, win) = fixture().await;
    let missing = DriverError::PermissionMissing {
        permission: "process token query".into(),
    };
    api.update(|s| s.failure = Some(missing.clone()));
    assert_eq!(
        driver
            .capture(&win, &CaptureOpts::default(), &Cancel::default())
            .await
            .unwrap_err(),
        missing
    );
    assert_eq!(
        driver
            .perform(&DriverAction::Activate { win }, &Cancel::default())
            .await
            .unwrap_err(),
        missing
    );
    api.update(|s| s.failure = None);
    assert!(api.state().unwrap().launches.is_empty());
}
