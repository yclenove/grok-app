//! Action proof is input: live target refusals must precede its dispatch.
use super::*;
use gcu_driver_windows::gate::IntegrityLevel;

#[tokio::test]
async fn action_proof_invalid_live_target_preserves_error_without_probe_or_input() {
    for scenario in [
        "window",
        "target",
        "engine",
        "permission",
        "desktop",
        "uiaccess",
    ] {
        let (native, driver, _, win) = common::fixture().await;
        let api = FakeSafetyApi::default();
        let guard = api.start(Default::default(), Arc::new(Latch::default()));
        let input = Arc::new(FakeInput::default());
        let driver = driver
            .with_input(Arc::new(InputBackend::new(input.clone())))
            .with_safety(guard.clone());
        // A wrongly ordered proof would return CapabilityUnavailable, hiding
        // the original target error. It must never even be requested.
        api.remove_hooks();
        guard.activity_clock().invalidate();
        let error = match scenario {
            "window" => DriverError::WindowGone,
            "permission" => DriverError::PermissionMissing {
                permission: "process token query".into(),
            },
            "desktop" => DriverError::SecureDesktop,
            _ => DriverError::ElevationRequired,
        };
        native.update(|s| match scenario {
            "window" => s.windows[0].created += 1,
            "target" => s.processes.get_mut(&7).unwrap().integrity = IntegrityLevel::High,
            "engine" => s.engine.ui_access = true,
            "permission" => s.failure = Some(error.clone()),
            "desktop" => s.processes.get_mut(&7).unwrap().secure_desktop = true,
            "uiaccess" => s.processes.get_mut(&7).unwrap().ui_access = true,
            _ => unreachable!(),
        });
        for request in common::actions(&win) {
            assert_eq!(
                driver.perform(&request, &Cancel::default()).await,
                Err(error.clone()),
                "scenario={scenario}"
            );
        }
        assert_eq!(api.proof_requests(), 0, "scenario={scenario}");
        assert!(input.state().sent.is_empty());
        assert_eq!(input.state().activations, 0);
        assert!(!guard.is_stopped());
    }
}

#[tokio::test]
async fn action_proof_other_or_unknown_virtual_desktop_refuses_without_probe() {
    for desktop in [Some(false), None] {
        let (native, driver, _, win) = common::fixture().await;
        let api = FakeSafetyApi::default();
        let guard = api.start(Default::default(), Arc::new(Latch::default()));
        let input = Arc::new(FakeInput::default());
        let driver = driver
            .with_input(Arc::new(InputBackend::new(input.clone())))
            .with_safety(guard.clone());
        guard.activity_clock().invalidate();
        native.update(|s| {
            s.desktops.insert(8, desktop);
        });
        assert_eq!(
            driver
                .perform(
                    &click(&win, PointerRoute::AllowForeground),
                    &Cancel::default()
                )
                .await,
            Err(DriverError::CapabilityUnavailable)
        );
        assert_eq!(api.proof_requests(), 0);
        assert!(input.state().sent.is_empty());
        assert_eq!(input.state().activations, 0);
        assert!(!guard.is_stopped());
    }
}

#[tokio::test]
async fn input_backend_invalid_live_target_preserves_error_before_injection_lease_check() {
    let (_, driver, _, win) = common::fixture().await;
    let api = FakeSafetyApi::default();
    let guard = api.start(Default::default(), Arc::new(Latch::default()));
    let input = Arc::new(FakeInput::default());
    input.update(|s| s.validation_error = Some(DriverError::ElevationRequired));
    let backend = Arc::new(InputBackend::new(input.clone()));
    let _driver = driver
        .with_input(backend.clone())
        .with_safety(guard.clone());
    guard.activity_clock().invalidate();
    assert_eq!(
        backend
            .perform(
                gcu_driver_windows::testing::window(8, 7),
                &click(&win, PointerRoute::AllowForeground),
                &Cancel::default(),
                tokio::time::Instant::now() + DRIVER_DEADLINE,
            )
            .await,
        Err(DriverError::ElevationRequired)
    );
    assert_eq!(api.proof_requests(), 0);
    assert!(input.state().sent.is_empty());
    assert_eq!(input.state().activations, 0);
    assert!(!guard.is_stopped());
}
