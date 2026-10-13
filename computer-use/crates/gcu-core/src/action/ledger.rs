//! Bounded session memory; metadata only, no input payloads or native handles.
use gcu_protocol::{
    result::{ActionResult, ActionTarget, Delivery, Effect, Outcome},
    ToolName,
};
use std::{collections::VecDeque, time::Duration};

/// Default number of recent actions retained per session.
pub const DEFAULT_LEDGER_LIMIT: usize = 256;
/// A single delivery attempt. Unknown means the caller must not blindly replay it.
#[derive(Debug, Clone, PartialEq)]
pub struct LedgerEntry {
    /// Session-scoped Host call identifier.
    pub call_id: String,
    /// Invoked tool.
    pub tool: ToolName,
    /// Sanitized target; never typed text.
    pub target: Option<ActionTarget>,
    /// Actual route, or None when definitely not delivered.
    pub delivery: Delivery,
    /// Delivery certainty.
    pub outcome: Outcome,
    /// Observed visible change.
    pub effect: Effect,
    /// Timestamp on the router's monotonic clock.
    pub ts: Duration,
}
/// FIFO action history owned by a Session. All operations are synchronous memory
/// operations without IO, timeout, blocking waits, or cancellation.
#[derive(Debug, Clone)]
pub struct ActionLedger {
    entries: VecDeque<LedgerEntry>,
    limit: usize,
}
impl Default for ActionLedger {
    fn default() -> Self {
        Self::with_limit(DEFAULT_LEDGER_LIMIT)
    }
}
impl ActionLedger {
    /// Create a bounded history; zero retains no entries. No IO or waits.
    pub fn with_limit(limit: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            limit,
        }
    }
    /// Append one attempt and evict the oldest entry if needed. No IO or waits.
    pub fn record(&mut self, call_id: &str, action: &ActionResult, ts: Duration) {
        if self.limit == 0 {
            return;
        }
        if self.entries.len() == self.limit {
            self.entries.pop_front();
        }
        self.entries.push_back(LedgerEntry {
            call_id: call_id.to_owned(),
            tool: action.tool,
            target: action.target.clone(),
            delivery: action.delivery,
            outcome: action.outcome,
            effect: action.effect,
            ts,
        });
    }
    /// Retained entries in attempt order. No IO or waits.
    pub fn entries(&self) -> &VecDeque<LedgerEntry> {
        &self.entries
    }
    /// Whether any retained attempt has uncertain execution. No IO or waits.
    pub fn has_unknown(&self) -> bool {
        self.entries.iter().any(|e| e.outcome == Outcome::Unknown)
    }
    /// Find a retained uncertain attempt by call id. No IO or waits.
    pub fn unknown(&self, call_id: &str) -> Option<&LedgerEntry> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.call_id == call_id && e.outcome == Outcome::Unknown)
    }
}
