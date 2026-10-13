//! Typed UI readback and exact task decisions. No adapter/discovery calls.
use super::*;
use crate::task_consent::{
    host_connection_id, HostGrantView, HostRequestKey, HostRequestView, HostScopeMemberView,
    TransportContext,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostTaskDecision {
    Deny,
    Cancel,
    Stop,
}

/// Acknowledgement only, never proof that physical cleanup finished.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostTaskDecisionReceipt {
    pub session_id: String,
    pub key: HostRequestKey,
    pub decision: HostTaskDecision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostExecutionPhase {
    Preparing,
    AwaitingObservation,
    Idle,
    Busy,
    Paused,
    Resuming,
    Switching,
    Unknown,
    TargetClosed,
    StopRequested,
    Stopped,
}

/// No action payload, raw adapter reason, native proof or credential crosses
/// the UI boundary. Stop/trace eviction do not erase a prior business failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostExecutionFailure {
    ActionRejected,
    OutcomeUnknown,
}

impl HostExecutionFailure {
    pub(super) fn from_outcome(kind: OutcomeKind) -> Option<Self> {
        match kind {
            OutcomeKind::Rejected => Some(Self::ActionRejected),
            OutcomeKind::Unknown => Some(Self::OutcomeUnknown),
            OutcomeKind::Applied | OutcomeKind::Verified => None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostExecutionView {
    pub key: HostRequestKey,
    pub run_id: String,
    /// Decimal string preserves the full Host generation in JavaScript.
    pub generation: String,
    pub phase: HostExecutionPhase,
    pub target: Option<HostScopeMemberView>,
    /// Retained metadata only. This is not a liveness witness.
    pub target_name: Option<String>,
    /// Descriptor retained from the exact authorized executor, not discovery.
    pub backend: Option<String>,
    pub cleanup_pending: bool,
    /// Eligibility from retained authority only; native readiness is rechecked
    /// by the owned recovery transaction after the user's explicit gesture.
    pub resume_available: bool,
    pub has_fresh_observation: bool,
    pub last_failure: Option<HostExecutionFailure>,
    /// Original task-local file intent; no filesystem path or content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload: Option<HostUploadView>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostTaskView {
    pub session_id: String,
    pub connection_id: String,
    pub task_id: Option<String>,
    pub feature_enabled: bool,
    pub request: Option<HostRequestView>,
    pub grants: Vec<HostGrantView>,
    pub executions: Vec<HostExecutionView>,
    /// Host integration descriptors only; never a grant or native discovery.
    /// Core has no product selection producers. The App fills its compiled
    /// picker/managed-constructor list; it does not promise runtime readiness.
    pub picker_surfaces: Vec<String>,
}

impl ComputerUseBroker {
    /// Opening a card never starts a runtime, enumerates windows, observes,
    /// calls liveness, consumes a decision, or mints a continuation.
    pub fn host_task_view(&self, token: &str) -> Result<HostTaskView, BrokerError> {
        let ctx = self.live_context(token)?;
        let result = {
            let mut inner = self.inner.lock();
            let busy = retained_busy_runs(&inner);
            inner.consent.sweep_from_host(&busy);
            read_locked(&inner, &ctx, &self.tabs)
        };
        // Deadline cleanup is handed to the existing owned consumer. It does
        // not run native cleanup on this readback thread.
        self.handoff_expired();
        result
    }

    /// The original request key and owner are checked in the same transaction
    /// as the decision. A session's newly focused/current run is never used.
    pub fn decide_host_task(
        &self,
        token: &str,
        key: &HostRequestKey,
        decision: HostTaskDecision,
    ) -> Result<HostTaskDecisionReceipt, BrokerError> {
        let _change = self.changes.notice();
        let ctx = self.live_context(token)?;
        let result = {
            let mut inner = self.inner.lock();
            let busy = retained_busy_runs(&inner);
            inner.consent.sweep_from_host(&busy);
            inner
                .consent
                .require_host_request(&ctx, key)
                .and_then(|()| {
                    match decision {
                        HostTaskDecision::Deny => {
                            inner.consent.deny(&ctx, &key.request_id, key.revision)?;
                        }
                        HostTaskDecision::Cancel => {
                            inner
                                .consent
                                .host_cancel(&ctx, &key.request_id, key.revision)?;
                        }
                        HostTaskDecision::Stop => {
                            let runs = inner.consent.stop_host_task(&ctx, key)?;
                            // Authority is already fenced. Keep original run ids
                            // for the cleanup handoff after releasing this lock.
                            return Ok(runs);
                        }
                    }
                    Ok(Vec::new())
                })
        };
        self.handoff_expired();
        self.handoff_fences(&result?);
        Ok(HostTaskDecisionReceipt {
            session_id: ctx.app_session_id,
            key: key.clone(),
            decision,
        })
    }
}

fn retained_busy_runs(inner: &Inner) -> std::collections::HashSet<String> {
    inner
        .runs
        .iter()
        .filter(|(_, run)| {
            run.in_flight.load(Ordering::SeqCst)
                || run.stop_cleanup_pending
                || super::activity::unknown_work_pending(run)
        })
        .map(|(id, _)| id.clone())
        .collect()
}

fn read_locked(
    inner: &Inner,
    ctx: &TransportContext,
    tabs: &ExistingTabHost,
) -> Result<HostTaskView, BrokerError> {
    let task_id = inner.consent.host_current_task(ctx)?;
    let request = inner.consent.host_request_view(ctx)?;
    // Terminal readback may still show cleanup for the last task after Stop
    // cleared the live task id. A new task never adopts that execution.
    let displayed_task = task_id
        .as_deref()
        .or_else(|| request.as_ref().map(|request| request.key.task_id.as_str()));
    let mut executions = Vec::new();
    for (id, run) in &inner.runs {
        let binding = match &run.consent {
            ConsentGate::Bound(binding) => Some(binding),
            ConsentGate::Pending => None,
            ConsentGate::Legacy => continue,
        };
        let Some(key) = inner.consent.host_execution_key(ctx, id, binding) else {
            continue;
        };
        if run.app_session_id != ctx.app_session_id || displayed_task != Some(key.task_id.as_str())
        {
            continue;
        }
        executions.push(HostExecutionView {
            key,
            run_id: id.clone(),
            generation: run.generation.to_string(),
            phase: execution_phase(run),
            target: run.target.as_ref().map(|target| HostScopeMemberView {
                target_id: target.target_id.clone(),
                surface: target.surface.as_wire().into(),
                display_name: Some(target.title.clone()),
            }),
            target_name: run.target.as_ref().map(|target| target.title.clone()),
            backend: run.target.as_ref().map(|target| target.backend.clone()),
            cleanup_pending: run.stop_cleanup_pending
                || run.pause_cleanup_pending
                || run.switch_cleanup_pending
                // The original extension read/input may outlive the logical
                // Pause fence. Read its retained Host records, not an adapter.
                // The task/generation selection and this read are synchronous;
                // no late native query can decorate a replacement execution.
                || ((run.paused || run.stop != StopState::Running)
                    && run.target.as_ref().is_some_and(|target| target.surface == SurfaceKind::ExistingTab)
                    && !tabs.retained_existing_operations_idle(id)),
            resume_available: binding.is_some_and(|binding| {
                super::task_resume::host_resume_available(inner, ctx, run, binding)
                    && (!run
                        .target
                        .as_ref()
                        .is_some_and(|target| target.surface == SurfaceKind::ManagedBrowser)
                        || tabs.managed_resume_available(id))
            }),
            has_fresh_observation: run.stop == StopState::Running
                && !run.paused
                && !run.resume_in_flight
                && !run.switch_in_flight
                && !run.target.as_ref().is_some_and(|target| target.closed)
                && run.snapshot_id.is_some(),
            last_failure: run.last_failure,
            upload: super::webview_upload::upload_view(inner, run),
        });
    }
    executions.sort_by(|left, right| left.run_id.cmp(&right.run_id));
    Ok(HostTaskView {
        session_id: ctx.app_session_id.clone(),
        connection_id: host_connection_id(ctx),
        task_id,
        feature_enabled: inner.feature_enabled,
        request,
        grants: inner
            .consent
            .host_grant_views(ctx, inner.feature_enabled, inner.feature_epoch),
        executions,
        picker_surfaces: Vec::new(),
    })
}

fn execution_phase(run: &RunState) -> HostExecutionPhase {
    match run.stop {
        StopState::Stopped => HostExecutionPhase::Stopped,
        StopState::StopRequested => HostExecutionPhase::StopRequested,
        StopState::Running if matches!(run.consent, ConsentGate::Pending) => {
            HostExecutionPhase::Preparing
        }
        StopState::Running if run.resume_in_flight => HostExecutionPhase::Resuming,
        StopState::Running if run.target.as_ref().is_some_and(|target| target.closed) => {
            HostExecutionPhase::TargetClosed
        }
        StopState::Running if run.paused => HostExecutionPhase::Paused,
        StopState::Running if run.switch_in_flight => HostExecutionPhase::Switching,
        StopState::Running if run.last_unknown => HostExecutionPhase::Unknown,
        StopState::Running if run.in_flight.load(Ordering::SeqCst) => HostExecutionPhase::Busy,
        StopState::Running if run.snapshot_id.is_some() => HostExecutionPhase::Idle,
        StopState::Running => HostExecutionPhase::AwaitingObservation,
    }
}
