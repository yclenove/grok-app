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
    assert_eq!(guard.activity_clock().ms_since_physical_input(), Some(0));
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
