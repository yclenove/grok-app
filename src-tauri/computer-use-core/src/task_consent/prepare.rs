//! Two-phase commit. Approved, the grant, and the ticket appear only in finalize.
use crate::error::BrokerError;

use super::types::{
    binding_from_grant, fresh_id, schema, AbortEffect, CommitOutcome, CommitReady, GrantRecord,
    PrepPhase, Preparation, PrepareTicket, RequestState, ScopeMember, ScopeMemory, Ticket,
    TransportContext, GRANT_IDLE_TTL, MAX_GRANTS_GLOBAL,
};
use super::ConsentState;

impl ConsentState {
    pub(crate) fn supersede_preparations_for_explicit_enable(&mut self) {
        for prep in &mut self.preparations {
            prep.phase = PrepPhase::Aborted;
            // The explicit user choice owns the enabled preference now. An
            // older first-use transaction must not roll that choice back.
            prep.we_enabled = false;
        }
    }

    pub(crate) fn fence_preparation(&mut self, id: u64) -> Option<String> {
        let prep = self.preparations.iter_mut().find(|prep| prep.id == id)?;
        prep.phase = PrepPhase::Aborted;
        prep.opened_run.then(|| prep.run_id.clone())
    }

    pub fn start_preparation(
        &mut self,
        ctx: &TransportContext,
        request_id: &str,
        revision: u64,
        feature_enabled: bool,
        feature_epoch: u64,
    ) -> Result<CommitReady, BrokerError> {
        self.require_model_owner(ctx)?;
        if let Some(outcome) = self.already_committed(ctx, request_id, revision) {
            return Ok(CommitReady::Already(outcome));
        }
        if self
            .preparations
            .iter()
            .any(|prep| prep.request_id == request_id && prep.phase != PrepPhase::Aborted)
        {
            return Err(schema("request rejected"));
        }
        let built = {
            let request = self
                .requests
                .iter()
                .find(|row| row.request_id == request_id)
                .ok_or_else(|| schema("request rejected"))?;
            if request.app_session_id != ctx.app_session_id
                || request.owner_nonce != ctx.owner_nonce
                || request.incarnation != ctx.incarnation
                || request.connection_generation != ctx.connection_generation
                || request.revision != revision
            {
                return Err(BrokerError::IdentityMismatch("revision"));
            }
            if request.state == RequestState::Approved {
                return Err(schema("request rejected"));
            }
            if !request.state.is_pending() || self.clock.now() >= request.deadline {
                return Err(schema("request rejected"));
            }
            let bound = request
                .bound
                .as_ref()
                .and_then(|members| members.first())
                .filter(|bound| {
                    (bound.synthetic || bound.native.is_some())
                        && request.surfaces.contains(&bound.surface)
                })
                .ok_or_else(|| schema("scope is pending"))?;
            let members = request
                .bound
                .as_ref()
                .ok_or_else(|| schema("scope is pending"))?;
            if !(1..=super::MAX_SCOPE_MEMBERS).contains(&members.len())
                || members.iter().any(|bound| {
                    !(bound.synthetic || bound.native.is_some())
                        || !request.surfaces.contains(&bound.surface)
                })
            {
                return Err(schema("scope is pending"));
            }
            let owner = self
                .owners
                .get(&ctx.app_session_id)
                .ok_or(BrokerError::IdentityMismatch("owner"))?;
            Ok::<PrepareParts, BrokerError>(PrepareParts {
                target_id: bound.target_id.clone(),
                surface: bound.surface,
                native: bound.native.clone(),
                members: members
                    .iter()
                    .map(|bound| ScopeMember {
                        target_id: bound.target_id.clone(),
                        surface: bound.surface,
                        native: bound.native.clone(),
                        display_name: bound.display_name.clone(),
                    })
                    .collect(),
                task_id: request.task_id.clone(),
                stop_generation: owner.stop_generation,
                prepare_cancel: owner.prepare_cancel,
                managed: None,
            })
        }?;
        // Reject mixed selected/executable groups before publishing Approving
        // or allocating a preparation that the caller cannot own/abort.
        let selected_webview = |member: &ScopeMember| {
            matches!(
                member.native.as_ref().map(|proof| &proof.identity),
                Some(crate::adapter::NativeScopeIdentity::WebViewSelected { .. })
            )
        };
        if built.members.iter().any(selected_webview)
            && (built.surface != crate::adapter::SurfaceKind::WebView
                || built.members.iter().any(|member| {
                    member.surface != crate::adapter::SurfaceKind::WebView
                        || !selected_webview(member)
                }))
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        let id = self.insert_preparation(
            ctx,
            request_id,
            revision,
            (feature_enabled, feature_epoch),
            built,
        )?;
        if self
            .preparations
            .iter()
            .any(|prep| prep.id == id && prep.phase == PrepPhase::Resources)
        {
            Ok(CommitReady::WebViewSelection(
                self.webview_selection_ticket(id)?,
            ))
        } else {
            Ok(CommitReady::Prepare(self.bound_preparation_ticket(id)?))
        }
    }

