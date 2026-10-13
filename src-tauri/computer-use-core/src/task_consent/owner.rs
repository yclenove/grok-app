//! Owner identity. A retired nonce is never issued again.
use std::collections::HashSet;

use crate::error::BrokerError;

use super::types::{
    fresh_id, require_session, schema, BearerClass, Decision, IssuedTransport, Owner, PrepPhase,
    RequestState, TokenBody, TransportContext, MAX_OWNERS, MAX_RETIRED_TOKENS,
};
use super::ConsentState;

impl ConsentState {
    pub fn issue(
        &mut self,
        app_session_id: &str,
        feature_epoch: u64,
        runtime_retained: &HashSet<String>,
    ) -> Result<IssuedTransport, BrokerError> {
        let session = require_session(app_session_id)?;
        self.reclaim_terminal_owners(runtime_retained);
        match self.owners.get(&session).map(|owner| owner.retired) {
            Some(true) => {
                // A retired row that still owns a resource keeps its nonce.
                // Recycling it would detach cleanup from the captured identity.
                return Err(schema("owner limit"));
            }
            Some(false) => {}
            None => {
                if self.owners.len() >= MAX_OWNERS {
                    return Err(schema("owner limit"));
                }
                let nonce = self.allocate_nonce();
                self.owners.insert(
                    session.clone(),
                    Owner {
                        nonce,
                        incarnation: 1,
                        connection_generation: 1,
                        agent_session_id: None,
                        retired: false,
                        stop_generation: 0,
                        prepare_cancel: 0,
                        task_id: None,
                    },
                );
            }
        }
        self.reuse_or_mint(&session, feature_epoch)
    }

    pub fn classify(&self, token: &str) -> BearerClass {
        if let Some(body) = self.tokens.get(token) {
            return BearerClass::Live(TransportContext {
                app_session_id: body.app_session_id.clone(),
                incarnation: body.incarnation,
                connection_generation: body.connection_generation,
                agent_session_id: body.agent_session_id.clone(),
                feature_epoch_at_issue: body.feature_epoch_at_issue,
                owner_nonce: body.owner_nonce,
            });
        }
        if self.retired_lookup.contains(token) {
            BearerClass::Retired
        } else {
            BearerClass::Unknown
        }
    }

    /// First bind keeps the reserved token. A different agent retires it.
    /// A prior grant with no agent is dropped, not adopted.
    pub fn bind_agent(
        &mut self,
        app_session_id: &str,
        agent_session_id: &str,
    ) -> Result<Vec<String>, BrokerError> {
        if agent_session_id.is_empty() {
            return Err(BrokerError::IdentityMismatch("agentSessionId"));
        }
        let session = require_session(app_session_id)?;
        let kind = {
            let owner = self
                .owners
                .get(&session)
                .ok_or(BrokerError::IdentityMismatch("owner"))?;
            if owner.retired {
                return Err(BrokerError::IdentityMismatch("owner"));
            }
            match &owner.agent_session_id {
                Some(current) if current == agent_session_id => BindKind::Same,
                None => BindKind::First,
                Some(_) => BindKind::Replace,
            }
        };
        match kind {
            BindKind::Same => Ok(Vec::new()),
            BindKind::First => {
                let (incarnation, generation, nonce) = {
                    let owner = self.owners.get(&session).expect("owner");
                    (owner.incarnation, owner.connection_generation, owner.nonce)
                };
                self.drop_unbound_grants(&session);
                let owner = self.owners.get_mut(&session).expect("owner");
                owner.agent_session_id = Some(agent_session_id.to_string());
                for body in self.tokens.values_mut() {
                    if body.app_session_id == session
                        && body.owner_nonce == nonce
                        && body.incarnation == incarnation
                        && body.connection_generation == generation
                        && body.agent_session_id.is_none()
                    {
                        body.agent_session_id = Some(agent_session_id.to_string());
                    }
                }
                Ok(Vec::new())
            }
            BindKind::Replace => {
                let runs = self.owned_run_ids(&session);
                let nonce = self.allocate_nonce();
                {
                    let owner = self.owners.get_mut(&session).expect("owner");
                    owner.nonce = nonce;
                    owner.incarnation = owner.incarnation.saturating_add(1);
                    owner.connection_generation = owner.connection_generation.saturating_add(1);
                    owner.agent_session_id = Some(agent_session_id.to_string());
                    owner.stop_generation = owner.stop_generation.saturating_add(1);
                    owner.prepare_cancel = owner.prepare_cancel.saturating_add(1);
                    owner.task_id = None;
                }
                self.retire_tokens_for(&session);
                self.cancel_authority(&session, "retired");
                Ok(runs)
            }
        }
    }

