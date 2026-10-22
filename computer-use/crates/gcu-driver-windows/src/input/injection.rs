//! Deterministic interleave of an action move with an absolute probe.
//! The gate itself lives in `hook_probe` so the `#[path]` copies used by the
//! integration tests compile the same function.
//! Real-hardware origin: a heartbeat probe that sampled the old cursor published it after the
//! action's move and pulled the pointer back (independent WH_MOUSE_LL log, 2026-10-21).

#[cfg(test)]
mod tests {
    use super::super::hook_probe::{
        dispatch_for_action, gate_held, with_admitted_gate_on, with_injection_gate,
        with_shared_gate, Cursor, Desktop, GateBusy, MouseProbe, MouseProbeApi, MouseProof,
        ProbeIdle, RecentInput, SharedGate,
    };
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    const OLD: Cursor = Cursor { x: 40, y: 50 };
    const TARGET: Cursor = Cursor { x: 320, y: 180 };

    struct RacingApi {
        cursor: Cursor,
        /// Action move that runs in the sample/send gap unless the gate is held.
        pending_move: Option<Cursor>,
        sampled: Cursor,
        clicked: Option<Cursor>,
    }
    impl RacingApi {
        fn armed() -> Self {
            Self {
                cursor: OLD,
                pending_move: Some(TARGET),
                sampled: OLD,
                clicked: None,
            }
        }
        fn click_at_cursor(&mut self) {
            if let Some(target) = self.pending_move.take() {
                self.cursor = target;
            }
            // Button events carry no position. The click lands on the cursor.
            self.clicked = Some(self.cursor);
        }
    }
    impl MouseProbeApi for RacingApi {
        fn recent_input(&mut self) -> Result<RecentInput, String> {
            Ok(RecentInput {
                now: 10_000,
                last: 0,
            })
        }
        fn cursor(&mut self) -> Result<Cursor, String> {
            let sampled = self.cursor;
            self.sampled = sampled;
            if !gate_held() {
                if let Some(target) = self.pending_move.take() {
                    self.cursor = target;
                }
            }
            Ok(sampled)
        }
        fn desktop(&mut self) -> Result<Desktop, String> {
            Ok(Desktop {
                left: 0,
                top: 0,
                width: 1000,
                height: 800,
            })
        }
        fn send(
            &mut self,
            _mode: MouseProbe,
            absolute: Option<(i32, i32)>,
        ) -> Result<(u32, u32, u128), String> {
            if absolute.is_none() {
                return Err("absolute probe missing".into());
            }
            // The probe publishes the pixel it sampled, not a later action move.
            self.cursor = self.sampled;
            Ok((1, 0, 0))
        }
    }

    #[test]
    fn probe_sample_overlapping_action_move_clicks_the_action_target() {
        let idle = ProbeIdle::default();
        let mut api = RacingApi::armed();
        dispatch_for_action(&mut api, MouseProbe::AbsoluteSamePosition, &idle)
            .expect("same-position probe");
        api.click_at_cursor();
        assert_eq!(
            api.clicked,
            Some(TARGET),
            "a probe that sampled the old cursor must not run its absolute move between the action move and the click"
        );
    }

    #[test]
    fn gated_probe_send_without_callback_is_not_a_lease() {
        let mut proof = MouseProof::new(Some(MouseProbe::AbsoluteSamePosition));
        proof.begin_cycle(true);
        let request = proof.next().expect("action probe");
        let idle = ProbeIdle::default();
        let mut api = RacingApi {
            pending_move: None,
            ..RacingApi::armed()
        };
        dispatch_for_action(&mut api, request.mode, &idle).expect("probe sent");
        proof.complete(Ok(())).expect("send accepted");
        assert!(
            proof.proven().is_none(),
            "acceptance under the gate is not a hook callback and must not renew the lease"
        );
    }

    #[test]
    fn injection_gate_is_released_after_the_section() {
        assert!(!gate_held());
        assert_eq!(with_injection_gate(|| assert!(gate_held())), Ok(()));
        assert!(!gate_held());
    }

