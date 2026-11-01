//! Forward steps. Each one returns before its deadline and does not sleep.

use super::super::super::Probe;
use super::super::deadline::{
    CloneDeadline, GroupDeadline, LeaseDeadline, LoginDeadline, QueryDeadline, RunDeadline,
    StageDeadline, StartDeadline,
};
use super::super::guest::{GuestChannel, GUEST_CHANNEL_VERSION};
use super::super::identity::{CloneId, RunId};
use super::super::ledger::AttemptLedger;
use super::super::lock::CleanupLock;
use super::super::manifest::InstallManifest;
use super::super::parse::{self, LoginMark};
use super::super::policy::SoftnetFlags;
use super::super::receipt::{NotRunReason, Phase};
use super::super::runner::TartRunner;
use std::num::NonZeroU64;

use super::super::deadline::HostClock as Clock;
use super::{call_reason, ledger_reason, score_of, Attempt, StepError};

/// Inputs for [`Attempt::reserve`]. Digests are compared, not trusted.
pub struct ReserveInput {
    /// Run id written before the permit exists.
    pub run: RunId,
    /// Clone id written before the permit exists. Not a golden name.
    pub clone_id: CloneId,
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
    C: Clock,
{
    /// Validate the manifest, Stop, and owner, then reserve.
    ///
    /// Does not clone. An unsealed or quarantined ledger row is refused and
    /// the phase stays `Idle`. A refusal before a row exists moves to
    /// `Destroying` so the caller can seal a rejection receipt. Does not block.
    pub fn reserve(
        &mut self,
        input: ReserveInput,
        deadline: QueryDeadline,
    ) -> Result<(), StepError> {
        let effective = self.gate(&[Phase::Idle], deadline.at(), self.budgets.query, false)?;
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
        let query = self.query_deadline(effective)?;
        let rows = match self.ledger.rows(query) {
            Ok(rows) => rows,
            Err(err) => {
                self.fail(ledger_reason(&err));
                return Err(StepError::Ledger(err));
            }
        };
        if rows.iter().any(|row| !row.is_sealed()) {
            return Err(StepError::MustRecover);
        }
        if rows.iter().any(|row| {
            row.facts()
                .contains(&super::super::ledger::LedgerFact::Quarantined)
        }) {
            self.halted = true;
            return Err(StepError::Halted);
        }
        match self.ledger.reserve(
            input.run.clone(),
            input.clone_id.clone(),
            input.absolute,
            query,
        ) {
            Ok(permit) => {
                self.absolute = Some(permit.absolute_deadline());
                self.permit = Some(permit);
                self.run = Some(input.run);
                self.clone_id = Some(input.clone_id);
                self.origin = Some(self.clock.now());
                self.phase = Phase::Reserved;
                Ok(())
            }
            Err(err) => {
                self.fail(ledger_reason(&err));
                Err(StepError::Ledger(err))
            }
        }
    }

    /// Clone the golden image with the reserved permit.
    ///
    /// The permit is consumed on every path that reaches the runner, including
    /// failure. The effective deadline is the earlier of `deadline` and the
    /// absolute attempt deadline. A lost reply still owns the clone id.
    /// Does not block.
    pub fn clone_vm(
        &mut self,
        golden: super::super::identity::GoldenImage,
        deadline: CloneDeadline,
    ) -> Result<(), StepError> {
        let effective = self.gate(
            &[Phase::Reserved],
            deadline.at(),
            self.budgets.clone_limit,
            true,
        )?;
        let Some(permit) = self.permit.take() else {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        };
        let call = CloneDeadline::from_instant(effective);
        let result = self.runner.clone_vm(permit, golden, call);
        if !matches!(result, Err(super::super::output::CallError::Expired)) {
            if let Err(err) =
                self.append_fact(super::super::ledger::LedgerFact::CloneIssued, effective)
            {
                self.fail(ledger_reason(&err));
                return Err(StepError::Ledger(err));
            }
        }
        match result {
            Ok(_) => {
                self.phase = Phase::Cloned;
                Ok(())
            }
            Err(err) => {
                self.fail(call_reason(&err));
                Err(StepError::Call(err))
            }
        }
    }

    /// Start the clone with the required Softnet flags.
    ///
    /// There is no start without those flags and no restart. Does not block.
    pub fn start(
        &mut self,
        flags: &SoftnetFlags,
        deadline: StartDeadline,
    ) -> Result<(), StepError> {
        let effective = self.gate(&[Phase::Cloned], deadline.at(), self.budgets.start, true)?;
        let Some(clone) = self.clone_id.clone() else {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        };
        let call = StartDeadline::from_instant(effective);
        match self.runner.start(&clone, flags, call) {
            Ok(_) => {
                self.phase = Phase::Booting;
                Ok(())
            }
            Err(err) => {
                self.fail(call_reason(&err));
                Err(StepError::Call(err))
            }
        }
    }

    /// Inventory and every probe. A failed probe enters cleanup immediately.
    ///
    /// Unknown probes stay recorded and do not open the client. Each probe
    /// uses the probe budget, inside `deadline` and the absolute deadline.
    /// Does not block.
    pub fn isolate(&mut self, deadline: GroupDeadline) -> Result<(), StepError> {
        let effective = self.gate(
            &[Phase::Booting],
            deadline.at(),
            self.budgets.probe_group,
            true,
        )?;
        if self.guest.protocol_version() != GUEST_CHANNEL_VERSION {
            self.fail(NotRunReason::MissingProof);
            return Err(StepError::Protocol);
        }
        let body = self.guest_query(effective, |guest, call| guest.inventory(call))?;
        let view = match parse::parse_guest(body.as_bytes()) {
            Ok(view) => view,
            Err(_) => {
                self.fail(NotRunReason::Malformed);
                return Err(StepError::MalformedBody);
            }
        };
        self.evidence.boot = Some(view.boot);
        self.evidence.uid = Some(view.uid);
        self.evidence.desktop = Some(view.desktop);
        self.evidence.config = Some(view.config);
        for probe in Probe::ALL {
            let at = match self.earlier(effective, self.budgets.probe, self.absolute) {
                Ok(at) => at,
                Err(reason) => {
                    self.fail(reason);
                    return Err(StepError::Expired);
                }
            };
            let call = super::super::deadline::ProbeDeadline::from_instant(at);
            let body = match self.guest.probe(probe, call) {
                Ok(body) => body,
                Err(err) => {
                    self.fail(call_reason(&err));
                    return Err(StepError::Call(err));
                }
            };
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
        let effective = self.gate(&[Phase::Isolated], deadline.at(), self.budgets.stage, true)?;
        let Some(run) = self.run.clone() else {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        };
        let body = self.guest_stage(effective, |guest, call| guest.bootstrap(call))?;
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
        let effective = self.gate(
            &[Phase::Bootstrapped],
            deadline.at(),
            self.budgets.login,
            true,
        )?;
        let body = self.guest_login(effective)?;
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
        let effective = self.gate(
            &[Phase::CredentialReady],
            deadline.at(),
            self.budgets.query,
            true,
        )?;
        let body = self.guest_query(effective, |guest, call| guest.inventory(call))?;
        let view = match parse::parse_guest(body.as_bytes()) {
            Ok(view) => view,
            Err(_) => {
                self.fail(NotRunReason::Malformed);
                return Err(StepError::MalformedBody);
            }
        };
        if self.evidence.boot.as_deref() != Some(view.boot.as_str()) {
            self.fail(NotRunReason::BootChanged);
            return Err(StepError::Breach);
        }
        if self.evidence.uid.as_deref() != Some(view.uid.as_str())
            || self.evidence.desktop.as_deref() != Some(view.desktop.as_str())
            || self.evidence.config.as_deref() != Some(view.config.as_str())
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
    /// provisional until seal. Does not block.
    pub fn run(&mut self, deadline: RunDeadline) -> Result<(), StepError> {
        let effective = self.gate(&[Phase::Ready], deadline.at(), self.budgets.run, false)?;
        if self.client_starts > 0 {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        }
        let at = match self.earlier(effective, self.budgets.run, self.absolute) {
            Ok(at) => at,
            Err(reason) => {
                self.fail(reason);
                return Err(StepError::Expired);
            }
        };
        self.client_starts = self.client_starts.saturating_add(1);
        let call = RunDeadline::from_instant(at);
        let body = match self.guest.run(call) {
            Ok(body) => body,
            Err(err) => {
                self.fail(call_reason(&err));
                return Err(StepError::Call(err));
            }
        };
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
    /// Does not block. The sequence starts at 1 and only increases.
    pub fn arm_owner_lease(&mut self, deadline: LeaseDeadline) -> Result<(), StepError> {
        self.ensure_phase(&[Phase::Running])?;
        if let Some(until) = self.lease_until {
            if self.clock.now() >= until {
                return Err(StepError::LeaseExpired);
            }
        }
        let effective = self.admit(deadline.at(), self.budgets.lease, false)?;
        let next = self.lease_seq.checked_add(1).ok_or(StepError::Overflow)?;
        let Some(seq) = NonZeroU64::new(next) else {
            return Err(StepError::Overflow);
        };
        let call = LeaseDeadline::from_instant(effective);
        match self.guest.lease(seq, call) {
            Ok(_) => {
                self.lease_seq = next;
                self.lease_until = Some(effective);
                Ok(())
            }
            Err(err) => {
                self.fail(call_reason(&err));
                Err(StepError::Call(err))
            }
        }
    }

    fn guest_query(
        &mut self,
        effective: super::super::deadline::MonoInstant,
        call: impl FnOnce(
            &mut G,
            QueryDeadline,
        ) -> Result<
            super::super::output::BoundedOutput,
            super::super::output::CallError,
        >,
    ) -> Result<super::super::output::BoundedOutput, StepError> {
        let at = match self.earlier(effective, self.budgets.query, self.absolute) {
            Ok(at) => at,
            Err(reason) => {
                self.fail(reason);
                return Err(StepError::Expired);
            }
        };
        match call(self.guest, QueryDeadline::from_instant(at)) {
            Ok(body) => Ok(body),
            Err(err) => {
                self.fail(call_reason(&err));
                Err(StepError::Call(err))
            }
        }
    }

    fn guest_stage(
        &mut self,
        effective: super::super::deadline::MonoInstant,
        call: impl FnOnce(
            &mut G,
            StageDeadline,
        ) -> Result<
            super::super::output::BoundedOutput,
            super::super::output::CallError,
        >,
    ) -> Result<super::super::output::BoundedOutput, StepError> {
        let at = match self.earlier(effective, self.budgets.stage, self.absolute) {
            Ok(at) => at,
            Err(reason) => {
                self.fail(reason);
                return Err(StepError::Expired);
            }
        };
        match call(self.guest, StageDeadline::from_instant(at)) {
            Ok(body) => Ok(body),
            Err(err) => {
                self.fail(call_reason(&err));
                Err(StepError::Call(err))
            }
        }
    }

    fn guest_login(
        &mut self,
        effective: super::super::deadline::MonoInstant,
    ) -> Result<super::super::output::BoundedOutput, StepError> {
        let at = match self.earlier(effective, self.budgets.login, self.absolute) {
            Ok(at) => at,
            Err(reason) => {
                self.fail(reason);
                return Err(StepError::Expired);
            }
        };
        match self.guest.login(LoginDeadline::from_instant(at)) {
            Ok(body) => Ok(body),
            Err(err) => {
                self.fail(call_reason(&err));
                Err(StepError::Call(err))
            }
        }
    }
}
