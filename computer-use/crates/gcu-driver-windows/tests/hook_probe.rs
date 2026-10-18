//! Portable product probe tests: injected sender, cursor and hook callbacks.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]
#[path = "../src/input/hook_probe.rs"]
mod policy;
use policy::*;
use std::{ffi::OsStr, sync::Arc, time::Instant};

struct Fake {
    recent: Result<RecentInput, String>,
    recent_reads: usize,
    before: Cursor,
    after: Result<Cursor, String>,
    reads: usize,
    record_injected: bool,
    during_sample: Option<Arc<ProbeIdle>>,
    during_cursor: Option<Arc<ProbeIdle>>,
    during_send: Option<Arc<ProbeIdle>>,
    sends: Vec<(MouseProbe, Option<(i32, i32)>)>,
}
impl Default for Fake {
    fn default() -> Self {
        Self {
            recent: Ok(RecentInput {
                now: 5000,
                last: 3000,
            }),
            recent_reads: 0,
            before: Cursor { x: -100, y: 100 },
            after: Ok(Cursor { x: -100, y: 100 }),
            reads: 0,
            record_injected: false,
            during_sample: None,
            during_cursor: None,
            during_send: None,
            sends: Vec::new(),
        }
    }
}
impl MouseProbeApi for Fake {
    fn recent_input(&mut self) -> Result<RecentInput, String> {
        self.recent_reads += 1;
        if let Some(idle) = &self.during_sample {
            idle.observe(false, false);
        }
        self.recent.clone()
    }
    fn cursor(&mut self) -> Result<Cursor, String> {
        self.reads += 1;
        if let Some(idle) = &self.during_cursor {
            idle.observe(false, false);
        }
        if self.reads == 1 {
            Ok(self.before)
        } else {
            self.after.clone()
        }
    }
    fn desktop(&mut self) -> Result<Desktop, String> {
        Ok(Desktop {
            left: -1920,
            top: -200,
            width: 5760,
            height: 2360,
        })
    }
    fn send(&mut self, mode: MouseProbe, absolute: Option<(i32, i32)>) -> Result<Dispatch, String> {
        self.sends.push((mode, absolute));
        if let Some(idle) = &self.during_send {
            idle.observe(false, false);
        }
        if self.record_injected {
            if let Ok(recent) = &mut self.recent {
                recent.last = recent.now as u32;
            }
        }
        Ok((mode.requested(), 0, 0))
    }
}
fn fourth(proof: &mut MouseProof, fake: &mut Fake) -> ProbeRequest {
    for mode in [
        MouseProbe::ZeroMove,
        MouseProbe::ZeroWheel,
        MouseProbe::ZeroHWheel,
    ] {
        let request = proof.next().unwrap();
        assert_eq!(request.mode, mode);
        proof
            .complete(dispatch(fake, request.mode, proof.idle()).map(|_| ()))
            .unwrap();
        assert_eq!(proof.proven(), None);
    }
    let request = proof.next().unwrap();
    assert_eq!(request.mode, MouseProbe::AbsoluteSamePosition);
    request
}

#[test]
fn first_three_silent_fourth_tagged_injected_and_verified_proves_mouse() {
    let mut proof = MouseProof::new(None);
    let mut fake = Fake::default();
    let request = fourth(&mut proof, &mut fake);
    let at = Instant::now();
    proof.callback(request.tag, 1, at);
    assert_eq!(
        proof.proven(),
        None,
        "callback cannot precede cursor verification"
    );
    proof
        .complete(dispatch(&mut fake, request.mode, proof.idle()).map(|_| ()))
        .unwrap();
    assert_eq!(proof.proven(), Some(at));
    assert_eq!(fake.sends.len(), 4);
    assert_eq!(proof.next().unwrap().mode, MouseProbe::AbsoluteSamePosition);
}

#[test]
fn fourth_unmarked_or_non_injected_or_missing_callback_never_proves_or_retries() {
    for callback in [None, Some((false, 1)), Some((true, 0))] {
        let mut proof = MouseProof::new(None);
        let mut fake = Fake::default();
        let request = fourth(&mut proof, &mut fake);
        if let Some((tagged, flags)) = callback {
            proof.callback(if tagged { request.tag } else { 0 }, flags, Instant::now());
        }
        proof
            .complete(dispatch(&mut fake, request.mode, proof.idle()).map(|_| ()))
            .unwrap();
        assert_eq!(proof.proven(), None);
        for _ in 0..10 {
            assert!(proof.next().is_none());
        }
        assert_eq!(fake.sends.len(), 4);
    }
}

