//! Typed continuation ledger. Claim does not consume. Status does not mint.
use crate::error::BrokerError;

use super::types::{
    claim_from_ticket, schema, AuthorityClaim, Decision, NoAuthorityNotice, Notice, RequestState,
    Ticket,
};
use super::ConsentState;

impl ConsentState {
    /// A drained notice is a delivery record, never an execution permit. Recheck
    /// its original owner and terminal request before Host dispatch.
    pub fn validate_no_authority(&self, notice: &NoAuthorityNotice) -> Result<(), BrokerError> {
        let owner = self
            .owners
            .get(&notice.app_session_id)
            .ok_or(BrokerError::IdentityMismatch("owner"))?;
        if owner.retired
            || owner.nonce != notice.owner_nonce
            || owner.incarnation != notice.incarnation
            || owner.connection_generation != notice.connection_generation
            || owner.agent_session_id.as_deref() != Some(notice.agent_session_id.as_str())
            || owner.task_id.as_deref() != Some(notice.task_id.as_str())
            || notice.id.is_empty()
            || notice.id != notice.dispatch_id
        {
            return Err(BrokerError::IdentityMismatch("generation"));
        }
        let expected = match notice.reason {
            "denied" => RequestState::Denied,
            "cancelled" => RequestState::Cancelled,
            "expired" => RequestState::Expired,
            _ => return Err(schema("notice is not resumable")),
        };
        if !self.requests.iter().any(|request| {
            request.request_id == notice.request_id
                && request.app_session_id == notice.app_session_id
                && request.owner_nonce == notice.owner_nonce
                && request.incarnation == notice.incarnation
                && request.connection_generation == notice.connection_generation
                && request.task_id == notice.task_id
                && request.revision == notice.revision
                && request.feature_epoch == notice.feature_epoch
                && request.state == expected
        }) {
            return Err(BrokerError::IdentityMismatch("request"));
        }
        Ok(())
    }

    pub fn claim_continuation(&self, app_session_id: &str) -> Option<AuthorityClaim> {
        self.decisions.iter().find_map(|decision| match decision {
            Decision::Continuation(ticket)
                if ticket.app_session_id == app_session_id
                    && !ticket.consumed
                    && !ticket.invalid
                    && ticket_complete(ticket) =>
            {
                Some(claim_from_ticket(ticket))
            }
            _ => None,
        })
    }

    pub fn validate_continuation(
        &self,
        claim: &AuthorityClaim,
        feature_enabled: bool,
        feature_epoch: u64,
    ) -> Result<(), BrokerError> {
        let ticket = self
            .decisions
            .iter()
            .find_map(|decision| match decision {
                Decision::Continuation(ticket) if ticket.id == claim.id => Some(ticket),
                _ => None,
            })
            .ok_or_else(|| schema("request rejected"))?;
        if ticket.consumed || ticket.invalid || !ticket_matches(ticket, claim) {
            return Err(schema("request rejected"));
        }
        if !feature_enabled
            || feature_epoch != claim.feature_epoch
            || feature_epoch != ticket.feature_epoch
        {
            return Err(BrokerError::FeatureDisabled);
        }
        self.require_claim_owner(claim)?;
        let grant_live = self.grants.iter().any(|grant| {
            grant.app_session_id == claim.app_session_id
                && grant.request_id == claim.request_id
                && grant.task_id == claim.task_id
                && grant.revision == claim.grant_revision
                && grant.feature_epoch == claim.feature_epoch
                && grant.owner_nonce == claim.owner_nonce
                && grant.incarnation == claim.incarnation
                && grant.connection_generation == claim.connection_generation
                && grant.agent_session_id == claim.agent_session_id
                && self.grant_executable(grant)
                && grant.execution_run_id.is_some()
        });
        if !grant_live {
            return Err(BrokerError::TargetUnauthorized);
        }
        Ok(())
    }

    pub fn consume_continuation(
        &mut self,
        claim: &AuthorityClaim,
        feature_enabled: bool,
        feature_epoch: u64,
    ) -> Result<(), BrokerError> {
        self.validate_continuation(claim, feature_enabled, feature_epoch)?;
        let ticket = self
            .decisions
            .iter_mut()
            .find_map(|decision| match decision {
                Decision::Continuation(ticket) if ticket.id == claim.id => Some(ticket),
                _ => None,
            })
            .ok_or_else(|| schema("request rejected"))?;
        if ticket.consumed {
            return Err(schema("request rejected"));
        }
        ticket.consumed = true;
        Ok(())
    }

    pub fn continuation_count(&self, app_session_id: &str) -> usize {
        self.decisions
            .iter()
            .filter(|decision| {
                matches!(decision, Decision::Continuation(ticket) if ticket.app_session_id == app_session_id)
            })
            .count()
    }

    /// Remove this owner's notices once. Taking them does not admit a tool call.
    pub fn take_no_authority(
        &mut self,
        app_session_id: &str,
        owner_nonce: u64,
    ) -> Vec<NoAuthorityNotice> {
        let mut kept = std::collections::VecDeque::new();
        let mut taken = Vec::new();
        for decision in self.decisions.drain(..) {
            match decision {
                Decision::NoAuthority(notice)
                    if notice.app_session_id == app_session_id
                        && notice.owner_nonce == owner_nonce
                        && notice_readable(&notice) =>
                {
                    taken.push(public_notice(notice));
                }
                other => kept.push_back(other),
            }
        }
        self.decisions = kept;
        taken
    }

