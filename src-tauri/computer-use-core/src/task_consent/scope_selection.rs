//! A Host selection is tied to the exact pending request and picker generation.
use super::types::{schema, OpaqueIdentity, PrepPhase, RequestRecord, RequestState};
use super::{
    ConsentState, NativeScopeProof, ScopeMember, ScopeSelectionPermit, TransportContext,
    MAX_SCOPE_MEMBERS,
};
use crate::adapter::SurfaceKind;
use crate::error::BrokerError;

impl ConsentState {
    /// Connection metadata may bridge the original reservation before enable.
    /// This does not authorize new Share mutations, observation or execution.
    pub(crate) fn recheck_existing_pairing_preparation(
        &self,
        permit: &ScopeSelectionPermit,
        enabled: bool,
        epoch: u64,
    ) -> Result<(), BrokerError> {
        if permit.surface != SurfaceKind::ExistingTab {
            return Err(BrokerError::TargetUnauthorized);
        }
        if self.recheck_scope_selection(permit, epoch).is_ok() {
            return Ok(());
        }
        self.require_model_owner(&permit.context)?;
        let request = self
            .requests
            .iter()
            .find(|request| {
                request.request_id == permit.request_id
                    && request.revision == permit.revision
                    && request.selection_revision == permit.selection_revision
                    && request.app_session_id == permit.context.app_session_id
                    && request.owner_nonce == permit.context.owner_nonce
                    && request.incarnation == permit.context.incarnation
                    && request.connection_generation == permit.context.connection_generation
                    && request.state == RequestState::Approving
                    && self.clock.now() < request.deadline
            })
            .ok_or(BrokerError::StopRequested)?;
        if self.preparations.iter().any(|prep| {
            prep.request_id == request.request_id
                && prep.revision == request.revision
                && prep.app_session_id == request.app_session_id
                && prep.owner_nonce == permit.context.owner_nonce
                && prep.incarnation == permit.context.incarnation
                && prep.connection_generation == permit.context.connection_generation
                && prep.observed_epoch == permit.feature_epoch
                && prep.surface == SurfaceKind::ExistingTab
                && self.preparation_current(prep.id, enabled, epoch)
        }) {
            Ok(())
        } else {
            Err(BrokerError::StopRequested)
        }
    }

    pub(crate) fn scope_members_for_permit(
        &self,
        permit: &super::ExecutionPermit,
    ) -> Vec<ScopeMember> {
        self.grants
            .iter()
            .find(|grant| {
                grant.request_id == permit.request_id
                    && grant.revision == permit.grant_revision
                    && grant.execution_run_id.as_deref() == Some(&permit.run_id)
            })
            .map(|grant| grant.members.clone())
            .unwrap_or_default()
    }

    /// Fence only this picker generation. A late cancellation cannot retire a
    /// newer selection or a grant that already crossed the commit boundary.
    pub(crate) fn fence_scope_selection(&mut self, permit: &ScopeSelectionPermit) -> Vec<String> {
        if self.require_model_owner(&permit.context).is_err() {
            return Vec::new();
        }
        let Some(request) = self.requests.iter_mut().find(|row| {
            row.request_id == permit.request_id
                && row.revision == permit.revision
                && row.app_session_id == permit.context.app_session_id
                && row.owner_nonce == permit.context.owner_nonce
                && row.incarnation == permit.context.incarnation
                && row.connection_generation == permit.context.connection_generation
                && row.selection_revision == permit.selection_revision
                && row.state.is_pending()
        }) else {
            return Vec::new();
        };
        if let Some(next) = request.selection_revision.checked_add(1) {
            request.selection_revision = next;
        } else {
            request.state = RequestState::Cancelled;
        }
        request.bound = None;
        request.selecting = false;
        self.preparations
            .iter_mut()
            .filter_map(|prep| {
                if prep.request_id != permit.request_id || prep.revision != permit.revision {
                    return None;
                }
                prep.phase = PrepPhase::Aborted;
                prep.opened_run.then(|| prep.run_id.clone())
            })
            .collect()
    }

    pub(crate) fn committed_selection_outcome(
        &self,
        ctx: &TransportContext,
        request_id: &str,
        revision: u64,
        targets: &[(SurfaceKind, &str)],
    ) -> Result<Option<super::CommitOutcome>, BrokerError> {
        self.require_model_owner(ctx)?;
        let Some(outcome) = self.already_committed(ctx, request_id, revision) else {
            return Ok(None);
        };
        let grant = self
            .grants
            .iter()
            .find(|row| row.request_id == request_id && row.revision == revision)
            .ok_or_else(|| schema("scope selection retired"))?;
        if grant.selected_targets.len() != targets.len()
            || targets
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != targets.len()
            || targets.iter().any(|(surface, target)| {
                !grant
                    .selected_targets
                    .iter()
                    .any(|(selected_surface, selected_target)| {
                        selected_surface == surface && selected_target == target
                    })
            })
        {
            return Err(schema("committed scope differs from selection"));
        }
        Ok(Some(outcome))
    }

