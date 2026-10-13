//! One original upload intent, one Host file choice, one guarded assignment.
//! No path/bytes/selector from the model and no file access granted by TaskGrant.
use super::*;
use crate::adapter::{HostSelectedUploadFile, UploadError, UploadRequest};
use crate::task_consent::ExecutionPermit;
use parking_lot::Condvar;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WebViewUploadIntent {
    pub run_id: String,
    pub target_id: String,
    pub target_generation: u64,
    pub action_id: String,
    pub snapshot_id: String,
    pub geometry_revision: u64,
    pub element_ref: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostUploadPhase {
    AwaitingFile,
    Selecting,
    Assigning,
}

/// Task-local metadata only. Neither this id nor its renderer can choose a path.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostUploadView {
    pub request_id: String,
    pub action_id: String,
    pub phase: HostUploadPhase,
    pub target_name: String,
    pub field_name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebViewUploadCompletion {
    /// Verified means field assignment, not eventual website submission.
    pub outcome: ActionOutcome,
    pub bytes: Option<u64>,
}

struct Progress {
    phase: HostUploadPhase,
    terminal: Option<WebViewUploadCompletion>,
    file_fingerprint: Option<[u8; 32]>,
}

pub(super) struct PendingUpload {
    id: String,
    intent: WebViewUploadIntent,
    permit: ExecutionPermit,
    target: AuthorizedTarget,
    executor: surface_executor::SurfaceExecutor,
    fingerprint: [u8; 32],
    cancellation: crate::execution::ActionCancellation,
    dispatched: AtomicBool,
    field_name: String,
    progress: Mutex<Progress>,
    wake: Condvar,
}

#[derive(Clone)]
pub(super) struct UploadReplay {
    permit: ExecutionPermit,
    fingerprint: [u8; 32],
    // The immutable chosen contents and metadata are bound to the one result.
    // Only the digest remains after dispatch; no path or content is retained.
    file_fingerprint: Option<[u8; 32]>,
    completion: WebViewUploadCompletion,
}

pub enum WebViewUploadReady {
    Pending(WebViewUploadCall),
    Replay(WebViewUploadCompletion),
}

/// Original model waiter. Dropping it fences intent, never finishes a picker.
pub struct WebViewUploadCall {
    broker: Arc<ComputerUseBroker>,
    pending: Arc<PendingUpload>,
    completed: bool,
}

/// A cancellation handle can revoke but cannot release the physical owner.
#[derive(Clone)]
pub struct HostUploadCancel {
    broker: Arc<ComputerUseBroker>,
    pending: Arc<PendingUpload>,
}

impl HostUploadCancel {
    pub fn fence(&self) {
        // Retire immediately even while original preparation still holds the
        // Broker lock. Final ledger cleanup may wait; native admission cannot.
        self.pending.cancellation.cancel();
        self.broker
            .finish_upload(&self.pending, None, "file selection interrupted");
    }
}

impl WebViewUploadCall {
    pub fn request_id(&self) -> &str {
        &self.pending.id
    }

    pub fn cancel_handle(&self) -> HostUploadCancel {
        HostUploadCancel {
            broker: self.broker.clone(),
            pending: self.pending.clone(),
        }
    }

    /// Wait only for this original choice. No polling tool, new chat, or retry.
    /// The Host must retain its worker until the actual dialog/assignment ends.
    pub fn wait(mut self) -> Result<WebViewUploadCompletion, BrokerError> {
        loop {
            // Even an already-committed result cannot cross a retired transport.
            self.broker.recheck_model_call(&self.pending.permit)?;
            let terminal = self.pending.progress.lock().terminal.clone();
            if let Some(terminal) = terminal {
                self.completed = true;
                return Ok(terminal);
            }
            self.broker.check_pending_upload(&self.pending)?;
            let mut progress = self.pending.progress.lock();
            if progress.terminal.is_none() {
                self.pending
                    .wake
                    .wait_for(&mut progress, Duration::from_millis(50));
            }
        }
    }
}

impl Drop for WebViewUploadCall {
    fn drop(&mut self) {
        if !self.completed {
            self.cancel_handle().fence();
        }
    }
}

/// Rust-only, single-use choice owner. It belongs inside the real picker worker,
/// not the lifetime of a renderer IPC future. Dropping a waiter is not dialog exit.
pub struct HostUploadSelection {
    broker: Arc<ComputerUseBroker>,
    pending: Arc<PendingUpload>,
    activity: Option<Arc<crate::task_consent::PhysicalActivity>>,
    _inflight: InFlightClear,
}

impl HostUploadSelection {
    pub fn cancel_handle(&self) -> HostUploadCancel {
        HostUploadCancel {
            broker: self.broker.clone(),
            pending: self.pending.clone(),
        }
    }

    /// Check before opening a dialog and before reading the selected handle.
    pub fn require_current(&self) -> Result<(), BrokerError> {
        self.broker.check_pending_upload(&self.pending)
    }

    pub fn cancel(self) -> WebViewUploadCompletion {
        self.broker
            .finish_upload(&self.pending, None, "user cancelled file selection")
    }

    /// Consumes the original user choice; never reads or reopens a local path.
    pub fn assign(
        self,
        file: HostSelectedUploadFile,
    ) -> Result<WebViewUploadCompletion, BrokerError> {
        self.require_current()?;
        let bytes = file.bytes().len() as u64;
        let fingerprint = file_fingerprint(&file);
        {
            let inner = self.broker.inner.lock();
            require_pending(&inner, &self.pending)?;
            let mut progress = self.pending.progress.lock();
            if progress.phase != HostUploadPhase::Selecting || progress.file_fingerprint.is_some() {
                return Err(BrokerError::DuplicateAction);
            }
            progress.file_fingerprint = Some(fingerprint);
            progress.phase = HostUploadPhase::Assigning;
        }
        self.broker.changes.publish();
        let intent = &self.pending.intent;
        let request = UploadRequest {
            run_id: intent.run_id.clone(),
            target_id: intent.target_id.clone(),
            target_generation: intent.target_generation,
            action_id: intent.action_id.clone(),
            snapshot_id: intent.snapshot_id.clone(),
            element_ref: intent.element_ref.clone(),
            file,
            cancellation: self.pending.cancellation.clone(),
        };
        // Stage's native owner remains accountable on timeout/retirement.
        self.pending.dispatched.store(true, Ordering::SeqCst);
        let staged = match self.pending.executor.adapter.stage_upload(&request) {
            Ok(staged) => staged,
            Err(UploadError::NotDispatched(_)) => {
                self.pending.dispatched.store(false, Ordering::SeqCst);
                return Ok(self.broker.finish_upload(
                    &self.pending,
                    None,
                    "file assignment rejected",
                ));
            }
            Err(UploadError::DispatchedUnknown(_)) => {
                return Ok(self.broker.finish_upload(
                    &self.pending,
                    None,
                    "file assignment outcome unknown",
                ));
            }
        };
        let result = staged.result();
        if !result.applied
            || !result.verifiable
            || !result.postcondition_ok
            || result
                .outcome
                .is_some_and(|kind| kind != OutcomeKind::Verified)
        {
            drop(staged);
            return Ok(self.broker.finish_upload(
                &self.pending,
                None,
                "file assignment not verified",
            ));
        }
        let commit = {
            let mut inner = self.broker.inner.lock();
            require_pending(&inner, &self.pending)
                .and_then(|()| {
                    let mut commit = || {
                        let run = inner
                            .runs
                            .get_mut(&intent.run_id)
                            .ok_or(BrokerError::RunNotFound)?;
                        super::file_action::invalidate_observation(run);
                        Ok(())
                    };
                    staged.commit(&mut commit)
                })
                .map(|()| {
                    finish_locked(
                        &mut inner,
                        &self.pending,
                        Some(bytes),
                        "file assigned; verify website separately",
                    )
                })
        };
        Ok(match commit {
            Ok(completion) => completion,
            Err(_) => self.broker.finish_upload(
                &self.pending,
                None,
                "file assignment publication retired; outcome unknown",
            ),
        })
    }
}

impl Drop for HostUploadSelection {
    fn drop(&mut self) {
        self.broker
            .finish_upload(&self.pending, None, "file selection owner ended");
        // A picker is physically gone now. A timed-out native assignment may
        // still be running; its adapter, not this Rust stack, witnesses idle.
        if !self.pending.dispatched.load(Ordering::SeqCst)
            || self
                .pending
                .executor
                .adapter
                .is_idle(&self.pending.intent.run_id)
        {
            if let Some(activity) = &self.activity {
                activity.finish();
            }
        }
    }
}

impl ComputerUseBroker {
    /// Read-only ACP permission gate. Same original identity/replay rules,
    /// without reserving intent, consuming budget or choosing a local file.
    pub(crate) fn preflight_webview_upload(
        &self,
        intent: &WebViewUploadIntent,
        permit: &ExecutionPermit,
    ) -> Result<(), BrokerError> {
        crate::browser::validate_browser_action_id(&intent.action_id)?;
        if permit.capability != "input"
            || intent.run_id != permit.run_id
            || intent.target_id != permit.target_id
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        self.recheck_model_call(permit)?;
        let inner = self.inner.lock();
        Self::recheck_model_call_locked(&inner, permit)?;
        let run = inner
            .runs
            .get(&intent.run_id)
            .ok_or(BrokerError::RunNotFound)?;
        if let Some(previous) = run.fingerprints.get(&intent.action_id) {
            if previous != &intent_fingerprint(intent) {
                return Err(BrokerError::Schema("actionId fingerprint conflict".into()));
            }
            let replay = run
                .upload_replay
                .get(&intent.action_id)
                .ok_or(BrokerError::DuplicateAction)?;
            if !same_owner(&replay.permit, permit)
                || (replay.completion.outcome.kind == OutcomeKind::Verified
                    && replay.file_fingerprint.is_none())
            {
                return Err(BrokerError::DuplicateAction);
            }
            return Ok(());
        }
        require_upload_observation(run, intent)
    }
    /// Reserve intent, not arbitrary file access. One pending intent per run.
    /// Model callers use the upload-only transport; Host/UI chooses the file.
    pub fn request_webview_upload(
        self: &Arc<Self>,
        intent: WebViewUploadIntent,
        permit: &ExecutionPermit,
    ) -> Result<WebViewUploadReady, BrokerError> {
        self.request_webview_upload_for_caller(intent, permit, |_| true)
    }

    /// Attach the original caller's cancel handle before publishing the Host
    /// intent. The registrar is bounded metadata-only, never native work.
    pub(crate) fn request_webview_upload_for_caller(
        self: &Arc<Self>,
        intent: WebViewUploadIntent,
        permit: &ExecutionPermit,
        register: impl FnOnce(HostUploadCancel) -> bool,
    ) -> Result<WebViewUploadReady, BrokerError> {
        crate::browser::validate_browser_action_id(&intent.action_id)?;
        if permit.capability != "input"
            || intent.run_id != permit.run_id
            || intent.target_id != permit.target_id
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        self.recheck_model_call(permit)?;
        let fingerprint = intent_fingerprint(&intent);
        let _change = self.changes.notice();
        let mut inner = self.inner.lock();
        Self::recheck_model_call_locked(&inner, permit)?;
        let run = inner
            .runs
            .get_mut(&intent.run_id)
            .ok_or(BrokerError::RunNotFound)?;
        if let Some(previous) = run.fingerprints.get(&intent.action_id) {
            if previous != &fingerprint {
                return Err(BrokerError::Schema("actionId fingerprint conflict".into()));
            }
            let replay = run
                .upload_replay
                .get(&intent.action_id)
                .ok_or(BrokerError::DuplicateAction)?;
            if !same_owner(&replay.permit, permit) || replay.fingerprint != fingerprint {
                return Err(BrokerError::DuplicateAction);
            }
            // Success without the single immutable file binding is never replayable.
            if replay.completion.outcome.kind == OutcomeKind::Verified
                && replay.file_fingerprint.is_none()
            {
                return Err(BrokerError::DuplicateAction);
            }
            return Ok(WebViewUploadReady::Replay(replay.completion.clone()));
        }
        require_upload_observation(run, &intent)?;
        if run.upload.is_some() || run.in_flight.load(Ordering::SeqCst) {
            return Err(BrokerError::LeaseHeld {
                run_id: intent.run_id.clone(),
            });
        }
        if run.seen.contains_key(&intent.action_id) {
            return Err(BrokerError::DuplicateAction);
        }
        if run.seen.len() >= self.action_budget {
            return Err(BrokerError::Schema(
                "action budget exhausted; start a new run".into(),
            ));
        }
        let target = run
            .target
            .as_ref()
            .ok_or(BrokerError::TargetUnauthorized)?
            .clone();
        let executor = self.executor_for_target(&target)?;
        let pending = Arc::new(PendingUpload {
            id: Uuid::new_v4().to_string(),
            field_name: run
                .upload_refs
                .get(&intent.element_ref)
                .cloned()
                .unwrap_or_default(),
            intent,
            permit: permit.clone(),
            target,
            executor,
            fingerprint,
            cancellation: crate::execution::ActionCancellation::default(),
            dispatched: AtomicBool::new(false),
            progress: Mutex::new(Progress {
                phase: HostUploadPhase::AwaitingFile,
                terminal: None,
                file_fingerprint: None,
            }),
            wake: Condvar::new(),
        });
        if !register(HostUploadCancel {
            broker: self.clone(),
            pending: pending.clone(),
        }) {
            return Err(BrokerError::StopRequested);
        }
        run.fingerprints
            .insert(pending.intent.action_id.clone(), fingerprint);
        run.seen.insert(
            pending.intent.action_id.clone(),
            outcome(&pending, OutcomeKind::Unknown, "awaiting user file choice"),
        );
        run.upload = Some(pending.clone());
        Ok(WebViewUploadReady::Pending(WebViewUploadCall {
            broker: self.clone(),
            pending,
            completed: false,
        }))
    }

    /// Exact main-App task control supplies this opaque id, not a path or bytes.
    pub fn begin_host_upload_selection(
        self: &Arc<Self>,
        token: &str,
        address: &HostExecutionAddress,
        request_id: &str,
    ) -> Result<HostUploadSelection, BrokerError> {
        let ctx = self.live_context(token)?;
        let pending = {
            let inner = self.inner.lock();
            super::host_execution::require_execution(&inner, &ctx, address)?;
            let pending = inner.runs[&address.run_id]
                .upload
                .clone()
                .ok_or(BrokerError::DuplicateAction)?;
            if pending.id != request_id {
                return Err(BrokerError::IdentityMismatch("upload request"));
            }
            require_pending(&inner, &pending)?;
            pending
        };
        self.check_pending_upload(&pending)?;
        let inner = self.inner.lock();
        super::host_execution::require_execution(&inner, &ctx, address)?;
        require_pending(&inner, &pending)?;
        let run = &inner.runs[&address.run_id];
        let mut progress = pending.progress.lock();
        if progress.phase != HostUploadPhase::AwaitingFile
            || run
                .in_flight
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            return Err(BrokerError::LeaseHeld {
                run_id: address.run_id.clone(),
            });
        }
        progress.phase = HostUploadPhase::Selecting;
        if let Some(activity) = &run.activity {
            activity.begin();
        }
        Ok(HostUploadSelection {
            broker: self.clone(),
            pending: pending.clone(),
            activity: run.activity.clone(),
            _inflight: InFlightClear::new(run.in_flight.clone(), &self.changes),
        })
    }

    /// Retire only this intent. The original picker/native owner keeps its
    /// in-flight reservation until it physically returns.
    pub fn cancel_host_upload(
        &self,
        token: &str,
        address: &HostExecutionAddress,
        request_id: &str,
    ) -> Result<WebViewUploadCompletion, BrokerError> {
        let ctx = self.live_context(token)?;
        let _change = self.changes.notice();
        let mut inner = self.inner.lock();
        super::host_execution::require_execution(&inner, &ctx, address)?;
        let pending = inner.runs[&address.run_id]
            .upload
            .clone()
            .ok_or(BrokerError::DuplicateAction)?;
        if pending.id != request_id {
            return Err(BrokerError::IdentityMismatch("upload request"));
        }
        require_pending(&inner, &pending)?;
        Ok(finish_locked(
            &mut inner,
            &pending,
            None,
            "user cancelled file selection",
        ))
    }

    fn check_pending_upload(&self, pending: &Arc<PendingUpload>) -> Result<(), BrokerError> {
        self.recheck_model_call(&pending.permit)?;
        require_pending(&self.inner.lock(), pending)
    }

    fn finish_upload(
        &self,
        pending: &Arc<PendingUpload>,
        bytes: Option<u64>,
        reason: &str,
    ) -> WebViewUploadCompletion {
        let _change = self.changes.notice();
        finish_locked(&mut self.inner.lock(), pending, bytes, reason)
    }
}

