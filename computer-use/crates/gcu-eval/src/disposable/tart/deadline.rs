//! Monotonic deadlines for one Tart call.
//!
//! No value in this module means "wait forever". A deadline is a typed cap
//! plus a caller-chosen limit. Reading a clock does not block or sleep.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// Instant on an injected monotonic clock. This is not wall time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonoInstant {
    nanos: u64,
}

impl MonoInstant {
    /// The clock's zero. This is not a deadline.
    pub const EPOCH: Self = Self { nanos: 0 };

    /// Build from a nanosecond count that the clock itself produced.
    #[must_use]
    pub const fn from_nanos(nanos: u64) -> Self {
        Self { nanos }
    }

    /// Nanoseconds since [`Self::EPOCH`].
    #[must_use]
    pub const fn nanos(self) -> u64 {
        self.nanos
    }

    pub(super) fn checked_add(self, limit: Duration) -> Result<Self, DeadlineError> {
        let nanos = u64::try_from(limit.as_nanos()).map_err(|_| DeadlineError::Overflow)?;
        match self.nanos.checked_add(nanos) {
            Some(sum) => Ok(Self { nanos: sum }),
            None => Err(DeadlineError::Overflow),
        }
    }
}

/// Why a deadline could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DeadlineError {
    /// A zero limit does not bound the call.
    #[error("deadline limit is zero")]
    Zero,
    /// The limit is above this call's fixed cap.
    #[error("deadline limit is above the call cap")]
    AboveCap,
    /// The instant does not fit in the monotonic counter.
    #[error("deadline overflowed the monotonic clock")]
    Overflow,
}

pub(super) fn bounded_deadline(
    now: MonoInstant,
    limit: Duration,
    cap: Duration,
) -> Result<MonoInstant, DeadlineError> {
    if limit.is_zero() {
        return Err(DeadlineError::Zero);
    }
    if limit > cap {
        return Err(DeadlineError::AboveCap);
    }
    now.checked_add(limit)
}

macro_rules! deadline_type {
    ($name:ident, $secs:literal, $doc:literal) => {
        #[doc = $doc]
        #[must_use = "pass this deadline to one call; it is not a default"]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name {
            at: MonoInstant,
        }

        impl $name {
            /// Fixed cap for this call. Not a deadline and not a default limit.
            pub const CAP: Duration = Duration::from_secs($secs);

            /// Deadline `limit` after `now`.
            ///
            /// Does not block. `limit` must be greater than zero and at most
            /// [`Self::CAP`]. There is no default and no infinite value.
            pub fn after(now: MonoInstant, limit: Duration) -> Result<Self, DeadlineError> {
                Ok(Self {
                    at: bounded_deadline(now, limit, Self::CAP)?,
                })
            }

            /// Instant by which the call must have returned.
            #[must_use]
            pub const fn at(self) -> MonoInstant {
                self.at
            }
        }
    };
}

deadline_type!(
    QueryDeadline,
    5,
    "Bound for one list, status, inventory, or ledger read/write. Cap is 5 seconds (ADR 0018 §2). Zero and any longer limit are rejected."
);
deadline_type!(
    CloneDeadline,
    60,
    "Bound for one clone. Cap is 60 seconds (ADR 0018 §2). Zero and any longer limit are rejected."
);
deadline_type!(
    StartDeadline,
    120,
    "Bound for one start, including boot ready. Cap is 120 seconds (ADR 0018 §2). Zero and any longer limit are rejected."
);
deadline_type!(
    StopDeadline,
    20,
    "Bound for one stop plus confirmation. Cap is 20 seconds (ADR 0018 §2). Zero and any longer limit are rejected."
);
deadline_type!(
    DeleteDeadline,
    20,
    "Bound for one delete plus confirmation. Cap is 20 seconds (ADR 0018 §2). Zero and any longer limit are rejected."
);
deadline_type!(
    ProbeDeadline,
    2,
    "Bound for one guest probe. Cap is 2 seconds (ADR 0018 §2). Zero and any longer limit are rejected."
);
deadline_type!(
    StageDeadline,
    5,
    "Bound for one bootstrap or ACK stage. Cap is 5 seconds (ADR 0018 §2). Zero and any longer limit are rejected."
);
deadline_type!(
    LoginDeadline,
    300,
    "Bound for one fixed login call. Cap is 5 minutes (ADR 0018 §2). Zero and any longer limit are rejected. The call takes no secret."
);
deadline_type!(
    RunDeadline,
    900,
    "Bound for one guest run call. Cap is 15 minutes (ADR 0018 §2). Zero and any longer limit are rejected."
);
deadline_type!(
    LeaseDeadline,
    3,
    "Bound for one guest lease. Cap is 3 seconds (ADR 0018 §2). Zero and any longer limit are rejected."
);
deadline_type!(
    RevokeDeadline,
    10,
    "Bound for one revoke or guest drain. Cap is 10 seconds (ADR 0018 §2). Zero and any longer limit are rejected."
);
deadline_type!(
    SealDeadline,
    10,
    "Bound for sealing a ledger record. Cap is 10 seconds (ADR 0018 §2). Zero and any longer limit are rejected."
);
deadline_type!(
    AttemptDeadline,
    1560,
    "Absolute attempt deadline stored before clone. Cap is 10 minutes of prep plus 15 minutes of task plus 60 seconds of cleanup (ADR 0018 §2). Zero and any longer limit are rejected. Retries cannot extend it."
);

/// Monotonic clock a Tart call reads. Implementations must not block or sleep.
pub trait HostClock {
    /// Current monotonic instant.
    ///
    /// Successive calls on the same clock are non-decreasing. This does not
    /// read the wall clock and it does not wait.
    fn now(&self) -> MonoInstant;
}

/// In-memory monotonic clock. Cloning shares the same instant.
///
/// `advance` jumps the counter. It does not sleep. The type is single-threaded
/// and takes no lock.
#[derive(Debug, Clone)]
pub struct FakeClock {
    nanos: Rc<Cell<u64>>,
}

impl FakeClock {
    /// Clock at [`MonoInstant::EPOCH`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            nanos: Rc::new(Cell::new(0)),
        }
    }

    /// Move the clock forward by `step`.
    ///
    /// Does not sleep. Zero and a step that does not fit in the counter are
    /// errors. There is no rewind.
    pub fn advance(&self, step: Duration) -> Result<(), ClockError> {
        if step.is_zero() {
            return Err(ClockError::ZeroStep);
        }
        let add = u64::try_from(step.as_nanos()).map_err(|_| ClockError::Overflow)?;
        let next = self
            .nanos
            .get()
            .checked_add(add)
            .ok_or(ClockError::Overflow)?;
        self.nanos.set(next);
        Ok(())
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl HostClock for FakeClock {
    fn now(&self) -> MonoInstant {
        MonoInstant::from_nanos(self.nanos.get())
    }
}

/// Why a fake clock refused to advance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ClockError {
    /// A zero step is not a forward move.
    #[error("clock step is zero")]
    ZeroStep,
    /// The step does not fit in the monotonic counter.
    #[error("clock step overflowed")]
    Overflow,
}
