//! Production Host callbacks until the approval card replaces them.
//!
//! Policy loads return the baseline snapshot. Approvals deny. Confirmation is
//! false. Overlay, handoff, events, and health return an empty object.

use std::sync::Arc;

use gcu_host_protocol::{
    server::HostCallbacks,
    supervisor::{baseline_policy, closed_callbacks},
};

/// Callbacks for a supervised engine that has no approval UI yet.
pub fn production_callbacks() -> Arc<dyn HostCallbacks> {
    closed_callbacks(baseline_policy())
}
