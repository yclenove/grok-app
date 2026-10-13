//! Executable grants and idle expiry. Expiry hands the exact run back for fence.
use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::error::BrokerError;

use super::types::{
    binding_from_grant, schema, tool_need, Capability, ConsentBinding, ExecutionPermit,
    ExecutionTargetClaim, PrepPhase, RequestState, ScopeMemoryView, ToolNeed, TransportContext,
};
use super::ConsentState;

impl ConsentState {
    /// Inventory is metadata from committed read scopes, not target discovery.
    pub(crate) fn readable_bindings(
        &self,
        ctx: &TransportContext,
        feature_enabled: bool,
        feature_epoch: u64,
    ) -> Result<Vec<ConsentBinding>, BrokerError> {
        self.require_model_owner(ctx)?;
        if !feature_enabled {
            return Err(BrokerError::FeatureDisabled);
        }
        let agent = ctx
            .agent_session_id
            .as_deref()
            .ok_or(BrokerError::IdentityMismatch("agentSessionId"))?;
        let mut bindings = Vec::new();
        for grant in &self.grants {
            if !grant_matches(grant, ctx, agent, feature_epoch)
                || !self.grant_executable(grant)
                || !grant.capabilities.contains(&Capability::Observe)
            {
                continue;
            }
            if let Some(run_id) = &grant.execution_run_id {
                for member in &grant.members {
                    let mut binding = binding_from_grant(grant, run_id);
                    binding.target_id = member.target_id.clone();
                    bindings.push(binding);
                }
            }
        }
        if bindings.is_empty() {
            return Err(BrokerError::TargetUnauthorized);
        }
        Ok(bindings)
    }

    /// Share the broker admission flag so deadline checks use actual work,
    /// including the final admission and result publication under its lock.
    pub(crate) fn bind_physical_work(
        &mut self,
        run_id: &str,
        work: Arc<AtomicBool>,
    ) -> Option<Arc<super::activity::PhysicalActivity>> {
        for grant in &mut self.grants {
            if grant.execution_run_id.as_deref() == Some(run_id) {
                grant.physical_work = work.clone();
                return Some(grant.completion.clone());
            }
        }
        None
    }

    pub(super) fn grant_executable(&self, grant: &super::types::GrantRecord) -> bool {
        !grant.suspended
            && (self.clock.now() < grant.idle_deadline.max(grant.completion.deadline())
                || grant.physical_work.load(Ordering::SeqCst))
    }

    pub fn begin_model_call(
        &mut self,
        ctx: &TransportContext,
        tool: &str,
        feature_enabled: bool,
        feature_epoch: u64,
        busy: &HashSet<String>,
        claim: ExecutionTargetClaim<'_>,
    ) -> Result<ExecutionPermit, BrokerError> {
        self.sweep(busy);
        let claimed_run = claim.run_id;
        let claimed_target = claim.target_id;
        self.require_model_owner(ctx)?;
        if !feature_enabled {
            return Err(BrokerError::FeatureDisabled);
        }
        let (required, capability) = match tool_need(tool) {
            ToolNeed::Read => (Some(Capability::Observe), "observe"),
            ToolNeed::Write => (Some(Capability::Input), "input"),
            ToolNeed::Stop => (None, "stop"),
            ToolNeed::Access | ToolNeed::Unknown => return Err(schema("tool is unavailable")),
        };
        let agent = ctx
            .agent_session_id
            .as_deref()
            .ok_or(BrokerError::IdentityMismatch("agentSessionId"))?;
        let mut saw_grant = false;
        let mut matched: Vec<&super::types::GrantRecord> = Vec::new();
        for grant in &self.grants {
            if !grant_matches(grant, ctx, agent, feature_epoch) || !self.grant_executable(grant) {
                continue;
            }
            saw_grant = true;
            if required.is_some_and(|cap| !grant.capabilities.contains(&cap)) {
                continue;
            }
            if claimed_run.is_some_and(|run| grant.execution_run_id.as_deref() != Some(run)) {
                continue;
            }
            if claimed_target.is_some_and(|target| {
                !grant
                    .members
                    .iter()
                    .any(|member| member.target_id == target)
            }) {
                continue;
            }
            matched.push(grant);
        }
        if matched.len() != 1 {
            if matched.is_empty() && saw_grant && claimed_run.is_none() && claimed_target.is_none()
            {
                return Err(schema("capability does not allow this tool"));
            }
            // More than one member matches, or a claimed run/target misses.
            // Do not guess the focused target.
            return Err(BrokerError::TargetUnauthorized);
        }
        let grant = matched[0];
        let run_id = grant
            .execution_run_id
            .clone()
            .ok_or(BrokerError::TargetUnauthorized)?;
        let target_id = grant
            .members
            .iter()
            .find(|member| member.target_id == claimed_target.unwrap_or(&grant.active_target_id))
            .map(|member| member.target_id.clone())
            .ok_or(BrokerError::TargetUnauthorized)?;
        Ok(ExecutionPermit {
            session_id: grant.app_session_id.clone(),
            run_id,
            task_id: grant.task_id.clone(),
            request_id: grant.request_id.clone(),
            grant_revision: grant.revision,
            feature_epoch: grant.feature_epoch,
            incarnation: grant.incarnation,
            connection_generation: grant.connection_generation,
            owner_nonce: grant.owner_nonce,
            agent_session_id: grant.agent_session_id.clone(),
            target_id,
            execution_generation: 0,
            capability,
        })
    }