#[test]
fn fourth_cursor_changed_invalidates_even_tagged_callback_without_compensation() {
    let mut proof = MouseProof::new(None);
    let mut fake = Fake {
        after: Ok(Cursor { x: -99, y: 100 }),
        ..Fake::default()
    };
    let request = fourth(&mut proof, &mut fake);
    proof.callback(request.tag, 1, Instant::now());
    let error = proof
        .complete(dispatch(&mut fake, request.mode, proof.idle()).map(|_| ()))
        .unwrap_err();
    assert!(error.contains("cursor changed during probe"));
    assert_eq!(proof.proven(), None);
    assert!(proof.next().is_none());
    assert_eq!(fake.sends.len(), 4, "one absolute move, no retry or repair");
}

#[test]
fn fourth_idle_below_threshold_unknown_or_wrapped_sends_no_additional_input() {
    for recent in [
        Ok(RecentInput {
            now: 5000,
            last: 3001,
        }),
        Err("GetLastInputInfo failed".into()),
        Err("noninteractive desktop".into()),
        Ok(RecentInput {
            now: 10,
            last: u32::MAX,
        }),
        Ok(RecentInput {
            now: u64::from(u32::MAX) + 1,
            last: 0,
        }),
    ] {
        let mut proof = MouseProof::new(None);
        let mut fake = Fake {
            recent,
            ..Fake::default()
        };
        let request = fourth(&mut proof, &mut fake);
        assert!(proof
            .complete(dispatch(&mut fake, request.mode, proof.idle()).map(|_| ()))
            .is_err());
        assert!(proof.next().is_none());
        assert_eq!(proof.proven(), None);
        assert_eq!(fake.sends.len(), 3);
        assert_eq!(fake.reads, 0);
    }
}

#[test]
fn fourth_exact_threshold_and_longer_idle_sends_once() {
    for now in [5000, 5001, 10000] {
        let mut fake = Fake {
            recent: Ok(RecentInput { now, last: 3000 }),
            ..Fake::default()
        };
        dispatch(
            &mut fake,
            MouseProbe::AbsoluteSamePosition,
            &ProbeIdle::default(),
        )
        .unwrap();
        assert_eq!(fake.sends.len(), 1);
    }
}

#[test]
fn physical_input_during_initial_sample_wins_over_idle_read() {
    let mut proof = MouseProof::new(None);
    let mut fake = Fake {
        during_sample: Some(proof.idle().clone()),
        ..Fake::default()
    };
    let request = fourth(&mut proof, &mut fake);
    assert!(proof
        .complete(dispatch(&mut fake, request.mode, proof.idle()).map(|_| ()))
        .is_err());
    assert_eq!(fake.sends.len(), 3);
    assert_eq!(fake.recent_reads, 1);
    assert!(!proof.idle().allowed());
}

#[test]
fn physical_input_during_cursor_queries_prevents_absolute_send() {
    let mut proof = MouseProof::new(None);
    let mut fake = Fake::default();
    let request = fourth(&mut proof, &mut fake);
    fake.during_cursor = Some(proof.idle().clone());
    assert!(proof
        .complete(dispatch(&mut fake, request.mode, proof.idle()).map(|_| ()))
        .is_err());
    assert_eq!(fake.sends.len(), 3);
    assert!(proof.next().is_none());
}

#[test]
fn physical_input_during_absolute_send_invalidates_even_if_cursor_returns_to_same_pixel() {
    let mut proof = MouseProof::new(None);
    let mut fake = Fake::default();
    let request = fourth(&mut proof, &mut fake);
    proof.callback(request.tag, 1, Instant::now());
    fake.during_send = Some(proof.idle().clone());
    assert!(proof
        .complete(dispatch(&mut fake, request.mode, proof.idle()).map(|_| ()))
        .is_err());
    assert_eq!(proof.proven(), None);
    assert!(proof.next().is_none());
    assert_eq!(fake.sends.len(), 4, "no compensating input");
}

#[test]
fn initial_idle_failure_cannot_be_resampled_after_zeros() {
    let mut proof = MouseProof::new(None);
    let mut fake = Fake {
        recent: Err("clock unavailable".into()),
        ..Fake::default()
    };
    let request = fourth(&mut proof, &mut fake);
    fake.recent = Ok(RecentInput {
        now: 10000,
        last: 0,
    });
    assert!(proof
        .complete(dispatch(&mut fake, request.mode, proof.idle()).map(|_| ()))
        .is_err());
    assert_eq!(fake.recent_reads, 1);
    assert_eq!(fake.sends.len(), 3);
}