    pub(super) fn insert_preparation(
        &mut self,
        ctx: &TransportContext,
        request_id: &str,
        revision: u64,
        feature: (bool, u64),
        built: PrepareParts,
    ) -> Result<u64, BrokerError> {
        self.active_preparation_room(&ctx.app_session_id)?;
        let agent = ctx
            .agent_session_id
            .clone()
            .ok_or(BrokerError::IdentityMismatch("agentSessionId"))?;
        let (feature_enabled, feature_epoch) = feature;
        if let Some(request) = self
            .requests
            .iter_mut()
            .find(|row| row.request_id == request_id)
        {
            request.state = RequestState::Approving;
        }
        let id = self.next_preparation;
        self.next_preparation = self.next_preparation.saturating_add(1);
        // Each preparation owns a distinct run, including explicit retries of
        // the same request. Failed attempts keep their stopped run and cleanup
        // owner; an already committed request returns above without allocating.
        let run_id = format!("cu-task-{}-{}-{}", built.task_id, request_id, fresh_id());
        let selection_revision = self
            .requests
            .iter()
            .find(|row| row.request_id == request_id)
            .ok_or_else(|| schema("scope selection retired"))?
            .selection_revision;
        let unbound_webview = built.members.iter().any(|member| {
            matches!(
                member.native.as_ref().map(|proof| &proof.identity),
                Some(crate::adapter::NativeScopeIdentity::WebViewSelected { .. })
            )
        });
        self.preparations.push(Preparation {
            id,
            request_id: request_id.to_string(),
            revision,
            selection_revision,
            app_session_id: ctx.app_session_id.clone(),
            owner_nonce: ctx.owner_nonce,
            incarnation: ctx.incarnation,
            connection_generation: ctx.connection_generation,
            agent_session_id: agent,
            stop_generation: built.stop_generation,
            prepare_cancel: built.prepare_cancel,
            observed_epoch: feature_epoch,
            observed_enabled: feature_enabled,
            previous_enabled: feature_enabled,
            owned_epoch: feature_epoch,
            we_enabled: false,
            published: false,
            rolled_back: false,
            phase: if built.managed.is_some() || unbound_webview {
                PrepPhase::Resources
            } else {
                PrepPhase::Binding
            },
            run_id: run_id.clone(),
            target_id: built.target_id.clone(),
            task_id: built.task_id.clone(),
            surface: built.surface,
            native: built.native.clone(),
            members: built.members.clone(),
            selected_targets: built
                .members
                .iter()
                .map(|member| (member.surface, member.target_id.clone()))
                .collect(),
            opened_run: false,
            managed: built.managed,
        });
        Ok(id)
    }

    pub(super) fn bound_preparation_ticket(&self, id: u64) -> Result<PrepareTicket, BrokerError> {
        let prep = self
            .preparations
            .iter()
            .find(|prep| prep.id == id && prep.phase == PrepPhase::Binding)
            .filter(|prep| !prep.members.is_empty())
            .ok_or_else(|| schema("scope is pending"))?;
        Ok(PrepareTicket {
            id: prep.id,
            session_id: prep.app_session_id.clone(),
            run_id: prep.run_id.clone(),
            target_id: prep.target_id.clone(),
            surface: prep.surface,
            native: prep.native.clone(),
            members: prep.members.clone(),
            request_id: prep.request_id.clone(),
            revision: prep.revision,
            task_id: prep.task_id.clone(),
        })
    }

