//! Task consent authority. Preparation is not a grant.
//!
//! A model request stays pending until a real bind succeeds and the authority
//! lock rechecks the full owner binding. Epoch only moves forward.

mod access_status;
mod activity;
mod bounds;
mod completion;
mod grant;
mod host_view;
mod managed_resources;
mod owner;
mod parse;
mod prepare;
mod recovery;
mod request;
mod scope_selection;
mod ticket;
mod transport;
mod types;
mod webview_selection;

pub(crate) use activity::PhysicalActivity;
pub(crate) use host_view::connection_id as host_connection_id;
pub use host_view::{
    HostGrantView, HostRequestKey, HostRequestPhase, HostRequestView, HostScopeMemberView,
};
pub use types::{
    tool_need, AbortEffect, AuthorityClaim, BearerClass, CommitOutcome, CommitReady,
    ConsentBinding, ExecutionPermit, ExecutionTargetClaim, IssuedTransport, NoAuthorityNotice,
    PrepareTicket, ScopeMember, ScopeMemoryView, ToolNeed, TransportContext,
    WebViewSelectionTicket, MAX_SCOPE_MEMBERS, SYNTHETIC_DECISION_CLASS,
};
pub(crate) use types::{
    ManagedResourceReady, ManagedResourceTicket, NativeScopeProof, ScopeSelectionPermit,
};

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
#[cfg(test)]
use std::time::Duration;

use types::{
    Clock, Decision, GrantRecord, Owner, Preparation, RequestRecord, ScopeMemory, TokenBody,
    MAX_EXPIRED_RUNS,
};

/// Counts of authority rows. Test code uses this to check caps; it grants nothing.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorityCensus {
    pub owners: usize,
    pub requests: usize,
    pub preparations: usize,
    pub decisions: usize,
    pub grants: usize,
    pub scopes: usize,
    pub retired_tokens: usize,
    pub retired_outbox: usize,
    pub expired_runs: usize,
    pub cleanup_tickets: usize,
}

pub struct ConsentState {
    pub(in crate::task_consent) clock: Clock,
    pub(in crate::task_consent) owners: HashMap<String, Owner>,
    pub(in crate::task_consent) tokens: HashMap<String, TokenBody>,
    pub(in crate::task_consent) retired_tokens: VecDeque<String>,
    pub(in crate::task_consent) retired_lookup: HashSet<String>,
    pub(in crate::task_consent) retired_outbox: Vec<String>,
    pub(in crate::task_consent) requests: Vec<RequestRecord>,
    pub(in crate::task_consent) grants: Vec<GrantRecord>,
    pub(in crate::task_consent) preparations: Vec<Preparation>,
    pub(in crate::task_consent) decisions: VecDeque<Decision>,
    pub(in crate::task_consent) scopes: Vec<ScopeMemory>,
    pub(in crate::task_consent) expired_runs: Vec<String>,
    pub(in crate::task_consent) next_nonce: u64,
    pub(in crate::task_consent) next_preparation: u64,
    before_enable: Option<Box<dyn FnOnce() + Send>>,
    after_enable: Option<Box<dyn FnOnce() + Send>>,
    before_open: Option<Box<dyn FnOnce() + Send>>,
    after_open: Option<Box<dyn FnOnce() + Send>>,
    after_bind: Option<Box<dyn FnOnce() + Send>>,
    before_finalize: Option<Box<dyn FnOnce() + Send>>,
    after_target_switch: Option<Box<dyn FnOnce() + Send>>,
    before_final_admit: Option<Box<dyn FnOnce() + Send>>,
    execution_barrier: Option<Box<dyn FnOnce() + Send>>,
}

impl fmt::Debug for ConsentState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConsentState")
            .field("owners", &self.owners.len())
            .field("live_transports", &self.tokens.len())
            .field("retired_transports", &self.retired_tokens.len())
            .field("requests", &self.requests.len())
            .field("grants", &self.grants.len())
            .field("preparations", &self.preparations.len())
            .field("decisions", &self.decisions.len())
            .field("scopes", &self.scopes.len())
            .finish()
    }
}

impl Default for ConsentState {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsentState {
    pub(crate) fn sweep_from_host(&mut self, busy: &HashSet<String>) {
        self.sweep(busy);
    }

    pub fn new() -> Self {
        Self {
            clock: Clock::new(),
            owners: HashMap::new(),
            tokens: HashMap::new(),
            retired_tokens: VecDeque::new(),
            retired_lookup: HashSet::new(),
            retired_outbox: Vec::new(),
            requests: Vec::new(),
            grants: Vec::new(),
            preparations: Vec::new(),
            decisions: VecDeque::new(),
            scopes: Vec::new(),
            expired_runs: Vec::new(),
            next_nonce: 1,
            next_preparation: 1,
            before_enable: None,
            after_enable: None,
            before_open: None,
            after_open: None,
            after_bind: None,
            before_finalize: None,
            after_target_switch: None,
            before_final_admit: None,
            execution_barrier: None,
        }
    }

