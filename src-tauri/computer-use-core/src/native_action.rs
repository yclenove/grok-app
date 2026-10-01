//! Occupancy for synchronous native input adapters. Cancellation revokes future
//! dispatch; only the owner returning from native execution may release the slot.
//! Async/native-callback adapters must retain occupancy until their callback,
//! either with the guard or its explicitly deferred, owner-bound recovery ticket.
//! A business deadline alone is never permission to release the slot.

use crate::execution::ActionCancellation;
use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Default)]
pub struct NativeActionSlot {
    active: Mutex<Option<Active>>,
}

struct Active {
    run_id: String,
    generation: u64,
    cancellation: ActionCancellation,
    completion: Arc<Completion>,
}

#[derive(Default)]
struct Completion {
    owner_returned: AtomicBool,
    native_confirmed: AtomicBool,
}

/// A single owner's deferred completion, not a slot reset. The backend must
/// retain exact native-call correlation and retire stale authority before
/// confirming. A timeout, cancellation, healthy connection or matching screen
/// is not completion evidence. Losing this ticket leaves input quarantined.
#[must_use]
pub struct NativeActionRecovery {
    completion: Arc<Completion>,
}

impl NativeActionRecovery {
    pub fn confirm_native_completion(self) {
        self.completion
            .native_confirmed
            .store(true, Ordering::Release);
    }
}

#[must_use = "retain native occupancy until execution and owned input cleanup finish"]
pub struct NativeActionGuard<'a> {
    slot: &'a NativeActionSlot,
    uncertain: bool,
    completion: Arc<Completion>,
}

impl NativeActionSlot {
    pub fn begin(
        &self,
        run_id: &str,
        generation: u64,
        cancellation: &ActionCancellation,
    ) -> Result<NativeActionGuard<'_>, String> {
        let mut active = self.active.lock();
        cancellation.check()?;
        reap(&mut active);
        if active.is_some() {
            return Err("native input is still occupied by an earlier operation".into());
        }
        let completion = Arc::new(Completion::default());
        *active = Some(Active {
            run_id: run_id.to_owned(),
            generation,
            cancellation: cancellation.clone(),
            completion: completion.clone(),
        });
        Ok(NativeActionGuard {
            slot: self,
            uncertain: false,
            completion,
        })
    }

    /// Broker Stop/Pause increments the run generation before invoking abort.
    /// A delayed abort at that fence must not cancel a resumed generation.
    /// This never releases occupancy or synthesizes unowned input releases.
    pub fn cancel_before(&self, run_id: &str, fence_generation: u64) {
        let active = self.active.lock();
        if let Some(owner) = active.as_ref() {
            if owner.run_id == run_id && owner.generation < fence_generation {
                owner.cancellation.cancel();
            }
        }
    }

    /// Native input is shared, so another run's active input is not idle either.
    pub fn is_idle(&self) -> bool {
        let mut active = self.active.lock();
        reap(&mut active);
        active.is_none()
    }
}

impl NativeActionGuard<'_> {
    /// Use when the native transport fails after dispatch and cannot prove that
    /// input has quiesced. Ordinary Stop cannot turn this into idle. Recovery
    /// needs a backend-specific proof, not a timeout or an unconditional reset.
    pub fn retain_until_native_recovery(&mut self) {
        self.uncertain = true;
    }

    /// No more native dispatch may follow this handoff. Even an early native
    /// completion cannot release the slot until this guard returns normally.
    pub fn defer_until_native_completion(&mut self) -> NativeActionRecovery {
        self.uncertain = true;
        NativeActionRecovery {
            completion: self.completion.clone(),
        }
    }
}

fn reap(active: &mut Option<Active>) {
    if active.as_ref().is_some_and(|owner| {
        owner.completion.owner_returned.load(Ordering::Acquire)
            && owner.completion.native_confirmed.load(Ordering::Acquire)
    }) {
        active.take();
    }
}

