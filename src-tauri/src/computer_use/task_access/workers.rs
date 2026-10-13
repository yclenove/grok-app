//! Process-owned approval workers share the bounded original-handle registry.
use grok_computer_use_core::task_consent::CommitOutcome;
use std::sync::{Arc, OnceLock};

pub(super) type Workers = super::super::owned_tasks::OwnedTasks<Result<CommitOutcome, String>>;

pub(super) fn product() -> Arc<Workers> {
    static WORKERS: OnceLock<Arc<Workers>> = OnceLock::new();
    WORKERS.get_or_init(|| Arc::new(Workers::default())).clone()
}
