//! Immutable authority captured during Broker admission, never at dispatch.
use std::sync::Arc;

use super::{ManagedBrowserWorker, WorkerRunRevision};
use crate::execution::ActionCancellation;

#[derive(Clone, Debug)]
pub struct ManagedRequestIdentity {
    pub(crate) owner: String,
    pub(crate) revision: WorkerRunRevision,
    pub(crate) cancellation: ActionCancellation,
}

impl ManagedRequestIdentity {
    pub fn owner(&self) -> &str {
        &self.owner
    }
    pub fn revision(&self) -> WorkerRunRevision {
        self.revision
    }
    pub fn cancellation(&self) -> &ActionCancellation {
        &self.cancellation
    }
}

#[derive(Clone)]
pub struct ManagedRequest {
    pub(crate) identity: ManagedRequestIdentity,
    pub(crate) worker: Arc<dyn ManagedBrowserWorker>,
}

impl std::fmt::Debug for ManagedRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The worker may contain a Bearer credential; never debug-print it.
        f.debug_struct("ManagedRequest")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}