    pub fn recheck_permit(
        &self,
        permit: &ExecutionPermit,
        feature_enabled: bool,
        feature_epoch: u64,
    ) -> Result<(), BrokerError> {
        if !feature_enabled || feature_epoch != permit.feature_epoch {
            return Err(BrokerError::FeatureDisabled);
        }
        let owner = self
            .owners
            .get(&permit.session_id)
            .ok_or(BrokerError::IdentityMismatch("owner"))?;
        if owner.retired
            || owner.nonce != permit.owner_nonce
            || owner.incarnation != permit.incarnation
            || owner.connection_generation != permit.connection_generation
            || owner.agent_session_id.as_deref() != Some(permit.agent_session_id.as_str())
        {
            return Err(BrokerError::IdentityMismatch("generation"));
        }
        let live = self.grants.iter().any(|grant| {
            grant.app_session_id == permit.session_id
                && grant.request_id == permit.request_id
                && grant.task_id == permit.task_id
                && grant.revision == permit.grant_revision
                && grant.feature_epoch == permit.feature_epoch
                && grant.owner_nonce == permit.owner_nonce
                && grant.execution_run_id.as_deref() == Some(permit.run_id.as_str())
                && grant.agent_session_id == permit.agent_session_id
                && grant
                    .members
                    .iter()
                    .any(|member| member.target_id == permit.target_id)
                && capability_ok(grant, permit.capability)
                && self.grant_executable(grant)
        });
        if live {
            Ok(())
        } else {
            Err(BrokerError::TargetUnauthorized)
        }
    }

    pub(crate) fn scope_member_for_permit(
        &self,
        permit: &ExecutionPermit,
    ) -> Option<super::ScopeMember> {
        self.grants
            .iter()
            .find(|grant| {
                grant.request_id == permit.request_id
                    && grant.revision == permit.grant_revision
                    && grant.execution_run_id.as_deref() == Some(&permit.run_id)
            })
            .and_then(|grant| {
                grant
                    .members
                    .iter()
                    .find(|member| member.target_id == permit.target_id)
            })
            .cloned()
    }

    /// Caller holds the Broker authority lock after checking the exact run,
    /// cleanup terminal state, and the member witness outside that lock.
    pub(crate) fn select_active_member(
        &mut self,
        permit: &ExecutionPermit,
    ) -> Result<ConsentBinding, BrokerError> {
        let grant = self
            .grants
            .iter_mut()
            .find(|grant| {
                grant.request_id == permit.request_id
                    && grant.revision == permit.grant_revision
                    && grant.execution_run_id.as_deref() == Some(&permit.run_id)
            })
            .ok_or(BrokerError::TargetUnauthorized)?;
        if !grant
            .members
            .iter()
            .any(|member| member.target_id == permit.target_id)
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        grant.active_target_id = permit.target_id.clone();
        Ok(binding_from_grant(grant, &permit.run_id))
    }

