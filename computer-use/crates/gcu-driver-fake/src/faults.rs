//! One-shot deterministic faults; no real process crashes or OS input.
use gcu_core::model::Geometry;
use std::time::Duration;

/// Injected at the next tree read, capture, or action. Structural faults take
/// effect before that operation; uncertainty faults apply a mutation at most once.
#[derive(Debug, Clone)]
pub enum Fault {
    /// Return timeout, optionally after one successful delivery.
    Timeout {
        /// Execute a mutation once before reporting uncertainty.
        maybe_executed: bool,
    },
    /// Simulate driver crash, optionally after one successful delivery.
    Crash {
        /// Execute a mutation once and leave simulated input held.
        maybe_executed: bool,
    },
    /// Remove the exact window; no substitute is selected.
    WindowGone {
        /// Window ID.
        window: String,
    },
    /// Move/resize/change DPI before the next operation.
    GeometryChanged {
        /// Window ID.
        window: String,
        /// Replacement geometry.
        geometry: Geometry,
    },
    /// Remove an exact element, making existing handles stale.
    ElementDeleted {
        /// Window ID.
        window: String,
        /// Unique node identifier.
        identifier: String,
    },
    /// Stall the next native read/action, subject to cancellation and hard timeout.
    Delay(Duration),
    /// Deliver now, expose the effect only after the specified settle interval.
    /// Supported only for actions, otherwise returns InvalidRequest.
    SlowSettle(Duration),
}