impl Drop for NativeActionGuard<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        if self.uncertain {
            self.completion
                .owner_returned
                .store(true, Ordering::Release);
        } else {
            self.slot.active.lock().take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    #[test]
    fn same_run_and_generation_do_not_transfer_proof_between_native_slots() {
        let first = NativeActionSlot::default();
        let second = NativeActionSlot::default();
        let token = ActionCancellation::default();
        let mut one = first.begin("same", 1, &token).unwrap();
        let mut two = second.begin("same", 1, &token).unwrap();
        let proof = one.defer_until_native_completion();
        two.retain_until_native_recovery();
        drop(one);
        drop(two);
        proof.confirm_native_completion();
        assert!(first.is_idle());
        assert!(!second.is_idle());
    }

    #[test]
    fn early_native_completion_waits_for_owner_return() {
        let slot = NativeActionSlot::default();
        let mut owner = slot.begin("run", 1, &Default::default()).unwrap();
        owner
            .defer_until_native_completion()
            .confirm_native_completion();
        assert!(!slot.is_idle());
        assert!(slot.begin("other", 1, &Default::default()).is_err());
        drop(owner);
        assert!(slot.is_idle());
    }

    #[test]
    fn late_completion_is_owner_bound_and_stop_does_not_confirm_it() {
        let slot = NativeActionSlot::default();
        let mut owner = slot.begin("run", 1, &Default::default()).unwrap();
        let proof = owner.defer_until_native_completion();
        let obsolete = owner.defer_until_native_completion();
        drop(owner);
        slot.cancel_before("run", 2);
        assert!(!slot.is_idle());
        proof.confirm_native_completion();
        let next = slot.begin("run", 2, &Default::default()).unwrap();
        obsolete.confirm_native_completion();
        assert!(!slot.is_idle());
        drop(next);
        assert!(slot.is_idle());
    }

    #[test]
    fn lost_ticket_or_unwound_owner_never_recovers() {
        let slot = NativeActionSlot::default();
        let mut owner = slot.begin("run", 1, &Default::default()).unwrap();
        drop(owner.defer_until_native_completion());
        drop(owner);
        assert!(!slot.is_idle());
        let other = NativeActionSlot::default();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut owner = other.begin("run", 1, &Default::default()).unwrap();
            owner
                .defer_until_native_completion()
                .confirm_native_completion();
            panic!("native cleanup after handoff panicked");
        }));
        assert!(result.is_err());
        assert!(!other.is_idle());
    }

    #[test]
    fn stop_revokes_input_without_releasing_native_occupancy() {
        let slot = NativeActionSlot::default();
        let token = ActionCancellation::default();
        let owner = slot.begin("run", 4, &token).unwrap();
        slot.cancel_before("run", 5);
        assert!(token.check().is_err());
        assert!(!slot.is_idle());
        assert!(slot.begin("next", 1, &Default::default()).is_err());
        drop(owner);
        assert!(slot.is_idle());
        assert!(slot.begin("next", 1, &Default::default()).is_ok());
    }

    #[test]
    fn late_stop_cannot_cancel_a_resumed_generation_or_another_run() {
        let slot = NativeActionSlot::default();
        let token = ActionCancellation::default();
        let _owner = slot.begin("new", 8, &token).unwrap();
        slot.cancel_before("old", u64::MAX);
        slot.cancel_before("new", 7);
        slot.cancel_before("new", 8);
        assert!(token.check().is_ok());
        assert!(!slot.is_idle());
    }

    #[test]
    fn cancelled_admission_does_not_create_an_owner() {
        let slot = NativeActionSlot::default();
        let token = ActionCancellation::default();
        token.cancel();
        assert!(slot.begin("run", 1, &token).is_err());
        assert!(slot.is_idle());
    }

    #[test]
    fn uncertain_native_transport_cannot_be_cleared_by_stop_or_guard_drop() {
        let slot = NativeActionSlot::default();
        let token = ActionCancellation::default();
        let mut owner = slot.begin("run", 1, &token).unwrap();
        owner.retain_until_native_recovery();
        drop(owner);
        slot.cancel_before("run", 2);
        assert!(token.check().is_err());
        assert!(!slot.is_idle());
        assert!(slot.begin("new", 1, &Default::default()).is_err());
    }

    #[test]
    fn unwinding_does_not_assert_native_quiescence() {
        let slot = NativeActionSlot::default();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _owner = slot.begin("run", 1, &Default::default()).unwrap();
            panic!("native completion was not observed");
        }));
        assert!(result.is_err());
        assert!(!slot.is_idle());
    }

    #[test]
    fn abort_does_not_wait_for_or_fake_native_completion() {
        let slot = Arc::new(NativeActionSlot::default());
        let token = ActionCancellation::default();
        let (started_tx, started_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel();
        let worker_slot = slot.clone();
        let worker_token = token.clone();
        let worker = std::thread::spawn(move || {
            let _owner = worker_slot.begin("run", 1, &worker_token).unwrap();
            started_tx.send(()).unwrap();
            finish_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            assert!(worker_token.check().is_err());
            // Simulates the native callback/queue barrier returning, not Stop.
        });
        started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        slot.cancel_before("run", 2);
        assert!(!slot.is_idle());
        assert!(token.check().is_err());
        finish_tx.send(()).unwrap();
        worker.join().unwrap();
        assert!(slot.is_idle());
    }
}