    fn selectable_request(
        &self,
        ctx: &TransportContext,
        request_id: &str,
        revision: u64,
        surface: SurfaceKind,
    ) -> Result<&RequestRecord, BrokerError> {
        self.require_model_owner(ctx)?;
        self.requests
            .iter()
            .find(|request| {
                request.request_id == request_id
                    && request.revision == revision
                    && request.app_session_id == ctx.app_session_id
                    && request.owner_nonce == ctx.owner_nonce
                    && request.incarnation == ctx.incarnation
                    && request.connection_generation == ctx.connection_generation
                    && request.state == RequestState::Awaiting
                    && self.clock.now() < request.deadline
                    && request.surfaces.contains(&surface)
            })
            .ok_or_else(|| schema("scope selection retired"))
    }

    pub(crate) fn check_selection_surface(
        &self,
        ctx: &TransportContext,
        request_id: &str,
        revision: u64,
        surface: SurfaceKind,
    ) -> Result<(), BrokerError> {
        self.selectable_request(ctx, request_id, revision, surface)
            .map(|_| ())
    }

    pub(crate) fn begin_scope_selection(
        &mut self,
        ctx: &TransportContext,
        request_id: &str,
        revision: u64,
        surface: SurfaceKind,
        feature_epoch: u64,
    ) -> Result<ScopeSelectionPermit, BrokerError> {
        let next = self
            .selectable_request(ctx, request_id, revision, surface)?
            .selection_revision
            .checked_add(1)
            .ok_or_else(|| schema("scope selection exhausted"))?;
        let request = self
            .requests
            .iter_mut()
            .find(|row| row.request_id == request_id)
            .ok_or_else(|| schema("scope selection retired"))?;
        request.selection_revision = next;
        request.bound = None;
        request.selecting = true;
        Ok(ScopeSelectionPermit {
            context: ctx.clone(),
            request_id: request_id.into(),
            revision,
            selection_revision: next,
            feature_epoch,
            surface,
        })
    }

    pub(crate) fn recheck_scope_selection(
        &self,
        permit: &ScopeSelectionPermit,
        feature_epoch: u64,
    ) -> Result<(), BrokerError> {
        let request = self.selectable_request(
            &permit.context,
            &permit.request_id,
            permit.revision,
            permit.surface,
        )?;
        if request.selection_revision != permit.selection_revision
            || feature_epoch != permit.feature_epoch
        {
            return Err(schema("scope selection retired"));
        }
        Ok(())
    }

    pub(crate) fn bind_host_scope_selection(
        &mut self,
        permit: &ScopeSelectionPermit,
        members: Vec<ScopeMember>,
        feature_epoch: u64,
    ) -> Result<(), BrokerError> {
        self.recheck_scope_selection(permit, feature_epoch)?;
        if !(1..=MAX_SCOPE_MEMBERS).contains(&members.len()) {
            return Err(schema("scope selection rejected"));
        }
        let request = self
            .requests
            .iter_mut()
            .find(|row| row.request_id == permit.request_id)
            .ok_or_else(|| schema("scope selection retired"))?;
        let mut bound = Vec::with_capacity(members.len());
        for member in members {
            let proof = member
                .native
                .ok_or_else(|| schema("scope identity unavailable"))?;
            if proof.identity.target_id().as_deref() != Some(&member.target_id)
                || !request.surfaces.contains(&member.surface)
                || bound
                    .iter()
                    .any(|previous: &OpaqueIdentity| previous.target_id == member.target_id)
            {
                return Err(schema("scope selection rejected"));
            }
            bound.push(OpaqueIdentity {
                target_id: member.target_id,
                surface: member.surface,
                display_name: member.display_name,
                synthetic: false,
                native: Some(proof),
            });
        }
        request.bound = Some(bound);
        request.selecting = false;
        Ok(())
    }

    pub(crate) fn scope_proof_for_permit(
        &self,
        permit: &super::ExecutionPermit,
    ) -> Option<NativeScopeProof> {
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
            .and_then(|member| member.native.clone())
    }

    pub(crate) fn scope_member_for_binding(
        &self,
        binding: &super::ConsentBinding,
        enabled: bool,
        epoch: u64,
    ) -> Result<ScopeMember, BrokerError> {
        self.recheck_binding(binding, enabled, epoch)?;
        self.grants
            .iter()
            .find(|grant| {
                grant.request_id == binding.request_id
                    && grant.revision == binding.grant_revision
                    && grant.execution_run_id.as_deref() == Some(&binding.run_id)
            })
            .and_then(|grant| {
                grant
                    .members
                    .iter()
                    .find(|member| member.target_id == binding.target_id)
            })
            .cloned()
            .ok_or(BrokerError::TargetUnauthorized)
    }
}
