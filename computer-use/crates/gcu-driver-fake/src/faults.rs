//! One-shot deterministic faults; no real process crashes or OS input.
use gcu_core::{
    driver::DriverError,
    model::{Geometry, NodeValue},
};
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
    /// Replace visible content in place, preserving geometry and native handle.
    /// Test-only desktop mutation; fingerprints are refreshed by the next read.
    NodeChanged {
        /// Exact window ID.
        window: String,
        /// Existing unique identifier.
        identifier: String,
        /// Replacement accessible name.
        name: String,
        /// Replacement value, including None to clear it.
        value: Option<NodeValue>,
    },
    /// Replace a synthetic process instance without changing its App/window IDs.
    ProcessChanged {
        /// Exact App ID.
        app: String,
        /// New synthetic process ID.
        pid: u32,
    },
    /// Typed native refusal, useful for zero-side-effect gate contract tests.
    Error(DriverError),
    /// Stall the next native read/action, subject to cancellation and hard timeout.
    Delay(Duration),
    /// Deliver now, expose the effect only after the specified settle interval.
    /// Supported only for actions, otherwise returns InvalidRequest.
    SlowSettle(Duration),
}
