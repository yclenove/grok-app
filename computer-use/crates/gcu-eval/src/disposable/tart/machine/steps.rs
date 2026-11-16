//! Forward steps. Each call opens one 100ms step and advances one cursor stage.

use super::super::deadline::{
    CloneDeadline, GroupDeadline, HostClock, LeaseDeadline, LoginDeadline, MonoInstant,
    QueryDeadline, RunDeadline, StageDeadline, StartDeadline,
};
use super::super::guest::GuestChannel;
use super::super::identity::GoldenImage;
use super::super::ledger::AttemptLedger;
use super::super::lock::CleanupLock;
use super::super::manifest::InstallManifest;
use super::super::owner::OwnerSource;
use super::super::policy::SoftnetFlags;
use super::super::runner::TartRunner;
use super::drive::DriveKind;
use super::{Attempt, CompletedStep, StepError, StepProgress};

/// Inputs for [`Attempt::reserve`]. Digests are compared, not trusted.
pub struct ReserveInput {
    /// Run id written before the permit exists.
    pub run: super::super::identity::RunId,
    /// Clone id written before the permit exists. Not a golden name.
    pub clone_id: super::super::identity::CloneId,
    /// Absolute attempt deadline. Retries cannot extend it.
    pub absolute: super::super::deadline::AttemptDeadline,
    /// Presented install digests.
    pub manifest: InstallManifest,
    /// Approved digests. Equality is necessary and is not an installation proof.
    pub approved: InstallManifest,
    /// Latched Stop refuses the reserve and is not cleared here.
    ///
    /// The machine reads Stop from the owner source. This flag is ignored.
    pub stop_latched: bool,
    /// Retained so existing callers still compile.
    ///
    /// Owner authority comes from the owner source. This flag is ignored.
    pub owner_present: bool,
}

impl<'a, R, G, L, K, O, C> Attempt<'a, R, G, L, K, O, C>
where
    R: TartRunner,
    G: GuestChannel,
    L: AttemptLedger,
    K: CleanupLock,
    O: OwnerSource,
    C: HostClock,
{
    /// Validate Stop and the owner proof, then reserve one stage at a time.
    ///
    /// Does not clone. The prep aggregate starts at this entry. `Pending` is
    /// not a reservation. Does not block.
    pub fn reserve(
        &mut self,
        input: ReserveInput,
        deadline: QueryDeadline,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        if self.origin.is_none() {
            self.origin = Some(self.clock.now());
        }
        self.open_and_tick(DriveKind::Reserve(input), deadline.at(), true)
    }

    /// Clone with the reserved permit. One stage per call. Does not block.
    pub fn clone_vm(
        &mut self,
        golden: GoldenImage,
        deadline: CloneDeadline,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        self.open_and_tick(DriveKind::Clone(golden), deadline.at(), true)
    }

    /// Start the clone. Records start before the runner when that fact lands.
    ///
    /// Does not block.
    pub fn start(
        &mut self,
        flags: &SoftnetFlags,
        deadline: StartDeadline,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        self.open_and_tick(DriveKind::Start(*flags), deadline.at(), true)
    }

    /// Inventory, then one probe per call. Does not block.
    pub fn isolate(
        &mut self,
        deadline: GroupDeadline,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        self.open_and_tick(DriveKind::Isolate, deadline.at(), true)
    }

    /// Record a fake bootstrap ACK. Does not block.
    pub fn bootstrap(
        &mut self,
        deadline: StageDeadline,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        self.open_and_tick(DriveKind::Bootstrap, deadline.at(), true)
    }

    /// Record fake login evidence. Does not block.
    pub fn login(
        &mut self,
        deadline: LoginDeadline,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        self.open_and_tick(DriveKind::Login, deadline.at(), true)
    }

    /// Re-read the guest identity. Does not block.
    pub fn ready(
        &mut self,
        deadline: QueryDeadline,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        self.open_and_tick(DriveKind::Ready, deadline.at(), true)
    }

    /// Issue the fake run once. Does not block.
    pub fn run(&mut self, deadline: RunDeadline) -> Result<StepProgress<CompletedStep>, StepError> {
        self.open_and_tick(DriveKind::Run, deadline.at(), true)
    }

    /// Send one owner lease. A late call does not extend it. Does not block.
    pub fn arm_owner_lease(
        &mut self,
        deadline: LeaseDeadline,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        self.open_and_tick(DriveKind::Lease, deadline.at(), true)
    }

    /// Continue the open cursor for one stage.
    ///
    /// No cursor and no held result is [`StepError::NoActiveStep`]. That poll
    /// does not read the owner source and does not dispatch.
    pub fn poll(&mut self, caller: MonoInstant) -> Result<StepProgress<CompletedStep>, StepError> {
        self.open_poll(caller)
    }

    /// Read the owner source once. Does not dispatch guest, runner, or ledger work.
    ///
    /// A pending read returns `Ok` and does not accept a proof. Call again
    /// after the cost is paid. An expired saved end returns [`StepError::OwnerDead`]
    /// and does not accept a queued proof.
    pub fn sync_owner(&mut self, caller: MonoInstant) -> Result<(), StepError> {
        self.open_sync(caller)
    }
}
