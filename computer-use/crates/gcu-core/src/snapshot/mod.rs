//! Session-local observations. All methods are synchronous pure memory operations:
//! no IO, native blocking, timeouts, retries, or cancellation waits. Each Host owns
//! one store per session; sharing a store between sessions is forbidden.

mod diff;
mod index;
mod store;
pub use diff::{Change, Diff, RenderedNode, RenderedState};
pub use index::ElementIndex;
pub use store::{
    Observation, ObservationMode, Snapshot, SnapshotId, SnapshotStore, WindowKey,
    DEFAULT_MEMORY_LIMIT, SNAPSHOTS_PER_WINDOW,
};

/// Payload-free snapshot/index failure, suitable for safe logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotError {
    /// Evicted snapshot, removed element, wrong window, or unknown index.
    #[error("stale element reference")]
    StaleElement,
    /// Driver supplied ambiguous fingerprints; indices cannot be safely aligned.
    #[error("duplicate element fingerprint")]
    DuplicateFingerprint,
    /// The monotonically increasing index/snapshot identifier space is exhausted.
    #[error("snapshot identifier space exhausted")]
    IndexExhausted,
    /// A single snapshot exceeds the whole session's memory limit.
    #[error("snapshot exceeds session memory limit")]
    MemoryLimit,
    /// Image is a derivative or does not match the tree version/window geometry.
    #[error("screenshot does not match this observation")]
    StaleScreenshot,
    /// A diff was applied to an incompatible baseline.
    #[error("diff baseline is stale")]
    StaleDiff,
}
