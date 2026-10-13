//! Test-only barrier BEFORE the unchanged original App/ACP publication pin.
use super::*;
use crate::acp_client::computer_use::TaskApprovalPin;
use grok_computer_use_core::task_consent::CommitOutcome;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub(super) struct PublicationProbe {
    entered: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
    worker: Option<tokio::task::JoinHandle<Result<CommitOutcome, String>>>,
    run: Option<String>,
    original_entry: Option<uuid::Uuid>,
    dropped: bool,
    entry_hits: Arc<AtomicUsize>,
}

impl PublicationProbe {
    pub(super) fn start(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        client: Arc<crate::acp_client::AcpClient>,
        selection: (&HostRequestKey, &str, Vec<String>),
        original: TaskApprovalPin,
        approvals: Arc<FixtureApprovals>,
    ) -> Result<(), String> {
        if self.worker.is_some() || self.entered.load(Ordering::SeqCst) {
            return Err("native publication probe repeated its original worker".into());
        }
        let (key, picker, candidates) = selection;
        let key = key.clone();
        let picker = picker.to_string();
        let entered = self.entered.clone();
        let release = self.release.clone();
        let entry_hits = self.entry_hits.clone();
        let pin = Arc::new(
            move |owner: &crate::acp_client::computer_use::TaskApprovalOwner,
                  approval: &mut grok_computer_use_core::broker::AccessApproval| {
                entry_hits.fetch_add(1, Ordering::SeqCst);
                entered.store(true, Ordering::SeqCst);
                let deadline = Instant::now() + Duration::from_secs(15);
                while !release.load(Ordering::SeqCst) {
                    if Instant::now() >= deadline {
                        return Err("native publication probe was not released".into());
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                // Still use the real App/process/agent/ACP pin. No substitute commit.
                original(owner, approval)
            },
        );
        self.worker = Some(runtime.spawn(async move {
            client
                .approve_existing_fixture(&key, &picker, candidates, pin, approvals)
                .await
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
        let state = manager.computer_use_task_view("view-app")?;
        if !state.feature_enabled
            || !state.grants.is_empty()
            || state.executions.len() != 1
            || approvals.owners() != 1
            || approvals.started() != 1
            || self.entry_count() != 1
            || approvals.settings()["computerUseEnabled"] != true
            || broker.executable_grant_count("view-app") != 0
            || broker.continuation_count("view-app") != 0
        {
            return Err("native publication gate is not the original owned preparation".into());
        }
        let run = state.executions[0].run_id.clone();
        if self.run.as_ref().is_some_and(|original| original != &run) {
            return Err("native publication gate followed a replacement run".into());
        }
        self.run = Some(run.clone());
        let entry = approvals.original_entry()?;
        if self
            .original_entry
            .is_some_and(|original| original != entry)
        {
            return Err("original publication registry owner changed".into());
        }
        self.original_entry = Some(entry);
        Ok(
            json!({"ready":true,"runId":run,"state":state,"ownedWorkers":1,"stored":true,
            "originalRegistryCaptured":true,"publicationEntries":self.entry_count()}),
        )
    }

    pub(super) fn release_and_join(
        &mut self,
        runtime: &tokio::runtime::Runtime,
    ) -> Result<(String, Result<CommitOutcome, String>), String> {
        let run = self.run.clone().ok_or("original publication run missing")?;
        self.release.store(true, Ordering::SeqCst);
        let outcome = runtime
            .block_on(
                self.worker
                    .as_mut()
                    .ok_or("original publication worker missing")?,
            )
            .map_err(|_| "original publication worker did not join")?;
        self.worker.take();
        self.original_entry.take();
        Ok((run, outcome))
    }

    pub(super) fn entry_count(&self) -> usize {
        self.entry_hits.load(Ordering::SeqCst)
    }

    pub(super) fn original_run(&self) -> Result<&str, String> {
        self.run
            .as_deref()
            .ok_or("original publication run missing".into())
    }

    pub(super) fn drop_and_join_original(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        approvals: &FixtureApprovals,
    ) -> Result<(String, Result<CommitOutcome, String>), String> {
        if self.entry_count() != 1 || self.original_entry.is_none() {
            return Err("original publication registry owner not captured".into());
        }
        let worker = self
            .worker
            .as_mut()
            .ok_or("original publication waiter missing")?;
        self.dropped = true;
        worker.abort();
        let result = runtime.block_on(worker);
        self.worker.take();
        if !result.is_err_and(|error| error.is_cancelled()) {
            return Err("original publication outer waiter was not cancelled".into());
        }
        if approvals.original_entry()? != self.original_entry.unwrap() {
            return Err("original dropped publication lost its inner owner".into());
        }
        self.release.store(true, Ordering::SeqCst);
        let run = self.original_run()?.to_string();
        let outcome = self.join_original_abandoned(runtime, approvals)?;
        Ok((run, outcome))
    }

    fn join_original_abandoned(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        approvals: &FixtureApprovals,
    ) -> Result<Result<CommitOutcome, String>, String> {
        let id = self
            .original_entry
            .ok_or("original publication registry owner missing")?;
        let outcome = runtime.block_on(approvals.join_abandoned(id))?;
        self.original_entry.take();
        Ok(outcome)
    }

    pub(super) fn finish(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        broker: &crate::computer_use::ComputerUseBroker,
        approvals: &FixtureApprovals,
    ) -> Result<(), String> {
        if let Some(worker) = self.worker.as_mut() {
            // On probe failure, fence first: releasing a fixture barrier is not approval.
            broker.fence_task_consent("view-app");
            self.release.store(true, Ordering::SeqCst);
            let _outcome = runtime
                .block_on(worker)
                .map_err(|_| "original publication worker cleanup did not join")?;
            self.worker.take();
            self.original_entry.take();
        }
        if self.dropped && self.original_entry.is_some() {
            broker.fence_task_consent("view-app");
            self.release.store(true, Ordering::SeqCst);
            let _outcome = self.join_original_abandoned(runtime, approvals)?;
        }
        Ok(())
    }
}