#[test]
fn initially_idle_self_injection_preserves_gate_and_fourth_sends() {
    let mut fake = Fake {
        record_injected: true,
        ..Fake::default()
    };
    let mut proof = MouseProof::new(None);
    let request = fourth(&mut proof, &mut fake);
    proof
        .complete(dispatch(&mut fake, request.mode, proof.idle()).map(|_| ()))
        .unwrap();
    assert_eq!(fake.sends.len(), 4);
    assert_eq!(
        fake.recent_reads, 1,
        "sample precedes every probe's own input"
    );
    assert!(proof.idle().allowed());
}

#[test]
fn physical_callback_clears_gate_and_cannot_rearm_this_attempt() {
    // Native callback classification supplies these injected/ours booleans for
    // both keyboard and mouse. Either real device revokes the same shared gate.
    for after_probe in 1..=3 {
        let mut fake = Fake::default();
        let mut proof = MouseProof::new(None);
        for index in 1..=3 {
            let request = proof.next().unwrap();
            proof
                .complete(dispatch(&mut fake, request.mode, proof.idle()).map(|_| ()))
                .unwrap();
            if index == after_probe {
                proof.idle().observe(false, false);
            }
        }
        assert!(!proof.idle().allowed());
        let request = proof.next().unwrap();
        assert_eq!(request.mode, MouseProbe::AbsoluteSamePosition);
        assert!(proof
            .complete(dispatch(&mut fake, request.mode, proof.idle()).map(|_| ()))
            .is_err());
        assert!(proof.next().is_none());
        assert_eq!(proof.proven(), None);
        assert_eq!(fake.sends.len(), 3);
        assert_eq!(
            fake.recent_reads, 1,
            "never resample to rearm after revocation"
        );
    }
}

#[test]
fn injected_or_ours_with_stripped_flags_never_clears_idle() {
    use gcu_driver_windows::input::hooks::HookEvent;
    let idle = ProbeIdle::default();
    let mut fake = Fake::default();
    dispatch(&mut fake, MouseProbe::ZeroMove, &idle).unwrap();
    for (event, ours) in [
        (HookEvent::Mouse(1), true),
        (HookEvent::Mouse(2), false),
        (HookEvent::Keyboard(0x10), true),
        (HookEvent::Keyboard(2), false),
        (HookEvent::Mouse(0), true),
        (HookEvent::Keyboard(0), true),
    ] {
        idle.observe(event.injected(), ours);
        assert!(idle.allowed());
    }
    idle.observe(false, false);
    idle.observe(true, true);
    assert!(!idle.allowed());
    assert!(dispatch(&mut fake, MouseProbe::AbsoluteSamePosition, &idle).is_err());
    assert_eq!(fake.recent_reads, 1);
    assert_eq!(fake.sends.len(), 1);
}

#[test]
fn real_keyboard_or_mouse_callback_revokes_fourth_send_permission() {
    use gcu_driver_windows::input::hooks::HookEvent;
    for event in [HookEvent::Keyboard(0), HookEvent::Mouse(0)] {
        let mut proof = MouseProof::new(None);
        let mut fake = Fake::default();
        let request = fourth(&mut proof, &mut fake);
        proof.idle().observe(event.injected(), false);
        assert!(proof
            .complete(dispatch(&mut fake, request.mode, proof.idle()).map(|_| ()))
            .is_err());
        assert_eq!(fake.sends.len(), 3);
        assert_eq!(proof.proven(), None);
        assert!(proof.next().is_none());
    }
}

#[test]
fn initially_idle_allows_renewal_dispatch_but_only_new_hook_callback_renews_proof() {
    let mut fake = Fake {
        record_injected: true,
        ..Fake::default()
    };
    let mut proof = MouseProof::new(None);
    let request = fourth(&mut proof, &mut fake);
    let first = Instant::now();
    proof.callback(request.tag, 1, first);
    proof
        .complete(dispatch(&mut fake, request.mode, proof.idle()).map(|_| ()))
        .unwrap();
    let next = proof.next().unwrap();
    assert_eq!(next.mode, MouseProbe::AbsoluteSamePosition);
    proof
        .complete(dispatch(&mut fake, next.mode, proof.idle()).map(|_| ()))
        .unwrap();
    assert_eq!(
        proof.proven(),
        Some(first),
        "idle/send acceptance cannot renew"
    );
    assert!(
        proof.next().is_none(),
        "one absolute attempt per lease proof"
    );
    assert_eq!(fake.sends.len(), 5);
    assert_eq!(
        fake.recent_reads, 1,
        "own probes never reset the idle marker"
    );
}

