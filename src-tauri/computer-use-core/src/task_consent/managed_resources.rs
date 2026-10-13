//! Resource reservation precedes an actual tab, never execution authority.
use super::prepare::PrepareParts;
use super::types::{fresh_id, schema, ManagedResourcePolicy, OpaqueIdentity, PrepPhase};
use super::{
    ConsentState, ManagedResourceReady, ManagedResourceTicket, ScopeMember, TransportContext,
};
use crate::adapter::{NativeScopeIdentity, SurfaceKind};
use crate::error::BrokerError;

impl ConsentState {
    pub(crate) fn start_managed_resources(
        &mut self,
        ctx: &TransportContext,
        request_id: &str,
        revision: u64,
        enabled: bool,
        epoch: u64,
    ) -> Result<ManagedResourceReady, BrokerError> {
        self.require_model_owner(ctx)?;
        if let Some(outcome) = self.already_committed(ctx, request_id, revision) {
            let managed = self.grants.iter().any(|grant| {
                grant.request_id == request_id && grant.revision == revision && grant.members.len() == 1
                    && matches!(grant.members[0].native.as_ref().map(|proof| &proof.identity),
                        Some(NativeScopeIdentity::ManagedPage { identity }) if identity.owner == outcome.run_id)
            });
            return if managed {
                Ok(ManagedResourceReady::Already(outcome))
            } else {
                Err(schema("committed scope differs from selection"))
            };
        }
        // Check capacity before changing the current picker/selection identity.
        self.active_preparation_room(&ctx.app_session_id)?;
        let permit = self.begin_scope_selection(
            ctx,
            request_id,
            revision,
            SurfaceKind::ManagedBrowser,
            epoch,
        )?;
        let request = self
            .requests
            .iter()
            .find(|row| row.request_id == request_id)
            .ok_or_else(|| schema("scope selection retired"))?;
        let task_id = request.task_id.clone();
        let owner = self
            .owners
            .get(&ctx.app_session_id)
            .ok_or(BrokerError::IdentityMismatch("owner"))?;
        let profile = format!("task-{}", fresh_id());
        let policy = ManagedResourcePolicy {
            selection_revision: permit.selection_revision,
            profile: profile.clone(),
        };
        let built = PrepareParts {
            target_id: String::new(),
            surface: SurfaceKind::ManagedBrowser,
            native: None,
            members: Vec::new(),
            task_id: task_id.clone(),
            stop_generation: owner.stop_generation,
            prepare_cancel: owner.prepare_cancel,
            managed: Some(policy),
        };
        let id = self.insert_preparation(ctx, request_id, revision, (enabled, epoch), built)?;
        let prep = self
            .preparations
            .iter()
            .find(|prep| prep.id == id)
            .ok_or(BrokerError::StopRequested)?;
        Ok(ManagedResourceReady::Prepare(ManagedResourceTicket {
            id,
            session_id: ctx.app_session_id.clone(),
            run_id: prep.run_id.clone(),
            request_id: request_id.into(),
            revision,
            task_id,
            selection_revision: permit.selection_revision,
            profile,
        }))
    }

    pub(crate) fn managed_resource_current(
        &self,
        ticket: &ManagedResourceTicket,
        enabled: bool,
        epoch: u64,
    ) -> bool {
        self.preparation_current(ticket.id, enabled, epoch)
            && self.preparations.iter().any(|prep| {
                prep.id == ticket.id
                    && prep.phase == PrepPhase::Resources
                    && prep.app_session_id == ticket.session_id
                    && prep.run_id == ticket.run_id
                    && prep.request_id == ticket.request_id
                    && prep.revision == ticket.revision
                    && prep.task_id == ticket.task_id
                    && prep.native.is_none()
                    && prep.members.is_empty()
                    && prep.target_id.is_empty()
                    && prep.managed.as_ref().is_some_and(|policy| {
                        policy.profile == ticket.profile
                            && policy.selection_revision == ticket.selection_revision
                    })
            })
    }

    pub(crate) fn bind_managed_resource_scope(
        &mut self,
        ticket: &ManagedResourceTicket,
        member: ScopeMember,
        enabled: bool,
        epoch: u64,
    ) -> Result<super::PrepareTicket, BrokerError> {
        if !self.managed_resource_current(ticket, enabled, epoch) {
            return Err(BrokerError::StopRequested);
        }
        let proof = member
            .native
            .as_ref()
            .ok_or_else(|| schema("scope identity unavailable"))?;
        if member.surface != SurfaceKind::ManagedBrowser
            || proof.identity.target_id().as_deref() != Some(&member.target_id)
            || !matches!(&proof.identity, NativeScopeIdentity::ManagedPage { identity }
                if identity.owner == ticket.run_id && identity.profile == ticket.profile)
        {
            return Err(BrokerError::IdentityMismatch("managed reservation scope"));
        }
        let request = self
            .requests
            .iter_mut()
            .find(|row| row.request_id == ticket.request_id)
            .ok_or(BrokerError::StopRequested)?;
        request.bound = Some(vec![OpaqueIdentity {
            target_id: member.target_id.clone(),
            surface: member.surface,
            display_name: member.display_name.clone(),
            synthetic: false,
            native: member.native.clone(),
        }]);
        request.selecting = false;
        let prep = self
            .preparations
            .iter_mut()
            .find(|prep| prep.id == ticket.id)
            .ok_or(BrokerError::StopRequested)?;
        prep.target_id = member.target_id.clone();
        prep.native = member.native.clone();
        prep.members = vec![member];
        prep.phase = PrepPhase::Binding;
        self.bound_preparation_ticket(ticket.id)
    }
}