    pub fn no_authority_reasons(&self, app_session_id: &str) -> Vec<&'static str> {
        self.decisions
            .iter()
            .filter_map(|decision| match decision {
                Decision::NoAuthority(notice)
                    if notice.app_session_id == app_session_id && notice_readable(notice) =>
                {
                    Some(notice.reason)
                }
                _ => None,
            })
            .collect()
    }

    pub(super) fn continuation_id(
        &self,
        session: &str,
        request_id: &str,
        revision: u64,
    ) -> Option<String> {
        self.decisions.iter().find_map(|decision| match decision {
            Decision::Continuation(ticket)
                if ticket.app_session_id == session
                    && ticket.request_id == request_id
                    && ticket.grant_revision == revision
                    && !ticket.invalid =>
            {
                Some(ticket.id.clone())
            }
            _ => None,
        })
    }

    pub(super) fn invalidate_tickets(&mut self, session: &str, request_id: Option<&str>) {
        for decision in &mut self.decisions {
            if let Decision::Continuation(ticket) = decision {
                let same_request = request_id.is_none_or(|id| ticket.request_id == id);
                if ticket.app_session_id == session && same_request {
                    ticket.invalid = true;
                }
            }
        }
    }

    pub(super) fn invalidate_task(&mut self, session: &str, task_id: &str) {
        for decision in &mut self.decisions {
            if let Decision::Continuation(ticket) = decision {
                if ticket.app_session_id == session && ticket.task_id == task_id {
                    ticket.invalid = true;
                }
            }
        }
    }

    pub(super) fn push_retired_notice(
        &mut self,
        session: &str,
        owner_nonce: u64,
        request_id: String,
        reason: &'static str,
    ) {
        self.push_notice(self.notice_for(session, owner_nonce, &request_id, reason));
    }

    pub(super) fn notice_for(
        &self,
        session: &str,
        owner_nonce: u64,
        request_id: &str,
        reason: &'static str,
    ) -> Notice {
        let request = self
            .requests
            .iter()
            .find(|row| row.request_id == request_id);
        let owner = self.owners.get(session);
        let id = super::types::fresh_id();
        Notice {
            id: id.clone(),
            dispatch_id: id,
            app_session_id: session.to_string(),
            owner_nonce,
            incarnation: request.map(|row| row.incarnation).unwrap_or(0),
            connection_generation: request
                .map(|row| row.connection_generation)
                .or_else(|| owner.map(|row| row.connection_generation))
                .unwrap_or(0),
            agent_session_id: owner
                .and_then(|row| row.agent_session_id.clone())
                .unwrap_or_default(),
            request_id: request_id.to_string(),
            task_id: request.map(|row| row.task_id.clone()).unwrap_or_default(),
            revision: request.map(|row| row.revision).unwrap_or(0),
            feature_epoch: request.map(|row| row.feature_epoch).unwrap_or(0),
            reason,
        }
    }

    fn require_claim_owner(&self, claim: &AuthorityClaim) -> Result<(), BrokerError> {
        let owner = self
            .owners
            .get(&claim.app_session_id)
            .ok_or(BrokerError::IdentityMismatch("owner"))?;
        if owner.retired
            || owner.nonce != claim.owner_nonce
            || owner.incarnation != claim.incarnation
            || owner.connection_generation != claim.connection_generation
            || owner.agent_session_id.as_deref() != Some(claim.agent_session_id.as_str())
        {
            return Err(BrokerError::IdentityMismatch("generation"));
        }
        Ok(())
    }
}

fn ticket_complete(ticket: &Ticket) -> bool {
    !ticket.id.is_empty()
        && !ticket.app_session_id.is_empty()
        && !ticket.agent_session_id.is_empty()
        && !ticket.request_id.is_empty()
        && !ticket.task_id.is_empty()
        && ticket.grant_revision > 0
        && ticket.owner_nonce > 0
}

fn ticket_matches(ticket: &Ticket, claim: &AuthorityClaim) -> bool {
    ticket.app_session_id == claim.app_session_id
        && ticket.agent_session_id == claim.agent_session_id
        && ticket.incarnation == claim.incarnation
        && ticket.connection_generation == claim.connection_generation
        && ticket.owner_nonce == claim.owner_nonce
        && ticket.request_id == claim.request_id
        && ticket.task_id == claim.task_id
        && ticket.grant_revision == claim.grant_revision
        && ticket.feature_epoch == claim.feature_epoch
        && ticket.dispatch_id == claim.dispatch_id
        && ticket.id == claim.id
}

fn notice_readable(notice: &Notice) -> bool {
    !notice.id.is_empty()
        && notice.id == notice.dispatch_id
        && !notice.request_id.is_empty()
        && notice.owner_nonce > 0
}

fn public_notice(notice: Notice) -> NoAuthorityNotice {
    NoAuthorityNotice {
        id: notice.id,
        dispatch_id: notice.dispatch_id,
        app_session_id: notice.app_session_id,
        owner_nonce: notice.owner_nonce,
        incarnation: notice.incarnation,
        connection_generation: notice.connection_generation,
        agent_session_id: notice.agent_session_id,
        request_id: notice.request_id,
        task_id: notice.task_id,
        revision: notice.revision,
        feature_epoch: notice.feature_epoch,
        reason: notice.reason,
    }
}
