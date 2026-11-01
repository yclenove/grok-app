//! Ledger that records a reservation before any clone permit exists.
//!
//! The fake keeps rows in memory. That is not a durability proof. Dropping a
//! permit does not clone, delete, or seal. `rows` returns the exact owned
//! records. It is not a VM inventory and it has no prefix lookup.

use std::cell::RefCell;
use std::collections::VecDeque;

use super::deadline::{
    AttemptDeadline, FakeClock, HostClock, MonoInstant, QueryDeadline, SealDeadline,
};
use super::identity::{CloneId, RunId};
use super::output::{ScriptOutcome, TraceEntry};

/// Closed notes a caller may append. There is no free-text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerFact {
    /// Clone was issued for this exact reservation.
    CloneIssued,
    /// Start was issued for this reservation. Not a process-exit proof.
    StartIssued,
    /// A recovery pass saw this exact reservation. This is not a deletion and not a score.
    RecoveryNoted,
    /// Owner death left cleanup unconfirmed. Not a deletion and not a score.
    CleanupUnknown,
    /// This row forbids another attempt. Not a task score.
    Quarantined,
}

/// One ledger call recorded with the deadline the fake enforced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerOp {
    /// `reserve`.
    Reserve,
    /// `rows`.
    Rows,
    /// `query`.
    Query,
    /// `append` of this fact.
    Append(LedgerFact),
    /// `note_identity`.
    NoteIdentity,
    /// `seal`.
    Seal,
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
    /// The write was stored, then the acknowledgement was lost.
    ///
    /// Unlike [`Self::ReplyLost`], the row does change. Callers still see
    /// [`LedgerError::ReplyLost`] and must read the row to see the write.
    CommittedLost,
}

/// Why a ledger call failed. A [`LedgerFault::CommittedLost`] write is the exception.
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
    /// The acknowledgement did not come back.
    ///
    /// [`LedgerFault::ReplyLost`] leaves the row unchanged.
    /// [`LedgerFault::CommittedLost`] stores the write and then returns this error.
    #[error("ledger reply was lost")]
    ReplyLost,
}

/// Fake VM incarnation and configuration measured from a clone body.
///
/// This is not a host identity and not a boot id. Destruction matches it
/// against a later authoritative status. A guest reboot is a separate fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmIdentity {
    incarnation: String,
    config: String,
}

impl VmIdentity {
    pub(crate) fn from_parts(incarnation: String, config: String) -> Self {
        Self {
            incarnation,
            config,
        }
    }

    /// Incarnation token from the clone body.
    #[must_use]
    pub fn incarnation(&self) -> &str {
        &self.incarnation
    }

    /// Configuration token from the clone body.
    #[must_use]
    pub fn config(&self) -> &str {
        &self.config
    }
}