    pub(crate) fn preparation_matches(
        &self,
        ticket: &PrepareTicket,
        feature_enabled: bool,
        feature_epoch: u64,
    ) -> bool {
        self.preparation_current(ticket.id, feature_enabled, feature_epoch)
            && self.preparations.iter().any(|prep| {
                prep.id == ticket.id
                    && prep.phase == PrepPhase::Binding
                    && prep.app_session_id == ticket.session_id
                    && prep.run_id == ticket.run_id
                    && prep.target_id == ticket.target_id
                    && prep.surface == ticket.surface
                    && prep.native == ticket.native
                    && prep.members == ticket.members
                    && prep.request_id == ticket.request_id
                    && prep.revision == ticket.revision
                    && prep.task_id == ticket.task_id
            })
    }

    pub fn preparation_current(&self, id: u64, feature_enabled: bool, feature_epoch: u64) -> bool {
        let Some(prep) = self.preparations.iter().find(|prep| prep.id == id) else {
            return false;
        };
        if prep.phase == PrepPhase::Aborted || prep.rolled_back {
            return false;
        }
        let Some(owner) = self.owners.get(&prep.app_session_id) else {
            return false;
        };
        let owner_ok = !owner.retired
            && owner.nonce == prep.owner_nonce
            && owner.incarnation == prep.incarnation
            && owner.connection_generation == prep.connection_generation
            && owner.stop_generation == prep.stop_generation
            && owner.prepare_cancel == prep.prepare_cancel
            && owner.agent_session_id.as_deref() == Some(prep.agent_session_id.as_str());
        if !owner_ok {
            return false;
        }
        let request_ok = self.requests.iter().any(|request| {
            request.request_id == prep.request_id
                && request.revision == prep.revision
                && request.state == RequestState::Approving
                && request.selection_revision == prep.selection_revision
                && self.clock.now() < request.deadline
                && prep
                    .managed
                    .as_ref()
                    .is_none_or(|managed| request.selection_revision == managed.selection_revision)
        });
        if !request_ok {
            return false;
        }
        if prep.published {
            feature_enabled && feature_epoch == prep.owned_epoch
        } else {
            feature_enabled == prep.observed_enabled && feature_epoch == prep.observed_epoch
        }
    }

    /// Publish the feature transition this preparation owns. A superseded epoch is left alone.
    /// Returns the feature bool and epoch after this preparation's publication.
    pub fn publish_feature(
        &mut self,
        id: u64,
        mut feature_enabled: bool,
        mut feature_epoch: u64,
    ) -> Result<(bool, u64), BrokerError> {
        if !self.preparation_current(id, feature_enabled, feature_epoch) {
            return Err(BrokerError::StopRequested);
        }
        let prep = self
            .preparations
            .iter_mut()
            .find(|prep| prep.id == id)
            .ok_or(BrokerError::StopRequested)?;
        if prep.published {
            return Ok((feature_enabled, feature_epoch));
        }
        if !feature_enabled {
            feature_enabled = true;
            feature_epoch = feature_epoch.saturating_add(1);
            prep.we_enabled = true;
            prep.previous_enabled = false;
            prep.owned_epoch = feature_epoch;
        } else {
            prep.we_enabled = false;
            prep.previous_enabled = true;
            prep.owned_epoch = feature_epoch;
        }
        prep.published = true;
        Ok((feature_enabled, feature_epoch))
    }

    pub fn note_opened_run(&mut self, id: u64) {
        if let Some(prep) = self.preparations.iter_mut().find(|prep| prep.id == id) {
            prep.opened_run = true;
        }
    }

