//! Single-attempt action delivery, fresh state, and session-local audit records.
//! Native waits are bounded and cancellable; uncertain mutations are never replayed.
mod authorize;
mod discovery;
mod epoch;
mod expect;
mod failure;
mod fixture;
/// Snapshot, geometry, and exact-handle preflight gates.
pub mod gate;
/// Bounded metadata-only action history.
pub mod ledger;
pub(crate) mod observe;
mod provenance;
mod publish_gate;
mod route;
/// Policy-aware portable action executor.
pub mod router;
mod settle;
mod wait;
mod wire;

use crate::{driver::PointerRoute, redact::RedactedImage, snapshot::SnapshotStore};
use gcu_protocol::{result::ActionResult, tools::BatchStep, AppState, ToolError, WindowInfo};
pub use ledger::{ActionLedger, LedgerEntry};
/// Pause point for publication tests. Production leaves it disarmed.
pub use publish_gate::PublishGate;
pub use router::ActionRouter;
use serde::Serialize;
use std::{
    fmt,
    sync::{Mutex, MutexGuard},
};

/// One standalone action, also reusable by batch/script. Debug omits payloads.
#[derive(Clone, Serialize)]
pub struct ToolCall {
    /// Host-generated call identifier, scoped to the session.
    pub call_id: String,
    /// App query; resolved by the driver, never used as authorization identity.
    pub app: String,
    /// Exact window ID; omission resolves the key window once only.
    pub window: Option<String>,
    /// Protocol action payload. `wait_for` is executed by [`ActionRouter::wait_for`].
    /// `expect` is executed by the batch path on this same router, with no input.
    pub action: BatchStep,
    /// Explicit route ceiling. TargetedOnly never falls back to foreground.
    #[serde(skip)]
    pub pointer_route: PointerRoute,
    /// Override the settle ceiling in milliseconds; maximum 1500.
    pub settle_budget_ms: Option<u64>,
    /// Explicitly request a safe post-action image.
    pub attach_image: bool,
}
impl ToolCall {
    /// Construct an action with background-first routing and default settle budget.
    /// Pure memory operation; no waits, timeout, or cancellation.
    pub fn new(call_id: impl Into<String>, app: impl Into<String>, action: BatchStep) -> Self {
        Self {
            call_id: call_id.into(),
            app: app.into(),
            window: None,
            action,
            pointer_route: PointerRoute::Auto,
            settle_budget_ms: None,
            attach_image: false,
        }
    }
}
impl fmt::Debug for ToolCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolCall")
            .field("call_id", &self.call_id)
            .finish_non_exhaustive()
    }
}
/// Delivered observation. Images can only be produced by the mandatory redact API.
/// Debug omits text; native handles and raw captures cannot enter this type.
pub struct ActionState {
    /// Full or fingerprint-aligned diff text.
    pub text: String,
    /// Full sanitized text for a private overflow file, when inline text contracted.
    /// Never deliver this directly as inline MCP content.
    pub overflow_text: Option<String>,
    /// Compact protocol state, also used by ToolError.state.
    pub state: AppState,
    /// Optional safe image, before encoding for the transport.
    pub image: Option<RedactedImage>,
    /// Fixture run that produced this observation. `None` outside a scope.
    /// A scoped window-gone line carries the epoch that listed the remaining
    /// windows. Not a protocol field.
    pub scope_run: Option<String>,
    /// Fixture generation that produced this observation.
    ///
    /// Memory only. No blocking, timeout, or cancellation. A later encode must
    /// not substitute the current generation when this is missing or stale.
    pub scope_generation: Option<u64>,
}
impl fmt::Debug for ActionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionState")
            .field("snapshot_id", &self.state.snapshot_id)
            .finish_non_exhaustive()
    }
}
/// One attempt's result, including failures and uncertain delivery. Never a retry token.
#[derive(Debug)]
pub struct ToolOutcome {
    /// Architecture §6 action facts.
    pub action: ActionResult,
    /// Error when the call could not complete normally; never echoes typed input.
    pub error: Option<ToolError>,
    /// Fresh state after delivery or a stale target.
    pub state: Option<ActionState>,
    /// Fresh discovery on window_gone; no alternate window is operated on.
    pub windows: Vec<WindowInfo>,
}
#[derive(Default)]
pub(crate) struct SessionActions {
    pub snapshots: SnapshotStore,
    pub ledger: ActionLedger,
}
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