    #[cfg(test)]
    pub fn advance_clock(&mut self, by: Duration, busy: &HashSet<String>) {
        self.clock.advance(by);
        self.sweep(busy);
    }

    #[cfg(test)]
    pub(crate) fn advance_clock_without_sweep(&mut self, by: Duration) {
        self.clock.advance(by);
    }

    #[cfg(test)]
    pub fn arm_before_enable(&mut self, hook: Box<dyn FnOnce() + Send>) {
        self.before_enable = Some(hook);
    }

    #[cfg(test)]
    pub fn arm_after_enable(&mut self, hook: Box<dyn FnOnce() + Send>) {
        self.after_enable = Some(hook);
    }

    #[cfg(test)]
    pub fn arm_before_open(&mut self, hook: Box<dyn FnOnce() + Send>) {
        self.before_open = Some(hook);
    }

    #[cfg(test)]
    pub fn arm_after_bind(&mut self, hook: Box<dyn FnOnce() + Send>) {
        self.after_bind = Some(hook);
    }

    #[cfg(test)]
    pub fn arm_after_open(&mut self, hook: Box<dyn FnOnce() + Send>) {
        self.after_open = Some(hook);
    }

    #[cfg(test)]
    pub fn arm_before_finalize(&mut self, hook: Box<dyn FnOnce() + Send>) {
        self.before_finalize = Some(hook);
    }

    #[cfg(test)]
    pub fn arm_after_target_switch(&mut self, hook: Box<dyn FnOnce() + Send>) {
        self.after_target_switch = Some(hook);
    }

    #[cfg(test)]
    pub fn arm_before_final_admit(&mut self, hook: Box<dyn FnOnce() + Send>) {
        self.before_final_admit = Some(hook);
    }

    #[cfg(test)]
    pub fn arm_execution_barrier(&mut self, hook: Box<dyn FnOnce() + Send>) {
        self.execution_barrier = Some(hook);
    }

    pub fn take_before_enable(&mut self) -> Option<Box<dyn FnOnce() + Send>> {
        self.before_enable.take()
    }

    pub fn take_after_enable(&mut self) -> Option<Box<dyn FnOnce() + Send>> {
        self.after_enable.take()
    }

    pub fn take_before_open(&mut self) -> Option<Box<dyn FnOnce() + Send>> {
        self.before_open.take()
    }

    pub fn take_after_bind(&mut self) -> Option<Box<dyn FnOnce() + Send>> {
        self.after_bind.take()
    }

    pub fn take_after_open(&mut self) -> Option<Box<dyn FnOnce() + Send>> {
        self.after_open.take()
    }

    pub fn take_before_finalize(&mut self) -> Option<Box<dyn FnOnce() + Send>> {
        self.before_finalize.take()
    }

    pub fn take_after_target_switch(&mut self) -> Option<Box<dyn FnOnce() + Send>> {
        self.after_target_switch.take()
    }

    pub fn take_before_final_admit(&mut self) -> Option<Box<dyn FnOnce() + Send>> {
        self.before_final_admit.take()
    }

    pub fn take_execution_barrier(&mut self) -> Option<Box<dyn FnOnce() + Send>> {
        self.execution_barrier.take()
    }

    #[cfg(test)]
    pub fn reservation_count(&self) -> usize {
        self.preparations.len()
    }

    pub fn take_expired_runs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.expired_runs)
    }

    #[cfg(test)]
    pub fn census(&self) -> AuthorityCensus {
        AuthorityCensus {
            owners: self.owners.len(),
            requests: self.requests.len(),
            preparations: self.preparations.len(),
            decisions: self.decisions.len(),
            grants: self.grants.len(),
            scopes: self.scopes.len(),
            retired_tokens: self.retired_tokens.len(),
            retired_outbox: self.retired_outbox.len(),
            expired_runs: self.expired_runs.len(),
            cleanup_tickets: 0,
        }
    }

    /// Record a run that still needs a fence. At the cap, leave the caller holding
    /// the grant or the cleanup flag instead of dropping the obligation.
    pub(in crate::task_consent) fn queue_expired_run(&mut self, run_id: &str) -> bool {
        if run_id.is_empty() {
            return false;
        }
        if self.expired_runs.iter().any(|id| id == run_id) {
            return true;
        }
        if self.expired_runs.len() >= MAX_EXPIRED_RUNS {
            return false;
        }
        self.expired_runs.push(run_id.to_string());
        true
    }
}

#[cfg(test)]
mod tests_deadlines;

#[cfg(test)]
mod tests_presentation;