    /// Replace exactly one active execution member after a document-scoped
    /// navigation. The original picker decision remains immutable; only the
    /// retained execution witness advances to the successor document.
    pub(crate) fn replace_active_member(
        &mut self,
        binding: &ConsentBinding,
        previous: &super::ScopeMember,
        successor: super::ScopeMember,
        feature_enabled: bool,
        feature_epoch: u64,
    ) -> Result<ConsentBinding, BrokerError> {
        self.recheck_binding(binding, feature_enabled, feature_epoch)?;
        if previous.target_id != binding.target_id
            || previous.surface != successor.surface
            || successor.target_id.is_empty()
            || successor
                .native
                .as_ref()
                .and_then(|proof| proof.identity.target_id())
                != Some(successor.target_id.clone())
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        let grant = self
            .grants
            .iter_mut()
            .find(|grant| {
                grant.app_session_id == binding.app_session_id
                    && grant.agent_session_id == binding.agent_session_id
                    && grant.owner_nonce == binding.owner_nonce
                    && grant.incarnation == binding.incarnation
                    && grant.connection_generation == binding.connection_generation
                    && grant.task_id == binding.task_id
                    && grant.request_id == binding.request_id
                    && grant.revision == binding.grant_revision
                    && grant.feature_epoch == binding.feature_epoch
                    && grant.execution_run_id.as_deref() == Some(binding.run_id.as_str())
                    && grant.active_target_id == binding.target_id
            })
            .ok_or(BrokerError::TargetUnauthorized)?;
        let mut matched = grant
            .members
            .iter()
            .enumerate()
            .filter(|(_, member)| *member == previous);
        let index = matched
            .next()
            .map(|(index, _)| index)
            .ok_or(BrokerError::TargetUnauthorized)?;
        if matched.next().is_some()
            || grant
                .members
                .iter()
                .enumerate()
                .any(|(other, member)| other != index && member.target_id == successor.target_id)
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        grant.members[index] = successor;
        grant.active_target_id = grant.members[index].target_id.clone();
        Ok(binding_from_grant(grant, &binding.run_id))
    }

    /// Physical completion of admitted model work. A replacement, stop, or
    /// feature change does not move this grant, and does not restore it.
    pub fn note_model_activity(
        &mut self,
        binding: &ConsentBinding,
        feature_enabled: bool,
        feature_epoch: u64,
    ) {
        if !feature_enabled || feature_epoch != binding.feature_epoch {
            return;
        }
        let Some(owner) = self.owners.get(&binding.app_session_id) else {
            return;
        };
        if owner.retired
            || owner.nonce != binding.owner_nonce
            || owner.incarnation != binding.incarnation
            || owner.connection_generation != binding.connection_generation
            || owner.agent_session_id.as_deref() != Some(binding.agent_session_id.as_str())
        {
            return;
        }
        let now = self.clock.now();
        for grant in &mut self.grants {
            if grant.app_session_id == binding.app_session_id
                && grant.request_id == binding.request_id
                && grant.task_id == binding.task_id
                && grant.revision == binding.grant_revision
                && grant.feature_epoch == binding.feature_epoch
                && grant.owner_nonce == binding.owner_nonce
                && grant.execution_run_id.as_deref() == Some(binding.run_id.as_str())
                && grant.active_target_id == binding.target_id
                && grant
                    .members
                    .iter()
                    .any(|member| member.target_id == binding.target_id)
                && !grant.suspended
            {
                grant.idle_deadline = now.saturating_add(super::types::GRANT_IDLE_TTL);
            }
        }
    }

    pub fn recheck_binding(
        &self,
        binding: &ConsentBinding,
        feature_enabled: bool,
        feature_epoch: u64,
    ) -> Result<(), BrokerError> {
        self.recheck_binding_state(binding, feature_enabled, feature_epoch, false)
    }

    /// A Host pause only removes authority. Its original retained grant may
    /// already be suspended, but every owner/scope identity must still match.
    pub(crate) fn recheck_pause_binding(
        &self,
        binding: &ConsentBinding,
        feature_enabled: bool,
        feature_epoch: u64,
    ) -> Result<(), BrokerError> {
        self.recheck_binding_state(binding, feature_enabled, feature_epoch, true)
    }

    fn recheck_binding_state(
        &self,
        binding: &ConsentBinding,
        feature_enabled: bool,
        feature_epoch: u64,
        allow_suspended: bool,
    ) -> Result<(), BrokerError> {
        if !feature_enabled || feature_epoch != binding.feature_epoch {
            return Err(BrokerError::FeatureDisabled);
        }
        let owner = self
            .owners
            .get(&binding.app_session_id)
            .ok_or(BrokerError::IdentityMismatch("owner"))?;
        if owner.retired
            || owner.nonce != binding.owner_nonce
            || owner.incarnation != binding.incarnation
            || owner.connection_generation != binding.connection_generation
            || owner.agent_session_id.as_deref() != Some(binding.agent_session_id.as_str())
        {
            return Err(BrokerError::IdentityMismatch("generation"));
        }
        let live = self.grants.iter().any(|grant| {
            grant.app_session_id == binding.app_session_id
                && grant.request_id == binding.request_id
                && grant.task_id == binding.task_id
                && grant.revision == binding.grant_revision
                && grant.feature_epoch == binding.feature_epoch
                && grant.owner_nonce == binding.owner_nonce
                && grant.execution_run_id.as_deref() == Some(binding.run_id.as_str())
                && grant.active_target_id == binding.target_id
                && grant.agent_session_id == binding.agent_session_id
                && grant
                    .members
                    .iter()
                    .any(|member| member.target_id == binding.target_id)
                && (self.grant_executable(grant) || allow_suspended && grant.suspended)
        });
        if live {
            Ok(())
        } else {
            Err(BrokerError::TargetUnauthorized)
        }
    }

