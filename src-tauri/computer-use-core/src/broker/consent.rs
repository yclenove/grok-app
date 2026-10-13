//! Broker entry points for task consent. The lock is not held across adapter work.
use std::collections::HashSet;

use super::*;
use crate::task_consent::{
    BearerClass, CommitOutcome, CommitReady, IssuedTransport, ScopeMemoryView,
};
use serde_json::Value;

impl ComputerUseBroker {
    pub fn issue_transport_credential(
        &self,
        app_session_id: &str,
    ) -> Result<IssuedTransport, BrokerError> {
        let _change = self.changes.notice();
        let mut inner = self.inner.lock();
        let epoch = inner.feature_epoch;
        let retained = runtime_retained(&inner);
        inner.consent.issue(app_session_id, epoch, &retained)
    }

    pub fn bind_agent_session(
        &self,
        app_session_id: &str,
        agent_session_id: &str,
    ) -> Result<(), BrokerError> {
        let _change = self.changes.notice();
        let runs = {
            let mut inner = self.inner.lock();
            inner.consent.bind_agent(app_session_id, agent_session_id)?
        };
        self.handoff_fences(&runs);
        Ok(())
    }

    pub fn rotate_connection(&self, app_session_id: &str) -> Result<(), BrokerError> {
        let _change = self.changes.notice();
        let runs = {
            let mut inner = self.inner.lock();
            inner.consent.rotate_connection(app_session_id)?
        };
        self.handoff_fences(&runs);
        Ok(())
    }

    pub fn retire_owner(&self, app_session_id: &str) {
        let _change = self.changes.notice();
        let runs = {
            let mut inner = self.inner.lock();
            inner.consent.retire_owner(app_session_id)
        };
        self.handoff_fences(&runs);
    }

    pub fn classify_bearer(&self, token: &str) -> BearerClass {
        self.inner.lock().consent.classify(token)
    }

    pub fn take_retired_tokens(&self) -> Vec<String> {
        self.inner.lock().consent.take_retired_tokens()
    }