fn require_upload_observation(
    run: &RunState,
    intent: &WebViewUploadIntent,
) -> Result<(), BrokerError> {
    if run.stop != StopState::Running || run.paused {
        return Err(BrokerError::StopRequested);
    }
    super::file_action::require_file_ref(
        run,
        &intent.target_id,
        intent.target_generation,
        &intent.snapshot_id,
        &intent.element_ref,
    )?;
    if run.geometry_revision != intent.geometry_revision {
        return Err(BrokerError::IdentityMismatch("geometryRevision"));
    }
    if !run
        .target
        .as_ref()
        .is_some_and(|target| target.surface == SurfaceKind::WebView && !target.closed)
    {
        return Err(BrokerError::TargetUnauthorized);
    }
    if !run.upload_refs.contains_key(&intent.element_ref) {
        return Err(BrokerError::Schema(
            "retained element is not an upload file input".into(),
        ));
    }
    Ok(())
}

fn require_pending(inner: &Inner, pending: &Arc<PendingUpload>) -> Result<(), BrokerError> {
    ComputerUseBroker::recheck_model_call_locked(inner, &pending.permit)?;
    pending.cancellation.check().map_err(BrokerError::Adapter)?;
    let run = inner
        .runs
        .get(&pending.intent.run_id)
        .ok_or(BrokerError::RunNotFound)?;
    if !run
        .upload
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, pending))
        || pending.progress.lock().terminal.is_some()
        || run
            .target
            .as_ref()
            .is_none_or(|target| target.executor_generation != pending.target.executor_generation)
    {
        return Err(BrokerError::DuplicateAction);
    }
    require_upload_observation(run, &pending.intent)
}

