//! Action timeline events for the Host panel.

use crate::types::{ImageRef, SessionCtx};

/// One action the engine wants on the Host timeline.
///
/// `thumbnail` is in memory. The event does not carry a filesystem path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuEvent {
    /// Tool that ran.
    pub tool: gcu_protocol::ToolName,
    /// Short target label, such as an element name or a point.
    pub target: String,
    /// Architecture §6 outcome.
    pub outcome: gcu_protocol::result::Outcome,
    /// Wall time of the tool call, in milliseconds.
    pub duration_ms: u64,
    /// Optional in-memory thumbnail.
    pub thumbnail: Option<ImageRef>,
}

/// Receives timeline events. The Host owns display and retention.
pub trait EventSink: Send + Sync {
    /// Accept one event.
    ///
    /// The sink owns `event` after the call, including any thumbnail bytes.
    ///
    /// Blocking: must not wait on the user, disk, or a full UI queue. Return
    /// after the event is queued or deliberately dropped.
    /// Timeout: none. If the Host cannot enqueue immediately, it drops the
    /// event instead of waiting.
    /// Cancellation: none. A closed session stops further calls; `emit` itself
    /// is not cancelled.
    /// Thread safety: `Send + Sync`. Concurrent `emit` calls are allowed.
    fn emit(&self, session: &SessionCtx, event: CuEvent);
}
