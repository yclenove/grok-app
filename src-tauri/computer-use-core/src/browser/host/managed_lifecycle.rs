//! Local fences precede independent, lock-free worker control exchanges.
use super::*;
use crate::browser::{ManagedRequest, ManagedRequestIdentity, WorkerRunPhase, WorkerRunRevision};
use crate::execution::ActionCancellation;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Running,
    Paused,
    Resuming,
    Uncertain,
}

pub(super) struct ManagedRun {
    worker: Arc<dyn ManagedBrowserWorker>,
    revision: WorkerRunRevision,
    phase: Phase,
    cleanup_confirmed: bool,
    pause_epoch: u64,
}

pub(crate) struct ManagedResume {
    owner: String,
    revision: WorkerRunRevision,
    pause_epoch: u64,
    worker: Arc<dyn ManagedBrowserWorker>,
}

impl ExistingTabHost {
    /// Local-only: safe under the Broker admission lock. Worker/revision/token
    /// are captured together and remain immutable for every sub-request.
    pub fn admit_managed_request(
        &self,
        owner: &str,
        cancellation: ActionCancellation,
    ) -> Result<ManagedRequest, BrokerError> {
        cancellation.check().map_err(BrokerError::Adapter)?;
        let mut g = self.inner.lock();
        if g.stopped_runs.contains(owner) {
            return Err(BrokerError::StopRequested);
        }
        if !g.managed_runs.contains_key(owner) {
            if g.managed_runs.len() >= 256 {
                return Err(BrokerError::Schema("managed run capacity exhausted".into()));
            }
            let worker = self.worker.lock().clone().ok_or_else(|| {
                BrokerError::Schema("managed browser worker is unavailable".into())
            })?;
            g.managed_runs.insert(
                owner.into(),
                ManagedRun {
                    worker,
                    revision: WorkerRunRevision::INITIAL,
                    phase: Phase::Running,
                    cleanup_confirmed: false,
                    pause_epoch: 0,
                },
            );
        }
        let run = &g.managed_runs[owner];
        if run.phase != Phase::Running {
            return Err(BrokerError::Schema("managed run is fenced".into()));
        }
        Ok(ManagedRequest {
            identity: ManagedRequestIdentity {
                owner: owner.into(),
                revision: run.revision,
                cancellation,
            },
            worker: run.worker.clone(),
        })
    }

    /// A request-local facade shares the registry, but pins its own worker.
    /// Nothing is written into the root Host's worker slot or thread-local state.
    pub fn for_managed_request(&self, request: &ManagedRequest) -> Result<Self, BrokerError> {
        let host = Self {
            inner: self.inner.clone(),
            worker: Mutex::new(None),
            request: Some(request.clone()),
        };
        host.check_request_locked(&host.inner.lock(), request.identity.owner())?;
        let worker = request
            .worker
            .clone()
            .bind_request(request.identity.clone())
            .map_err(BrokerError::BrowserWorker)?;
        *host.worker.lock() = Some(worker);
        Ok(host)
    }

    pub(super) fn check_request_locked(&self, g: &Inner, owner: &str) -> Result<(), BrokerError> {
        if g.stopped_runs.contains(owner) {
            return Err(BrokerError::StopRequested);
        }
        if let Some(request) = &self.request {
            request
                .identity
                .cancellation
                .check()
                .map_err(BrokerError::Adapter)?;
            let run = g
                .managed_runs
                .get(owner)
                .ok_or(BrokerError::TargetUnauthorized)?;
            if request.identity.owner != owner
                || request.identity.revision != run.revision
                || run.phase != Phase::Running
                || !Arc::ptr_eq(&request.worker, &run.worker)
            {
                return Err(BrokerError::IdentityMismatch("managed request"));
            }
        }
        Ok(())
    }

    pub(super) fn pinned_worker(
        &self,
        owner: &str,
    ) -> Result<Arc<dyn ManagedBrowserWorker>, BrokerError> {
        if let Some(run) = self.inner.lock().managed_runs.get(owner) {
            return Ok(run.worker.clone());
        }
        self.worker()
    }

    pub(super) fn managed_worker_locked(
        g: &Inner,
        owner: &str,
    ) -> Option<Arc<dyn ManagedBrowserWorker>> {
        g.managed_runs.get(owner).map(|run| run.worker.clone())
    }

