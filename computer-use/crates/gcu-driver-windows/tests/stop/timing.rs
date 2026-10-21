//! Scheduler-independent regression coverage for fake listener health.
use super::*;
use std::future::Future;

#[tokio::test]
async fn fake_listener_ignores_runtime_elapsed_time_during_input_settle() {
    let (_, driver, _, win) = common::fixture().await;
    let api = FakeSafetyApi::default();
    let guard = api.start(Default::default(), Arc::new(Latch::default()));
    let input = Arc::new(FakeInput::default());
    let backend = Arc::new(InputBackend::new(input.clone()));
    let _driver = driver
        .with_input(backend.clone())
        .with_safety(guard.clone());
    // Discovery is a real OS thread; pause only around the pure async boundary.
    tokio::time::pause();
    assert_eq!(
        backend
            .perform(
                gcu_driver_windows::testing::window(8, 7),
                &click(&win, PointerRoute::TargetedOnly),
                &Cancel::default(),
                tokio::time::Instant::now() + DRIVER_DEADLINE,
            )
            .await,
        Ok(Delivery::TargetedEvent)
    );
    // Settle consumed 300ms of runtime time without a listener poll. A real
    // 100ms heartbeat here would close the context and produce unknown/Timeout.
    assert_eq!(input.state().samples, 3);
    assert!(guard.available());
    assert!(!guard.is_stopped());
    assert_eq!(guard.activity_clock().ms_since_physical_input(), None);
}

#[test]
fn controlled_heartbeat_expiry_latches_stop_at_the_production_boundary() {
    let api = FakeSafetyApi::default();
    let latch = Arc::new(Latch::default());
    let guard = api.start(Default::default(), latch.clone());
    api.advance(HEARTBEAT_TIMEOUT);
    assert!(guard.available());
    api.advance(Duration::from_millis(1));
    assert!(!guard.available());
    assert_eq!(guard.activity_clock().ms_since_physical_input(), None);
    assert!(latch.is_stopped());
    // Late completion cannot renew the lease or clear Stop.
    api.poll();
    assert!(!guard.available());
    assert!(guard.is_stopped());
}

#[tokio::test]
async fn watchdog_stop_closes_active_context_before_driver_deadline() {
    let (_, driver, _, win) = common::fixture().await;
    let api = FakeSafetyApi::default();
    let guard = api.start(Default::default(), Arc::new(Latch::default()));
    let input = Arc::new(FakeInput::default());
    input.update(|s| s.send_pause = Some((0, Duration::from_secs(1))));
    let backend = Arc::new(InputBackend::new(input.clone()));
    let _driver = driver.with_input(backend.clone()).with_safety(guard);
    tokio::time::pause();
    let request = click(&win, PointerRoute::TargetedOnly);
    let cancel = Cancel::default();
    let operation = backend.perform(
        gcu_driver_windows::testing::window(8, 7),
        &request,
        &cancel,
        tokio::time::Instant::now() + DRIVER_DEADLINE,
    );
    tokio::pin!(operation);
    // Poll the action up to its first held down, without a busy polling loop or
    // relying on which OS/runtime thread the scheduler happens to run first.
    std::future::poll_fn(|cx| {
        assert!(operation.as_mut().poll(cx).is_pending());
        if input.state().sent.is_empty() {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(())
        }
    })
    .await;
    api.advance(HEARTBEAT_TIMEOUT + Duration::from_millis(1));
    assert_eq!(
        operation.await,
        Err(DriverError::Timeout {
            maybe_executed: true
        })
    );
    assert!(backend.ledger_empty());
}