    /// Roll back only the feature transition this preparation still owns.
    /// The bool moves, and the epoch moves forward with it.
    pub fn abort_preparation(
        &mut self,
        id: u64,
        feature_enabled: bool,
        feature_epoch: u64,
    ) -> AbortEffect {
        let mut feature = None;
        let mut run_id = String::new();
        let mut opened_run = false;
        if let Some(prep) = self.preparations.iter_mut().find(|prep| prep.id == id) {
            run_id = prep.run_id.clone();
            opened_run = prep.opened_run;
            prep.phase = PrepPhase::Aborted;
            let owes = prep.we_enabled
                && prep.published
                && !prep.rolled_back
                && feature_enabled
                && feature_epoch == prep.owned_epoch;
            if owes {
                let new_epoch = prep.owned_epoch.saturating_add(1);
                feature = Some((prep.previous_enabled, new_epoch));
            }
            prep.rolled_back = true;
            let request_id = prep.request_id.clone();
            let revision = prep.revision;
            let managed_selection = prep
                .managed
                .as_ref()
                .map(|policy| policy.selection_revision);
            if let Some(request) = self
                .requests
                .iter_mut()
                .find(|row| row.request_id == request_id)
            {
                if request.state == RequestState::Approving
                    && request.revision == revision
                    && managed_selection.is_none_or(|owned| request.selection_revision == owned)
                {
                    request.state = RequestState::Awaiting;
                    if managed_selection.is_some() {
                        // The resource reservation owns this selection too.
                        // Abort before proof must not leave Host readback busy
                        // or reuse a tab proof from a failed resource attempt.
                        request.selecting = false;
                        request.bound = None;
                    }
                }
            }
        }
        self.preparations
            .retain(|prep| prep.id != id || !prep.rolled_back);
        AbortEffect {
            feature,
            run_id,
            opened_run,
        }
    }

