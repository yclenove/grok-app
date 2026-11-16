//! Ledger that records a reservation before any clone permit exists.
//!
//! The fake keeps rows in memory. That is not a durability proof. Dropping a
//! permit does not clone, delete, or seal. `rows` returns the exact owned
//! records. It is not a VM inventory and it has no prefix lookup.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::time::Duration;

use super::deadline::{
    charge_quantum, AttemptDeadline, FakeClock, HostClock, MonoInstant, QueryDeadline, SealDeadline,
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
    /// Incarnation and config measured from one clone body.
    #[must_use]
    pub fn from_parts(incarnation: String, config: String) -> Self {
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

/// One ledger begin/poll handle. Copying it does not copy a permit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerHandle {
    id: u64,
}

impl LedgerHandle {
    /// Id of this ledger operation.
    #[must_use]
    pub const fn id(self) -> u64 {
        self.id
    }
}

/// A ledger poll that has not failed.
#[derive(Debug)]
pub enum LedgerPoll {
    /// The scripted cost is not paid. The row is unchanged.
    Pending,
    /// The only permit for a row that was just written.
    Permit(ClonePermit),
    /// One exact row.
    Read(Reservation),
    /// Every owned row, in insertion order.
    Rows(Vec<Reservation>),
    /// A write finished.
    Wrote,
}

/// Persistent reservation API.
///
/// Begin returns a handle. Poll observes that handle. A write is stored only
/// when its cost is fully paid. There is no blocking default and no prefix lookup.
pub trait AttemptLedger {
    /// Begin `reserve`. An already-due call expires and writes nothing.
    fn begin_reserve(
        &mut self,
        run: RunId,
        clone: CloneId,
        absolute: AttemptDeadline,
        call: QueryDeadline,
    ) -> Result<LedgerHandle, LedgerError>;

    /// Begin a read of one exact clone id.
    fn begin_query(
        &mut self,
        clone: &CloneId,
        call: QueryDeadline,
    ) -> Result<LedgerHandle, LedgerError>;

    /// Begin appending one fixed fact.
    fn begin_append(
        &mut self,
        clone: &CloneId,
        fact: LedgerFact,
        call: QueryDeadline,
    ) -> Result<LedgerHandle, LedgerError>;

    /// Begin storing the clone body's incarnation.
    fn begin_note_identity(
        &mut self,
        clone: &CloneId,
        identity: &VmIdentity,
        call: QueryDeadline,
    ) -> Result<LedgerHandle, LedgerError>;

    /// Begin sealing one exact row.
    fn begin_seal(
        &mut self,
        clone: &CloneId,
        call: SealDeadline,
    ) -> Result<LedgerHandle, LedgerError>;

    /// Begin reading every owned row.
    fn begin_rows(&mut self, call: QueryDeadline) -> Result<LedgerHandle, LedgerError>;

    /// Poll `handle`. A second poll of a permit fails; the permit moved once.
    fn poll_ledger(&mut self, handle: &LedgerHandle) -> Result<LedgerPoll, LedgerError>;
}

#[derive(Debug)]
enum Job {
    Reserve {
        run: RunId,
        clone: CloneId,
        absolute: MonoInstant,
    },
    Query {
        clone: CloneId,
    },
    Append {
        clone: CloneId,
        fact: LedgerFact,
    },
    Note {
        clone: CloneId,
        identity: VmIdentity,
    },
    Seal {
        clone: CloneId,
    },
    Rows,
}

impl Job {
    fn op(&self) -> LedgerOp {
        match self {
            Self::Reserve { .. } => LedgerOp::Reserve,
            Self::Query { .. } => LedgerOp::Query,
            Self::Append { fact, .. } => LedgerOp::Append(*fact),
            Self::Note { .. } => LedgerOp::NoteIdentity,
            Self::Seal { .. } => LedgerOp::Seal,
            Self::Rows => LedgerOp::Rows,
        }
    }
}

#[derive(Debug)]
enum Ready {
    Permit(Option<ClonePermit>),
    Read(Reservation),
    Rows(Vec<Reservation>),
    Wrote,
}

#[derive(Debug)]
struct Flight {
    id: u64,
    deadline: MonoInstant,
    remaining: Duration,
    job: Option<Job>,
    ready: Option<Result<Ready, LedgerError>>,
}

/// Memory ledger. Not durable, and not evidence that a VM was reserved.
#[derive(Debug)]
pub struct FakeLedger {
    clock: FakeClock,
    rows: RefCell<Vec<Reservation>>,
    faults: RefCell<VecDeque<LedgerFault>>,
    /// Consumed by `seal` before the shared fault queue.
    seal_faults: RefCell<VecDeque<LedgerFault>>,
    /// Consumed by `query` only.
    read_faults: RefCell<VecDeque<LedgerFault>>,
    trace: RefCell<Vec<TraceEntry<LedgerOp>>>,
    io_cost: Cell<Duration>,
    inflight: RefCell<Option<Flight>>,
    next_id: Cell<u64>,
}

impl FakeLedger {
    /// Empty ledger that reads `clock`.
    ///
    /// Does not create a file or take a lock.
    #[must_use]
    pub fn new(clock: FakeClock) -> Self {
        Self {
            clock,
            rows: RefCell::new(Vec::new()),
            faults: RefCell::new(VecDeque::new()),
            seal_faults: RefCell::new(VecDeque::new()),
            read_faults: RefCell::new(VecDeque::new()),
            trace: RefCell::new(Vec::new()),
            io_cost: Cell::new(Duration::ZERO),
            inflight: RefCell::new(None),
            next_id: Cell::new(1),
        }
    }

    /// Recorded calls, in order. Deadlines are the instants the fake enforced.
    #[must_use]
    pub fn trace(&self) -> Vec<TraceEntry<LedgerOp>> {
        self.trace.borrow().clone()
    }

    /// Charge `cost` before the next write or read mutates a row.
    ///
    /// Zero keeps the one-shot path. A partial charge does not consume a fault.
    /// Does not sleep.
    pub fn set_io_cost(&mut self, cost: Duration) {
        self.io_cost.set(cost);
    }

    /// Queue a fault for the next `reserve`, `append`, `note_identity`, or `seal`.
    ///
    /// An empty queue is the success path. A due deadline does not consume
    /// the fault. Does not block. `query` does not consume this queue.
    pub fn push_fault(&mut self, fault: LedgerFault) {
        self.faults.borrow_mut().push_back(fault);
    }

    /// Queue a fault that the next `seal` consumes before [`Self::push_fault`].
    ///
    /// Earlier writes in the same call keep the shared queue. A due deadline
    /// does not consume it. An already sealed row does not consume it. Does not block.
    pub fn push_seal_fault(&mut self, fault: LedgerFault) {
        self.seal_faults.borrow_mut().push_back(fault);
    }

    /// Queue a fault for the next `query`. The row is not changed.
    ///
    /// `rows` and every write ignore this queue. A due deadline does not
    /// consume it. [`LedgerFault::CommittedLost`] is reported as
    /// [`LedgerError::ReplyLost`] because a query stores nothing. Does not block.
    pub fn push_read_fault(&mut self, fault: LedgerFault) {
        self.read_faults.borrow_mut().push_back(fault);
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
            .borrow()
            .iter()
            .position(|row| &row.clone_id == clone)
            .ok_or(LedgerError::NotFound)
    }

    /// `Ok(true)` means the write must be stored and then reported as lost.
    fn take_fault(&self) -> Result<bool, LedgerError> {
        match self.faults.borrow_mut().pop_front() {
            None => Ok(false),
            Some(fault) => Self::fault_committed(fault),
        }
    }

    fn fault_committed(fault: LedgerFault) -> Result<bool, LedgerError> {
        match fault {
            LedgerFault::CommittedLost => Ok(true),
            LedgerFault::Failed => Err(LedgerError::Failed),
            LedgerFault::TimedOut => Err(LedgerError::TimedOut),
            LedgerFault::Malformed => Err(LedgerError::Malformed),
            LedgerFault::Overlong => Err(LedgerError::Overlong),
            LedgerFault::ReplyLost => Err(LedgerError::ReplyLost),
        }
    }

    fn read_error(fault: LedgerFault) -> LedgerError {
        match Self::fault_committed(fault) {
            Ok(_) | Err(LedgerError::ReplyLost) => LedgerError::ReplyLost,
            Err(err) => err,
        }
    }

    fn finish_write(committed_lost: bool) -> Result<Ready, LedgerError> {
        if committed_lost {
            Err(LedgerError::ReplyLost)
        } else {
            Ok(Ready::Wrote)
        }
    }

    fn begin_job(&self, job: Job, deadline: MonoInstant) -> Result<LedgerHandle, LedgerError> {
        if self.inflight.borrow().is_some() {
            return Err(LedgerError::Failed);
        }
        if self.clock.now() >= deadline {
            self.stamp(job.op(), deadline)?;
            return Err(LedgerError::Expired);
        }
        let id = self.next_id.get();
        self.next_id.set(id.saturating_add(1));
        *self.inflight.borrow_mut() = Some(Flight {
            id,
            deadline,
            remaining: self.io_cost.get(),
            job: Some(job),
            ready: None,
        });
        self.pump();
        Ok(LedgerHandle { id })
    }

    fn pump(&self) {
        let (deadline, remaining, runnable) = {
            let guard = self.inflight.borrow();
            let Some(flight) = guard.as_ref() else {
                return;
            };
            (
                flight.deadline,
                flight.remaining,
                flight.job.is_some() && flight.ready.is_none(),
            )
        };
        if !runnable {
            return;
        }
        let mut remaining = remaining;
        if !remaining.is_zero() {
            let paid = charge_quantum(&self.clock, remaining, deadline);
            remaining = remaining.saturating_sub(paid);
            if let Some(flight) = self.inflight.borrow_mut().as_mut() {
                flight.remaining = remaining;
            }
        }
        if self.clock.now() >= deadline {
            let op = self
                .inflight
                .borrow()
                .as_ref()
                .and_then(|flight| flight.job.as_ref().map(Job::op));
            if let Some(op) = op {
                let _ = self.stamp(op, deadline);
            }
            if let Some(flight) = self.inflight.borrow_mut().as_mut() {
                flight.job = None;
                flight.remaining = Duration::ZERO;
                flight.ready = Some(Err(LedgerError::Expired));
            }
            return;
        }
        if !remaining.is_zero() {
            return;
        }
        let job = self
            .inflight
            .borrow_mut()
            .as_mut()
            .and_then(|flight| flight.job.take());
        let Some(job) = job else {
            return;
        };
        let result = self.execute(job, deadline);
        if let Some(flight) = self.inflight.borrow_mut().as_mut() {
            flight.ready = Some(result);
        }
    }

    fn execute(&self, job: Job, deadline: MonoInstant) -> Result<Ready, LedgerError> {
        self.stamp(job.op(), deadline)?;
        match job {
            Job::Reserve {
                run,
                clone,
                absolute,
            } => self.exec_reserve(run, clone, absolute),
            Job::Query { clone } => self.exec_query(&clone),
            Job::Append { clone, fact } => self.exec_append(&clone, fact),
            Job::Note { clone, identity } => self.exec_note(&clone, &identity),
            Job::Seal { clone } => self.exec_seal(&clone),
            Job::Rows => Ok(Ready::Rows(self.rows.borrow().clone())),
        }
    }

    fn exec_reserve(
        &self,
        run: RunId,
        clone: CloneId,
        absolute: MonoInstant,
    ) -> Result<Ready, LedgerError> {
        if absolute <= self.clock.now() {
            return Err(LedgerError::AttemptAlreadyExpired);
        }
        let committed_lost = self.take_fault()?;
        {
            let rows = self.rows.borrow();
            if rows.iter().any(|row| row.run == run) {
                return Err(LedgerError::DuplicateRun);
            }
            if rows.iter().any(|row| row.clone_id == clone) {
                return Err(LedgerError::DuplicateClone);
            }
        }
        self.rows.borrow_mut().push(Reservation {
            run: run.clone(),
            clone_id: clone.clone(),
            absolute,
            expected: None,
            facts: Vec::new(),
            sealed: false,
        });
        if committed_lost {
            return Err(LedgerError::ReplyLost);
        }
        let permit = Ok(ClonePermit {
            run,
            clone_id: clone,
            absolute,
        })?;
        Ok(Ready::Permit(Some(permit)))
    }

    fn exec_query(&self, clone: &CloneId) -> Result<Ready, LedgerError> {
        if let Some(fault) = self.read_faults.borrow_mut().pop_front() {
            return Err(Self::read_error(fault));
        }
        let index = self.row_index(clone)?;
        Ok(Ready::Read(self.rows.borrow()[index].clone()))
    }

    fn exec_append(&self, clone: &CloneId, fact: LedgerFact) -> Result<Ready, LedgerError> {
        let index = self.row_index(clone)?;
        if self.rows.borrow()[index].sealed {
            return Err(LedgerError::Sealed);
        }
        let committed_lost = self.take_fault()?;
        self.rows.borrow_mut()[index].facts.push(fact);
        Self::finish_write(committed_lost)
    }

    fn exec_note(&self, clone: &CloneId, identity: &VmIdentity) -> Result<Ready, LedgerError> {
        let index = self.row_index(clone)?;
        if self.rows.borrow()[index].sealed {
            return Err(LedgerError::Sealed);
        }
        let committed_lost = self.take_fault()?;
        self.rows.borrow_mut()[index].expected = Some(identity.clone());
        Self::finish_write(committed_lost)
    }

    fn exec_seal(&self, clone: &CloneId) -> Result<Ready, LedgerError> {
        let index = self.row_index(clone)?;
        if self.rows.borrow()[index].sealed {
            return Err(LedgerError::Sealed);
        }
        let committed_lost = match self.seal_faults.borrow_mut().pop_front() {
            Some(fault) => Self::fault_committed(fault)?,
            None => self.take_fault()?,
        };
        self.rows.borrow_mut()[index].sealed = true;
        Self::finish_write(committed_lost)
    }

    fn poll_handle(&self, handle: &LedgerHandle) -> Result<LedgerPoll, LedgerError> {
        let matches = self
            .inflight
            .borrow()
            .as_ref()
            .is_some_and(|flight| flight.id == handle.id);
        if !matches {
            return Err(LedgerError::Failed);
        }
        self.pump();
        let ready = {
            let mut guard = self.inflight.borrow_mut();
            let flight = guard.as_mut().ok_or(LedgerError::Failed)?;
            if flight.ready.is_none() {
                return Ok(LedgerPoll::Pending);
            }
            flight.ready.take()
        };
        self.inflight.borrow_mut().take();
        match ready {
            Some(Ok(Ready::Permit(Some(permit)))) => Ok(LedgerPoll::Permit(permit)),
            Some(Ok(Ready::Read(row))) => Ok(LedgerPoll::Read(row)),
            Some(Ok(Ready::Rows(rows))) => Ok(LedgerPoll::Rows(rows)),
            Some(Ok(Ready::Wrote)) => Ok(LedgerPoll::Wrote),
            Some(Ok(Ready::Permit(None))) | None => Err(LedgerError::Failed),
            Some(Err(err)) => Err(err),
        }
    }

    fn finish_inherent(&self, handle: LedgerHandle) -> Result<LedgerPoll, LedgerError> {
        match self.poll_handle(&handle)? {
            LedgerPoll::Pending => Err(LedgerError::TimedOut),
            other => Ok(other),
        }
    }

    /// Reserve, then poll once. A pending cost is [`LedgerError::TimedOut`].
    pub fn reserve(
        &mut self,
        run: RunId,
        clone: CloneId,
        absolute: AttemptDeadline,
        call: QueryDeadline,
    ) -> Result<ClonePermit, LedgerError> {
        let handle = self.begin_job(
            Job::Reserve {
                run,
                clone,
                absolute: absolute.at(),
            },
            call.at(),
        )?;
        match self.finish_inherent(handle)? {
            LedgerPoll::Permit(permit) => Ok(permit),
            _ => Err(LedgerError::Failed),
        }
    }

    /// Read one exact row, then poll once.
    pub fn query(&self, clone: &CloneId, call: QueryDeadline) -> Result<Reservation, LedgerError> {
        let handle = self.begin_job(
            Job::Query {
                clone: clone.clone(),
            },
            call.at(),
        )?;
        match self.finish_inherent(handle)? {
            LedgerPoll::Read(row) => Ok(row),
            _ => Err(LedgerError::Failed),
        }
    }

    /// Append one fact, then poll once.
    pub fn append(
        &mut self,
        clone: &CloneId,
        fact: LedgerFact,
        call: QueryDeadline,
    ) -> Result<(), LedgerError> {
        let handle = self.begin_job(
            Job::Append {
                clone: clone.clone(),
                fact,
            },
            call.at(),
        )?;
        match self.finish_inherent(handle)? {
            LedgerPoll::Wrote => Ok(()),
            _ => Err(LedgerError::Failed),
        }
    }

    /// Store an incarnation, then poll once.
    pub fn note_identity(
        &mut self,
        clone: &CloneId,
        identity: &VmIdentity,
        call: QueryDeadline,
    ) -> Result<(), LedgerError> {
        let handle = self.begin_job(
            Job::Note {
                clone: clone.clone(),
                identity: identity.clone(),
            },
            call.at(),
        )?;
        match self.finish_inherent(handle)? {
            LedgerPoll::Wrote => Ok(()),
            _ => Err(LedgerError::Failed),
        }
    }

    /// Seal one row, then poll once.
    pub fn seal(&mut self, clone: &CloneId, call: SealDeadline) -> Result<(), LedgerError> {
        let handle = self.begin_job(
            Job::Seal {
                clone: clone.clone(),
            },
            call.at(),
        )?;
        match self.finish_inherent(handle)? {
            LedgerPoll::Wrote => Ok(()),
            _ => Err(LedgerError::Failed),
        }
    }

    /// Read every owned row, then poll once.
    pub fn rows(&self, call: QueryDeadline) -> Result<Vec<Reservation>, LedgerError> {
        let handle = self.begin_job(Job::Rows, call.at())?;
        match self.finish_inherent(handle)? {
            LedgerPoll::Rows(rows) => Ok(rows),
            _ => Err(LedgerError::Failed),
        }
    }
}

impl AttemptLedger for FakeLedger {
    fn begin_reserve(
        &mut self,
        run: RunId,
        clone: CloneId,
        absolute: AttemptDeadline,
        call: QueryDeadline,
    ) -> Result<LedgerHandle, LedgerError> {
        self.begin_job(
            Job::Reserve {
                run,
                clone,
                absolute: absolute.at(),
            },
            call.at(),
        )
    }

    fn begin_query(
        &mut self,
        clone: &CloneId,
        call: QueryDeadline,
    ) -> Result<LedgerHandle, LedgerError> {
        self.begin_job(
            Job::Query {
                clone: clone.clone(),
            },
            call.at(),
        )
    }

    fn begin_append(
        &mut self,
        clone: &CloneId,
        fact: LedgerFact,
        call: QueryDeadline,
    ) -> Result<LedgerHandle, LedgerError> {
        self.begin_job(
            Job::Append {
                clone: clone.clone(),
                fact,
            },
            call.at(),
        )
    }

    fn begin_note_identity(
        &mut self,
        clone: &CloneId,
        identity: &VmIdentity,
        call: QueryDeadline,
    ) -> Result<LedgerHandle, LedgerError> {
        self.begin_job(
            Job::Note {
                clone: clone.clone(),
                identity: identity.clone(),
            },
            call.at(),
        )
    }

    fn begin_seal(
        &mut self,
        clone: &CloneId,
        call: SealDeadline,
    ) -> Result<LedgerHandle, LedgerError> {
        self.begin_job(
            Job::Seal {
                clone: clone.clone(),
            },
            call.at(),
        )
    }

    fn begin_rows(&mut self, call: QueryDeadline) -> Result<LedgerHandle, LedgerError> {
        self.begin_job(Job::Rows, call.at())
    }

    fn poll_ledger(&mut self, handle: &LedgerHandle) -> Result<LedgerPoll, LedgerError> {
        self.poll_handle(handle)
    }
}