    pub fn binding_capability(&self, binding: &ConsentBinding, capability: &str) -> bool {
        self.grants.iter().any(|grant| {
            grant.app_session_id == binding.app_session_id
                && grant.request_id == binding.request_id
                && grant.task_id == binding.task_id
                && grant.revision == binding.grant_revision
                && grant.feature_epoch == binding.feature_epoch
                && grant.owner_nonce == binding.owner_nonce
                && grant.execution_run_id.as_deref() == Some(binding.run_id.as_str())
                && grant.active_target_id == binding.target_id
                && grant
                    .members
                    .iter()
                    .any(|member| member.target_id == binding.target_id)
                && capability_ok(grant, capability)
                && self.grant_executable(grant)
        })
    }

    pub fn grant_count(&self, app_session_id: &str) -> usize {
        self.grants
            .iter()
            .filter(|grant| grant.app_session_id == app_session_id && self.grant_executable(grant))
            .count()
    }

    pub fn scope_memories(&self, app_session_id: &str) -> Vec<ScopeMemoryView> {
        self.scopes
            .iter()
            .filter(|row| row.app_session_id == app_session_id)
            .map(|row| ScopeMemoryView {
                purpose: row.purpose.clone(),
                surfaces: row
                    .surfaces
                    .iter()
                    .copied()
                    .map(crate::adapter::SurfaceKind::as_wire)
                    .map(str::to_string)
                    .collect(),
                scope_intent: row.scope_intent.clone(),
            })
            .collect()
    }

    pub fn suspend_run(&mut self, run_id: &str) {
        let mut suspended = Vec::new();
        for grant in &mut self.grants {
            if grant.execution_run_id.as_deref() == Some(run_id) {
                grant.suspended = true;
                // Retain cleanup correlation, never execution authority. Stop,
                // completion and retirement must still find the original run.
                suspended.push((grant.app_session_id.clone(), grant.request_id.clone()));
            }
        }
        for (session, request_id) in suspended {
            self.invalidate_tickets(&session, Some(&request_id));
        }
    }

    /// Drop the grant for this run only. The caller fences the run; this does not stop the owner.
    pub fn revoke_run(&mut self, run_id: &str) {
        let dropped: Vec<(String, String, u64)> = self
            .grants
            .iter()
            .filter(|grant| grant.execution_run_id.as_deref() == Some(run_id))
            .map(|grant| {
                (
                    grant.app_session_id.clone(),
                    grant.request_id.clone(),
                    grant.owner_nonce,
                )
            })
            .collect();
        if dropped.is_empty() {
            return;
        }
        self.grants
            .retain(|grant| grant.execution_run_id.as_deref() != Some(run_id));
        for (session, request_id, owner_nonce) in dropped {
            self.invalidate_tickets(&session, Some(&request_id));
            if let Some(request) = self
                .requests
                .iter_mut()
                .find(|row| row.request_id == request_id && row.state == RequestState::Approved)
            {
                request.state = RequestState::Cancelled;
                request.continuation_issued = false;
            }
            self.push_notice(self.notice_for(&session, owner_nonce, &request_id, "stopped"));
        }
    }

    pub fn complete_task(&mut self, app_session_id: &str) -> Vec<String> {
        let Some(task_id) = self
            .owners
            .get(app_session_id)
            .and_then(|owner| owner.task_id.clone())
        else {
            return Vec::new();
        };
        let runs = self.owned_run_ids(app_session_id);
        self.cancel_authority(app_session_id, "completed");
        for request in &mut self.requests {
            if request.app_session_id == app_session_id && request.task_id == task_id {
                request.state = RequestState::Completed;
                request.continuation_issued = false;
            }
        }
        if let Some(owner) = self.owners.get_mut(app_session_id) {
            owner.task_id = None;
            owner.prepare_cancel = owner.prepare_cancel.saturating_add(1);
        }
        runs
    }

