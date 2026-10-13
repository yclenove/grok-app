//! Actual original App load and OwnedTasks discovery, not a ready picker mutation.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub(super) struct DiscoveryProbe {
    entered: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
    started: Option<Instant>,
    worker: Option<tokio::task::JoinHandle<Result<Value, String>>>,
    original_entry: Option<uuid::Uuid>,
    dropped: bool,
}

impl DiscoveryProbe {
    pub(super) fn start(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        manager: Arc<SessionManager>,
        key: &HostRequestKey,
        picker: &str,
        broker: &crate::computer_use::ComputerUseBroker,
    ) -> Result<(), String> {
        if self.worker.is_some() || self.entered.load(Ordering::SeqCst) {
            return Err("original discovery worker repeated".into());
        }
        let entered = self.entered.clone();
        let release = self.release.clone();
        broker.arm_scope_discovery_hook_for_test(Box::new(move |count| {
            assert_eq!(
                count, 3,
                "original discovery did not capture all three real shares"
            );
            entered.store(true, Ordering::SeqCst);
            let deadline = Instant::now() + Duration::from_secs(15);
            while !release.load(Ordering::SeqCst) {
                assert!(
                    Instant::now() < deadline,
                    "original discovery barrier not released"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }));
        let key = key.clone();
        let picker = picker.to_owned();
        self.started = Some(Instant::now());
        self.worker = Some(runtime.spawn(async move {
            let view = manager
                .load_computer_use_task_picker("view-app", &key, &picker)
                .await?;
            serde_json::to_value(view).map_err(|_| "original discovery serialization failed".into())
        }));
        Ok(())
    }

    pub(super) fn ready(
        &mut self,
        manager: &SessionManager,
        broker: &crate::computer_use::ComputerUseBroker,
        approvals: &FixtureApprovals,
    ) -> Result<Value, String> {
        if !self.entered.load(Ordering::SeqCst) {
            return Ok(json!({"ready":false}));
        }
        let original_entry = crate::computer_use::task_picker::original_discovery_entry_for_test()?;
        if self.original_entry.is_some_and(|id| id != original_entry) {
            return Err("original discovery registry owner changed".into());
        }
        self.original_entry = Some(original_entry);
        let state = manager.computer_use_task_view("view-app")?;
        let owners = crate::computer_use::task_picker::discovery_owners_for_test();
        if state.feature_enabled
            || !state.grants.is_empty()
            || !state.executions.is_empty()
            || approvals.owners() != 0
            || owners != 1
            || approvals.settings()["computerUseEnabled"] != false
            || broker.continuation_count("view-app") != 0
        {
            return Err("original discovery gate has unexpected authority or owners".into());
        }
        Ok(
            json!({"ready":true,"state":state,"discoveryWorkers":owners,"originalCandidates":3,"stored":false}),
        )
    }

    pub(super) fn release_and_join(
        &mut self,
        runtime: &tokio::runtime::Runtime,
    ) -> Result<Result<Value, String>, String> {
        self.within_original_budget()?;
        self.release.store(true, Ordering::SeqCst);
        let outcome = runtime
            .block_on(
                self.worker
                    .as_mut()
                    .ok_or("original discovery worker missing")?,
            )
            .map_err(|_| "original discovery worker did not join")?;
        self.worker.take();
        self.check_join_deadline()?;
        Ok(outcome)
    }

    fn within_original_budget(&self) -> Result<(), String> {
        if !self.entered.load(Ordering::SeqCst) || self.original_entry.is_none() {
            return Err("original discovery registry owner not captured".into());
        }
        // The production discovery deadline is five seconds. Reject a slow
        // fixture instead of mistaking expiry for proof-revocation evidence.
        if self
            .started
            .ok_or("original discovery start missing")?
            .elapsed()
            >= Duration::from_secs(4)
        {
            return Err("native revocation exceeded the original discovery evidence budget".into());
        }
        Ok(())
    }

    fn check_join_deadline(&self) -> Result<(), String> {
        if self.started.unwrap().elapsed() >= Duration::from_secs(5) {
            return Err("native discovery result exceeded the original production deadline".into());
        }
        Ok(())
    }

    pub(super) fn drop_and_join_original(
        &mut self,
        runtime: &tokio::runtime::Runtime,
    ) -> Result<(), String> {
        self.within_original_budget()?;
        let worker = self
            .worker
            .as_mut()
            .ok_or("original discovery worker missing")?;
        self.dropped = true;
        worker.abort();
        let aborted = runtime.block_on(worker);
        self.worker.take();
        if !aborted.is_err_and(|error| error.is_cancelled()) {
            return Err("original discovery outer waiter was not cancelled".into());
        }
        if crate::computer_use::task_picker::discovery_owners_for_test() != 1 {
            return Err("original dropped discovery lost its inner owner".into());
        }
        self.release.store(true, Ordering::SeqCst);
        self.join_original_abandoned(runtime)?;
        self.check_join_deadline()
    }

    fn join_original_abandoned(&mut self, runtime: &tokio::runtime::Runtime) -> Result<(), String> {
        if let Some(id) = self.original_entry {
            let outcome = runtime.block_on(
                crate::computer_use::task_picker::join_abandoned_discovery_for_test(id),
            )?;
            self.original_entry.take();
            if outcome.err().as_deref() != Some("Computer Use scope picker unavailable or retired")
            {
                return Err("original dropped discovery did not reject its fenced call".into());
            }
        }
        Ok(())
    }

    pub(super) fn finish(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        broker: &crate::computer_use::ComputerUseBroker,
    ) -> Result<(), String> {
        if let Some(worker) = self.worker.as_mut() {
            broker.fence_task_consent("view-app");
            self.release.store(true, Ordering::SeqCst);
            let _outcome = runtime
                .block_on(worker)
                .map_err(|_| "original discovery cleanup did not join")?;
            self.worker.take();
        }
        if self.dropped && self.original_entry.is_some() {
            broker.fence_task_consent("view-app");
            self.release.store(true, Ordering::SeqCst);
            self.join_original_abandoned(runtime)?;
        }
        Ok(())
    }
}
