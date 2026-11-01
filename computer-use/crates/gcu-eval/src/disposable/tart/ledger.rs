//! Ledger that records a reservation before any clone permit exists.
//!
//! The fake keeps rows in memory. That is not a durability proof. Dropping a
//! permit does not clone, delete, or seal. `rows` returns the exact owned
//! records. It is not a VM inventory and it has no prefix lookup.

use std::collections::VecDeque;

use super::deadline::{
    AttemptDeadline, FakeClock, HostClock, MonoInstant, QueryDeadline, SealDeadline,
};
use super::identity::{CloneId, RunId};

/// Closed notes a caller may append. There is no free-text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerFact {
    /// Clone was issued for this exact reservation.
    CloneIssued,
    /// A recovery pass saw this exact reservation. This is not a deletion and not a score.
    RecoveryNoted,
    /// Owner death left cleanup unconfirmed. Not a deletion and not a score.
    CleanupUnknown,
    /// This row forbids another attempt. Not a task score.
    Quarantined,
}

/// Scripted ledger fault. An empty queue keeps the success path.
///
/// An already-due deadline does not consume a queued fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerFault {
    /// Command failed. The row is unchanged.
    Failed,
    /// Timed out. The fake does not sleep.
    TimedOut,
    /// The reply cannot be used. The row is unchanged.
    Malformed,
    /// The reply was over the cap. Nothing is stored.
    Overlong,
    /// The call was issued and the reply did not arrive. Nothing is written.
    ReplyLost,
}

/// Why a ledger call failed. The row is unchanged on failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LedgerError {
    /// The call deadline was already due. Nothing was written.
    #[error("ledger call deadline already expired")]
    Expired,
    /// The absolute attempt deadline was already due. Nothing was written.
    #[error("attempt deadline already expired")]
    AttemptAlreadyExpired,
    /// That run id is already reserved. Match is exact.
    #[error("run id is already reserved")]
    DuplicateRun,
    /// That clone id is already reserved. Match is exact.
    #[error("clone id is already reserved")]
    DuplicateClone,
    /// No row has this exact clone id.
    #[error("clone id is not reserved")]
    NotFound,
    /// The row is sealed. Append and a second seal are refused.
    #[error("reservation is sealed")]
    Sealed,
    /// Scripted failure. The row is unchanged.
    #[error("ledger command failed")]
    Failed,
    /// Scripted timeout. The fake did not sleep.
    #[error("ledger command timed out")]
    TimedOut,
    /// Scripted unusable reply. The row is unchanged.
    #[error("ledger reply was malformed")]
    Malformed,
    /// Scripted overlong reply. Nothing was stored.
    #[error("ledger reply exceeded the cap")]
    Overlong,
    /// The reply was lost. Nothing was written.
    #[error("ledger reply was lost")]
    ReplyLost,
}

/// One reservation. The fake stores this value. Fields are a snapshot, not a live capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reservation {
    run: RunId,
    clone_id: CloneId,
    absolute: MonoInstant,
    facts: Vec<LedgerFact>,
    sealed: bool,
}

impl Reservation {
    /// Exact run id.
    #[must_use]
    pub fn run(&self) -> &RunId {
        &self.run
    }

    /// Exact clone id.
    #[must_use]
    pub fn clone_id(&self) -> &CloneId {
        &self.clone_id
    }

    /// Absolute attempt deadline recorded at reserve time.
    #[must_use]
    pub fn absolute_deadline(&self) -> MonoInstant {
        self.absolute
    }

    /// Facts appended since reserve, in order.
    #[must_use]
    pub fn facts(&self) -> &[LedgerFact] {
        &self.facts
    }

    /// The row has been sealed.
    #[must_use]
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
}

/// Permission to call clone once for the reservation that was just written.
///
/// Not duplicated. There is no public constructor. Dropping it does not clone
/// or delete. It does not admit a process.
#[derive(Debug)]
#[must_use = "dropping a permit does not clone or delete; pass it to clone_vm"]
pub struct ClonePermit {
    run: RunId,
    clone_id: CloneId,
    absolute: MonoInstant,
}

impl ClonePermit {
    /// Run id written before this permit was returned.
    #[must_use]
    pub fn run(&self) -> &RunId {
        &self.run
    }

    /// Clone id written before this permit was returned.
    #[must_use]
    pub fn clone_id(&self) -> &CloneId {
        &self.clone_id
    }

    /// Absolute deadline written before this permit was returned.
    #[must_use]
    pub fn absolute_deadline(&self) -> MonoInstant {
        self.absolute
    }
}

// `Clone` on ClonePermit would let one reservation mint two clones.
// `Copy: Clone`, so this check covers both. The call below is ambiguous
// exactly when the bound holds (static_assertions style, no extra crate).
mod clone_permit_not_duplicated {
    use super::ClonePermit;

    trait NotIfClone<A> {
        fn some_item() {}
    }
    impl<T: ?Sized> NotIfClone<()> for T {}
    struct CloneBound;
    impl<T: Clone> NotIfClone<CloneBound> for T {}

    fn assert_not_clone() {
        let _ = <ClonePermit as NotIfClone<_>>::some_item;
    }

    const _: fn() = assert_not_clone;
}