    pub fn rotate_connection(&mut self, app_session_id: &str) -> Result<Vec<String>, BrokerError> {
        let session = require_session(app_session_id)?;
        let retired = self
            .owners
            .get(&session)
            .ok_or(BrokerError::IdentityMismatch("owner"))?
            .retired;
        if retired {
            return Err(BrokerError::IdentityMismatch("owner"));
        }
        let runs = self.owned_run_ids(&session);
        let nonce = self.allocate_nonce();
        {
            let owner = self.owners.get_mut(&session).expect("owner");
            owner.nonce = nonce;
            owner.connection_generation = owner.connection_generation.saturating_add(1);
            owner.stop_generation = owner.stop_generation.saturating_add(1);
            owner.prepare_cancel = owner.prepare_cancel.saturating_add(1);
            owner.task_id = None;
        }
        self.retire_tokens_for(&session);
        self.cancel_authority(&session, "retired");
        Ok(runs)
    }

    pub fn retire_owner(&mut self, app_session_id: &str) -> Vec<String> {
        let Some(owner) = self.owners.get(app_session_id) else {
            return Vec::new();
        };
        if owner.retired {
            return Vec::new();
        }
        let runs = self.owned_run_ids(app_session_id);
        let nonce = self.allocate_nonce();
        if let Some(owner) = self.owners.get_mut(app_session_id) {
            owner.retired = true;
            owner.nonce = nonce;
            owner.agent_session_id = None;
            owner.stop_generation = owner.stop_generation.saturating_add(1);
            owner.prepare_cancel = owner.prepare_cancel.saturating_add(1);
            owner.task_id = None;
        }
        self.retire_tokens_for(app_session_id);
        self.cancel_authority(app_session_id, "retired");
        runs
    }

    pub fn stop_owner(&mut self, app_session_id: &str) -> Vec<String> {
        let live = match self.owners.get_mut(app_session_id) {
            Some(owner) if !owner.retired => {
                owner.stop_generation = owner.stop_generation.saturating_add(1);
                owner.prepare_cancel = owner.prepare_cancel.saturating_add(1);
                owner.task_id = None;
                true
            }
            _ => false,
        };
        if !live {
            return Vec::new();
        }
        let runs = self.owned_run_ids(app_session_id);
        self.cancel_authority(app_session_id, "stopped");
        runs
    }

    pub(super) fn require_model_owner(&self, ctx: &TransportContext) -> Result<(), BrokerError> {
        self.require_owner(ctx, true)
    }

    pub(super) fn require_owner(
        &self,
        ctx: &TransportContext,
        agent_required: bool,
    ) -> Result<(), BrokerError> {
        let owner = self
            .owners
            .get(&ctx.app_session_id)
            .ok_or(BrokerError::IdentityMismatch("owner"))?;
        if owner.retired
            || owner.nonce != ctx.owner_nonce
            || owner.incarnation != ctx.incarnation
            || owner.connection_generation != ctx.connection_generation
        {
            return Err(BrokerError::IdentityMismatch("generation"));
        }
        match (&owner.agent_session_id, &ctx.agent_session_id) {
            (Some(owner_agent), Some(ctx_agent)) if owner_agent == ctx_agent => Ok(()),
            (None, None) if !agent_required => Ok(()),
            _ => Err(BrokerError::IdentityMismatch("agentSessionId")),
        }
    }

    pub(super) fn owned_run_ids(&self, session: &str) -> Vec<String> {
        let mut ids = Vec::new();
        for grant in &self.grants {
            if grant.app_session_id == session {
                if let Some(run_id) = &grant.execution_run_id {
                    push_unique(&mut ids, run_id);
                }
            }
        }
        for prep in &self.preparations {
            if prep.app_session_id == session && !prep.rolled_back && !prep.run_id.is_empty() {
                push_unique(&mut ids, &prep.run_id);
            }
        }
        ids
    }

    pub(super) fn cancel_authority(&mut self, session: &str, reason: &'static str) {
        let mut notices = Vec::new();
        for request in &mut self.requests {
            if request.app_session_id == session
                && (request.state.is_pending() || request.state == RequestState::Approved)
            {
                notices.push((request.request_id.clone(), request.owner_nonce));
                request.state = RequestState::Cancelled;
                request.continuation_issued = false;
            }
        }
        self.grants.retain(|grant| grant.app_session_id != session);
        for prep in &mut self.preparations {
            if prep.app_session_id == session && prep.phase != PrepPhase::Aborted {
                prep.phase = PrepPhase::Aborted;
            }
        }
        self.invalidate_tickets(session, None);
        for (request_id, owner_nonce) in notices {
            self.push_retired_notice(session, owner_nonce, request_id, reason);
        }
        self.preparations
            .retain(|prep| prep.app_session_id != session || !prep.rolled_back);
    }

    pub(super) fn task_id_for(&mut self, session: &str) -> Result<String, BrokerError> {
        let owner = self
            .owners
            .get_mut(session)
            .ok_or(BrokerError::IdentityMismatch("owner"))?;
        if owner.retired {
            return Err(BrokerError::IdentityMismatch("owner"));
        }
        if owner.task_id.is_none() {
            owner.task_id = Some(fresh_id());
        }
        Ok(owner.task_id.clone().expect("task id"))
    }

    fn drop_unbound_grants(&mut self, session: &str) {
        self.grants.retain(|grant| {
            !(grant.app_session_id == session && grant.agent_session_id.is_empty())
        });
    }

