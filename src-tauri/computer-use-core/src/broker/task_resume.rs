//! Explicit Host recovery owns its original paused run through cancellation.
use super::*;
use crate::task_consent::{ConsentBinding, ScopeMember, TransportContext};

#[path = "task_resume/runtime.rs"]
mod runtime;

pub enum HostTaskResumeReady {
    /// A worker for exactly this original paused identity already owns recovery.
    InProgress,
    Prepare(Box<HostTaskResume>),
}

pub struct HostTaskResume {
    broker: Arc<ComputerUseBroker>,
    context: TransportContext,
    address: HostExecutionAddress,
    origin: u64,
    generation: u64,
    binding: ConsentBinding,
    members: Vec<ScopeMember>,
    target: AuthorizedTarget,
    executor: surface_executor::SurfaceExecutor,
    admission: Option<InFlightClear>,
    prepared: Option<RecoveryObservation>,
    completed: bool,
}

struct RecoveryObservation {
    observation: Observation,
    geometry_revision: u64,
    capture_ms: u64,
}

/// UI acknowledgement contains neither model refs nor native/transport proof.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostTaskResumeReceipt {
    pub address: HostExecutionAddress,
}

pub struct HostTaskResumeCancel {
    broker: Arc<ComputerUseBroker>,
    context: TransportContext,
    address: HostExecutionAddress,
    origin: u64,
}

fn switch_recovery_allowed(run: &RunState) -> bool {
    !run.switch_in_flight
        && (!run.switch_cleanup_pending
            || run
                .target
                .as_ref()
                .is_some_and(|target| target.surface == SurfaceKind::ExistingTab))
}

pub(super) fn host_resume_available(
    inner: &Inner,
    context: &TransportContext,
    run: &RunState,
    binding: &ConsentBinding,
) -> bool {
    run.stop == StopState::Running
        && run.paused
        && !run.resume_in_flight
        && switch_recovery_allowed(run)
        && run.target.as_ref().is_some_and(|target| {
            matches!(
                target.surface,
                SurfaceKind::Desktop
                    | SurfaceKind::ExistingTab
                    | SurfaceKind::ManagedBrowser
                    | SurfaceKind::WebView
            ) && !target.closed
        })
        && inner
            .consent
            .suspended_scope(context, binding, inner.feature_enabled, inner.feature_epoch)
            .is_ok()
}

impl ComputerUseBroker {
    /// No native query or work starts until the Host-owned worker consumes this
    /// reservation. Neither MCP nor a legacy resume can enter this transaction.
    pub fn reserve_host_task_resume(
        self: &Arc<Self>,
        token: &str,
        address: &HostExecutionAddress,
    ) -> Result<HostTaskResumeReady, BrokerError> {
        let _change = self.changes.notice();
        let context = self.live_context(token)?;
        let mut inner = self.inner.lock();
        let mut current_address = address.clone();
        let duplicate = inner.runs.get(&address.run_id).is_some_and(|run| {
            run.resume_in_flight
                && run
                    .resume_origin
                    .is_some_and(|origin| origin.to_string() == address.generation)
        });
        if duplicate {
            current_address.generation = inner.runs[&address.run_id].generation.to_string();
        }
        let binding = recovery_identity(&inner, &context, &current_address)?;
        let members = inner.consent.suspended_scope(
            &context,
            &binding,
            inner.feature_enabled,
            inner.feature_epoch,
        )?;
        let run = inner
            .runs
            .get_mut(&address.run_id)
            .ok_or(BrokerError::RunNotFound)?;
        if run.stop != StopState::Running || !run.paused {
            return Err(BrokerError::StopRequested);
        }
        if run.resume_in_flight {
            return if duplicate {
                Ok(HostTaskResumeReady::InProgress)
            } else {
                Err(BrokerError::IdentityMismatch("resume origin"))
            };
        }
        if !switch_recovery_allowed(run) {
            return Err(BrokerError::LeaseHeld {
                run_id: address.run_id.clone(),
            });
        }
        let target = run.target.clone().ok_or(BrokerError::TargetUnauthorized)?;
        let executor = self.executor_for_target(&target)?;
        // ExistingTab retains its exact active borrow through Pause; the
        // suspended group proof and fresh capture still gate publication.
        // ManagedBrowser resumes its exact paused worker before a fresh capture.
        // WebView retains every exact native/document binding through Pause;
        // fresh capture and its native group guard gate recovery publication.
        if !matches!(
            target.surface,
            SurfaceKind::Desktop
                | SurfaceKind::ExistingTab
                | SurfaceKind::ManagedBrowser
                | SurfaceKind::WebView
        ) {
            return Err(BrokerError::SurfaceUnavailable {
                surface: target.surface,
            });
        }
        if target.surface == SurfaceKind::ManagedBrowser
            && !self.tabs.managed_resume_available(&address.run_id)
        {
            return Err(BrokerError::Schema("managed run is not resumable".into()));
        }
        run.resume_in_flight = true;
        run.resume_origin = Some(run.generation);
        Ok(HostTaskResumeReady::Prepare(Box::new(HostTaskResume {
            broker: self.clone(),
            context,
            address: address.clone(),
            origin: run.generation,
            generation: run.generation,
            binding,
            members,
            target,
            executor,
            admission: None,
            prepared: None,
            completed: false,
        })))
    }
}

