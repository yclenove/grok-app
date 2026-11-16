//! Startup recovery for unsealed ledger rows.
//!
//! The caller drives one stage per poll. It does not clone, start, or restart
//! a VM. It does not use the dead row's absolute deadline. The old attempt
//! stays `not_run` with no score.

use super::super::deadline::HostClock;
use super::super::guest::GuestChannel;
use super::super::ledger::AttemptLedger;
use super::super::lock::CleanupLock;
use super::super::owner::OwnerSource;
use super::super::runner::TartRunner;
use super::drive::DriveKind;
use super::{Attempt, CleanupDeadlines, CompletedStep, StepError, StepProgress};

impl<'a, R, G, L, K, O, C> Attempt<'a, R, G, L, K, O, C>
where
    R: TartRunner,
    G: GuestChannel,
    L: AttemptLedger,
    K: CleanupLock,
    O: OwnerSource,
    C: HostClock,
{
    /// Reclaim every unsealed row, one stage per call.
    ///
    /// Does not clone or start. Appends `RecoveryNoted` before stop or delete.
    /// An append failure deletes nothing. A row with `CleanupUnknown` is
    /// `OwnerDeath`; any other unsealed row is `HostCrash`. A row whose start
    /// was issued is not treated as cleaned up just because a list omits the
    /// name. The entry returns [`StepProgress::Pending`] when the receipts are
    /// held. [`Attempt::poll`] delivers them once. Does not block.
    pub fn recover(
        &mut self,
        deadlines: &CleanupDeadlines,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        self.open_and_tick(DriveKind::Recover(*deadlines), deadlines.query.at(), true)
    }
}