/// Persistent reservation API.
///
/// `reserve` writes the row and only then returns a [`ClonePermit`]. Query,
/// append, and seal address one exact clone id. There is no prefix lookup.
/// Implementations must return by the call deadline and must not block past it.
/// The fake returns immediately.
pub trait AttemptLedger {
    /// Write `run`, `clone`, and `absolute`, then return the only permit for that row.
    ///
    /// On error nothing is written and no permit is returned.
    fn reserve(
        &mut self,
        run: RunId,
        clone: CloneId,
        absolute: AttemptDeadline,
        call: QueryDeadline,
    ) -> Result<ClonePermit, LedgerError>;

    /// Read the row with this exact clone id.
    fn query(&self, clone: &CloneId, call: QueryDeadline) -> Result<Reservation, LedgerError>;

    /// Append one fixed fact to the exact row. Sealed rows are unchanged.
    fn append(
        &mut self,
        clone: &CloneId,
        fact: LedgerFact,
        call: QueryDeadline,
    ) -> Result<(), LedgerError>;

    /// Seal the exact row. A second seal fails and leaves the row sealed.
    fn seal(&mut self, clone: &CloneId, call: SealDeadline) -> Result<(), LedgerError>;

    /// Every owned row, in insertion order.
    ///
    /// The ids are complete. This is not a prefix search and not a VM list.
    /// An expired call returns [`LedgerError::Expired`] and no rows.
    fn rows(&self, call: QueryDeadline) -> Result<Vec<Reservation>, LedgerError>;
}

/// Memory ledger. Not durable, and not evidence that a VM was reserved.
#[derive(Debug)]
pub struct FakeLedger {
    clock: FakeClock,
    rows: Vec<Reservation>,
    faults: VecDeque<LedgerFault>,
}

impl FakeLedger {
    /// Empty ledger that reads `clock`.
    ///
    /// Does not create a file or take a lock.
    #[must_use]
    pub fn new(clock: FakeClock) -> Self {
        Self {
            clock,
            rows: Vec::new(),
            faults: VecDeque::new(),
        }
    }

    /// Queue a fault for the next `reserve`, `append`, or `seal`.
    ///
    /// An empty queue is the success path. A due deadline does not consume
    /// the fault. Does not block.
    pub fn push_fault(&mut self, fault: LedgerFault) {
        self.faults.push_back(fault);
    }

    fn ensure_call(&self, at: MonoInstant) -> Result<(), LedgerError> {
        if self.clock.now() >= at {
            Err(LedgerError::Expired)
        } else {
            Ok(())
        }
    }

    fn row_index(&self, clone: &CloneId) -> Result<usize, LedgerError> {
        self.rows
            .iter()
            .position(|row| &row.clone_id == clone)
            .ok_or(LedgerError::NotFound)
    }

    fn take_fault(&mut self) -> Result<(), LedgerError> {
        match self.faults.pop_front() {
            None => Ok(()),
            Some(LedgerFault::Failed) => Err(LedgerError::Failed),
            Some(LedgerFault::TimedOut) => Err(LedgerError::TimedOut),
            Some(LedgerFault::Malformed) => Err(LedgerError::Malformed),
            Some(LedgerFault::Overlong) => Err(LedgerError::Overlong),
            Some(LedgerFault::ReplyLost) => Err(LedgerError::ReplyLost),
        }
    }
}

impl AttemptLedger for FakeLedger {
    fn reserve(
        &mut self,
        run: RunId,
        clone: CloneId,
        absolute: AttemptDeadline,
        call: QueryDeadline,
    ) -> Result<ClonePermit, LedgerError> {
        self.ensure_call(call.at())?;
        if absolute.at() <= self.clock.now() {
            return Err(LedgerError::AttemptAlreadyExpired);
        }
        self.take_fault()?;
        if self.rows.iter().any(|row| row.run == run) {
            return Err(LedgerError::DuplicateRun);
        }
        if self.rows.iter().any(|row| row.clone_id == clone) {
            return Err(LedgerError::DuplicateClone);
        }
        let stored = absolute.at();
        self.rows.push(Reservation {
            run: run.clone(),
            clone_id: clone.clone(),
            absolute: stored,
            facts: Vec::new(),
            sealed: false,
        });
        Ok(ClonePermit {
            run,
            clone_id: clone,
            absolute: stored,
        })
    }

    fn query(&self, clone: &CloneId, call: QueryDeadline) -> Result<Reservation, LedgerError> {
        self.ensure_call(call.at())?;
        let index = self.row_index(clone)?;
        Ok(self.rows[index].clone())
    }

    fn append(
        &mut self,
        clone: &CloneId,
        fact: LedgerFact,
        call: QueryDeadline,
    ) -> Result<(), LedgerError> {
        self.ensure_call(call.at())?;
        let index = self.row_index(clone)?;
        if self.rows[index].sealed {
            return Err(LedgerError::Sealed);
        }
        self.take_fault()?;
        self.rows[index].facts.push(fact);
        Ok(())
    }

    fn seal(&mut self, clone: &CloneId, call: SealDeadline) -> Result<(), LedgerError> {
        self.ensure_call(call.at())?;
        let index = self.row_index(clone)?;
        if self.rows[index].sealed {
            return Err(LedgerError::Sealed);
        }
        self.take_fault()?;
        self.rows[index].sealed = true;
        Ok(())
    }

    fn rows(&self, call: QueryDeadline) -> Result<Vec<Reservation>, LedgerError> {
        self.ensure_call(call.at())?;
        Ok(self.rows.clone())
    }
}