fn recovery_identity(
    inner: &Inner,
    context: &TransportContext,
    address: &HostExecutionAddress,
) -> Result<ConsentBinding, BrokerError> {
    inner.consent.require_host_request(context, &address.key)?;
    let run = inner
        .runs
        .get(&address.run_id)
        .ok_or(BrokerError::RunNotFound)?;
    let ConsentGate::Bound(binding) = &run.consent else {
        return Err(BrokerError::TargetUnauthorized);
    };
    if run.app_session_id != context.app_session_id
        || address.generation != run.generation.to_string()
        || inner
            .consent
            .host_execution_key(context, &address.run_id, Some(binding))
            .as_ref()
            != Some(&address.key)
    {
        return Err(BrokerError::IdentityMismatch("execution"));
    }
    Ok(binding.clone())
}

impl HostTaskResume {
    pub fn cancel_handle(&self) -> HostTaskResumeCancel {
        HostTaskResumeCancel {
            broker: self.broker.clone(),
            context: self.context.clone(),
            address: self.address.clone(),
            origin: self.origin,
        }
    }

    fn recheck_locked(&self, inner: &Inner) -> Result<(), BrokerError> {
        let mut current = self.address.clone();
        current.generation = self.generation.to_string();
        recovery_identity(inner, &self.context, &current)?;
        let scope = inner.consent.suspended_scope(
            &self.context,
            &self.binding,
            inner.feature_enabled,
            inner.feature_epoch,
        )?;
        let run = &inner.runs[&self.address.run_id];
        if !run.paused
            || run.stop != StopState::Running
            || !run.resume_in_flight
            || run.resume_origin != Some(self.origin)
            || scope != self.members
            || run.target.as_ref().is_none_or(|target| {
                target.target_id != self.target.target_id
                    || target.surface != self.target.surface
                    || target.executor_generation != self.executor.generation
            })
        {
            return Err(BrokerError::IdentityMismatch("resume generation"));
        }
        Ok(())
    }

    fn recheck(&self) -> Result<(), BrokerError> {
        self.recheck_locked(&self.broker.inner.lock())
    }
}

impl HostTaskResumeCancel {
    /// A dropped UI waiter cannot release ownership of a late native result.
    /// It only fences the original recovery; the worker remains its cleanup owner.
    pub fn fence(&self) {
        let _change = self.broker.changes.notice();
        let mut inner = self.broker.inner.lock();
        if inner
            .consent
            .require_host_request(&self.context, &self.address.key)
            .is_err()
        {
            return;
        }
        let Some(run) = inner.runs.get_mut(&self.address.run_id) else {
            return;
        };
        if run.resume_origin != Some(self.origin)
            || !run.resume_in_flight
            || !run.paused
            || run.stop != StopState::Running
        {
            return;
        }
        // The original pause worker owns old cleanup; after recovery admission,
        // the recovery worker owns the new cleanup. This caller never aborts.
        if run.generation != self.origin {
            run.pause_cleanup_pending = true;
        }
        run.cancellation.cancel();
        run.generation = run.generation.saturating_add(1);
        run.snapshot_id = None;
        run.last_image_ok = false;
        inner.consent.suspend_run(&self.address.run_id);
    }
}

impl Drop for HostTaskResume {
    fn drop(&mut self) {
        let had_admission = self.admission.is_some();
        // A failed capture/claim is cleaned by this owned worker, never the UI
        // waiter. Keep admission until cancellation and native cleanup return.
        if !self.completed && self.admission.is_some() {
            let _ = self
                .broker
                .pause_due_to(&self.address.run_id, "recovery failed");
        }
        let _change = self.broker.changes.notice();
        if let Some(run) = self.broker.inner.lock().runs.get_mut(&self.address.run_id) {
            if run.resume_origin == Some(self.origin) {
                run.resume_in_flight = false;
                run.resume_origin = None;
            }
        }
        drop(self.admission.take());
        // Read idle only after this worker releases its own admission. The
        // lifecycle path releases a paused lease only after cleanup is complete.
        if had_admission {
            let _ = self.broker.stop_state(&self.address.run_id);
        }
    }
}