fn finish_locked(
    inner: &mut Inner,
    pending: &Arc<PendingUpload>,
    bytes: Option<u64>,
    reason: &str,
) -> WebViewUploadCompletion {
    let mut progress = pending.progress.lock();
    if let Some(terminal) = &progress.terminal {
        return terminal.clone();
    }
    let dispatched = pending.dispatched.load(Ordering::SeqCst);
    let kind = if bytes.is_some() {
        OutcomeKind::Verified
    } else if dispatched {
        OutcomeKind::Unknown
    } else {
        OutcomeKind::Rejected
    };
    let terminal = WebViewUploadCompletion {
        outcome: outcome(pending, kind, reason),
        bytes,
    };
    progress.terminal = Some(terminal.clone());
    pending.cancellation.cancel();
    pending.wake.notify_all();
    if let Some(run) = inner.runs.get_mut(&pending.intent.run_id) {
        let owns = run
            .upload
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, pending));
        if owns {
            run.upload = None;
            run.seen
                .insert(pending.intent.action_id.clone(), terminal.outcome.clone());
            run.upload_replay.insert(
                pending.intent.action_id.clone(),
                UploadReplay {
                    permit: pending.permit.clone(),
                    fingerprint: pending.fingerprint,
                    file_fingerprint: progress.file_fingerprint,
                    completion: terminal.clone(),
                },
            );
            if run.generation == pending.permit.execution_generation {
                run.last_failure = if !dispatched && reason == "user cancelled file selection" {
                    None
                } else {
                    HostExecutionFailure::from_outcome(kind)
                };
                if dispatched && kind != OutcomeKind::Verified {
                    super::file_action::invalidate_observation(run);
                    run.last_unknown = true;
                    run.unknown_idle_confirmed = false;
                }
            }
        }
    }
    inner.changes.publish();
    terminal
}