    fn reuse_or_mint(
        &mut self,
        session: &str,
        feature_epoch: u64,
    ) -> Result<IssuedTransport, BrokerError> {
        let (nonce, incarnation, generation, agent) = {
            let owner = self
                .owners
                .get(session)
                .ok_or(BrokerError::IdentityMismatch("owner"))?;
            (
                owner.nonce,
                owner.incarnation,
                owner.connection_generation,
                owner.agent_session_id.clone(),
            )
        };
        if let Some((token, body)) = self.tokens.iter().find(|(_, body)| {
            body.app_session_id == session
                && body.owner_nonce == nonce
                && body.incarnation == incarnation
                && body.connection_generation == generation
                && body.agent_session_id == agent
        }) {
            return Ok(IssuedTransport::from_parts(
                token.clone(),
                incarnation,
                generation,
                body.feature_epoch_at_issue,
            ));
        }
        let token = fresh_id() + &fresh_id();
        self.tokens.insert(
            token.clone(),
            TokenBody {
                app_session_id: session.to_string(),
                owner_nonce: nonce,
                incarnation,
                connection_generation: generation,
                agent_session_id: agent,
                feature_epoch_at_issue: feature_epoch,
            },
        );
        Ok(IssuedTransport::from_parts(
            token,
            incarnation,
            generation,
            feature_epoch,
        ))
    }

    fn allocate_nonce(&mut self) -> u64 {
        let nonce = self.next_nonce;
        self.next_nonce = self.next_nonce.saturating_add(1);
        nonce
    }

    pub(super) fn retire_tokens_for(&mut self, session: &str) {
        let stale: Vec<String> = self
            .tokens
            .iter()
            .filter(|(_, body)| body.app_session_id == session)
            .map(|(token, _)| token.clone())
            .collect();
        for token in stale {
            self.tokens.remove(&token);
            self.remember_retired(token);
        }
    }

    fn remember_retired(&mut self, token: String) {
        if !self.retired_lookup.contains(&token) {
            if self.retired_tokens.len() >= MAX_RETIRED_TOKENS {
                if let Some(old) = self.retired_tokens.pop_front() {
                    self.retired_lookup.remove(&old);
                }
            }
            self.retired_lookup.insert(token.clone());
            self.retired_tokens.push_back(token.clone());
        }
        self.retired_outbox
            .retain(|item| self.retired_lookup.contains(item));
        if self.retired_lookup.contains(&token) && !self.retired_outbox.contains(&token) {
            self.retired_outbox.push(token);
        }
    }

    fn reclaim_terminal_owners(&mut self, runtime_retained: &HashSet<String>) {
        let stale: Vec<String> = self
            .owners
            .iter()
            .filter(|(session, owner)| {
                owner.retired && !self.owner_has_obligation(session, runtime_retained)
            })
            .map(|(session, _)| session.clone())
            .collect();
        for session in stale {
            self.owners.remove(&session);
        }
    }

    fn owner_has_obligation(&self, session: &str, runtime_retained: &HashSet<String>) -> bool {
        if runtime_retained.contains(session) {
            return true;
        }
        if self
            .requests
            .iter()
            .any(|row| row.app_session_id == session && !row.state.is_terminal())
        {
            return true;
        }
        if self
            .grants
            .iter()
            .any(|grant| grant.app_session_id == session)
        {
            return true;
        }
        if self
            .preparations
            .iter()
            .any(|prep| prep.app_session_id == session && !prep.rolled_back)
        {
            return true;
        }
        self.decisions.iter().any(|decision| match decision {
            Decision::Continuation(ticket) => {
                ticket.app_session_id == session && !ticket.consumed && !ticket.invalid
            }
            Decision::NoAuthority(_) => false,
        })
    }

    pub fn take_retired_tokens(&mut self) -> Vec<String> {
        std::mem::take(&mut self.retired_outbox)
    }

    pub fn on_feature_off(&mut self) {
        let sessions: Vec<String> = self
            .owners
            .iter()
            .filter(|(_, owner)| !owner.retired)
            .map(|(session, _)| session.clone())
            .collect();
        for session in sessions {
            let _ = self.stop_owner(&session);
        }
    }

    pub fn begin_new_task(&mut self, app_session_id: &str) -> Result<Vec<String>, BrokerError> {
        let session = require_session(app_session_id)?;
        self.require_live_session(&session)?;
        let runs = self.owned_run_ids(&session);
        if let Some(owner) = self.owners.get_mut(&session) {
            owner.task_id = None;
            owner.prepare_cancel = owner.prepare_cancel.saturating_add(1);
        }
        self.cancel_authority(&session, "retired");
        Ok(runs)
    }

    fn require_live_session(&self, session: &str) -> Result<(), BrokerError> {
        let owner = self
            .owners
            .get(session)
            .ok_or(BrokerError::IdentityMismatch("owner"))?;
        if owner.retired {
            Err(BrokerError::IdentityMismatch("owner"))
        } else {
            Ok(())
        }
    }
}

enum BindKind {
    Same,
    First,
    Replace,
}

fn push_unique(ids: &mut Vec<String>, run_id: &str) {
    if !ids.iter().any(|id| id == run_id) {
        ids.push(run_id.to_string());
    }
}