    pub fn finalize(
        &mut self,
        id: u64,
        feature_enabled: bool,
        feature_epoch: u64,
    ) -> Result<CommitOutcome, BrokerError> {
        if !self.preparation_current(id, feature_enabled, feature_epoch)
            || !self
                .preparations
                .iter()
                .any(|prep| prep.id == id && prep.phase == PrepPhase::Binding)
        {
            return Err(BrokerError::StopRequested);
        }
        let (ticket, grant, memory, outcome) = {
            let prep = self
                .preparations
                .iter()
                .find(|prep| prep.id == id)
                .ok_or(BrokerError::StopRequested)?;
            let request = self
                .requests
                .iter()
                .find(|row| row.request_id == prep.request_id)
                .ok_or_else(|| schema("request rejected"))?;
            let bound = request
                .bound
                .as_ref()
                .and_then(|members| members.first())
                .filter(|bound| {
                    (bound.synthetic || bound.native.is_some())
                        && bound.native == prep.native
                        && bound.target_id == prep.target_id
                        && bound.surface == prep.surface
                        && request.surfaces.contains(&bound.surface)
                })
                .ok_or_else(|| schema("scope is pending"))?;
            let members: Vec<ScopeMember> = request
                .bound
                .as_ref()
                .ok_or_else(|| schema("scope is pending"))?
                .iter()
                .map(|bound| ScopeMember {
                    target_id: bound.target_id.clone(),
                    surface: bound.surface,
                    native: bound.native.clone(),
                    display_name: bound.display_name.clone(),
                })
                .collect();
            if members != prep.members {
                return Err(schema("scope selection retired"));
            }
            let dispatch_id = fresh_id();
            let ticket = Ticket {
                id: dispatch_id.clone(),
                app_session_id: prep.app_session_id.clone(),
                agent_session_id: prep.agent_session_id.clone(),
                incarnation: prep.incarnation,
                connection_generation: prep.connection_generation,
                owner_nonce: prep.owner_nonce,
                request_id: prep.request_id.clone(),
                task_id: prep.task_id.clone(),
                grant_revision: prep.revision,
                feature_epoch,
                dispatch_id,
                consumed: false,
                invalid: false,
            };
            let grant = GrantRecord {
                task_id: prep.task_id.clone(),
                request_id: prep.request_id.clone(),
                app_session_id: prep.app_session_id.clone(),
                agent_session_id: prep.agent_session_id.clone(),
                owner_nonce: prep.owner_nonce,
                incarnation: prep.incarnation,
                connection_generation: prep.connection_generation,
                feature_epoch,
                revision: prep.revision,
                capabilities: request.capabilities.clone(),
                members: prep.members.clone(),
                selected_targets: if prep.selected_targets.is_empty() {
                    prep.members
                        .iter()
                        .map(|member| (member.surface, member.target_id.clone()))
                        .collect()
                } else {
                    prep.selected_targets.clone()
                },
                idle_deadline: self.clock.now().saturating_add(GRANT_IDLE_TTL),
                physical_work: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                completion: std::sync::Arc::new(super::activity::PhysicalActivity::new(
                    self.clock.clone(),
                )),
                execution_run_id: Some(prep.run_id.clone()),
                active_target_id: prep.target_id.clone(),
                suspended: false,
            };
            let memory = ScopeMemory {
                app_session_id: prep.app_session_id.clone(),
                purpose: request.purpose.clone(),
                surfaces: request.surfaces.clone(),
                scope_intent: request.scope_intent.clone(),
                members: prep.members.clone(),
            };
            let outcome = CommitOutcome {
                request_id: prep.request_id.clone(),
                revision: prep.revision,
                run_id: prep.run_id.clone(),
                continuation_id: ticket.id.clone(),
                decision_class: bound.decision_class(),
                binding: binding_from_grant(&grant, &prep.run_id),
            };
            (ticket, grant, memory, outcome)
        };
        if self.grants.iter().filter(|grant| !grant.suspended).count() >= MAX_GRANTS_GLOBAL {
            return Err(schema("request limit"));
        }
        self.push_ticket(ticket)?;
        let request_id = outcome.request_id.clone();
        if let Some(request) = self
            .requests
            .iter_mut()
            .find(|row| row.request_id == request_id)
        {
            request.state = RequestState::Approved;
            request.continuation_issued = true;
        }
        self.grants.push(grant);
        // A successful task now relies on this enabled epoch. An unfinished
        // first-use approval may abort its own run, but no longer owns a
        // process-wide rollback that would revoke this committed task.
        for prep in &mut self.preparations {
            if prep.owned_epoch == feature_epoch {
                prep.we_enabled = false;
            }
        }
        self.remember_scope(memory);
        self.preparations.retain(|prep| prep.id != id);
        Ok(outcome)
    }

    pub(super) fn already_committed(
        &self,
        ctx: &TransportContext,
        request_id: &str,
        revision: u64,
    ) -> Option<CommitOutcome> {
        let request = self.requests.iter().find(|row| {
            row.request_id == request_id
                && row.revision == revision
                && row.state == RequestState::Approved
                && row.app_session_id == ctx.app_session_id
                && row.owner_nonce == ctx.owner_nonce
        })?;
        let grant = self.grants.iter().find(|grant| {
            grant.request_id == request.request_id
                && grant.revision == revision
                && self.grant_executable(grant)
                && grant.execution_run_id.is_some()
        })?;
        let run_id = grant.execution_run_id.clone()?;
        let continuation_id = self.continuation_id(&ctx.app_session_id, request_id, revision)?;
        Some(CommitOutcome {
            request_id: request.request_id.clone(),
            revision,
            run_id,
            continuation_id,
            decision_class: request.bound.as_ref()?.first()?.decision_class(),
            binding: binding_from_grant(grant, grant.execution_run_id.as_deref().unwrap_or("")),
        })
    }
}

pub(super) struct PrepareParts {
    pub(super) target_id: String,
    pub(super) surface: crate::adapter::SurfaceKind,
    pub(super) native: Option<super::NativeScopeProof>,
    pub(super) members: Vec<ScopeMember>,
    pub(super) task_id: String,
    pub(super) stop_generation: u64,
    pub(super) prepare_cancel: u64,
    pub(super) managed: Option<super::types::ManagedResourcePolicy>,
}
