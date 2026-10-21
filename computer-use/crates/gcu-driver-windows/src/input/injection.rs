//! Deterministic interleave of an action move with an absolute probe.
//! The gate itself lives in `hook_probe` so the `#[path]` copies used by the
//! integration tests compile the same function.
//! Real-hardware origin: a heartbeat probe that sampled the old cursor published it after the
//! action's move and pulled the pointer back (independent WH_MOUSE_LL log, 2026-10-21).

#[cfg(test)]
mod tests {
    use super::super::hook_probe::{
        dispatch_for_action, gate_held, with_injection_gate, with_injection_gate_for, Cursor,
        Desktop, GateBusy, MouseProbe, MouseProbeApi, MouseProof, ProbeIdle, RecentInput,
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

    #[test]
    fn a_stuck_holder_refuses_the_waiter_within_its_bound_and_sends_nothing() {
        // Same thread: the gate is not reentrant, so the inner waiter contends with the holder.
        with_injection_gate(|| {
            let started = std::time::Instant::now();
            let mut sent = false;
            let result =
                with_injection_gate_for(std::time::Duration::from_millis(30), || sent = true);
            let waited = started.elapsed();
            assert_eq!(result, Err(GateBusy));
            assert!(!sent, "a refused waiter must not run its body");
            assert!(
                waited >= std::time::Duration::from_millis(25)
                    && waited < std::time::Duration::from_millis(500),
                "bounded wait, got {waited:?}"
            );
        })
        .unwrap();
    }

    #[test]
    fn a_probe_that_cannot_get_the_gate_refuses_its_cycle() {
        let idle = ProbeIdle::default();
        let mut api = RacingApi {
            pending_move: None,
            ..RacingApi::armed()
        };
        let error = with_injection_gate(|| {
            dispatch_for_action(&mut api, MouseProbe::AbsoluteSamePosition, &idle)
        })
        .unwrap()
        .expect_err("no gate, no probe");
        assert!(error.contains("pointer input in flight"), "{error}");
        assert_eq!(api.clicked, None);
    }
}