pub(super) fn upload_view(inner: &Inner, run: &RunState) -> Option<HostUploadView> {
    let pending = run.upload.as_ref()?;
    require_pending(inner, pending).ok()?;
    Some(HostUploadView {
        request_id: pending.id.clone(),
        action_id: pending.intent.action_id.clone(),
        phase: pending.progress.lock().phase,
        target_name: pending.target.title.clone(),
        field_name: pending.field_name.clone(),
    })
}

fn outcome(pending: &PendingUpload, kind: OutcomeKind, reason: &str) -> ActionOutcome {
    ActionOutcome {
        action_id: pending.intent.action_id.clone(),
        run_id: pending.intent.run_id.clone(),
        kind,
        executed: kind == OutcomeKind::Verified,
        reason: Some(reason.into()),
        generation: pending.permit.execution_generation,
    }
}

fn intent_fingerprint(intent: &WebViewUploadIntent) -> [u8; 32] {
    crate::browser::action_ledger::fingerprint(&crate::browser::action_ledger::canonical_semantics(
        "upload",
        &intent.target_id,
        intent.target_generation,
        &intent.snapshot_id,
        Some(&intent.element_ref),
        &serde_json::json!({"geometryRevision": intent.geometry_revision}),
    ))
}

fn file_fingerprint(file: &HostSelectedUploadFile) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let metadata = serde_json::to_vec(&serde_json::json!({"filename":file.filename(),"mime":file.mime(),"bytes":file.bytes().len()})).expect("file metadata is serializable");
    let mut digest = Sha256::new();
    digest.update((metadata.len() as u64).to_be_bytes());
    digest.update(metadata);
    digest.update(file.bytes());
    digest.finalize().into()
}

fn same_owner(a: &ExecutionPermit, b: &ExecutionPermit) -> bool {
    a.session_id == b.session_id
        && a.agent_session_id == b.agent_session_id
        && a.owner_nonce == b.owner_nonce
        && a.incarnation == b.incarnation
        && a.connection_generation == b.connection_generation
        && a.task_id == b.task_id
        && a.request_id == b.request_id
        && a.grant_revision == b.grant_revision
        && a.feature_epoch == b.feature_epoch
        && a.run_id == b.run_id
        && a.target_id == b.target_id
        && a.execution_generation == b.execution_generation
}
