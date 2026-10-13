//! Per-owner and global caps. Live tickets and grants are not deleted to look bounded.
use crate::error::BrokerError;

use super::types::{
    schema, Decision, Notice, ScopeMemory, Ticket, MAX_DECISIONS_GLOBAL, MAX_DECISIONS_PER_OWNER,
    MAX_PREPARATIONS_GLOBAL, MAX_PREPARATIONS_PER_OWNER, MAX_REQUESTS_GLOBAL,
    MAX_REQUESTS_PER_OWNER, MAX_SCOPES_GLOBAL, MAX_SCOPES_PER_OWNER,
};
use super::ConsentState;

impl ConsentState {
    pub(super) fn trim_stored_requests(&mut self) {
        while self.requests.len() > MAX_REQUESTS_GLOBAL {
            let Some(index) = self.requests.iter().position(|row| row.state.is_terminal()) else {
                break;
            };
            self.requests.remove(index);
        }
        let sessions: Vec<String> = self
            .requests
            .iter()
            .map(|row| row.app_session_id.clone())
            .collect();
        for session in sessions {
            while self
                .requests
                .iter()
                .filter(|row| row.app_session_id == session)
                .count()
                > MAX_REQUESTS_PER_OWNER
            {
                let Some(index) = self
                    .requests
                    .iter()
                    .position(|row| row.app_session_id == session && row.state.is_terminal())
                else {
                    break;
                };
                self.requests.remove(index);
            }
        }
    }

    pub(super) fn stored_request_room(&self, session: &str) -> Result<(), BrokerError> {
        let owner_live = self
            .requests
            .iter()
            .filter(|row| row.app_session_id == session && !row.state.is_terminal())
            .count();
        let global_live = self
            .requests
            .iter()
            .filter(|row| !row.state.is_terminal())
            .count();
        if owner_live >= MAX_REQUESTS_PER_OWNER || global_live >= MAX_REQUESTS_GLOBAL {
            return Err(schema("request limit"));
        }
        Ok(())
    }

    pub(super) fn active_preparation_room(&self, session: &str) -> Result<(), BrokerError> {
        let global = self
            .preparations
            .iter()
            .filter(|row| !row.rolled_back)
            .count();
        let owner = self
            .preparations
            .iter()
            .filter(|row| !row.rolled_back && row.app_session_id == session)
            .count();
        if global >= MAX_PREPARATIONS_GLOBAL || owner >= MAX_PREPARATIONS_PER_OWNER {
            return Err(schema("request limit"));
        }
        Ok(())
    }

    pub(super) fn push_notice(&mut self, notice: Notice) {
        let session = notice.app_session_id.clone();
        if self.make_decision_room(&session) {
            self.decisions.push_back(Decision::NoAuthority(notice));
        }
    }

    pub(super) fn push_ticket(&mut self, ticket: Ticket) -> Result<(), BrokerError> {
        let session = ticket.app_session_id.clone();
        if !self.make_decision_room(&session) {
            return Err(schema("request limit"));
        }
        self.decisions.push_back(Decision::Continuation(ticket));
        Ok(())
    }

    pub(super) fn remember_scope(&mut self, memory: ScopeMemory) {
        if self.scopes.iter().any(|row| {
            row.app_session_id == memory.app_session_id
                && row.purpose == memory.purpose
                && row.scope_intent == memory.scope_intent
                && row.members == memory.members
        }) {
            return;
        }
        let session = memory.app_session_id.clone();
        self.scopes.push(memory);
        self.drop_oldest_scopes(MAX_SCOPES_GLOBAL, |_| true);
        self.drop_oldest_scopes(MAX_SCOPES_PER_OWNER, |row| row.app_session_id == session);
    }

    fn make_decision_room(&mut self, session: &str) -> bool {
        self.drop_decision_overflow(MAX_DECISIONS_GLOBAL, |_| true);
        self.drop_decision_overflow(MAX_DECISIONS_PER_OWNER, |decision| {
            decision_session(decision) == session
        });
        self.decisions.len() < MAX_DECISIONS_GLOBAL
            && self
                .decisions
                .iter()
                .filter(|decision| decision_session(decision) == session)
                .count()
                < MAX_DECISIONS_PER_OWNER
    }

    fn drop_decision_overflow(&mut self, cap: usize, count: impl Fn(&Decision) -> bool) {
        loop {
            let mut matching = 0usize;
            for row in &self.decisions {
                if count(row) {
                    matching += 1;
                }
            }
            if matching < cap {
                break;
            }
            let mut index = None;
            for (at, row) in self.decisions.iter().enumerate() {
                if count(row) && decision_droppable(row) {
                    index = Some(at);
                    break;
                }
            }
            let Some(index) = index else {
                break;
            };
            self.decisions.remove(index);
        }
    }

    fn drop_oldest_scopes(&mut self, cap: usize, count: impl Fn(&ScopeMemory) -> bool) {
        loop {
            let mut matching = 0usize;
            for row in &self.scopes {
                if count(row) {
                    matching += 1;
                }
            }
            if matching <= cap {
                break;
            }
            let mut index = None;
            for (at, row) in self.scopes.iter().enumerate() {
                if count(row) {
                    index = Some(at);
                    break;
                }
            }
            let Some(index) = index else {
                break;
            };
            self.scopes.remove(index);
        }
    }
}

fn decision_session(decision: &Decision) -> &str {
    match decision {
        Decision::Continuation(ticket) => ticket.app_session_id.as_str(),
        Decision::NoAuthority(notice) => notice.app_session_id.as_str(),
    }
}

fn decision_droppable(decision: &Decision) -> bool {
    match decision {
        Decision::NoAuthority(_) => true,
        Decision::Continuation(ticket) => ticket.consumed || ticket.invalid,
    }
}