#[test]
fn zero_callback_preserves_preference_and_skips_absolute_when_idle_unknown() {
    let mut proof = MouseProof::new(None);
    let mut fake = Fake {
        recent: Err("initial idle read fails; zero proofs remain usable".into()),
        ..Fake::default()
    };
    let request = proof.next().unwrap();
    proof
        .complete(dispatch(&mut fake, request.mode, proof.idle()).map(|_| ()))
        .unwrap();
    proof.callback(request.tag, 1, Instant::now());
    for _ in 0..10 {
        let next = proof.next().unwrap();
        assert_eq!(next.mode, MouseProbe::ZeroMove);
        proof.callback(next.tag, 1, Instant::now());
        proof
            .complete(dispatch(&mut fake, next.mode, proof.idle()).map(|_| ()))
            .unwrap();
    }
    assert_eq!(fake.reads, 0);
    assert_eq!(fake.recent_reads, 1);
}

#[test]
fn fourth_cursor_read_failure_invalidates_without_additional_input() {
    let mut proof = MouseProof::new(None);
    let mut fake = Fake {
        after: Err("cursor unreadable".into()),
        ..Fake::default()
    };
    let request = fourth(&mut proof, &mut fake);
    proof.callback(request.tag, 1, Instant::now());
    assert!(proof
        .complete(dispatch(&mut fake, request.mode, proof.idle()).map(|_| ()))
        .is_err());
    assert_eq!(proof.proven(), None);
    assert_eq!(fake.sends.len(), 4);
    assert!(proof.next().is_none());
}

#[test]
fn physical_coordinates_negative_monitors_and_mixed_dpi_map_to_same_pixel() {
    // Inputs are physical pixels; logical DPI scaling never enters this mapping.
    for (origin, size) in [
        (-1920, 5760),
        (-2160, 6000),
        (0, 3840),
        (0, 7680),
        (-100, 1),
        (-32768, 65535),
        (-32768, 65536),
    ] {
        for offset in 0..size {
            let absolute = normalize(origin + offset, origin, size).unwrap();
            assert!((0..=65535).contains(&absolute));
            assert_eq!(
                i64::from(absolute) * i64::from(size) / 65536,
                i64::from(offset)
            );
        }
    }
    for (point, origin, size) in [(0, 0, 0), (-1, 0, 100), (100, 0, 100), (0, 0, 65537)] {
        assert!(normalize(point, origin, size).is_err());
    }
}

#[test]
fn old_attempt_tag_cannot_prove_later_attempt() {
    let mut proof = MouseProof::new(None);
    let first = proof.next().unwrap();
    proof.complete(Ok(())).unwrap();
    let next = proof.next().unwrap();
    assert_ne!(first.tag, next.tag);
    proof.callback(first.tag, 1, Instant::now());
    proof.complete(Ok(())).unwrap();
    assert_eq!(proof.proven(), None);
}

