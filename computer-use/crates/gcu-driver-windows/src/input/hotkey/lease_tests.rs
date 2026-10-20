//! ADR 0013: only an action window requires the 100ms injected lease.
use super::{lease::FOREGROUND_PROOF_BUDGET, startup::ManualTime, *};
use crate::input::{hooks::HookEvent, watchdog::HEARTBEAT_TIMEOUT};
use gcu_core::driver::Cancel;

fn healthy() -> (Arc<ManualTime>, Arc<SafetyGuard>) {
    let time = Arc::new(ManualTime::default());
    let guard = SafetyGuard::new(Arc::new(startup::Unstopped), time.clone());
    guard.events.clock.confirm(time.now());
    beat(&guard);
    assert!(guard.available());
    (time, guard)
}
fn beat(guard: &SafetyGuard) {
    guard.events.heartbeat();
    guard.events.hotkey_heartbeat();
}

#[test]
fn idle_mouse_lease_expired_with_fresh_pumps_keeps_transport_and_activity() {
    let (time, guard) = healthy();
    assert_eq!(
        guard.ms_since_physical_input(),
        None,
        "confirm is not activity"
    );
    guard.events.clock.event(HookEvent::Mouse(0), time.now());
    time.advance(HEARTBEAT_TIMEOUT + Duration::from_millis(1));
    beat(&guard);
    guard.events.check_health();
    assert!(!guard.available());
    assert!(guard.transport_healthy());
    assert_eq!(guard.check(true), Ok(()));
    assert_eq!(guard.activity_clock().ms_since_physical_input(), Some(101));
    assert!(!guard.is_stopped());
    assert!(!guard.events.failed.load(Ordering::Acquire));
    let mut issues = Vec::new();
    guard.doctor_issues(&mut issues);
    assert!(issues.iter().any(|i| i == "user_activity_guard: available"));
    assert!(issues
        .iter()
        .any(|i| i.starts_with("injection_lease: not fresh")));
    assert!(issues.iter().any(|i| i == "stop_hotkey: available"));
    guard.events.clock.confirm(time.now());
    assert!(guard.available(), "idle expiry must not disable the clock");
    assert_eq!(
        guard.ms_since_physical_input(),
        Some(101),
        "confirm never overwrites physical age"
    );
}

#[test]
fn physical_callback_before_coverage_or_after_disable_cannot_create_activity() {
    let time = Arc::new(ManualTime::default());
    let clock = PhysicalClock::with_time(time.clone());
    clock.event(HookEvent::Mouse(0), time.now());
    clock.confirm(time.now());
    assert_eq!(clock.ms_since_physical_input(), None);
    clock.event(HookEvent::Keyboard(0), time.now());
    assert_eq!(clock.ms_since_physical_input(), Some(0));
    clock.disable();
    clock.event(HookEvent::Mouse(0), time.now());
    clock.confirm(time.now());
    assert_eq!(clock.ms_since_physical_input(), None);
    assert!(!clock.available());
}

#[test]
fn physical_callback_during_action_updates_activity_without_renewing_lease() {
    let (time, guard) = healthy();
    guard.events.window.lock().unwrap().phase = lease::Phase::Injecting;
    let released = Arc::new(AtomicBool::new(false));
    let signal = released.clone();
    guard.bind_stop(Arc::new(move || signal.store(true, Ordering::Release)));
    time.advance(HEARTBEAT_TIMEOUT);
    guard.events.clock.event(HookEvent::Mouse(0), time.now());
    beat(&guard);
    guard.events.check_health();
    assert!(!guard.is_stopped());
    time.advance(Duration::from_millis(1));
    beat(&guard);
    guard.events.check_health();
    assert!(guard.is_stopped());
    assert!(released.load(Ordering::Acquire));
    assert!(!guard.available());
    guard.events.clock.confirm(time.now());
    assert!(
        !guard.available(),
        "late completion cannot revive a failed listener"
    );
}