    fn wait_until_blocked(gate: &SharedGate) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while gate.waiters() == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "the other thread did not reach the gate"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_holder_on_another_thread_refuses_the_waiter_and_sends_nothing() {
        // Win32 mutexes reenter on the owning thread, so a same-thread nested acquire is not
        // contention. This std gate matches that, and the refusal is cross-thread.
        let gate = SharedGate::new();
        let held = gate.acquire(std::time::Duration::ZERO).unwrap();
        let shared = Arc::clone(&gate);
        let waiter = std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let mut sent = false;
            let result = with_admitted_gate_on(
                &shared,
                std::time::Duration::from_millis(30),
                || Ok::<(), &str>(()),
                || {
                    sent = true;
                    Ok(())
                },
            );
            (result, sent, started.elapsed())
        });
        wait_until_blocked(&gate);
        let (result, sent, waited) = waiter.join().unwrap();
        assert_eq!(result, Err(GateBusy));
        assert!(!sent, "a refused waiter must not run its body");
        assert!(
            waited >= std::time::Duration::from_millis(25)
                && waited < std::time::Duration::from_millis(500),
            "bounded wait, got {waited:?}"
        );
        drop(held);
    }

    #[test]
    fn two_threads_share_exclusion_on_one_gate() {
        let gate = SharedGate::new();
        let held = gate.acquire(std::time::Duration::ZERO).unwrap();
        let shared = Arc::clone(&gate);
        let entered = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&entered);
        let waiter = std::thread::spawn(move || {
            let guard = shared
                .acquire(std::time::Duration::from_secs(2))
                .expect("enters after the holder releases");
            flag.store(true, Ordering::SeqCst);
            drop(guard);
        });
        wait_until_blocked(&gate);
        assert!(
            !entered.load(Ordering::SeqCst),
            "the waiter must not enter while the gate is held"
        );
        drop(held);
        waiter.join().unwrap();
        assert!(entered.load(Ordering::SeqCst));
        assert!(gate.acquire(std::time::Duration::ZERO).is_ok());
    }

    #[test]
    fn same_thread_reentry_keeps_other_threads_out_until_the_outer_release() {
        let gate = SharedGate::new();
        let outer = gate.acquire(std::time::Duration::ZERO).unwrap();
        let inner = gate
            .acquire(std::time::Duration::ZERO)
            .expect("the owning thread reenters, matching a Win32 mutex");
        let shared = Arc::clone(&gate);
        let entered = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&entered);
        let waiter = std::thread::spawn(move || {
            let guard = shared.acquire(std::time::Duration::from_secs(2)).unwrap();
            flag.store(true, Ordering::SeqCst);
            drop(guard);
        });
        wait_until_blocked(&gate);
        assert!(!entered.load(Ordering::SeqCst));
        drop(inner);
        assert_eq!(
            gate.waiters(),
            1,
            "releasing the inner count must not admit the waiter"
        );
        assert!(!entered.load(Ordering::SeqCst));
        drop(outer);
        waiter.join().unwrap();
        assert!(entered.load(Ordering::SeqCst));
    }

    #[test]
    fn failed_admit_skips_the_body_and_releases_the_gate() {
        let gate = SharedGate::new();
        let mut ran = false;
        let result = with_admitted_gate_on(
            &gate,
            std::time::Duration::from_millis(20),
            || {
                assert!(
                    gate.held_by_current(),
                    "admission runs only while the gate is held"
                );
                Err("refused")
            },
            || {
                ran = true;
                Ok::<(), &str>(())
            },
        );
        assert_eq!(result, Ok(Err("refused")));
        assert!(!ran, "a failed admission must not send");
        assert!(
            !gate.held_by_current(),
            "a failed admission must release the gate"
        );
        assert!(gate.acquire(std::time::Duration::ZERO).is_ok());
    }

    #[test]
    fn admit_does_not_run_until_the_holding_thread_releases() {
        let gate = SharedGate::new();
        let held = gate.acquire(std::time::Duration::ZERO).unwrap();
        let shared = Arc::clone(&gate);
        let admitted = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&admitted);
        let waiter = std::thread::spawn(move || {
            with_admitted_gate_on(
                &shared,
                std::time::Duration::from_secs(2),
                || {
                    flag.store(true, Ordering::SeqCst);
                    Ok::<(), &str>(())
                },
                || Ok(()),
            )
        });
        wait_until_blocked(&gate);
        assert!(
            !admitted.load(Ordering::SeqCst),
            "admission must not run before this thread releases the gate"
        );
        drop(held);
        assert!(waiter.join().unwrap().is_ok());
        assert!(admitted.load(Ordering::SeqCst));
        assert!(gate.acquire(std::time::Duration::ZERO).is_ok());
    }

    #[test]
    fn a_probe_that_cannot_get_the_gate_refuses_its_cycle() {
        let gate = SharedGate::new();
        let shared = Arc::clone(&gate);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let held = shared.acquire(std::time::Duration::ZERO).unwrap();
            ready_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            drop(held);
        });
        ready_rx.recv().unwrap();
        let idle = ProbeIdle::default();
        let mut api = RacingApi {
            pending_move: None,
            ..RacingApi::armed()
        };
        let error = with_shared_gate(Arc::clone(&gate), || {
            dispatch_for_action(&mut api, MouseProbe::AbsoluteSamePosition, &idle)
        })
        .expect_err("no gate, no probe");
        assert!(error.contains("pointer input in flight"), "{error}");
        assert_eq!(api.clicked, None);
        release_tx.send(()).unwrap();
        holder.join().unwrap();
    }
}
