//! Barrier in the actual Core reservation invoked by the original owned worker.
use super::*;
use crate::acp_client::computer_use::TaskApprovalPin;
use grok_computer_use_core::task_consent::CommitOutcome;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub(super) struct ReservationProbe {
    entered: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
    worker: Option<tokio::task::JoinHandle<Result<CommitOutcome, String>>>,
    enabled_before: bool,
    publication_run: Arc<parking_lot::Mutex<Option<String>>>,
    original_entry: Option<uuid::Uuid>,
    dropped: bool,
    entry_hits: Arc<AtomicUsize>,
}

impl ReservationProbe {
    pub(super) fn record_original_pin(
        &self,
        original: TaskApprovalPin,
        read: impl Fn() -> Result<grok_computer_use_core::broker::HostTaskView, String>
            + Send
            + Sync
            + 'static,
    ) -> TaskApprovalPin {
        let run = self.publication_run.clone();
        Arc::new(move |owner, approval| {
            let state = read()?;
            if !state.feature_enabled || !state.grants.is_empty() || state.executions.len() != 1 {
                return Err("original reservation did not prepare exactly one execution".into());
            }
            let mut slot = run.lock();
            if slot.is_some() {
                return Err("original reservation repeated its publication pin".into());
            }
            *slot = Some(state.executions[0].run_id.clone());
            drop(slot);
            original(owner, approval)
        })
    }

    pub(super) fn publication_run(&self) -> Option<String> {
        self.publication_run.lock().clone()
    }

    pub(super) fn start(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        client: Arc<crate::acp_client::AcpClient>,
        selection: (&HostRequestKey, &str, Vec<String>),
        pin: TaskApprovalPin,
        approvals: Arc<FixtureApprovals>,
        broker: &crate::computer_use::ComputerUseBroker,
    ) -> Result<(), String> {
        if self.worker.is_some() || self.entered.load(Ordering::SeqCst) {
            return Err("native reservation probe repeated its original worker".into());
        }
        self.enabled_before = broker.feature_enabled();
        let entered = self.entered.clone();
        let release = self.release.clone();
        let entry_hits = self.entry_hits.clone();
        broker.arm_scope_reservation_hook_for_test(Box::new(move || {
            entry_hits.fetch_add(1, Ordering::SeqCst);
            entered.store(true, Ordering::SeqCst);
            let deadline = Instant::now() + Duration::from_secs(15);
            while !release.load(Ordering::SeqCst) {
                assert!(
                    Instant::now() < deadline,
                    "original reservation barrier not released"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }));
        let (key, picker, candidates) = selection;
        let key = key.clone();
        let picker = picker.to_owned();
        self.worker = Some(runtime.spawn(async move {
            // Original load/start_picker_approval/finish_picker_approval,
            // reservation OwnedTasks and constructor; the pin is unchanged.
            client
                .approve_existing_fixture(&key, &picker, candidates, pin, approvals)
                .await
        }));
        Ok(())
    }

    pub(super) fn ready(
        &self,
        manager: &SessionManager,
        broker: &crate::computer_use::ComputerUseBroker,
        approvals: &FixtureApprovals,
    ) -> Result<Value, String> {
        if !self.entered.load(Ordering::SeqCst) {
            return Ok(json!({"ready":false}));
        }
        let state = manager.computer_use_task_view("view-app")?;
        let owners = crate::computer_use::task_picker::reservation_owners_for_test();
        if state.feature_enabled != self.enabled_before
            || !state.grants.is_empty()
            || !state.executions.is_empty()
            || approvals.owners() != 0
            || owners != 1
            || approvals.settings()["computerUseEnabled"] != self.enabled_before
            || broker.continuation_count("view-app") != 0
        {
            return Err("native reservation gate is not the original non-executable worker".into());
        }
        Ok(
            json!({"ready":true,"state":state,"reservationWorkers":owners,"stored":self.enabled_before}),
        )
    }

    pub(super) fn release_and_join(
        &mut self,
        runtime: &tokio::runtime::Runtime,
    ) -> Result<Result<CommitOutcome, String>, String> {
        if !self.entered.load(Ordering::SeqCst) {
            return Err("original reservation not entered".into());
        }
        self.release.store(true, Ordering::SeqCst);
        let result = runtime
            .block_on(
                self.worker
                    .as_mut()
                    .ok_or("original reservation worker missing")?,
            )
            .map_err(|_| "original reservation worker did not join")?;
        self.worker.take();
        self.original_entry.take();
        Ok(result)
    }

    pub(super) fn entry_count(&self) -> usize {
        self.entry_hits.load(Ordering::SeqCst)
    }

    pub(super) fn capture_original_entry(&mut self) -> Result<(), String> {
        if self.entry_count() != 1 {
            return Err("original reservation worker has not entered".into());
        }
        let entry = crate::computer_use::task_picker::original_reservation_entry_for_test()?;
        if self
            .original_entry
            .is_some_and(|original| original != entry)
        {
            return Err("original reservation registry owner changed".into());
        }
        self.original_entry = Some(entry);
        Ok(())
    }

    pub(super) fn drop_and_join_original(
        &mut self,
        runtime: &tokio::runtime::Runtime,
    ) -> Result<(), String> {
        if !self.entered.load(Ordering::SeqCst) || self.original_entry.is_none() {
            return Err("original reservation registry owner not captured".into());
        }
        let worker = self
            .worker
            .as_mut()
            .ok_or("original reservation waiter missing")?;
        self.dropped = true;
        worker.abort();
        let result = runtime.block_on(worker);
        self.worker.take();
        if !result.is_err_and(|error| error.is_cancelled()) {
            return Err("original reservation outer waiter was not cancelled".into());
        }
        if crate::computer_use::task_picker::reservation_owners_for_test() != 1 {
            return Err("original dropped reservation lost its inner owner".into());
        }
        self.release.store(true, Ordering::SeqCst);
        self.join_original_abandoned(runtime)
    }

    fn join_original_abandoned(&mut self, runtime: &tokio::runtime::Runtime) -> Result<(), String> {
        if let Some(id) = self.original_entry {
            let outcome = runtime.block_on(
                crate::computer_use::task_picker::join_abandoned_reservation_for_test(id),
            )?;
            self.original_entry.take();
            if outcome.err().as_deref()
                != Some("Computer Use selected window instance retired or changed")
            {
                return Err("original dropped reservation did not reject its fenced call".into());
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
                .map_err(|_| "original reservation cleanup did not join")?;
            self.worker.take();
            self.original_entry.take();
        }
        if self.dropped && self.original_entry.is_some() {
            broker.fence_task_consent("view-app");
            self.release.store(true, Ordering::SeqCst);
            self.join_original_abandoned(runtime)?;
        }
        Ok(())
    }
}