/// One reservation. The fake stores this value. Fields are a snapshot, not a live capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reservation {
    run: RunId,
    clone_id: CloneId,
    absolute: MonoInstant,
    /// Set from a clone body. `None` does not own a VM that happens to share the name.
    expected: Option<VmIdentity>,
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

    /// Incarnation measured from the clone body, once that body was stored.
    #[must_use]
    pub fn expected(&self) -> Option<&VmIdentity> {
        self.expected.as_ref()
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
    /// On error no permit is returned. [`LedgerFault::ReplyLost`] also writes
    /// nothing. [`LedgerFault::CommittedLost`] stores the row and then returns
    /// [`LedgerError::ReplyLost`].
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

    /// Store the clone body's incarnation on the exact row.
    ///
    /// Does not block. A missing value means this reservation must not be
    /// treated as the owner of a VM that merely shares the clone name.
    fn note_identity(
        &mut self,
        clone: &CloneId,
        identity: &VmIdentity,
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
    /// `query` and `rows` take `&self`, so the trace sits behind a cell.
    trace: RefCell<Vec<TraceEntry<LedgerOp>>>,
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
            trace: RefCell::new(Vec::new()),
        }
    }

    /// Recorded calls, in order. Deadlines are the instants the fake enforced.
    #[must_use]
    pub fn trace(&self) -> Vec<TraceEntry<LedgerOp>> {
        self.trace.borrow().clone()
    }

    /// Queue a fault for the next `reserve`, `append`, `note_identity`, or `seal`.
    ///
    /// An empty queue is the success path. A due deadline does not consume
    /// the fault. Does not block.
    pub fn push_fault(&mut self, fault: LedgerFault) {
        self.faults.push_back(fault);
    }

    fn stamp(&self, op: LedgerOp, at: MonoInstant) -> Result<(), LedgerError> {
        let expired = self.clock.now() >= at;
        self.trace.borrow_mut().push(TraceEntry {
            command: op,
            deadline: at,
            outcome: if expired {
                ScriptOutcome::Expired
            } else {
                ScriptOutcome::Success
            },
        });
        if expired {
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

    /// `Ok(true)` means the write must be stored and then reported as lost.
    fn take_fault(&mut self) -> Result<bool, LedgerError> {
        match self.faults.pop_front() {
            None => Ok(false),
            Some(LedgerFault::CommittedLost) => Ok(true),
            Some(LedgerFault::Failed) => Err(LedgerError::Failed),
            Some(LedgerFault::TimedOut) => Err(LedgerError::TimedOut),
            Some(LedgerFault::Malformed) => Err(LedgerError::Malformed),
            Some(LedgerFault::Overlong) => Err(LedgerError::Overlong),
            Some(LedgerFault::ReplyLost) => Err(LedgerError::ReplyLost),
        }
    }

    fn finish_write(committed_lost: bool) -> Result<(), LedgerError> {
        if committed_lost {
            Err(LedgerError::ReplyLost)
        } else {
            Ok(())
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
        self.stamp(LedgerOp::Reserve, call.at())?;
        if absolute.at() <= self.clock.now() {
            return Err(LedgerError::AttemptAlreadyExpired);
        }
        let committed_lost = self.take_fault()?;
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
            expected: None,
            facts: Vec::new(),
            sealed: false,
        });
        if committed_lost {
            return Err(LedgerError::ReplyLost);
        }
        Ok(ClonePermit {
            run,
            clone_id: clone,
            absolute: stored,
        })
    }

    fn query(&self, clone: &CloneId, call: QueryDeadline) -> Result<Reservation, LedgerError> {
        self.stamp(LedgerOp::Query, call.at())?;
        let index = self.row_index(clone)?;
        Ok(self.rows[index].clone())
    }

    fn append(
        &mut self,
        clone: &CloneId,
        fact: LedgerFact,
        call: QueryDeadline,
    ) -> Result<(), LedgerError> {
        self.stamp(LedgerOp::Append(fact), call.at())?;
        let index = self.row_index(clone)?;
        if self.rows[index].sealed {
            return Err(LedgerError::Sealed);
        }
        let committed_lost = self.take_fault()?;
        self.rows[index].facts.push(fact);
        Self::finish_write(committed_lost)
    }

    fn note_identity(
        &mut self,
        clone: &CloneId,
        identity: &VmIdentity,
        call: QueryDeadline,
    ) -> Result<(), LedgerError> {
        self.stamp(LedgerOp::NoteIdentity, call.at())?;
        let index = self.row_index(clone)?;
        if self.rows[index].sealed {
            return Err(LedgerError::Sealed);
        }
        let committed_lost = self.take_fault()?;
        self.rows[index].expected = Some(identity.clone());
        Self::finish_write(committed_lost)
    }

    fn seal(&mut self, clone: &CloneId, call: SealDeadline) -> Result<(), LedgerError> {
        self.stamp(LedgerOp::Seal, call.at())?;
        let index = self.row_index(clone)?;
        if self.rows[index].sealed {
            return Err(LedgerError::Sealed);
        }
        let committed_lost = self.take_fault()?;
        self.rows[index].sealed = true;
        Self::finish_write(committed_lost)
    }

    fn rows(&self, call: QueryDeadline) -> Result<Vec<Reservation>, LedgerError> {
        self.stamp(LedgerOp::Rows, call.at())?;
        Ok(self.rows.clone())
    }
}
