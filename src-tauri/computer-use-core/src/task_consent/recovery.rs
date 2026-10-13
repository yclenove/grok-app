//! Host recovery checks suspended approval without admitting a model call.
use super::types::{
    fresh_id, schema, Capability, RequestState, Ticket, GRANT_IDLE_TTL, MAX_GRANTS_GLOBAL,
};
use super::{ConsentBinding, ConsentState, ScopeMember, TransportContext};
use crate::error::BrokerError;

impl ConsentState {
    pub(crate) fn suspended_scope(
        &self,
        ctx: &TransportContext,
        binding: &ConsentBinding,
        enabled: bool,
        epoch: u64,
    ) -> Result<Vec<ScopeMember>, BrokerError> {
        self.require_model_owner(ctx)?;
        if !enabled || epoch != binding.feature_epoch {
            return Err(BrokerError::FeatureDisabled);
        }
        if self.owners[&ctx.app_session_id].task_id.as_deref() != Some(&binding.task_id) {
            return Err(BrokerError::IdentityMismatch("task"));
        }
        if !self.requests.iter().any(|request| {
            request.app_session_id == ctx.app_session_id
                && request.owner_nonce == ctx.owner_nonce
                && request.request_id == binding.request_id
                && request.task_id == binding.task_id
                && request.revision == binding.grant_revision
                && request.state == RequestState::Approved
        }) {
            return Err(BrokerError::TargetUnauthorized);
        }
        let grant = self
            .grants
            .iter()
            .find(|grant| {
                grant.app_session_id == ctx.app_session_id
                    && grant.owner_nonce == ctx.owner_nonce
                    && grant.incarnation == ctx.incarnation
                    && grant.connection_generation == ctx.connection_generation
                    && ctx.agent_session_id.as_deref() == Some(&grant.agent_session_id)
                    && grant.app_session_id == binding.app_session_id
                    && grant.owner_nonce == binding.owner_nonce
                    && grant.incarnation == binding.incarnation
                    && grant.connection_generation == binding.connection_generation
                    && grant.agent_session_id == binding.agent_session_id
                    && grant.task_id == binding.task_id
                    && grant.request_id == binding.request_id
                    && grant.revision == binding.grant_revision
                    && grant.feature_epoch == epoch
                    && grant.execution_run_id.as_deref() == Some(&binding.run_id)
                    && grant.active_target_id == binding.target_id
                    && grant.suspended
            })
            .ok_or(BrokerError::TargetUnauthorized)?;
        // Resume requires a fresh model observation; input-only approval cannot
        // silently gain read permission. Native witnesses remain mandatory.
        if !grant.capabilities.contains(&Capability::Observe)
            || grant.members.is_empty()
            || grant.members.iter().any(|member| member.native.is_none())
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        Ok(grant.members.clone())
    }

    /// Called only by the final Broker transaction after native revalidation
    /// and fresh capture. Old tickets stay invalid, including consumed ones.
    pub(crate) fn publish_recovery(
        &mut self,
        ctx: &TransportContext,
        binding: &ConsentBinding,
        enabled: bool,
        epoch: u64,
    ) -> Result<String, BrokerError> {
        self.suspended_scope(ctx, binding, enabled, epoch)?;
        if self.grants.iter().filter(|grant| !grant.suspended).count() >= MAX_GRANTS_GLOBAL {
            return Err(schema("request limit"));
        }
        let id = fresh_id();
        self.push_ticket(Ticket {
            id: id.clone(),
            dispatch_id: id.clone(),
            app_session_id: binding.app_session_id.clone(),
            agent_session_id: binding.agent_session_id.clone(),
            incarnation: binding.incarnation,
            connection_generation: binding.connection_generation,
            owner_nonce: binding.owner_nonce,
            request_id: binding.request_id.clone(),
            task_id: binding.task_id.clone(),
            grant_revision: binding.grant_revision,
            feature_epoch: epoch,
            consumed: false,
            invalid: false,
        })?;
        let deadline = self.clock.now().saturating_add(GRANT_IDLE_TTL);
        let grant = self
            .grants
            .iter_mut()
            .find(|grant| {
                grant.request_id == binding.request_id
                    && grant.owner_nonce == binding.owner_nonce
                    && grant.execution_run_id.as_deref() == Some(&binding.run_id)
            })
            .expect("suspended_scope checked under the same authority lock");
        grant.suspended = false;
        grant.idle_deadline = deadline;
        Ok(id)
    }
}