#[test]
fn override_is_rejected_unless_debug_and_worker_faults_are_both_enabled() {
    assert_eq!(configured_mode(None).unwrap(), None);
    for mode in [
        "zero_move",
        "absolute_same_position",
        "relative_pair",
        "xbutton_pair",
        "invalid",
    ] {
        let selected = configured_mode(Some(OsStr::new(mode)));
        if cfg!(all(feature = "test-worker-faults", debug_assertions)) && mode != "invalid" {
            assert!(selected.unwrap().is_some());
        } else {
            assert!(selected.is_err());
        }
    }
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "disposable physical-mouse Windows Default desktop; one same-position absolute move may trigger hover"]
async fn native_product_probe_chain_proves_both_hooks_without_cursor_change() {
    use gcu_driver_windows::input::{
        hotkey::{HotkeyConfig, SafetyGuard, StopLatch},
        RealSafetyApi,
    };
    use std::{sync::Arc, time::Duration};
    use windows::Win32::{
        Foundation::POINT,
        UI::{
            Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
            WindowsAndMessaging::GetPhysicalCursorPos,
        },
    };
    struct Latch;
    impl StopLatch for Latch {
        fn is_stopped(&self) -> bool {
            false
        }
        fn request_stop(&self) {}
    }
    assert_eq!(std::env::var("GCU_DESKTOP").as_deref(), Ok("1"));
    assert!(
        std::env::var_os("GCU_HOOK_MOUSE_PROBE").is_none(),
        "test product defaults"
    );
    // Let launching the test cease to count as recent keyboard input.
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let mut before = POINT::default();
    // SAFETY: bounded writable physical POINT; no input submitted by this query.
    unsafe { GetPhysicalCursorPos(&mut before) }.unwrap();
    let guard = SafetyGuard::start(
        Arc::new(RealSafetyApi),
        HotkeyConfig::default(),
        Arc::new(Latch),
    )
    .await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while !guard.available() {
            assert!(
                guard.transport_healthy(),
                "transport failed during initial proof"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("one initial tagged dual-hook proof required");
    let mut after = POINT::default();
    // SAFETY: bounded writable physical POINT; never compensate a difference.
    unsafe { GetPhysicalCursorPos(&mut after) }.unwrap();
    let mut issues = Vec::new();
    guard.doctor_issues(&mut issues);
    assert!(
        guard.available(),
        "real tagged dual-hook proof required: {issues:?}"
    );
    assert_eq!((before.x, before.y), (after.x, after.y));
    let mut idle_before = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: read-only bounded session clock on this disposable desktop.
    assert!(unsafe { GetLastInputInfo(&mut idle_before) }.as_bool());
    // No foreground call: system input time must stay unchanged (idle gets older).
    tokio::time::sleep(Duration::from_millis(250)).await;
    let mut idle_after = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: initialized output; this query submits no input.
    assert!(unsafe { GetLastInputInfo(&mut idle_after) }.as_bool());
    assert_eq!(
        idle_before.dwTime, idle_after.dwTime,
        "idle listener submitted input"
    );
    guard.doctor_issues(&mut issues);
    assert!(
        !guard.available(),
        "idle injection lease must expire: {issues:?}"
    );
    assert!(guard.transport_healthy());
    assert!(!guard.is_stopped(), "missing idle proof cannot latch Stop");
}

#[test]
fn action_proof_after_physical_callback_bypasses_idle_gate_without_permanent_failure() {
    let mut proof = MouseProof::new(None);
    let mut fake = Fake::default();
    let request = fourth(&mut proof, &mut fake);
    proof.idle().observe(false, false);
    assert!(proof
        .complete(dispatch(&mut fake, request.mode, proof.idle()).map(|_| ()))
        .is_err());
    proof.begin_cycle(true);
    fake.reads = 0;
    let recent_reads = fake.recent_reads;
    // The next requested action gets a fresh finite round, even after the
    // initial unsolicited absolute probe was denied by real physical activity.
    let mut request = proof.next().unwrap();
    for _ in 0..3 {
        proof
            .complete(dispatch_for_action(&mut fake, request.mode, proof.idle()).map(|_| ()))
            .unwrap();
        request = proof.next().unwrap();
    }
    assert_eq!(request.mode, MouseProbe::AbsoluteSamePosition);
    proof.callback(request.tag, 1, Instant::now());
    proof
        .complete(dispatch_for_action(&mut fake, request.mode, proof.idle()).map(|_| ()))
        .unwrap();
    assert!(proof.proven().is_some());
    assert_eq!(
        fake.recent_reads, recent_reads,
        "action proofs never resample system idle"
    );
    proof.begin_cycle(true);
    assert_eq!(
        proof.next().unwrap().mode,
        MouseProbe::AbsoluteSamePosition,
        "known working absolute kind is pinned across action windows"
    );
}

#[test]
fn action_absolute_cursor_change_refuses_once_without_compensation_or_retry() {
    let mut proof = MouseProof::new(Some(MouseProbe::AbsoluteSamePosition));
    proof.begin_cycle(true);
    let mut fake = Fake {
        after: Ok(Cursor { x: -99, y: 100 }),
        ..Fake::default()
    };
    let request = proof.next().unwrap();
    proof.callback(request.tag, 1, Instant::now());
    assert!(proof
        .complete(dispatch_for_action(&mut fake, request.mode, proof.idle()).map(|_| ()))
        .is_err());
    assert_eq!(proof.proven(), None);
    assert!(proof.next().is_none());
    assert_eq!(fake.sends.len(), 1, "no retry, no compensating movement");
}

#[test]
fn preferred_zero_missing_callback_waits_without_retry_or_absolute_fallback() {
    let mut proof = MouseProof::new(None);
    let initial = proof.next().unwrap();
    proof.callback(initial.tag, 1, Instant::now());
    proof.complete(Ok(())).unwrap();
    proof.begin_cycle(true);
    let next = proof.next().unwrap();
    assert_eq!(next.mode, MouseProbe::ZeroMove);
    proof.complete(Ok(())).unwrap();
    assert_eq!(proof.proven(), None);
    for _ in 0..10 {
        assert!(proof.next().is_none());
    }
    proof.begin_cycle(true);
    assert_eq!(proof.next().unwrap().mode, MouseProbe::ZeroMove);
}