    pub(super) fn sweep(&mut self, busy: &HashSet<String>) {
        let now = self.clock.now();
        let mut expired_requests = Vec::new();
        for request in &mut self.requests {
            if request.state.is_pending() && now >= request.deadline {
                request.state = RequestState::Expired;
                request.continuation_issued = false;
                expired_requests.push((
                    request.app_session_id.clone(),
                    request.request_id.clone(),
                    request.owner_nonce,
                ));
            }
        }
        for (session, request_id, owner_nonce) in expired_requests {
            self.invalidate_tickets(&session, Some(&request_id));
            self.push_notice(self.notice_for(&session, owner_nonce, &request_id, "expired"));
        }
        // Preparations have physical owners before they have a grant. Preserve
        // the opaque handle for rollback/join, but hand off their fences even
        // while an original open/query is still pending. Revisit every sweep
        // so a full cleanup queue never loses an unfinished obligation.
        let retired_preparations: Vec<String> = self
            .preparations
            .iter_mut()
            .filter_map(|prep| {
                let valid_request = self.requests.iter().any(|request| {
                    request.request_id == prep.request_id
                        && request.revision == prep.revision
                        && request.state == RequestState::Approving
                        && now < request.deadline
                });
                if !prep.rolled_back
                    && prep.opened_run
                    && (!valid_request || prep.phase == PrepPhase::Aborted)
                {
                    prep.phase = PrepPhase::Aborted;
                    Some(prep.run_id.clone())
                } else {
                    None
                }
            })
            .collect();
        for run in retired_preparations {
            self.queue_expired_run(&run);
        }
        let expired_grants: Vec<(String, String, String, u64, String)> = self
            .grants
            .iter()
            .filter(|grant| {
                let Some(run_id) = grant.execution_run_id.as_deref() else {
                    return false;
                };
                !grant.suspended
                    && !busy.contains(run_id)
                    && !grant.physical_work.load(Ordering::SeqCst)
                    && now >= grant.idle_deadline.max(grant.completion.deadline())
            })
            .map(|grant| {
                (
                    grant.app_session_id.clone(),
                    grant.request_id.clone(),
                    grant.task_id.clone(),
                    grant.owner_nonce,
                    grant.execution_run_id.clone().unwrap_or_default(),
                )
            })
            .collect();
        if !expired_grants.is_empty() {
            let mut queued = Vec::new();
            for row in expired_grants {
                if !self.queue_expired_run(&row.4) {
                    // Keep the grant. Dropping it without a fence would lose the run.
                    continue;
                }
                queued.push(row);
            }
            let expired_requests: HashSet<String> = queued
                .iter()
                .map(|(_, request_id, _, _, _)| request_id.clone())
                .collect();
            if !expired_requests.is_empty() {
                self.grants.retain(|grant| {
                    !expired_requests.contains(&grant.request_id) || grant.suspended
                });
            }
            for (session, request_id, task_id, owner_nonce, _) in queued {
                self.invalidate_task(&session, &task_id);
                self.invalidate_tickets(&session, Some(&request_id));
                if let Some(request) = self
                    .requests
                    .iter_mut()
                    .find(|row| row.request_id == request_id)
                {
                    if request.state == RequestState::Approved {
                        request.state = RequestState::Expired;
                        request.continuation_issued = false;
                    }
                }
                self.push_notice(self.notice_for(&session, owner_nonce, &request_id, "expired"));
            }
        }
        self.trim_stored_requests();
    }
}

fn capability_ok(grant: &super::types::GrantRecord, capability: &str) -> bool {
    match capability {
        "observe" => grant.capabilities.contains(&Capability::Observe),
        "input" => grant.capabilities.contains(&Capability::Input),
        "stop" => !grant.capabilities.is_empty(),
        _ => false,
    }
}

fn grant_matches(
    grant: &super::types::GrantRecord,
    ctx: &TransportContext,
    agent: &str,
    feature_epoch: u64,
) -> bool {
    grant.app_session_id == ctx.app_session_id
        && grant.owner_nonce == ctx.owner_nonce
        && grant.incarnation == ctx.incarnation
        && grant.connection_generation == ctx.connection_generation
        && grant.agent_session_id == agent
        && !grant.suspended
        && grant.feature_epoch == feature_epoch
        && grant.execution_run_id.is_some()
}
