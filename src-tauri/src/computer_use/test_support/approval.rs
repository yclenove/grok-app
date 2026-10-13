//! Test-only injection into the unchanged owned approval worker transaction.
use super::super::{feature_preferences::FeaturePreferences, task_access, ComputerUseBroker};
use grok_computer_use_core::{broker::AccessApproval, task_consent::CommitOutcome};
use std::{path::Path, sync::Arc};

pub(crate) struct FixtureApprovals {
    _feature_guard: tokio::sync::MutexGuard<'static, ()>,
    preferences: FeaturePreferences,
    workers: Arc<super::super::owned_tasks::OwnedTasks<Result<CommitOutcome, String>>>,
    started: std::sync::atomic::AtomicUsize,
}

impl FixtureApprovals {
    /// Construct outside Tokio, with an already-created isolated fixture root.
    pub(crate) fn new(root: &Path) -> Self {
        let guard = super::super::feature::TEST_LOCK.blocking_lock();
        let preferences = FeaturePreferences::at(root.join("settings.json"));
        preferences.set_explicit(false).unwrap();
        Self {
            _feature_guard: guard,
            preferences,
            workers: Arc::new(Default::default()),
            started: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub(crate) fn settings(&self) -> serde_json::Value {
        self.preferences.snapshot()
    }

    pub(crate) fn owners(&self) -> usize {
        self.workers.test_entry_count()
    }

    #[cfg(windows)]
    pub(crate) fn started(&self) -> usize {
        self.started.load(std::sync::atomic::Ordering::SeqCst)
    }

    #[cfg(windows)]
    pub(crate) fn original_entry(&self) -> Result<uuid::Uuid, String> {
        self.workers.singleton_entry_for_test()
    }

    #[cfg(windows)]
    pub(crate) async fn join_abandoned(
        &self,
        id: uuid::Uuid,
    ) -> Result<Result<CommitOutcome, String>, String> {
        self.workers.join_abandoned_for_test(id).await
    }

    pub(crate) async fn run<F>(
        &self,
        broker: Arc<ComputerUseBroker>,
        approval: AccessApproval,
        publication: task_access::ApprovalPublication,
        prepare_runtime: F,
    ) -> Result<CommitOutcome, String>
    where
        F: FnOnce() -> Result<(), String> + Send + 'static,
    {
        self.started
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        task_access::approve_with_publication(
            broker,
            approval,
            self.preferences.clone(),
            self.workers.clone(),
            publication,
            prepare_runtime,
        )
        .await
    }

    pub(crate) async fn join(&self) -> Result<(), String> {
        self.workers.join().await
    }

    pub(crate) async fn disable(
        &self,
        manager: Arc<crate::session_manager::SessionManager>,
        broker: Arc<ComputerUseBroker>,
    ) -> Result<(), String> {
        let preferences = self.preferences.clone();
        super::super::feature_lifecycle::persist_feature_transition_with_broker(
            manager,
            broker,
            false,
            move || preferences.set_explicit(false),
        )
        .await
    }

    /// Synthetic explicit Host setup before any model request or task picker.
    pub(crate) async fn enable_before_request(
        &self,
        manager: Arc<crate::session_manager::SessionManager>,
        broker: Arc<ComputerUseBroker>,
    ) -> Result<(), String> {
        let preferences = self.preferences.clone();
        super::super::feature_lifecycle::persist_feature_transition_with_broker(
            manager,
            broker,
            true,
            move || preferences.set_explicit(true),
        )
        .await
    }

    pub(crate) fn reset_feature(&self, broker: &ComputerUseBroker) {
        assert_eq!(self.owners(), 0, "original approval owner has not joined");
        broker.set_feature_enabled(false);
        super::super::set_feature_enabled(false);
    }
}