#[tokio::test]
async fn foreground_preparation_without_new_dual_callbacks_never_admits_injection() {
    use std::{future::Future, task::Poll};
    let (time, guard) = healthy();
    time.advance(HEARTBEAT_TIMEOUT + Duration::from_millis(1));
    beat(&guard);
    let cancel = gcu_core::driver::Cancel::default();
    let operation = guard.begin_foreground(
        &cancel,
        tokio::time::Instant::now() + gcu_core::driver::DRIVER_DEADLINE,
    );
    tokio::pin!(operation);
    std::future::poll_fn(|cx| {
        assert!(operation.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(!guard.events.injecting());
    guard
        .events
        .confirm_hook_proofs(Some(time.now()), None)
        .unwrap();
    std::future::poll_fn(|cx| {
        assert!(operation.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(!guard.events.injecting(), "keyboard alone cannot arm/renew");
    guard
        .events
        .confirm_hook_proofs(Some(time.now()), Some(time.now()))
        .unwrap();
    let lease = operation.await.unwrap();
    assert!(guard.events.injecting());
    assert_eq!(guard.activity_clock().ms_since_physical_input(), None);
    lease.finish();
    assert!(!guard.events.injecting());
    assert!(
        !guard.available(),
        "completion discards the injection lease"
    );
    assert!(!guard.is_stopped());
}

#[test]
fn legacy_zero_chain_absolute_callback_four_or_five_pumps_late_expires_before_confirmation() {
    for pumps in [4, 5] {
        let (time, guard) = healthy();
        guard.events.window.lock().unwrap().phase = lease::Phase::Injecting;
        // Model the old renewal chain: three accepted silent zero probes, then
        // absolute mouse delivery, keyboard delivery and sender-report pumping.
        for _ in 0..pumps {
            time.advance(watchdog::POLL_INTERVAL);
            beat(&guard);
            guard
                .events
                .confirm_hook_proofs(Some(time.now()), None)
                .unwrap();
            guard.events.check_health();
        }
        let tentative_mouse = time.now();
        time.advance(Duration::from_millis(101 - pumps * 20));
        beat(&guard);
        guard.events.check_health();
        assert!(
            guard.is_stopped(),
            "old lease expires before the new pair completes"
        );
        guard
            .events
            .confirm_hook_proofs(Some(time.now()), Some(tentative_mouse))
            .unwrap();
        assert!(
            !guard.available(),
            "disable rejects the late native completion"
        );
    }
}

#[tokio::test]
async fn invalid_cursor_proof_before_injection_refuses_without_stop_and_next_action_can_prove() {
    use std::{future::Future, task::Poll};
    let (time, guard) = healthy();
    guard.events.clock.discard_lease();
    for refuse in [true, false] {
        let cancel = gcu_core::driver::Cancel::default();
        let operation = guard.begin_foreground(
            &cancel,
            tokio::time::Instant::now() + gcu_core::driver::DRIVER_DEADLINE,
        );
        tokio::pin!(operation);
        std::future::poll_fn(|cx| {
            assert!(operation.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        if refuse {
            guard
                .events
                .refuse_proof("cursor changed during probe".into());
            assert!(matches!(
                operation.await,
                Err(DriverError::CapabilityUnavailable)
            ));
            assert!(!guard.is_stopped());
            assert!(!guard.events.failed.load(Ordering::Acquire));
            guard.events.clock.event(HookEvent::Mouse(0), time.now());
        } else {
            guard
                .events
                .confirm_hook_proofs(Some(time.now()), Some(time.now()))
                .unwrap();
            operation.await.unwrap().finish();
        }
    }
}

#[test]
fn invalid_cursor_proof_during_injection_latches_and_signals_release() {
    let (_, guard) = healthy();
    guard.events.window.lock().unwrap().phase = lease::Phase::Injecting;
    let released = Arc::new(AtomicBool::new(false));
    let signal = released.clone();
    guard.bind_stop(Arc::new(move || signal.store(true, Ordering::Release)));
    guard
        .events
        .refuse_proof("cursor changed during probe".into());
    assert!(guard.is_stopped());
    assert!(released.load(Ordering::Acquire));
    assert!(!guard.available());
}

#[tokio::test]
async fn unhealthy_transport_or_latched_stop_foreground_request_sends_no_proof() {
    let (_, guard) = healthy();
    let proofs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = proofs.clone();
    *guard.events.proof_request.lock().unwrap() = Some(Arc::new(move || {
        count.fetch_add(1, Ordering::Relaxed);
    }));
    guard.disable("transport unavailable");
    let result = guard
        .begin_foreground(
            &gcu_core::driver::Cancel::default(),
            tokio::time::Instant::now() + gcu_core::driver::DRIVER_DEADLINE,
        )
        .await;
    assert!(matches!(
        result,
        Err(DriverError::Cancelled {
            maybe_executed: false
        })
    ));
    assert_eq!(proofs.load(Ordering::Relaxed), 0);
}

#[test]
fn keyboard_only_renewal_inflight_cannot_extend_mouse_lease() {
    let (time, guard) = healthy();
    let mouse = time.now();
    guard.events.window.lock().unwrap().phase = lease::Phase::Injecting;
    time.advance(HEARTBEAT_TIMEOUT);
    beat(&guard);
    guard
        .events
        .confirm_hook_proofs(Some(time.now()), Some(mouse))
        .unwrap();
    assert!(guard.available());
    time.advance(Duration::from_millis(1));
    beat(&guard);
    assert!(guard
        .events
        .confirm_hook_proofs(Some(time.now()), Some(mouse))
        .is_err());
    guard.events.check_health();
    assert!(guard.is_stopped());
    assert!(!guard.available());
}

#[tokio::test(start_paused = true)]
async fn foreground_proof_without_callbacks_gives_up_within_the_proof_budget() {
    let (time, guard) = healthy();
    time.advance(HEARTBEAT_TIMEOUT + Duration::from_millis(1));
    beat(&guard);
    assert!(!guard.available());
    let start = tokio::time::Instant::now();
    let deadline = start + Duration::from_millis(4750);
    let outcome = guard.begin_foreground(&Cancel::default(), deadline).await;
    assert!(matches!(outcome, Err(DriverError::CapabilityUnavailable)));
    assert!(
        start.elapsed() <= FOREGROUND_PROOF_BUDGET + Duration::from_millis(1),
        "a proof that never sees callbacks must not spend the driver deadline"
    );
    assert!(!guard.is_stopped());
    assert!(guard.transport_healthy());
}

#[test]
fn stale_action_proof_during_injection_latches_stop() {
    let (time, guard) = healthy();
    let proven = time.now();
    guard.events.window.lock().unwrap().phase = lease::Phase::Injecting;
    time.advance(HEARTBEAT_TIMEOUT + Duration::from_millis(1));
    beat(&guard);
    let error = guard
        .events
        .confirm_hook_proofs(Some(proven), Some(proven))
        .expect_err("a stale action-period pair is hook removal");
    assert!(error.contains("possibly silently removed"), "{error}");
    guard.events.fail(error);
    assert!(guard.is_stopped());
    assert!(!guard.available());
    assert!(guard.events.failed.load(Ordering::Acquire));
}

#[tokio::test]
async fn removed_hooks_during_injection_latches_stop() {
    let api = FakeSafetyApi::default();
    let guard = api.start(Default::default(), Arc::new(startup::Unstopped));
    assert!(guard.available());
    let lease = guard
        .begin_foreground(
            &Cancel::default(),
            tokio::time::Instant::now() + gcu_core::driver::DRIVER_DEADLINE,
        )
        .await
        .expect("fresh transport can enter an action window");
    assert!(guard.events.injecting());
    api.remove_hooks();
    api.advance(HEARTBEAT_TIMEOUT);
    assert!(
        !guard.is_stopped(),
        "exactly HEARTBEAT_TIMEOUT is still inside the lease"
    );
    guard.events.heartbeat();
    guard.events.hotkey_heartbeat();
    api.advance(Duration::from_millis(1));
    assert!(guard.is_stopped());
    assert!(!guard.available());
    let reason = guard.events.reason.lock().unwrap().clone().unwrap();
    assert!(reason.contains("during foreground injection"), "{reason}");
    drop(lease);
}

#[test]
fn missing_injection_renewal_latches_at_the_heartbeat_timeout() {
    let (time, guard) = healthy();
    guard.events.window.lock().unwrap().phase = lease::Phase::Injecting;
    time.advance(HEARTBEAT_TIMEOUT);
    beat(&guard);
    guard.events.check_health();
    assert!(
        !guard.is_stopped(),
        "exactly HEARTBEAT_TIMEOUT is still inside the lease"
    );
    time.advance(Duration::from_millis(1));
    beat(&guard);
    guard.events.check_health();
    assert!(guard.is_stopped());
    let reason = guard.events.reason.lock().unwrap().clone().unwrap();
    assert!(reason.contains("during foreground injection"), "{reason}");
    assert!(!reason.contains("possibly silently removed"), "{reason}");
}
