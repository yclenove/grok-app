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
    let cycle = guard.events.foreground_cycle();
    guard
        .events
        .confirm_hook_proofs(cycle, Some(time.now()), None)
        .unwrap();
    std::future::poll_fn(|cx| {
        assert!(operation.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(!guard.events.injecting(), "keyboard alone cannot arm/renew");
    guard
        .events
        .confirm_hook_proofs(cycle, Some(time.now()), Some(time.now()))
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
                .confirm_hook_proofs(0, Some(time.now()), None)
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
            .confirm_hook_proofs(0, Some(time.now()), Some(tentative_mouse))
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
            let cycle = guard.events.foreground_cycle();
            guard
                .events
                .confirm_hook_proofs(cycle, Some(time.now()), Some(time.now()))
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
        .confirm_hook_proofs(0, Some(time.now()), Some(mouse))
        .unwrap();
    assert!(guard.available());
    time.advance(Duration::from_millis(1));
    beat(&guard);
    assert!(guard
        .events
        .confirm_hook_proofs(0, Some(time.now()), Some(mouse))
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
    {
        let mut window = guard.events.window.lock().unwrap();
        window.phase = lease::Phase::Injecting;
        window.generation = 1;
        window.proof_floor = 1;
    }
    time.advance(HEARTBEAT_TIMEOUT + Duration::from_millis(1));
    beat(&guard);
    let error = guard
        .events
        .confirm_hook_proofs(1, Some(proven), Some(proven))
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

#[tokio::test]
async fn late_initial_probe_pair_overlapping_first_foreground_does_not_latch_stop() {
    use std::{future::Future, task::Poll};
    let time = Arc::new(ManualTime::default());
    let guard = SafetyGuard::new(Arc::new(startup::Unstopped), time.clone());
    // The 8s idle wait is not a startup-stage failure. Pumps stay healthy.
    guard.events.setup_complete.store(true, Ordering::Release);
    beat(&guard);
    let initial_keyboard = time.now();
    time.advance(Duration::from_secs(8));
    beat(&guard);
    guard
        .events
        .confirm_hook_proofs(0, Some(time.now()), Some(time.now()))
        .unwrap();
    beat(&guard);
    assert!(guard.available());
    let cancel = Cancel::default();
    let operation = guard.begin_foreground(
        &cancel,
        tokio::time::Instant::now() + gcu_core::driver::DRIVER_DEADLINE,
    );
    tokio::pin!(operation);
    let admitted = std::future::poll_fn(|cx| Poll::Ready(operation.as_mut().poll(cx))).await;
    assert!(
        matches!(admitted, Poll::Pending),
        "initial cycle must not admit injection"
    );
    assert!(!guard.events.injecting());
    assert!(!guard.is_stopped());
    assert!(
        !guard.available(),
        "initial lease is discarded at begin_foreground"
    );
    let initial = guard
        .events
        .confirm_hook_proofs(0, Some(initial_keyboard), Some(time.now()));
    assert!(initial.is_ok(), "{initial:?}");
    assert!(!guard.available(), "retired cycle must not renew the lease");
    assert!(!guard.is_stopped());
    let cycle = guard.events.foreground_cycle();
    guard
        .events
        .confirm_hook_proofs(cycle, Some(time.now()), Some(time.now()))
        .unwrap();
    let ready = std::future::poll_fn(|cx| Poll::Ready(operation.as_mut().poll(cx))).await;
    let lease = match ready {
        Poll::Ready(Ok(lease)) => lease,
        Poll::Ready(Err(error)) => panic!("action-cycle proof must admit: {error}"),
        Poll::Pending => panic!("action-cycle proof must admit"),
    };
    assert!(guard.events.injecting() && guard.available() && !guard.is_stopped());
    // After Injecting. The same pair while still Proving would be Ok without the floor.
    let overlap = guard
        .events
        .confirm_hook_proofs(0, Some(initial_keyboard), Some(time.now()));
    if let Err(reason) = overlap.clone() {
        guard.events.fail(reason);
    }
    assert!(overlap.is_ok(), "{overlap:?}");
    assert!(!guard.is_stopped());
    assert!(!guard.events.failed.load(Ordering::Acquire));
    assert!(guard.available(), "action lease survives the retired pair");
    lease.finish();
    assert!(!guard.events.injecting());
    assert!(!guard.is_stopped());
    assert!(!guard.available());
}

/// Drive `begin_foreground` with the fake's serial probe delay.
/// The first poll only submits. Mouse and keyboard callbacks are one poll apart,
/// and the pair is reported on the poll after the keyboard callback.
async fn admit_serial_pair(
    api: &FakeSafetyApi,
    guard: &Arc<SafetyGuard>,
) -> lease::ForegroundLease {
    use std::{future::Future, task::Poll};
    let cancel = Cancel::default();
    let operation = guard.begin_foreground(
        &cancel,
        tokio::time::Instant::now() + gcu_core::driver::DRIVER_DEADLINE,
    );
    tokio::pin!(operation);
    for _ in 0..16 {
        let polled = std::future::poll_fn(|cx| Poll::Ready(operation.as_mut().poll(cx))).await;
        match polled {
            Poll::Ready(Ok(lease)) => return lease,
            Poll::Ready(Err(error)) => panic!("serial action proof must admit: {error}"),
            Poll::Pending => {
                api.advance(Duration::from_millis(20));
                api.poll();
            }
        }
    }
    panic!("serial probe pair did not admit");
}

#[test]
fn confirmed_pair_dispatches_before_injection_is_visible() {
    assert!(should_dispatch_action_probe(false, true, false));
    assert!(should_dispatch_action_probe(false, false, false));
    assert!(should_dispatch_action_probe(false, true, true));
    assert!(!should_dispatch_action_probe(true, true, true));
}

#[tokio::test]
async fn serial_20ms_probe_pair_survives_a_70ms_injection() {
    let api = FakeSafetyApi::default();
    let guard = api.start(Default::default(), Arc::new(startup::Unstopped));
    assert!(guard.available());
    api.set_serial_probe_delay(Duration::from_millis(20), Duration::from_millis(20));
    let lease = admit_serial_pair(&api, &guard).await;
    let age = guard.events.clock().lease_age().expect("admitted pair");
    assert!(
        (Duration::from_millis(20)..=Duration::from_millis(50)).contains(&age),
        "admission anchor is the earlier callback, not the poll instant: {age:?}"
    );
    assert!(guard.events.injecting() && !guard.is_stopped());
    let mut elapsed = Duration::ZERO;
    for step in 1..=4 {
        api.advance(Duration::from_millis(20));
        assert!(
            !guard.is_stopped(),
            "step {step} before poll: {:?}",
            guard.events.reason.lock().unwrap().clone()
        );
        api.poll();
        assert!(
            !guard.is_stopped(),
            "step {step} after poll: {:?}",
            guard.events.reason.lock().unwrap().clone()
        );
        elapsed += Duration::from_millis(20);
    }
    assert!(elapsed >= Duration::from_millis(70));
    assert!(
        guard.available(),
        "a renewal pair must have moved the anchor"
    );
    assert!(guard.events.injecting());
    drop(lease);
}

#[tokio::test]
async fn delayed_serial_probes_still_latch_when_hooks_are_removed() {
    let api = FakeSafetyApi::default();
    let guard = api.start(Default::default(), Arc::new(startup::Unstopped));
    api.set_serial_probe_delay(Duration::from_millis(20), Duration::from_millis(20));
    let lease = admit_serial_pair(&api, &guard).await;
    let age = guard.events.clock().lease_age().expect("admitted pair");
    api.remove_hooks();
    api.poll();
    assert_eq!(
        guard.events.clock().lease_age(),
        Some(age),
        "a removed hook must not apply or refresh the in-flight pair"
    );
    assert!(!guard.is_stopped());
    api.advance(HEARTBEAT_TIMEOUT);
    assert!(guard.is_stopped());
    assert!(!guard.available());
    let reason = guard.events.reason.lock().unwrap().clone().unwrap();
    assert!(reason.contains("during foreground injection"), "{reason}");
    assert!(!reason.contains("possibly silently removed"), "{reason}");
    drop(lease);
}

#[test]
fn cross_attempt_pair_would_be_removal_if_confirmed_so_it_is_not_a_pair() {
    let (time, guard) = healthy();
    let mouse = time.now();
    {
        let mut window = guard.events.window.lock().unwrap();
        window.phase = lease::Phase::Injecting;
        window.generation = 1;
        window.proof_floor = 1;
    }
    time.advance(Duration::from_millis(110));
    beat(&guard);
    let keyboard = time.now();
    let error = guard
        .events
        .confirm_hook_proofs(1, Some(keyboard), Some(mouse))
        .expect_err("an old mouse plus a later keyboard is past the heartbeat");
    assert!(error.contains("possibly silently removed"), "{error}");
    assert!(
        !guard.is_stopped(),
        "confirm's error must not latch until the listener calls fail"
    );
    assert_eq!(
        super::super::hook_probe::keyboard_stamp_for_mouse(Some(1), Some((2, keyboard))),
        None,
        "a different attempt tag is not the mouse pair"
    );
    guard
        .events
        .confirm_hook_proofs(1, None, Some(mouse))
        .expect("omitting the foreign keyboard is not hook removal");
    assert!(!guard.is_stopped());
    assert!(!guard.events.failed.load(Ordering::Acquire));
}

#[test]
fn probe_trace_records_after_arm_without_payload() {
    let (_time, guard) = healthy();
    guard.events.probe_trace(
        "hook_mouse_dispatch",
        "call_ms=2; sent=2/2; packet=mouse+keyup",
    );
    let mut issues = Vec::new();
    guard.doctor_issues(&mut issues);
    let line = issues
        .iter()
        .find(|issue| issue.starts_with("hook_probe_trace:"))
        .expect("doctor keeps the latest probe trace");
    assert!(line.contains("hook_mouse_dispatch"));
    assert!(line.contains("call_ms=2"));
    assert!(line.contains("packet=mouse+keyup"));
    assert!(!line.contains("vk") && !line.contains("coord"));
    assert!(issues
        .iter()
        .any(|issue| { issue.starts_with("injection_lease: fresh") && issue.contains("age_ms=") }));
}
