//! Request cards. Pending history is not an approved scope.
use std::collections::HashSet;

use serde_json::Value;

use crate::adapter::SurfaceKind;
use crate::error::BrokerError;

use super::parse::parse_action;
#[cfg(any(test, feature = "test-support"))]
use super::types::OpaqueIdentity;
use super::types::{
    fresh_id, request_body, same_caps, schema, Capability, ParsedAction, PrepPhase, RequestRecord,
    RequestState, TransportContext, REQUEST_TTL,
};
use super::ConsentState;

impl ConsentState {
    pub fn submit(
        &mut self,
        ctx: &TransportContext,
        args: Value,
        busy: &HashSet<String>,
        feature_epoch: u64,
    ) -> Result<Value, BrokerError> {
        self.sweep(busy);
        self.require_model_owner(ctx)?;
        match parse_action(args)? {
            ParsedAction::Status => self.status(ctx),
            ParsedAction::Cancel => self.cancel_model(ctx),
            ParsedAction::Request {
                purpose,
                surfaces,
                capabilities,
                scope_intent,
            } => self.open_request(
                ctx,
                purpose,
                surfaces,
                capabilities,
                scope_intent,
                feature_epoch,
            ),
        }
    }

    /// Test-only synthetic target. This is not a production binder or a grant.
    #[cfg(any(test, feature = "test-support"))]
    pub fn bind_synthetic(&mut self, request_id: &str, target_id: &str) -> Result<(), BrokerError> {
        self.bind_synthetic_on_surface(request_id, target_id, SurfaceKind::Desktop)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn bind_synthetic_on_surface(
        &mut self,
        request_id: &str,
        target_id: &str,
        surface: SurfaceKind,
    ) -> Result<(), BrokerError> {
        if target_id.trim().is_empty() {
            return Err(schema("request rejected"));
        }
        let request = self
            .requests
            .iter_mut()
            .find(|row| row.request_id == request_id)
            .ok_or_else(|| schema("request rejected"))?;
        if !request.state.is_pending() || !request.surfaces.contains(&surface) {
            return Err(schema("request rejected"));
        }
        request.bound = Some(vec![OpaqueIdentity {
            target_id: target_id.to_string(),
            surface,
            display_name: None,
            synthetic: true,
            native: None,
        }]);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn bind_synthetic_group(
        &mut self,
        request_id: &str,
        targets: &[&str],
    ) -> Result<(), BrokerError> {
        if !(1..=super::types::MAX_SCOPE_MEMBERS).contains(&targets.len())
            || targets.iter().any(|id| id.trim().is_empty())
            || targets.iter().collect::<HashSet<_>>().len() != targets.len()
        {
            return Err(schema("scope selection rejected"));
        }
        self.bind_synthetic(request_id, targets[0])?;
        let request = self
            .requests
            .iter_mut()
            .find(|row| row.request_id == request_id)
            .unwrap();
        request.bound = Some(
            targets
                .iter()
                .map(|target| OpaqueIdentity {
                    target_id: (*target).into(),
                    surface: SurfaceKind::Desktop,
                    display_name: None,
                    synthetic: true,
                    native: None,
                })
                .collect(),
        );
        Ok(())
    }

    pub fn deny(
        &mut self,
        ctx: &TransportContext,
        request_id: &str,
        revision: u64,
    ) -> Result<(), BrokerError> {
        self.finish_terminal(ctx, request_id, revision, RequestState::Denied, "denied")
    }

    pub fn host_cancel(
        &mut self,
        ctx: &TransportContext,
        request_id: &str,
        revision: u64,
    ) -> Result<(), BrokerError> {
        self.finish_terminal(
            ctx,
            request_id,
            revision,
            RequestState::Cancelled,
            "cancelled",
        )
    }

    pub fn live_request_count(&self, app_session_id: &str) -> usize {
        self.requests
            .iter()
            .filter(|row| row.app_session_id == app_session_id && row.state.is_pending())
            .count()
    }

    pub fn stored_request_count(&self) -> usize {
        self.requests.len()
    }

    fn open_request(
        &mut self,
        ctx: &TransportContext,
        purpose: String,
        surfaces: Vec<SurfaceKind>,
        capabilities: Vec<Capability>,
        scope_intent: String,
        feature_epoch: u64,
    ) -> Result<Value, BrokerError> {
        let task_id = self.task_id_for(&ctx.app_session_id)?;
        let intent = super::types::intent_key(
            &purpose,
            &surfaces,
            &scope_intent,
            ctx.connection_generation,
            ctx.owner_nonce,
        );
        let canonical = super::types::canonical_key(&intent, &capabilities);
        if let Some(existing) = self.requests.iter().find(|row| {
            row.app_session_id == ctx.app_session_id
                && row.owner_nonce == ctx.owner_nonce
                && row.canonical == canonical
                && row.state.is_pending()
        }) {
            return Ok(request_body(existing));
        }
        if let Some(index) = self.requests.iter().position(|row| {
            row.app_session_id == ctx.app_session_id
                && row.owner_nonce == ctx.owner_nonce
                && row.intent == intent
                && row.state.is_pending()
        }) {
            let request = &mut self.requests[index];
            if same_caps(&request.capabilities, &capabilities) {
                return Ok(request_body(request));
            }
            request.revision = request.revision.saturating_add(1);
            request.capabilities = capabilities;
            request.canonical = canonical;
            request.bound = None;
            request.selecting = false;
            request.continuation_issued = false;
            request.feature_epoch = feature_epoch;
            request.deadline = self.clock.now().saturating_add(REQUEST_TTL);
            return Ok(request_body(request));
        }
        self.retire_other_pending(&ctx.app_session_id, ctx.owner_nonce);
        self.stored_request_room(&ctx.app_session_id)?;
        let record = RequestRecord {
            request_id: fresh_id(),
            task_id,
            app_session_id: ctx.app_session_id.clone(),
            owner_nonce: ctx.owner_nonce,
            incarnation: ctx.incarnation,
            connection_generation: ctx.connection_generation,
            purpose,
            surfaces,
            capabilities,
            scope_intent,
            canonical,
            intent,
            state: RequestState::Awaiting,
            revision: 1,
            selection_revision: 0,
            selecting: false,
            deadline: self.clock.now().saturating_add(REQUEST_TTL),
            bound: None,
            continuation_issued: false,
            feature_epoch,
        };
        let body = request_body(&record);
        self.requests.push(record);
        self.trim_stored_requests();
        Ok(body)
    }

    fn retire_other_pending(&mut self, session: &str, nonce: u64) {
        let mut retired = Vec::new();
        for request in &mut self.requests {
            if request.app_session_id == session
                && request.owner_nonce == nonce
                && request.state.is_pending()
            {
                retired.push((request.request_id.clone(), request.owner_nonce));
                request.state = RequestState::Retired;
                request.continuation_issued = false;
            }
        }
        for (request_id, owner_nonce) in retired {
            self.keep_unfinished_preparation(&request_id);
            self.invalidate_tickets(session, Some(&request_id));
            self.push_retired_notice(session, owner_nonce, request_id, "retired");
        }
    }

    fn status(&self, ctx: &TransportContext) -> Result<Value, BrokerError> {
        let request = self
            .latest_request(ctx)
            .ok_or_else(|| schema("no request"))?;
        let mut body = request_body(request);
        if request.state == RequestState::Approved {
            body["authority"] = serde_json::json!("grant");
            body["continuationIssued"] = serde_json::json!(request.continuation_issued);
            body["decisionClass"] = serde_json::json!(request
                .bound
                .as_ref()
                .and_then(|members| members.first())
                .map(|bound| bound.decision_class()));
        }
        Ok(body)
    }

    fn cancel_model(&mut self, ctx: &TransportContext) -> Result<Value, BrokerError> {
        let request_id = self
            .latest_request(ctx)
            .ok_or_else(|| schema("no request"))?
            .request_id
            .clone();
        let revision = self
            .requests
            .iter()
            .find(|row| row.request_id == request_id)
            .map(|row| row.revision)
            .ok_or_else(|| schema("request rejected"))?;
        self.finish_terminal(
            ctx,
            &request_id,
            revision,
            RequestState::Cancelled,
            "cancelled",
        )?;
        let request = self
            .requests
            .iter()
            .find(|row| row.request_id == request_id)
            .ok_or_else(|| schema("request rejected"))?;
        Ok(request_body(request))
    }

    fn finish_terminal(
        &mut self,
        ctx: &TransportContext,
        request_id: &str,
        revision: u64,
        state: RequestState,
        reason: &'static str,
    ) -> Result<(), BrokerError> {
        self.require_model_owner(ctx)?;
        let owner_nonce = {
            let request = self
                .requests
                .iter_mut()
                .find(|row| row.request_id == request_id)
                .ok_or_else(|| schema("request rejected"))?;
            if request.app_session_id != ctx.app_session_id
                || request.owner_nonce != ctx.owner_nonce
                || request.incarnation != ctx.incarnation
                || request.connection_generation != ctx.connection_generation
                || request.revision != revision
                || !request.state.is_pending()
            {
                return Err(BrokerError::IdentityMismatch("revision"));
            }
            request.state = state;
            request.continuation_issued = false;
            request.owner_nonce
        };
        self.grants.retain(|grant| grant.request_id != request_id);
        self.keep_unfinished_preparation(request_id);
        self.invalidate_tickets(&ctx.app_session_id, Some(request_id));
        self.push_retired_notice(
            &ctx.app_session_id,
            owner_nonce,
            request_id.to_string(),
            reason,
        );
        self.trim_stored_requests();
        Ok(())
    }

    fn keep_unfinished_preparation(&mut self, request_id: &str) {
        for prep in &mut self.preparations {
            if prep.request_id == request_id && !prep.rolled_back {
                prep.phase = PrepPhase::Aborted;
            }
        }
    }

    pub(super) fn latest_request(&self, ctx: &TransportContext) -> Option<&RequestRecord> {
        self.requests.iter().rev().find(|row| {
            row.app_session_id == ctx.app_session_id
                && row.owner_nonce == ctx.owner_nonce
                && row.incarnation == ctx.incarnation
                && row.connection_generation == ctx.connection_generation
        })
    }
}