    pub fn submit_access(&self, token: &str, args: Value) -> Result<Value, BrokerError> {
        let _change = self.changes.notice();
        let ctx = self.live_context(token)?;
        let busy = self.busy_run_ids();
        let value = {
            let mut inner = self.inner.lock();
            let epoch = inner.feature_epoch;
            inner.consent.submit(&ctx, args, &busy, epoch)
        };
        self.handoff_expired();
        value
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn bind_synthetic_scope(
        &self,
        request_id: &str,
        target_id: &str,
    ) -> Result<(), BrokerError> {
        self.inner
            .lock()
            .consent
            .bind_synthetic(request_id, target_id)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn bind_synthetic_scope_on_surface(
        &self,
        request_id: &str,
        target_id: &str,
        surface: SurfaceKind,
    ) -> Result<(), BrokerError> {
        self.inner
            .lock()
            .consent
            .bind_synthetic_on_surface(request_id, target_id, surface)
    }

    pub fn commit_access(
        &self,
        token: &str,
        request_id: &str,
        revision: u64,
    ) -> Result<CommitOutcome, BrokerError> {
        let ctx = self.live_context(token)?;
        let ready = {
            let _change = self.changes.notice();
            let mut inner = self.inner.lock();
            let enabled = inner.feature_enabled;
            let epoch = inner.feature_epoch;
            inner
                .consent
                .start_preparation(&ctx, request_id, revision, enabled, epoch)?
        };
        let prep = match ready {
            CommitReady::Already(outcome) => return Ok(outcome),
            CommitReady::Prepare(prep) => prep,
            CommitReady::WebViewSelection(ticket) => {
                return self.abort_commit(ticket.id, BrokerError::TargetUnauthorized, false)
            }
        };
        let before_enable = self.inner.lock().consent.take_before_enable();
        if let Some(hook) = before_enable {
            hook();
        }
        if !self.preparation_current(prep.id) {
            return self.abort_commit(prep.id, BrokerError::StopRequested, false);
        }
        if let Err(error) = self.publish_preparation(prep.id) {
            return self.abort_commit(prep.id, error, false);
        }
        let after_enable = self.inner.lock().consent.take_after_enable();
        if let Some(hook) = after_enable {
            hook();
        }
        if !self.preparation_current(prep.id) {
            return self.abort_commit(prep.id, BrokerError::StopRequested, false);
        }
        let before_open = self.inner.lock().consent.take_before_open();
        if let Some(hook) = before_open {
            hook();
        }
        if !self.preparation_current(prep.id) {
            return self.abort_commit(prep.id, BrokerError::StopRequested, false);
        }
        let existed = self.run_exists(&prep.run_id);
        let bound = self.bind_granted_target(&prep, existed);
        let after_bind = self.inner.lock().consent.take_after_bind();
        if let Some(hook) = after_bind {
            hook();
        }
        let bound = match bound {
            Ok(bound) => bound,
            Err(error) => return self.abort_commit(prep.id, error, !existed),
        };
        if !self.preparation_current(prep.id) {
            return self.abort_commit(prep.id, BrokerError::StopRequested, !existed);
        }
        let before_finalize = self.inner.lock().consent.take_before_finalize();
        if let Some(hook) = before_finalize {
            hook();
        }
        self.finalize_preparation(&prep, &bound)
    }

    pub(super) fn finalize_preparation(
        &self,
        prep: &crate::task_consent::PrepareTicket,
        expected: &AuthorizedTarget,
    ) -> Result<CommitOutcome, BrokerError> {
        if let Err(error) = self.recheck_prepared_scope(prep) {
            return self.abort_commit(prep.id, error, true);
        }
        self.finalize_preparation_authority(prep, expected)
    }

    /// Only retained authority is touched while App session/ACP pins are held.
    pub(super) fn finalize_preparation_authority(
        &self,
        prep: &crate::task_consent::PrepareTicket,
        expected: &AuthorizedTarget,
    ) -> Result<CommitOutcome, BrokerError> {
        let _change = self.changes.notice();
        let committed = {
            let mut inner = self.inner.lock();
            self.with_prepared_scope_authority(prep, || {
                let enabled = inner.feature_enabled;
                let epoch = inner.feature_epoch;
                let run_ok = inner.runs.get(&prep.run_id).is_some_and(|run| {
                    run.stop == StopState::Running
                        && !run.paused
                        && !run.resume_in_flight
                        && !run.switch_in_flight
                        && run.target.as_ref().is_some_and(|target| {
                            target.target_id == prep.target_id
                                && target.surface == prep.surface
                                && target.target_id == expected.target_id
                                && target.surface == expected.surface
                                && target.target_generation == expected.target_generation
                                && target.executor_generation == expected.executor_generation
                        })
                });
                let current = inner.consent.preparation_matches(prep, enabled, epoch);
                if !run_ok || !current {
                    return Err(BrokerError::StopRequested);
                }
                let outcome = inner.consent.finalize(prep.id, enabled, epoch)?;
                if let Some(work) = inner
                    .runs
                    .get(&outcome.run_id)
                    .map(|run| run.in_flight.clone())
                {
                    let activity = inner.consent.bind_physical_work(&outcome.run_id, work);
                    if let Some(run) = inner.runs.get_mut(&outcome.run_id) {
                        run.activity = activity;
                    }
                }
                if let Some(run) = inner.runs.get_mut(&outcome.run_id) {
                    if run.stop == StopState::Running
                        && run
                            .target
                            .as_ref()
                            .is_some_and(|target| target.target_id == outcome.binding.target_id)
                    {
                        run.consent = ConsentGate::Bound(outcome.binding.clone());
                    }
                }
                Ok(outcome)
            })
            .map_err(|error| self::abort_locked(&mut inner, prep.id, error))
        };
        match committed {
            Ok(outcome) => Ok(outcome),
            Err((error, effect)) => {
                if effect.opened_run {
                    self.handoff_fences(std::slice::from_ref(&effect.run_id));
                }
                Err(error)
            }
        }
    }

    pub fn deny_access(
        &self,
        token: &str,
        request_id: &str,
        revision: u64,
    ) -> Result<(), BrokerError> {
        let _change = self.changes.notice();
        let ctx = self.live_context(token)?;
        self.inner.lock().consent.deny(&ctx, request_id, revision)
    }

    pub fn cancel_access(
        &self,
        token: &str,
        request_id: &str,
        revision: u64,
    ) -> Result<(), BrokerError> {
        let _change = self.changes.notice();
        let ctx = self.live_context(token)?;
        self.inner
            .lock()
            .consent
            .host_cancel(&ctx, request_id, revision)
    }

    /// Fence owned runs and hand back cleanup tickets. Slow abort stays with the ticket.
    pub fn consent_stop(&self, app_session_id: &str) -> Vec<StopCleanupTicket> {
        let _change = self.changes.notice();
        let runs = {
            let mut inner = self.inner.lock();
            inner.consent.stop_owner(app_session_id)
        };
        self.fence_ids(&runs)
    }

    /// Host lifecycle fence with the original cleanup owners handed to the
    /// process consumer. No adapter work is performed at this boundary.
    pub fn fence_task_consent(&self, app_session_id: &str) {
        let _change = self.changes.notice();
        let runs = self.inner.lock().consent.stop_owner(app_session_id);
        self.handoff_fences(&runs);
    }

    pub fn complete_task(&self, app_session_id: &str) -> Result<(), BrokerError> {
        let _change = self.changes.notice();
        let runs = self.inner.lock().consent.complete_task(app_session_id);
        self.handoff_fences(&runs);
        Ok(())
    }

    pub fn begin_new_task(&self, app_session_id: &str) -> Result<(), BrokerError> {
        let _change = self.changes.notice();
        let runs = {
            let mut inner = self.inner.lock();
            inner.consent.begin_new_task(app_session_id)?
        };
        self.handoff_fences(&runs);
        Ok(())
    }

    pub fn execution_run(&self, token: &str, tool: &str) -> Result<String, BrokerError> {
        Ok(self.begin_model_call(token, tool)?.run_id)
    }

    pub fn begin_model_call(
        &self,
        token: &str,
        tool: &str,
    ) -> Result<crate::task_consent::ExecutionPermit, BrokerError> {
        self.begin_model_call_claiming(token, tool, None, None)
    }

    pub fn begin_model_call_claiming(
        &self,
        token: &str,
        tool: &str,
        claimed_run: Option<&str>,
        claimed_target: Option<&str>,
    ) -> Result<crate::task_consent::ExecutionPermit, BrokerError> {
        let ctx = self.live_context(token)?;
        let busy = self.busy_run_ids();
        let permit = (|| {
            let mut inner = self.inner.lock();
            let enabled = inner.feature_enabled;
            let epoch = inner.feature_epoch;
            let mut permit = inner.consent.begin_model_call(
                &ctx,
                tool,
                enabled,
                epoch,
                &busy,
                crate::task_consent::ExecutionTargetClaim {
                    run_id: claimed_run,
                    target_id: claimed_target,
                },
            )?;
            permit.execution_generation = inner
                .runs
                .get(&permit.run_id)
                .ok_or(BrokerError::RunNotFound)?
                .generation;
            Ok(permit)
        })();
        self.handoff_expired();
        permit
    }

    pub fn recheck_model_call(
        &self,
        permit: &crate::task_consent::ExecutionPermit,
    ) -> Result<(), BrokerError> {
        let proof = {
            let inner = self.inner.lock();
            Self::recheck_model_call_locked(&inner, permit)?;
            inner.consent.scope_proof_for_permit(permit)
        };
        if permit.capability != "stop" {
            if let Some(proof) = proof {
                let (target, _) = self.authorized_binding(&permit.run_id)?;
                self.recheck_scope_proof(&permit.target_id, target.surface, &proof)?;
            }
        }
        Self::recheck_model_call_locked(&self.inner.lock(), permit)
    }

    pub(super) fn recheck_model_call_locked(
        inner: &Inner,
        permit: &crate::task_consent::ExecutionPermit,
    ) -> Result<(), BrokerError> {
        inner
            .consent
            .recheck_permit(permit, inner.feature_enabled, inner.feature_epoch)?;
        let run = inner
            .runs
            .get(&permit.run_id)
            .ok_or(BrokerError::RunNotFound)?;
        if run.stop != StopState::Running {
            return Err(BrokerError::StopRequested);
        }
        if run.generation != permit.execution_generation {
            return Err(BrokerError::IdentityMismatch("execution generation"));
        }
        if permit.capability != "stop" && (run.switch_in_flight || run.switch_cleanup_pending) {
            return Err(BrokerError::LeaseHeld {
                run_id: permit.run_id.clone(),
            });
        }
        match &run.consent {
            ConsentGate::Bound(binding)
                if binding.owner_nonce == permit.owner_nonce
                    && binding.grant_revision == permit.grant_revision
                    && binding.task_id == permit.task_id
                    && binding.request_id == permit.request_id
                    && binding.run_id == permit.run_id
                    && binding.target_id == permit.target_id
                    && binding.feature_epoch == permit.feature_epoch
                    && run
                        .target
                        .as_ref()
                        .is_some_and(|target| target.target_id == permit.target_id) =>
            {
                Ok(())
            }
            ConsentGate::Bound(_) => Err(BrokerError::IdentityMismatch("revision")),
            ConsentGate::Pending | ConsentGate::Legacy => Err(BrokerError::TargetUnauthorized),
        }
    }

    pub fn scope_memories(&self, app_session_id: &str) -> Vec<ScopeMemoryView> {
        self.inner.lock().consent.scope_memories(app_session_id)
    }

    pub fn executable_grant_count(&self, app_session_id: &str) -> usize {
        self.inner.lock().consent.grant_count(app_session_id)
    }

    pub fn live_request_count(&self, app_session_id: &str) -> usize {
        self.inner.lock().consent.live_request_count(app_session_id)
    }

    pub fn stored_request_count(&self) -> usize {
        self.inner.lock().consent.stored_request_count()
    }

    pub fn claim_continuation(
        &self,
        app_session_id: &str,
    ) -> Option<crate::task_consent::AuthorityClaim> {
        self.inner.lock().consent.claim_continuation(app_session_id)
    }

    pub fn validate_continuation(
        &self,
        claim: &crate::task_consent::AuthorityClaim,
    ) -> Result<(), BrokerError> {
        let inner = self.inner.lock();
        inner
            .consent
            .validate_continuation(claim, inner.feature_enabled, inner.feature_epoch)
    }

    pub fn consume_continuation(
        &self,
        claim: &crate::task_consent::AuthorityClaim,
    ) -> Result<(), BrokerError> {
        let mut inner = self.inner.lock();
        let enabled = inner.feature_enabled;
        let epoch = inner.feature_epoch;
        inner.consent.consume_continuation(claim, enabled, epoch)
    }

    pub fn continuation_count(&self, app_session_id: &str) -> usize {
        self.inner.lock().consent.continuation_count(app_session_id)
    }

    pub fn no_authority_reasons(&self, app_session_id: &str) -> Vec<&'static str> {
        self.inner
            .lock()
            .consent
            .no_authority_reasons(app_session_id)
    }

    pub fn take_no_authority(
        &self,
        app_session_id: &str,
        owner_nonce: u64,
    ) -> Vec<crate::task_consent::NoAuthorityNotice> {
        self.inner
            .lock()
            .consent
            .take_no_authority(app_session_id, owner_nonce)
    }

    pub fn validate_no_authority(
        &self,
        notice: &crate::task_consent::NoAuthorityNotice,
    ) -> Result<(), BrokerError> {
        self.inner.lock().consent.validate_no_authority(notice)
    }

    pub fn feature_epoch(&self) -> u64 {
        self.inner.lock().feature_epoch
    }

    pub fn consent_debug(&self) -> String {
        format!("{:?}", self.inner.lock().consent)
    }

    pub fn take_cleanup_tickets(&self) -> Vec<StopCleanupTicket> {
        let mut inner = self.inner.lock();
        requeue_pending_cleanups(&mut inner);
        std::mem::take(&mut inner.cleanup_tickets)
    }

    #[cfg(test)]
    pub fn advance_consent_clock(&self, by: std::time::Duration) {
        let _change = self.changes.notice();
        let busy = self.busy_run_ids();
        {
            let mut inner = self.inner.lock();
            inner.consent.advance_clock(by, &busy);
        }
        self.handoff_expired();
    }

    /// Move the consent clock and sweep without fencing. The expired-run cap test
    /// needs the queue to stay full so a later grant is kept rather than dropped.
    #[cfg(test)]
    pub fn sweep_consent_clock_for_test(&self, by: std::time::Duration) {
        let busy = self.busy_run_ids();
        self.inner.lock().consent.advance_clock(by, &busy);
    }

    #[cfg(test)]
    pub fn authority_census(&self) -> crate::task_consent::AuthorityCensus {
        let inner = self.inner.lock();
        let mut census = inner.consent.census();
        census.cleanup_tickets = inner.cleanup_tickets.len();
        census
    }

    #[cfg(test)]
    pub fn arm_commit_interrupt_before_enable(&self, hook: Box<dyn FnOnce() + Send>) {
        self.inner.lock().consent.arm_before_enable(hook);
    }

    #[cfg(test)]
    pub fn arm_commit_interrupt_after_enable(&self, hook: Box<dyn FnOnce() + Send>) {
        self.inner.lock().consent.arm_after_enable(hook);
    }

    #[cfg(test)]
    pub fn arm_commit_interrupt_before_open(&self, hook: Box<dyn FnOnce() + Send>) {
        self.inner.lock().consent.arm_before_open(hook);
    }

    #[cfg(test)]
    pub fn arm_commit_interrupt_after_bind(&self, hook: Box<dyn FnOnce() + Send>) {
        self.inner.lock().consent.arm_after_bind(hook);
    }

    #[cfg(test)]
    pub fn arm_commit_interrupt_after_open(&self, hook: Box<dyn FnOnce() + Send>) {
        self.inner.lock().consent.arm_after_open(hook);
    }

    #[cfg(test)]
    pub fn arm_commit_interrupt_before_finalize(&self, hook: Box<dyn FnOnce() + Send>) {
        self.inner.lock().consent.arm_before_finalize(hook);
    }

    #[cfg(test)]
    pub fn arm_after_target_switch(&self, hook: Box<dyn FnOnce() + Send>) {
        self.inner.lock().consent.arm_after_target_switch(hook);
    }

    #[cfg(test)]
    pub fn arm_before_final_admit(&self, hook: Box<dyn FnOnce() + Send>) {
        self.inner.lock().consent.arm_before_final_admit(hook);
    }

    #[cfg(test)]
    pub fn arm_execution_barrier(&self, hook: Box<dyn FnOnce() + Send>) {
        self.inner.lock().consent.arm_execution_barrier(hook);
    }

    #[cfg(test)]
    pub fn take_execution_barrier(&self) -> Option<Box<dyn FnOnce() + Send>> {
        self.inner.lock().consent.take_execution_barrier()
    }

    #[cfg(test)]
    pub fn submit_captured(
        &self,
        ctx: &crate::task_consent::TransportContext,
        args: Value,
    ) -> Result<Value, BrokerError> {
        let busy = self.busy_run_ids();
        let value = {
            let mut inner = self.inner.lock();
            let epoch = inner.feature_epoch;
            inner.consent.submit(ctx, args, &busy, epoch)
        };
        self.handoff_expired();
        value
    }

    #[cfg(test)]
    pub fn execution_with_context(
        &self,
        ctx: &crate::task_consent::TransportContext,
        tool: &str,
    ) -> Result<String, BrokerError> {
        let busy = self.busy_run_ids();
        let permit = {
            let mut inner = self.inner.lock();
            let enabled = inner.feature_enabled;
            let epoch = inner.feature_epoch;
            inner.consent.begin_model_call(
                ctx,
                tool,
                enabled,
                epoch,
                &busy,
                crate::task_consent::ExecutionTargetClaim::default(),
            )
        };
        self.handoff_expired();
        permit.map(|permit| permit.run_id)
    }

    #[cfg(test)]
    pub fn reservation_count(&self) -> usize {
        self.inner.lock().consent.reservation_count()
    }

    #[cfg(test)]
    pub fn start_model_preparation(
        &self,
        ctx: &crate::task_consent::TransportContext,
        request_id: &str,
        revision: u64,
    ) -> Result<CommitReady, BrokerError> {
        let mut inner = self.inner.lock();
        let enabled = inner.feature_enabled;
        let epoch = inner.feature_epoch;
        inner
            .consent
            .start_preparation(ctx, request_id, revision, enabled, epoch)
    }

    #[cfg(test)]
    pub fn abort_model_preparation(&self, id: u64) -> bool {
        let effect = {
            let mut inner = self.inner.lock();
            let enabled = inner.feature_enabled;
            let epoch = inner.feature_epoch;
            let effect = inner.consent.abort_preparation(id, enabled, epoch);
            if let Some((enabled, epoch)) = effect.feature {
                inner.feature_enabled = enabled;
                inner.feature_epoch = epoch;
            }
            effect
        };
        if effect.opened_run {
            self.handoff_fences(std::slice::from_ref(&effect.run_id));
        }
        effect.opened_run || effect.run_id.is_empty() || self.stop_state(&effect.run_id).is_err()
    }

    pub(super) fn live_context(
        &self,
        token: &str,
    ) -> Result<crate::task_consent::TransportContext, BrokerError> {
        match self.classify_bearer(token) {
            BearerClass::Live(ctx) => Ok(ctx),
            BearerClass::Retired | BearerClass::Unknown => {
                Err(BrokerError::IdentityMismatch("transport"))
            }
        }
    }

    pub(super) fn preparation_current(&self, id: u64) -> bool {
        let inner = self.inner.lock();
        inner
            .consent
            .preparation_current(id, inner.feature_enabled, inner.feature_epoch)
    }

    pub(super) fn publish_preparation(&self, id: u64) -> Result<(), BrokerError> {
        let _change = self.changes.notice();
        let mut inner = self.inner.lock();
        let enabled = inner.feature_enabled;
        let epoch = inner.feature_epoch;
        let (enabled, epoch) = inner.consent.publish_feature(id, enabled, epoch)?;
        inner.feature_enabled = enabled;
        inner.feature_epoch = epoch;
        Ok(())
    }

    fn abort_commit(
        &self,
        id: u64,
        error: BrokerError,
        fence_new_run: bool,
    ) -> Result<CommitOutcome, BrokerError> {
        let _change = self.changes.notice();
        let effect = {
            let mut inner = self.inner.lock();
            let enabled = inner.feature_enabled;
            let epoch = inner.feature_epoch;
            let effect = inner.consent.abort_preparation(id, enabled, epoch);
            if let Some((enabled, epoch)) = effect.feature {
                inner.feature_enabled = enabled;
                inner.feature_epoch = epoch;
            }
            effect
        };
        if fence_new_run && effect.opened_run {
            self.handoff_fences(std::slice::from_ref(&effect.run_id));
        }
        Err(error)
    }

    pub(super) fn bind_granted_target(
        &self,
        prep: &crate::task_consent::PrepareTicket,
        existed: bool,
    ) -> Result<AuthorizedTarget, BrokerError> {
        self.open_run(&prep.session_id, &prep.run_id)?;
        let after_open = self.inner.lock().consent.take_after_open();
        if let Some(hook) = after_open {
            hook();
        }
        if !existed {
            self.mark_consent_pending(&prep.run_id)?;
            self.inner.lock().consent.note_opened_run(prep.id);
            self.changes.publish();
        }
        let generation = surface_router::SurfaceRouter::authorize_prepared(self, prep)?;
        self.recheck_prepared_scope(prep)?;
        // Keep the exact prepared binding, rather than looking up whichever
        // binding happens to share this target id when the user commit arrives.
        let inner = self.inner.lock();
        if !inner
            .consent
            .preparation_matches(prep, inner.feature_enabled, inner.feature_epoch)
        {
            return Err(BrokerError::StopRequested);
        }
        let run = inner
            .runs
            .get(&prep.run_id)
            .ok_or(BrokerError::RunNotFound)?;
        let target = run.target.as_ref().ok_or(BrokerError::TargetUnauthorized)?;
        if run.stop != StopState::Running
            || target.target_id != prep.target_id
            || target.surface != prep.surface
            || target.target_generation != generation
        {
            return Err(BrokerError::IdentityMismatch("preparedTargetGeneration"));
        }
        Ok(target.clone())
    }

    fn mark_consent_pending(&self, run_id: &str) -> Result<(), BrokerError> {
        let _change = self.changes.notice();
        let mut inner = self.inner.lock();
        let run = inner.runs.get_mut(run_id).ok_or(BrokerError::RunNotFound)?;
        if !matches!(run.consent, ConsentGate::Bound(_)) {
            run.consent = ConsentGate::Pending;
        }
        Ok(())
    }

    pub(super) fn run_exists(&self, run_id: &str) -> bool {
        self.inner.lock().runs.contains_key(run_id)
    }

    pub(super) fn busy_run_ids(&self) -> HashSet<String> {
        self.refresh_unknown_activity();
        self.inner
            .lock()
            .runs
            .iter()
            .filter(|(_, run)| {
                run.in_flight.load(Ordering::SeqCst)
                    || run.stop_cleanup_pending
                    || super::activity::unknown_work_pending(run)
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub(super) fn handoff_expired(&self) {
        let runs = self.inner.lock().consent.take_expired_runs();
        self.handoff_fences(&runs);
    }

    pub(super) fn handoff_fences(&self, run_ids: &[String]) {
        let tickets = self.fence_ids(run_ids);
        let mut inner = self.inner.lock();
        for ticket in tickets {
            enqueue_cleanup(&mut inner, ticket);
        }
        requeue_pending_cleanups(&mut inner);
        self.sweep_existing_share_preparation(&mut inner);
    }

    fn fence_ids(&self, run_ids: &[String]) -> Vec<StopCleanupTicket> {
        run_ids
            .iter()
            .filter_map(|run_id| self.fence_stop(run_id).ok().flatten())
            .collect()
    }
}

const MAX_CLEANUP_TICKETS: usize = 32;

fn abort_locked(
    inner: &mut Inner,
    id: u64,
    error: BrokerError,
) -> (BrokerError, crate::task_consent::AbortEffect) {
    let enabled = inner.feature_enabled;
    let epoch = inner.feature_epoch;
    let effect = inner.consent.abort_preparation(id, enabled, epoch);
    if let Some((enabled, epoch)) = effect.feature {
        inner.feature_enabled = enabled;
        inner.feature_epoch = epoch;
    }
    (error, effect)
}

pub(super) fn runtime_retained(inner: &Inner) -> HashSet<String> {
    inner
        .runs
        .iter()
        .filter(|(_, run)| {
            run.in_flight.load(Ordering::SeqCst)
                || run.stop_cleanup_pending
                || super::activity::unknown_work_pending(run)
        })
        .map(|(_, run)| run.app_session_id.clone())
        .collect()
}

fn enqueue_cleanup(inner: &mut Inner, ticket: StopCleanupTicket) {
    if inner
        .cleanup_tickets
        .iter()
        .any(|row| row.run_id == ticket.run_id && row.generation == ticket.generation)
    {
        return;
    }
    if inner.cleanup_tickets.len() >= MAX_CLEANUP_TICKETS {
        return;
    }
    inner.cleanup_tickets.push(ticket);
}

fn requeue_pending_cleanups(inner: &mut Inner) {
    if inner.cleanup_tickets.len() >= MAX_CLEANUP_TICKETS {
        return;
    }
    let pending: Vec<StopCleanupTicket> = inner
        .runs
        .iter()
        .filter_map(|(id, run)| {
            if !run.stop_cleanup_pending || run.stop == StopState::Running {
                return None;
            }
            if inner
                .cleanup_tickets
                .iter()
                .any(|ticket| ticket.run_id == *id)
            {
                return None;
            }
            let incarnation = match &run.consent {
                ConsentGate::Bound(binding) => binding.incarnation,
                ConsentGate::Pending | ConsentGate::Legacy => 0,
            };
            Some(StopCleanupTicket {
                run_id: id.clone(),
                generation: run.generation,
                incarnation,
            })
        })
        .collect();
    for ticket in pending {
        enqueue_cleanup(inner, ticket);
    }
}

#[cfg(test)]
mod rework2_gap {
    //! The open gap is after the last preparation check and before open_run.
    //! The before_open hook runs before that check and is not this test.
    use super::*;
    use crate::fake::FakeAdapter;
    use crate::task_consent::CommitReady;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn codex_stop_in_feature_on_prepare_open_gap_fences_late_owned_run() {
        let adapter = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(
            adapter.clone(),
            BrokerOptions {
                feature_enabled: true,
                lease_path: std::env::temp_dir()
                    .join(format!("cu-u01r2-gap-{}.lock", uuid::Uuid::new_v4())),
                ..BrokerOptions::default()
            },
        );
        let token = broker.issue_transport_credential("review-app").unwrap();
        broker
            .bind_agent_session("review-app", "synthetic-review-agent")
            .unwrap();
        let pending = broker
            .submit_access(
                token.token(),
                json!({
                    "action": "request",
                    "purpose": "review synthetic fixture",
                    "surfaces": ["desktop"],
                    "scopeIntent": "A",
                    "capabilities": ["observe"]
                }),
            )
            .unwrap();
        let request_id = pending["requestId"].as_str().unwrap();
        broker
            .bind_synthetic_scope(request_id, &adapter.fixture_id())
            .unwrap();
        let ctx = broker.live_context(token.token()).unwrap();
        let prep = {
            let mut inner = broker.inner.lock();
            let enabled = inner.feature_enabled;
            let epoch = inner.feature_epoch;
            match inner
                .consent
                .start_preparation(&ctx, request_id, 1, enabled, epoch)
                .unwrap()
            {
                CommitReady::Prepare(prep) => prep,
                CommitReady::Already(_) | CommitReady::WebViewSelection(_) => {
                    panic!("first approval must prepare")
                }
            }
        };
        broker.publish_preparation(prep.id).unwrap();
        assert!(broker.preparation_current(prep.id));
        let original_tickets = broker.consent_stop("review-app");
        assert!(
            original_tickets.is_empty(),
            "there was no execution run before the gap"
        );
        assert!(matches!(
            broker.bind_granted_target(&prep, false),
            Err(BrokerError::StopRequested)
        ));
        assert_eq!(
            adapter.list_calls(),
            0,
            "stopped reservation reached discovery"
        );
        assert!(!broker.preparation_current(prep.id));
        assert!(broker
            .abort_commit(prep.id, BrokerError::StopRequested, true)
            .is_err());
        let state = broker.stop_state(&prep.run_id).unwrap();
        assert_ne!(
            state,
            StopState::Running,
            "original preparation was removed before its late candidate could be fenced"
        );
        let tickets = broker.take_cleanup_tickets();
        for ticket in tickets {
            broker.finish_stop_cleanup(&ticket).unwrap();
        }
        assert_eq!(broker.executable_grant_count("review-app"), 0);
        assert_eq!(broker.continuation_count("review-app"), 0);
        assert_eq!(adapter.observe_calls(), 0);
        assert!(adapter.executions().is_empty());
    }

    #[test]
    fn codex_stop_in_feature_off_prepare_open_gap_rolls_back_only_this_enable() {
        let adapter = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(
            adapter.clone(),
            BrokerOptions {
                feature_enabled: false,
                lease_path: std::env::temp_dir()
                    .join(format!("cu-u01r2-gap-off-{}.lock", uuid::Uuid::new_v4())),
                ..BrokerOptions::default()
            },
        );
        let epoch = broker.feature_epoch();
        let token = broker.issue_transport_credential("review-app").unwrap();
        broker
            .bind_agent_session("review-app", "synthetic-review-agent")
            .unwrap();
        let pending = broker
            .submit_access(
                token.token(),
                json!({
                    "action": "request",
                    "purpose": "review synthetic fixture",
                    "surfaces": ["desktop"],
                    "scopeIntent": "A",
                    "capabilities": ["observe"]
                }),
            )
            .unwrap();
        let request_id = pending["requestId"].as_str().unwrap();
        broker
            .bind_synthetic_scope(request_id, &adapter.fixture_id())
            .unwrap();
        let ctx = broker.live_context(token.token()).unwrap();
        let prep = {
            let mut inner = broker.inner.lock();
            let enabled = inner.feature_enabled;
            let epoch = inner.feature_epoch;
            match inner
                .consent
                .start_preparation(&ctx, request_id, 1, enabled, epoch)
                .unwrap()
            {
                CommitReady::Prepare(prep) => prep,
                CommitReady::Already(_) | CommitReady::WebViewSelection(_) => {
                    panic!("first approval must prepare")
                }
            }
        };
        broker.publish_preparation(prep.id).unwrap();
        assert!(broker.feature_enabled());
        assert!(broker.preparation_current(prep.id));
        let original_tickets = broker.consent_stop("review-app");
        assert!(
            original_tickets.is_empty(),
            "there was no execution run before the gap"
        );
        assert!(matches!(
            broker.bind_granted_target(&prep, false),
            Err(BrokerError::StopRequested)
        ));
        assert_eq!(
            adapter.list_calls(),
            0,
            "stopped reservation reached discovery"
        );
        assert!(!broker.preparation_current(prep.id));
        assert!(broker
            .abort_commit(prep.id, BrokerError::StopRequested, true)
            .is_err());
        assert!(
            !broker.feature_enabled(),
            "this approval's feature enable survived the abort"
        );
        assert!(
            broker.feature_epoch() > epoch,
            "abort rewrote the epoch backwards"
        );
        let state = broker.stop_state(&prep.run_id).unwrap();
        assert_ne!(state, StopState::Running);
        let tickets = broker.take_cleanup_tickets();
        assert!(!tickets.is_empty());
        for ticket in tickets {
            broker.finish_stop_cleanup(&ticket).unwrap();
        }
        assert_eq!(broker.executable_grant_count("review-app"), 0);
        assert_eq!(broker.continuation_count("review-app"), 0);
        assert_eq!(adapter.observe_calls(), 0);
        assert!(adapter.executions().is_empty());
        let again = broker
            .submit_access(
                token.token(),
                json!({
                    "action": "request",
                    "purpose": "review synthetic fixture",
                    "surfaces": ["desktop"],
                    "scopeIntent": "retry",
                    "capabilities": ["observe"]
                }),
            )
            .unwrap();
        assert_eq!(again["state"], "pending");
        assert_ne!(again["requestId"].as_str().unwrap(), request_id);
    }
}