#[tokio::test]
async fn foreground_perform_expired_lease_proves_once_before_packet_assembly() {
    let (_, driver, _, win) = common::fixture().await;
    let api = FakeSafetyApi::default();
    let guard = api.start(Default::default(), Arc::new(Latch::default()));
    let input = Arc::new(FakeInput::default());
    let backend = Arc::new(InputBackend::new(input.clone()));
    let driver = driver.with_input(backend).with_safety(guard.clone());
    guard.activity_clock().invalidate();
    assert!(!guard.available());
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Policy
    );
    assert_eq!(
        driver
            .perform(
                &click(&win, PointerRoute::AllowForeground),
                &Cancel::default()
            )
            .await,
        Ok(Delivery::Foreground)
    );
    assert_eq!(
        api.proof_requests(),
        1,
        "outer and inner checks share one proof"
    );
    assert!(!input.state().sent.is_empty());
    assert!(
        !guard.available(),
        "release and action completion discard the lease"
    );
    assert!(!guard.is_stopped());
    assert_eq!(
        driver.ms_since_physical_input(),
        None,
        "only confirms, no physical callback"
    );
    assert_eq!(
        driver
            .perform(
                &click(&win, PointerRoute::AllowForeground),
                &Cancel::default()
            )
            .await,
        Ok(Delivery::Foreground)
    );
    assert_eq!(api.proof_requests(), 2);
}

#[tokio::test]
async fn foreground_perform_missing_new_pair_refuses_without_action_input_or_latch() {
    let (_, driver, _, win) = common::fixture().await;
    let api = FakeSafetyApi::default();
    let guard = api.start(Default::default(), Arc::new(Latch::default()));
    let input = Arc::new(FakeInput::default());
    let driver = driver
        .with_input(Arc::new(InputBackend::new(input.clone())))
        .with_safety(guard.clone());
    api.remove_hooks();
    guard.activity_clock().invalidate();
    let error = driver
        .perform(
            &click(&win, PointerRoute::AllowForeground),
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::CapabilityUnavailable);
    assert!(!error.maybe_executed());
    assert!(!guard.is_stopped());
    assert!(guard.transport_healthy());
    assert_eq!(api.proof_requests(), 1);
    assert!(input.state().sent.is_empty());
    assert_eq!(
        input.state().samples,
        0,
        "proof follows the live target gate and precedes action packet/effect sampling"
    );
}

#[tokio::test]
async fn foreground_inflight_missing_mouse_proof_latches_and_releases_held_input() {
    let (_, driver, _, win) = common::fixture().await;
    let api = FakeSafetyApi::default();
    let guard = api.start(Default::default(), Arc::new(Latch::default()));
    let input = Arc::new(FakeInput::default());
    // Events 0 and 1 are the approach and target Moves. Pause after event 2 (left
    // down), while its up is still pending, so the independent release path has a
    // real held input.
    input.update(|s| s.send_pause = Some((2, Duration::from_secs(1))));
    let backend = Arc::new(InputBackend::new(input.clone()));
    let driver = driver
        .with_input(backend.clone())
        .with_safety(guard.clone());
    let request = click(&win, PointerRoute::AllowForeground);
    let cancel = Cancel::default();
    let operation = driver.perform(&request, &cancel);
    tokio::pin!(operation);
    std::future::poll_fn(|cx| {
        assert!(operation.as_mut().poll(cx).is_pending());
        if input.state().sent.iter().any(|sent| {
            sent.recorded_before_down
                && matches!(
                    sent.event,
                    gcu_driver_windows::input::api::Event::Button(Button::Left, true, ..)
                )
        }) {
            std::task::Poll::Ready(())
        } else {
            std::task::Poll::Pending
        }
    })
    .await;
    assert!(!backend.ledger_empty(), "left down must have a pending up");
    api.remove_hooks();
    for _ in 0..5 {
        api.advance(Duration::from_millis(20));
        api.poll();
    }
    assert!(!guard.is_stopped(), "exact 100ms boundary remains valid");
    api.advance(Duration::from_millis(1));
    assert!(
        guard.is_stopped(),
        "synchronous existing watchdog, no new timer"
    );
    // Do not poll the blocked operation: release must run on the independently
    // signalled path, within the existing 500ms release budget.
    tokio::time::timeout(Duration::from_millis(500), async {
        while !input.state().sent.iter().any(|sent| {
            matches!(
                sent.event,
                gcu_driver_windows::input::api::Event::Button(Button::Left, false, ..)
            )
        }) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("in-flight lease failure must independently emit left up within 500ms");
    assert!(backend.ledger_empty());
    assert!(operation.await.unwrap_err().maybe_executed());
}
