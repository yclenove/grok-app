//! Forward steps. Each one returns before its deadline and does not sleep.

use std::num::NonZeroU64;

use super::super::super::Probe;
use super::super::deadline::{
    CloneDeadline, GroupDeadline, HostClock, LeaseDeadline, LoginDeadline, QueryDeadline,
    RunDeadline, StageDeadline, StartDeadline,
};
use super::super::guest::{GuestChannel, GUEST_CHANNEL_VERSION};
use super::super::identity::GoldenImage;
use super::super::ledger::{AttemptLedger, LedgerError, LedgerFact, VmIdentity};
use super::super::lock::CleanupLock;
use super::super::manifest::InstallManifest;
use super::super::parse::{self, LoginMark};
use super::super::policy::SoftnetFlags;
use super::super::receipt::{NotRunReason, Phase};
use super::super::runner::TartRunner;
use super::{ledger_reason, score_of, Attempt, Scope, StepError};

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
    pub stop_latched: bool,
    /// The external owner is already responsible for cleanup.
    pub owner_present: bool,
}

impl<'a, R, G, L, K, C> Attempt<'a, R, G, L, K, C>
where
    R: TartRunner,
    G: GuestChannel,
    L: AttemptLedger,
    K: CleanupLock,
    C: HostClock,
{
    /// Validate the manifest, Stop, and owner, then reserve.
    ///
    /// Does not clone. An unsealed or quarantined ledger row is refused and
    /// the phase stays `Idle`. A lost acknowledgement is read back inside the
    /// same deadline: a stored row keeps its ids and requires recovery, and a
    /// missing row does not. The prep aggregate starts at this entry, before
    /// the lock and the pre-reserve reads, and is not moved later. Does not block.
    pub fn reserve(
        &mut self,
        input: ReserveInput,
        deadline: QueryDeadline,
    ) -> Result<(), StepError> {
        let caller = deadline.at();
        let absolute = Some(input.absolute.at());
        if self.origin.is_none() {
            self.origin = Some(self.clock.now());
        }
        self.gate(
            &[Phase::Idle],
            caller,
            self.budgets.query,
            Scope::Prep,
            absolute,
        )?;
        if !input.owner_present {
            self.fail(NotRunReason::Refused);
            return Err(StepError::Refused);
        }
        if input.stop_latched {
            self.halted = true;
            self.fail(NotRunReason::Refused);
            return Err(StepError::StopLatched);
        }
        if input.manifest != input.approved {
            self.fail(NotRunReason::Refused);
            return Err(StepError::ManifestMismatch);
        }
        let now = self.clock.now();
        let attempt_end = now
            .checked_add(self.budgets.attempt)
            .map_err(|_| StepError::Overflow)?;
        if input.absolute.at() > attempt_end {
            return Err(StepError::AboveBudget);
        }
        let rows = match self.ledger_rows(caller, Scope::Prep, absolute) {
            Ok(rows) => rows,
            Err(err) => return Err(self.ledger_err(err)),
        };
        if rows.iter().any(|row| !row.is_sealed()) {
            return Err(StepError::MustRecover);
        }
        if rows
            .iter()
            .any(|row| row.facts().contains(&LedgerFact::Quarantined))
        {
            self.halted = true;
            return Err(StepError::Halted);
        }
        let names = match self.listed(caller, Scope::Prep, absolute) {
            Ok(names) => names,
            Err(reason) => {
                self.fail(reason);
                return Err(step_from_reason(reason));
            }
        };
        if names.iter().any(|name| name == input.clone_id.as_str()) {
            return Err(StepError::DestinationExists);
        }
        match self.ledger_reserve(
            input.run.clone(),
            input.clone_id.clone(),
            input.absolute,
            caller,
            Scope::Prep,
            absolute,
        ) {
            Ok(permit) => {
                self.absolute = Some(permit.absolute_deadline());
                self.permit = Some(permit);
                self.run = Some(input.run);
                self.clone_id = Some(input.clone_id);
                self.phase = Phase::Reserved;
                Ok(())
            }
            Err(LedgerError::ReplyLost) => self.reconcile_reserve(input, caller),
            Err(err) => Err(self.ledger_err(err)),
        }
    }

    /// A lost reserve ack is not proof the row is absent.
    fn reconcile_reserve(
        &mut self,
        input: ReserveInput,
        caller: super::super::deadline::MonoInstant,
    ) -> Result<(), StepError> {
        let absolute = Some(input.absolute.at());
        match self.ledger_query(&input.clone_id, caller, Scope::Prep, absolute) {
            Err(LedgerError::NotFound) => Err(self.ledger_err(LedgerError::ReplyLost)),
            Ok(_) | Err(_) => self.hold_unacked(input),
        }
    }

    fn hold_unacked(&mut self, input: ReserveInput) -> Result<(), StepError> {
        self.run = Some(input.run);
        self.clone_id = Some(input.clone_id);
        self.absolute = Some(input.absolute.at());
        self.trigger = Some(NotRunReason::ReplyLost);
        self.phase = Phase::Revoking;
        if self.cleanup_origin.is_none() {
            self.cleanup_origin = Some(self.clock.now());
        }
        self.receipt = Some(super::super::receipt::AttemptReceipt::new(
            self.run.clone(),
            self.clone_id.clone(),
            super::super::receipt::AttemptOutcome::NotRun {
                reason: NotRunReason::ReplyLost,
                client_started: false,
            },
            false,
            super::super::super::CleanupDisposition::Allocated,
            false,
            true,
        ));
        Err(StepError::Ledger(LedgerError::ReplyLost))
    }

    /// Clone the golden image with the reserved permit.
    ///
    /// The expected fake incarnation is written before the runner is called.
    /// The permit is taken only for that call. A lost identity write is cloned
    /// only when a successful read shows that exact pair. An unreadable read
    /// does not clone. A success body that does not match the stored
    /// expectation, including config alone, is not written over it. Does not block.
    pub fn clone_vm(
        &mut self,
        golden: GoldenImage,
        deadline: CloneDeadline,
    ) -> Result<(), StepError> {
        let caller = deadline.at();
        self.gate(
            &[Phase::Reserved],
            caller,
            self.budgets.clone_limit,
            Scope::Prep,
            None,
        )?;
        let Some(clone) = self.clone_id.clone() else {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        };
        let names = match self.listed(caller, Scope::Prep, None) {
            Ok(names) => names,
            Err(reason) => {
                self.fail(reason);
                return Err(step_from_reason(reason));
            }
        };
        if names.iter().any(|name| name == clone.as_str()) {
            self.fail(NotRunReason::IdentityConflict);
            return Err(StepError::Cleanup(NotRunReason::IdentityConflict));
        }
        let expected = Self::fake_allocation();
        let landed = match self.identity_landed(&clone, &expected, caller, Scope::Prep) {
            Ok(landed) => landed,
            Err(err) => return Err(self.ledger_err(err)),
        };
        if !landed {
            return Err(self.ledger_err(LedgerError::ReplyLost));
        }
        self.identity = Some(expected.clone());
        let Some(permit) = self.permit.take() else {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        };
        let body = self.runner_clone(permit, golden, caller, Scope::Prep)?;
        let mark = match parse::parse_clone(body.as_bytes()) {
            Ok(mark) => mark,
            Err(_) => {
                self.fail(NotRunReason::Malformed);
                return Err(StepError::MalformedBody);
            }
        };
        let got = VmIdentity::from_parts(mark.incarnation, mark.config);
        if got != expected {
            self.fail(NotRunReason::IdentityConflict);
            return Err(StepError::Cleanup(NotRunReason::IdentityConflict));
        }
        if !self.fact_ok(&clone, LedgerFact::CloneIssued, caller)? {
            return Err(self.ledger_err(LedgerError::ReplyLost));
        }
        self.phase = Phase::Cloned;
        Ok(())
    }

    /// Start the clone with the required Softnet flags.
    ///
    /// Records [`LedgerFact::StartIssued`] before the runner call. There is no
    /// start without those flags and no restart. If that fact cannot be
    /// confirmed, the runner is not called. Does not block.
    pub fn start(
        &mut self,
        flags: &SoftnetFlags,
        deadline: StartDeadline,
    ) -> Result<(), StepError> {
        let caller = deadline.at();
        self.gate(
            &[Phase::Cloned],
            caller,
            self.budgets.start,
            Scope::Prep,
            None,
        )?;
        let Some(clone) = self.clone_id.clone() else {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        };
        let Some(run) = self.run.clone() else {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        };
        let Some(identity) = self.identity.clone() else {
            self.fail(NotRunReason::IdentityConflict);
            return Err(StepError::Cleanup(NotRunReason::IdentityConflict));
        };
        if let Err(reason) = self.confirm_owned(&clone, &run, &identity, caller) {
            self.fail(reason);
            return Err(step_from_reason(reason));
        }
        if !self.fact_ok(&clone, LedgerFact::StartIssued, caller)? {
            return Err(self.ledger_err(LedgerError::ReplyLost));
        }
        self.vm_started = true;
        self.runner_start(&clone, flags, caller, Scope::Prep)?;
        self.phase = Phase::Booting;
        Ok(())
    }

    /// Inventory and every probe. A failed probe enters cleanup immediately.
    ///
    /// Unknown probes stay recorded and do not open the client. Each probe
    /// uses the probe budget, inside `deadline` and the absolute deadline.
    /// Does not block.
    pub fn isolate(&mut self, deadline: GroupDeadline) -> Result<(), StepError> {
        let caller = deadline.at();
        self.gate(
            &[Phase::Booting],
            caller,
            self.budgets.probe_group,
            Scope::Prep,
            None,
        )?;
        if self.guest.protocol_version() != GUEST_CHANNEL_VERSION {
            self.fail(NotRunReason::MissingProof);
            return Err(StepError::Protocol);
        }
        let body = self.guest_inventory(caller, Scope::Prep)?;
        let view = match parse::parse_guest(body.as_bytes()) {
            Ok(view) => view,
            Err(_) => {
                self.fail(NotRunReason::Malformed);
                return Err(StepError::MalformedBody);
            }
        };
        self.evidence.guest = Some(view);
        for probe in Probe::ALL {
            let body = self.guest_probe(probe, caller, Scope::Prep)?;
            let check = match parse::parse_probe(body.as_bytes()) {
                Ok(check) => check,
                Err(_) => {
                    self.fail(NotRunReason::Malformed);
                    return Err(StepError::MalformedBody);
                }
            };
            self.evidence.probes.push((probe, check));
            if check == super::super::super::Check::Failed {
                self.fail(NotRunReason::EnvironmentBreach);
                return Err(StepError::Breach);
            }
        }
        self.phase = Phase::Isolated;
        Ok(())
    }

    /// Record a fake bootstrap ACK. This does not launch a helper.
    ///
    /// Does not block. A missing or foreign ACK enters cleanup.
    pub fn bootstrap(&mut self, deadline: StageDeadline) -> Result<(), StepError> {
        let caller = deadline.at();
        self.gate(
            &[Phase::Isolated],
            caller,
            self.budgets.stage,
            Scope::Prep,
            None,
        )?;
        let Some(run) = self.run.clone() else {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        };
        let body = self.guest_bootstrap(caller, Scope::Prep)?;
        match parse::parse_ack(body.as_bytes(), run.as_str()) {
            Ok(generation) => {
                self.evidence.ack_generation = Some(generation);
                self.phase = Phase::Bootstrapped;
                Ok(())
            }
            Err(_) => {
                self.fail(NotRunReason::MissingProof);
                Err(StepError::MalformedBody)
            }
        }
    }

    /// Record fake login evidence. The call takes no secret and does not block.
    pub fn login(&mut self, deadline: LoginDeadline) -> Result<(), StepError> {
        let caller = deadline.at();
        self.gate(
            &[Phase::Bootstrapped],
            caller,
            self.budgets.login,
            Scope::Prep,
            None,
        )?;
        let body = self.guest_login(caller, Scope::Prep)?;
        match parse::parse_login(body.as_bytes()) {
            Ok(LoginMark::Ready) => {
                self.evidence.login_ready = true;
                self.phase = Phase::CredentialReady;
                Ok(())
            }
            Ok(LoginMark::Missing) => {
                self.fail(NotRunReason::MissingProof);
                Err(StepError::MalformedBody)
            }
            Err(_) => {
                self.fail(NotRunReason::Malformed);
                Err(StepError::MalformedBody)
            }
        }
    }

    /// Re-read the guest identity and require complete evidence.
    ///
    /// Does not mint a launch capability. A boot change is not adopted.
    /// Does not block.
    pub fn ready(&mut self, deadline: QueryDeadline) -> Result<(), StepError> {
        let caller = deadline.at();
        self.gate(
            &[Phase::CredentialReady],
            caller,
            self.budgets.query,
            Scope::Prep,
            None,
        )?;
        let body = self.guest_inventory(caller, Scope::Prep)?;
        let view = match parse::parse_guest(body.as_bytes()) {
            Ok(view) => view,
            Err(_) => {
                self.fail(NotRunReason::Malformed);
                return Err(StepError::MalformedBody);
            }
        };
        let Some(previous) = self.evidence.guest.clone() else {
            self.fail(NotRunReason::MissingProof);
            return Err(StepError::MalformedBody);
        };
        if previous.boot != view.boot {
            self.fail(NotRunReason::BootChanged);
            return Err(StepError::Breach);
        }
        if previous.uid != view.uid
            || previous.desktop != view.desktop
            || previous.config != view.config
        {
            self.fail(NotRunReason::EnvironmentBreach);
            return Err(StepError::Breach);
        }
        if !self.evidence.complete() {
            self.fail(NotRunReason::MissingProof);
            return Err(StepError::MalformedBody);
        }
        self.phase = Phase::Ready;
        Ok(())
    }

    /// Issue the fake run once and record its score body.
    ///
    /// A second call is refused and does not issue another run. The score is
    /// provisional until seal. The run budget is not shortened by prep.
    /// Does not block.
    pub fn run(&mut self, deadline: RunDeadline) -> Result<(), StepError> {
        let caller = deadline.at();
        self.gate(&[Phase::Ready], caller, self.budgets.run, Scope::Task, None)?;
        if self.client_starts > 0 {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        }
        let body = self.guest_run(caller, Scope::Task)?;
        match parse::parse_score(body.as_bytes()) {
            Ok(mark) => {
                self.evidence.provisional = Some(score_of(mark));
                self.phase = Phase::Running;
                Ok(())
            }
            Err(_) => {
                self.fail(NotRunReason::Malformed);
                Err(StepError::MalformedBody)
            }
        }
    }

    /// Send one owner lease. A late call does not extend or revive it.
    ///
    /// Does not block. The sequence starts at 1 and only increases. The lease
    /// end is the clipped instant of this call, not a second budget. The
    /// previous end is checked again immediately before the send and again
    /// before that instant is stored. Expiry enters [`Phase::Suspended`] and
    /// leaves the old end in place.
    pub fn arm_owner_lease(&mut self, deadline: LeaseDeadline) -> Result<(), StepError> {
        let caller = deadline.at();
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        if self.lease_expired() {
            self.expire_owner(Some(caller), Scope::Task);
            return Err(StepError::LeaseExpired);
        }
        self.ensure_phase(&[Phase::Running], Some(caller), Scope::Task)?;
        self.admit(caller, self.budgets.lease, Scope::Task, None)?;
        if self.lease_expired() {
            self.expire_owner(Some(caller), Scope::Task);
            return Err(StepError::LeaseExpired);
        }
        let next = self.lease_seq.checked_add(1).ok_or(StepError::Overflow)?;
        let Some(seq) = NonZeroU64::new(next) else {
            return Err(StepError::Overflow);
        };
        let until = self.guest_lease(seq, caller, Scope::Task)?;
        if self.lease_expired() {
            self.expire_owner(Some(caller), Scope::Task);
            return Err(StepError::LeaseExpired);
        }
        self.lease_seq = next;
        self.lease_until = Some(until);
        Ok(())
    }

    fn fact_ok(
        &mut self,
        clone: &super::super::identity::CloneId,
        fact: LedgerFact,
        caller: super::super::deadline::MonoInstant,
    ) -> Result<bool, StepError> {
        match self.fact_landed(clone, fact, caller, Scope::Prep) {
            Ok(landed) => Ok(landed),
            Err(err) => Err(self.ledger_err(err)),
        }
    }

    fn ledger_err(&mut self, err: LedgerError) -> StepError {
        self.fail(ledger_reason(&err));
        StepError::Ledger(err)
    }
}

fn step_from_reason(reason: NotRunReason) -> StepError {
    match reason {
        NotRunReason::Deadline => StepError::Expired,
        NotRunReason::Malformed => StepError::MalformedBody,
        other => StepError::Cleanup(other),
    }
}