    pub fn pause_managed_run(&self, owner: &str) -> Result<(), BrokerError> {
        let binding = {
            let mut g = self.inner.lock();
            if g.stopped_runs.contains(owner) {
                return Ok(());
            }
            let Some(run) = g.managed_runs.get_mut(owner) else {
                return Ok(());
            };
            run.pause_epoch = run.pause_epoch.saturating_add(1);
            // A second control cannot guess which revision a pending resume
            // has reached. Keep fenced; only terminal Stop can resolve it.
            if matches!(run.phase, Phase::Resuming | Phase::Uncertain) {
                run.phase = Phase::Uncertain;
                return Err(BrokerError::Schema(
                    "managed resume outcome unknown; stop required".into(),
                ));
            }
            run.phase = Phase::Paused;
            (run.worker.clone(), run.revision)
        };
        let state = binding
            .0
            .pause_run(owner, binding.1)
            .map_err(BrokerError::BrowserWorker)?;
        if state.revision != binding.1 || state.phase != WorkerRunPhase::Paused {
            return Err(BrokerError::BrowserWorker(WorkerError::invalid_response(
                200,
            )));
        }
        Ok(())
    }

    pub fn managed_run_idle(&self, owner: &str) -> bool {
        let binding = {
            let g = self.inner.lock();
            let Some(run) = g.managed_runs.get(owner) else {
                return true;
            };
            if g.stopped_runs.contains(owner) && run.cleanup_confirmed {
                return true;
            }
            if run.phase == Phase::Running && !g.stopped_runs.contains(owner) {
                return false;
            }
            if matches!(run.phase, Phase::Resuming | Phase::Uncertain)
                && !g.stopped_runs.contains(owner)
            {
                return false;
            }
            (
                run.worker.clone(),
                run.revision,
                g.stopped_runs.contains(owner),
            )
        };
        let Ok(state) = binding.0.run_status(owner, binding.1) else {
            return false;
        };
        let g = self.inner.lock();
        g.managed_runs.get(owner).is_some_and(|run| {
            run.revision == binding.1
                && state.revision == binding.1
                && state.is_idle()
                && if binding.2 {
                    state.phase == WorkerRunPhase::Stopped
                } else {
                    state.phase == WorkerRunPhase::Paused && run.phase == Phase::Paused
                }
        })
    }

    pub(super) fn confirm_managed_cleanup(g: &mut Inner, owner: &str) {
        if let Some(run) = g.managed_runs.get_mut(owner) {
            run.cleanup_confirmed = true;
        }
    }

    /// Capture the exact Pause being resumed under the Broker admission lock.
    pub(crate) fn prepare_managed_resume(
        &self,
        owner: &str,
    ) -> Result<Option<ManagedResume>, BrokerError> {
        let g = self.inner.lock();
        if g.stopped_runs.contains(owner) {
            return Err(BrokerError::StopRequested);
        }
        let Some(run) = g.managed_runs.get(owner) else {
            return Ok(None);
        };
        if run.phase != Phase::Paused {
            return Err(BrokerError::Schema("managed run is not resumable".into()));
        }
        Ok(Some(ManagedResume {
            owner: owner.into(),
            revision: run.revision,
            pause_epoch: run.pause_epoch,
            worker: run.worker.clone(),
        }))
    }

    #[cfg(test)]
    pub(crate) fn resume_managed_run(&self, owner: &str) -> Result<(), BrokerError> {
        if let Some(ticket) = self.prepare_managed_resume(owner)? {
            self.resume_managed_request(&ticket)?;
        }
        Ok(())
    }

    /// Explicit user recovery only; an unacknowledged transition is never retried.
    pub(crate) fn resume_managed_request(&self, ticket: &ManagedResume) -> Result<(), BrokerError> {
        let owner = ticket.owner.as_str();
        let binding = {
            let mut g = self.inner.lock();
            if g.stopped_runs.contains(owner) {
                return Err(BrokerError::StopRequested);
            }
            let Some(run) = g.managed_runs.get_mut(owner) else {
                return Ok(());
            };
            if run.phase != Phase::Paused
                || run.pause_epoch != ticket.pause_epoch
                || run.revision != ticket.revision
                || !Arc::ptr_eq(&run.worker, &ticket.worker)
            {
                return Err(BrokerError::Schema("managed run is not resumable".into()));
            }
            run.phase = Phase::Resuming;
            (run.worker.clone(), run.revision)
        };
        let result = binding.0.resume_run(owner, binding.1);
        let mut g = self.inner.lock();
        let stopped = g.stopped_runs.contains(owner);
        let run = g
            .managed_runs
            .get_mut(owner)
            .ok_or(BrokerError::RunNotFound)?;
        match result {
            Ok(state)
                if state.revision
                    == binding.1.successor().map_err(BrokerError::BrowserWorker)?
                    && state.phase == WorkerRunPhase::Running
                    && state.is_idle() =>
            {
                run.revision = state.revision;
                if stopped || run.phase != Phase::Resuming || run.pause_epoch != ticket.pause_epoch
                {
                    run.phase = Phase::Uncertain;
                    return Err(BrokerError::IdentityMismatch("superseded managed resume"));
                }
                run.phase = Phase::Running;
                Ok(())
            }
            other => {
                run.phase = Phase::Uncertain;
                Err(BrokerError::BrowserWorker(
                    other
                        .err()
                        .unwrap_or_else(|| WorkerError::invalid_response(200)),
                ))
            }
        }
    }
}
