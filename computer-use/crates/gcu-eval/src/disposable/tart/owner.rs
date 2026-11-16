//! Injected owner proofs and Stop events. `poll` does not mint a proof.

use std::collections::VecDeque;
use std::time::Duration;

use super::deadline::{charge_quantum, FakeClock, HostClock, MonoInstant};
use super::identity::RunId;
use super::mailbox::{AsciiId, AttemptBinding, AttemptNonce};
use super::manifest::Digest;

/// How long one accepted proof may live. A later proof cannot extend a dead one.
pub const OWNER_PROOF_LIFE: Duration = Duration::from_millis(1_000);

/// Why a scripted proof could not be queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OwnerBuildError {
    /// The run id is not an attempt-binding id.
    #[error("owner proof run id is not a binding id")]
    Run,
    /// `issued_at + 1000ms` did not fit in the clock.
    #[error("owner proof deadline overflowed")]
    Overflow,
}

/// One queued owner proof. Fields match the adapter binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerProof {
    /// Run, nonce, and VM digest.
    pub binding: AttemptBinding,
    /// Non-zero sequence. A repeat of the same fields does not extend `until`.
    pub seq: u64,
    /// Host instant when this proof was issued.
    pub issued_at: MonoInstant,
    /// Exclusive end. `until <= issued_at + 1000ms`.
    pub until: MonoInstant,
}

/// What one bounded service read returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerRead {
    /// The read's cost is not paid. Nothing was popped.
    Pending,
    /// Stop and proof were read separately. Either side may be absent.
    Ready {
        /// A Stop event was queued.
        stop: bool,
        /// The next proof, when one was queued.
        proof: Option<OwnerProof>,
        /// A priority lease should run before ordinary guest work.
        lease: bool,
    },
}

/// Offline owner control connection.
///
/// Implementations return on this call. They must not mint a proof inside
/// `poll_owner`.
pub trait OwnerSource {
    /// Read at most one Stop event and one proof.
    ///
    /// `deadline` bounds the read. A due deadline pops nothing.
    fn poll_owner(&mut self, now: MonoInstant, deadline: MonoInstant) -> OwnerRead;
}

/// Queued proofs and Stop bits for the fake machine.
///
/// Cloning is not supported. The fake does not generate a proof when polled.
#[derive(Debug)]
pub struct FakeOwner {
    clock: FakeClock,
    proofs: VecDeque<OwnerProof>,
    stops: VecDeque<()>,
    leases: VecDeque<()>,
    read_cost: Duration,
    remaining: Duration,
    reading: bool,
    next_seq: u64,
}

impl FakeOwner {
    /// Empty queues. The next read reports neither Stop nor a proof.
    #[must_use]
    pub fn new(clock: FakeClock) -> Self {
        Self {
            clock,
            proofs: VecDeque::new(),
            stops: VecDeque::new(),
            leases: VecDeque::new(),
            read_cost: Duration::ZERO,
            remaining: Duration::ZERO,
            reading: false,
            next_seq: 1,
        }
    }

    /// Charge `cost` before the next completed read pops an event.
    ///
    /// A cost larger than one step is paid across later polls. Does not sleep.
    pub fn set_read_cost(&mut self, cost: Duration) {
        self.read_cost = cost;
    }

    /// Drop queued proofs. Does not reset the sequence counter.
    pub fn clear_proofs(&mut self) {
        self.proofs.clear();
    }

    /// Queue one proof for `run` issued at `now` and living 1000ms.
    ///
    /// The sequence is the next unused value. The nonce and digest are the
    /// offline pair the machine expects. Does not apply the proof by itself.
    pub fn push_proof(&mut self, run: &str, now: MonoInstant) -> Result<(), OwnerBuildError> {
        let until = now
            .checked_add(OWNER_PROOF_LIFE)
            .map_err(|_| OwnerBuildError::Overflow)?;
        let binding = binding_for(run).ok_or(OwnerBuildError::Run)?;
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.proofs.push_back(OwnerProof {
            binding,
            seq,
            issued_at: now,
            until,
        });
        Ok(())
    }

    /// Replace the proof queue with one proof. The sequence keeps increasing.
    pub fn replace_proof(&mut self, run: &str, now: MonoInstant) -> Result<(), OwnerBuildError> {
        self.clear_proofs();
        self.push_proof(run, now)
    }

    /// Queue one Stop event. It is not a proof and does not hide one.
    pub fn push_stop(&mut self) {
        self.stops.push_back(());
    }

    /// Queue one priority lease. Isolate pays it before inventory.
    pub fn push_priority_lease(&mut self) {
        self.leases.push_back(());
    }

    /// Offline binding the machine compares against queued proofs.
    #[must_use]
    pub fn binding_for_run(run: &RunId) -> Option<AttemptBinding> {
        binding_for(run.as_str())
    }
}

fn binding_for(run: &str) -> Option<AttemptBinding> {
    let run_id = AsciiId::new(run).ok()?;
    let vm_config_digest = Digest::try_from_bytes([1; 32]).ok()?;
    Some(AttemptBinding {
        run_id,
        attempt_nonce: AttemptNonce::from_bytes([2; 16]),
        vm_config_digest,
    })
}

impl OwnerSource for FakeOwner {
    fn poll_owner(&mut self, now: MonoInstant, deadline: MonoInstant) -> OwnerRead {
        if now >= deadline {
            return OwnerRead::Pending;
        }
        if !self.reading {
            self.reading = true;
            self.remaining = self.read_cost;
        }
        if !self.remaining.is_zero() {
            let paid = charge_quantum(&self.clock, self.remaining, deadline);
            self.remaining = self.remaining.saturating_sub(paid);
            if !self.remaining.is_zero() || self.clock.now() >= deadline {
                return OwnerRead::Pending;
            }
        }
        self.reading = false;
        OwnerRead::Ready {
            stop: self.stops.pop_front().is_some(),
            proof: self.proofs.pop_front(),
            lease: self.leases.pop_front().is_some(),
        }
    }
}
