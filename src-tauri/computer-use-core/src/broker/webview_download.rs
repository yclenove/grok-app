//! Broker-owned App WebView download admission and run-staging publication.

use super::*;
use crate::adapter::{DownloadDisposition, DownloadRequest, DownloadResult, StagedDownload};
use crate::browser::action_ledger;

#[derive(Debug, Clone)]
pub(crate) struct WebViewDownloadIntent {
    pub(crate) run_id: String,
    pub(crate) target_id: String,
    pub(crate) target_generation: u64,
    pub(crate) action_id: String,
    pub(crate) snapshot_id: String,
    pub(crate) element_ref: String,
    pub(crate) filename: String,
    pub(crate) cancellation: crate::execution::ActionCancellation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WebViewDownloadCompletion {
    pub(crate) run_id: String,
    pub(crate) target_id: String,
    pub(crate) target_generation: u64,
    pub(crate) path: std::path::PathBuf,
    pub(crate) bytes: u64,
}

#[derive(Clone)]
enum DownloadTerminal {
    Success(WebViewDownloadCompletion),
    Rejected(String),
    Unknown(String),
}

#[derive(Clone)]
pub(super) struct DownloadReplay {
    binding: crate::task_consent::ConsentBinding,
    fingerprint: [u8; 32],
    origin_target_id: String,
    origin_target_generation: u64,
    outcome: DownloadTerminal,
}

enum Admit {
    Replay(Box<DownloadReplay>),
    Execute(Box<DownloadExecution>),
}

struct DownloadExecution {
    binding: crate::task_consent::ConsentBinding,
    cancellation: crate::execution::ActionCancellation,
    executor: surface_executor::SurfaceExecutor,
    fingerprint: [u8; 32],
    inflight: Arc<AtomicBool>,
    run_generation: u64,
    target: AuthorizedTarget,
}

impl ComputerUseBroker {
    pub(crate) fn dispatch_webview_download(
        &self,
        mut intent: WebViewDownloadIntent,
        permit: &crate::task_consent::ExecutionPermit,
    ) -> Result<WebViewDownloadCompletion, BrokerError> {
        match self.admit_webview_download(&intent, permit)? {
            Admit::Replay(replay) => replay_download(replay.outcome),
            Admit::Execute(execution) => {
                let DownloadExecution {
                    binding,
                    cancellation,
                    executor,
                    fingerprint,
                    inflight,
                    run_generation,
                    target,
                } = *execution;
                let _guard = InFlightClear::new(inflight, &self.changes);
                intent.cancellation = cancellation;
                if let Err(error) = self.recheck_model_call(permit) {
                    self.finish_webview_download(
                        run_generation,
                        &binding,
                        fingerprint,
                        &intent,
                        DownloadTerminal::Rejected(error.to_string()),
                        false,
                        false,
                    );
                    return Err(error);
                }

                let staging_root = self.tabs.staging_root();
                let destination = match crate::staging::staging_download_file(
                    &staging_root,
                    &intent.run_id,
                    &intent.filename,
                ) {
                    Ok(path) => path,
                    Err(error) => {
                        self.finish_webview_download(
                            run_generation,
                            &binding,
                            fingerprint,
                            &intent,
                            DownloadTerminal::Rejected(error.to_string()),
                            false,
                            false,
                        );
                        return Err(error);
                    }
                };
                self.trace_webview_download(&intent);
                let request = DownloadRequest {
                    run_id: intent.run_id.clone(),
                    target_id: intent.target_id.clone(),
                    target_generation: intent.target_generation,
                    action_id: intent.action_id.clone(),
                    snapshot_id: intent.snapshot_id.clone(),
                    element_ref: intent.element_ref.clone(),
                    destination: destination.clone(),
                    cancellation: intent.cancellation.clone(),
                };
                let staged = match executor.adapter.stage_download(&request) {
                    Ok(staged) => staged,
                    Err(error) => {
                        let unknown = error.disposition == DownloadDisposition::DispatchedUnknown;
                        let dispatched = error.disposition != DownloadDisposition::NotDispatched;
                        let outcome = if unknown {
                            DownloadTerminal::Unknown(error.detail.clone())
                        } else {
                            cleanup_undispatched_destination(&destination);
                            DownloadTerminal::Rejected(error.detail.clone())
                        };
                        self.finish_webview_download(
                            run_generation,
                            &binding,
                            fingerprint,
                            &intent,
                            outcome,
                            dispatched,
                            false,
                        );
                        return Err(download_error(error));
                    }
                };

                let result = staged.result().clone();
                let bytes = match validate_download_result(&result, &destination, &staging_root) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        drop(staged);
                        self.finish_webview_download(
                            run_generation,
                            &binding,
                            fingerprint,
                            &intent,
                            DownloadTerminal::Rejected(error.to_string()),
                            true,
                            false,
                        );
                        return Err(error);
                    }
                };
                let commit = self.commit_webview_download(
                    &intent,
                    permit,
                    &binding,
                    &target,
                    staged,
                    run_generation,
                );
                match commit {
                    Ok(()) => {
                        let completion = WebViewDownloadCompletion {
                            run_id: intent.run_id.clone(),
                            target_id: intent.target_id.clone(),
                            target_generation: intent.target_generation,
                            path: result.path,
                            bytes,
                        };
                        self.finish_webview_download(
                            run_generation,
                            &binding,
                            fingerprint,
                            &intent,
                            DownloadTerminal::Success(completion.clone()),
                            true,
                            true,
                        );
                        Ok(completion)
                    }
                    Err(error) => {
                        let _ = std::fs::remove_file(&result.path);
                        let detail = format!(
                            "WebView download completed but staging publication failed: {error}"
                        );
                        self.finish_webview_download(
                            run_generation,
                            &binding,
                            fingerprint,
                            &intent,
                            DownloadTerminal::Unknown(detail.clone()),
                            true,
                            false,
                        );
                        Err(BrokerError::Adapter(format!("{detail}; outcome unknown")))
                    }
                }
            }
        }
    }

    fn admit_webview_download(
        &self,
        intent: &WebViewDownloadIntent,
        permit: &crate::task_consent::ExecutionPermit,
    ) -> Result<Admit, BrokerError> {
        crate::browser::validate_browser_action_id(&intent.action_id)?;
        crate::staging::sanitize_filename(&intent.filename)?;
        if intent.run_id != permit.run_id
            || intent.target_id != permit.target_id
            || permit.capability != "input"
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        if intent.target_generation == 0 {
            return Err(BrokerError::IdentityMismatch("targetGeneration"));
        }
        if intent.snapshot_id.trim().is_empty() {
            return Err(BrokerError::Schema("snapshotId required".into()));
        }
        if intent.element_ref.trim().is_empty() {
            return Err(BrokerError::Schema("elementRef required".into()));
        }
        let fingerprint = download_fingerprint(intent);

        let mut inner = self.inner.lock();
        if !inner.feature_enabled {
            return Err(BrokerError::FeatureDisabled);
        }
        let binding = {
            let run = inner
                .runs
                .get(&intent.run_id)
                .ok_or(BrokerError::RunNotFound)?;
            super::admit_consent(
                &inner.consent,
                inner.feature_enabled,
                inner.feature_epoch,
                run,
            )?;
            inner
                .consent
                .recheck_permit(permit, inner.feature_enabled, inner.feature_epoch)?;
            let binding = match &run.consent {
                ConsentGate::Bound(binding) => binding.clone(),
                ConsentGate::Legacy | ConsentGate::Pending => {
                    return Err(BrokerError::TargetUnauthorized)
                }
            };
            if !inner.consent.binding_capability(&binding, "input") {
                return Err(BrokerError::Schema(
                    "capability does not allow this tool".into(),
                ));
            }
            binding
        };

        {
            let run = inner
                .runs
                .get_mut(&intent.run_id)
                .ok_or(BrokerError::RunNotFound)?;
            if run.stop != StopState::Running {
                return Err(BrokerError::StopRequested);
            }
            if run.paused {
                return Err(BrokerError::Schema("run is paused".into()));
            }
            if let Some(existing) = run.fingerprints.get(&intent.action_id) {
                if existing != &fingerprint {
                    return Err(BrokerError::Schema("actionId fingerprint conflict".into()));
                }
                if let Some(replay) = run.download_replay.get(&intent.action_id) {
                    if !same_download_owner(replay, permit) {
                        return Err(BrokerError::DuplicateAction);
                    }
                    return Ok(Admit::Replay(Box::new(replay.clone())));
                }
                if run.in_flight.load(Ordering::SeqCst) {
                    return Err(BrokerError::LeaseHeld {
                        run_id: intent.run_id.clone(),
                    });
                }
                return Err(BrokerError::DuplicateAction);
            }
        }

        let target = inner
            .runs
            .get(&intent.run_id)
            .and_then(|run| run.target.as_ref())
            .filter(|target| {
                target.target_id == intent.target_id
                    && target.target_generation == intent.target_generation
                    && target.surface == SurfaceKind::WebView
                    && !target.closed
            })
            .cloned()
            .ok_or(BrokerError::IdentityMismatch("targetGeneration"))?;
        let executor = self.executor_for_target(&target)?;
        let run = inner
            .runs
            .get_mut(&intent.run_id)
            .ok_or(BrokerError::RunNotFound)?;
        super::file_action::require_file_ref(
            run,
            &intent.target_id,
            intent.target_generation,
            &intent.snapshot_id,
            &intent.element_ref,
        )?;
        if run.seen.contains_key(&intent.action_id) {
            return Err(BrokerError::DuplicateAction);
        }
        if run.seen.len() >= self.action_budget {
            return Err(BrokerError::Schema(
                "action budget exhausted; start a new run".into(),
            ));
        }
        if run.in_flight.load(Ordering::SeqCst) {
            return Err(BrokerError::LeaseHeld {
                run_id: intent.run_id.clone(),
            });
        }
        intent.cancellation.check().map_err(BrokerError::Adapter)?;
        run.in_flight.store(true, Ordering::SeqCst);
        run.unknown_idle_confirmed = false;
        if let Some(activity) = &run.activity {
            activity.begin();
        }
        run.fingerprints
            .insert(intent.action_id.clone(), fingerprint);
        run.seen.insert(
            intent.action_id.clone(),
            ActionOutcome {
                action_id: intent.action_id.clone(),
                run_id: intent.run_id.clone(),
                kind: OutcomeKind::Unknown,
                executed: false,
                reason: Some("download pending".into()),
                generation: run.generation,
            },
        );
        Ok(Admit::Execute(Box::new(DownloadExecution {
            binding,
            cancellation: run.cancellation.clone(),
            executor,
            fingerprint,
            inflight: run.in_flight.clone(),
            run_generation: run.generation,
            target,
        })))
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_webview_download(
        &self,
        intent: &WebViewDownloadIntent,
        permit: &crate::task_consent::ExecutionPermit,
        binding: &crate::task_consent::ConsentBinding,
        target: &AuthorizedTarget,
        staged: Box<dyn StagedDownload>,
        run_generation: u64,
    ) -> Result<(), BrokerError> {
        let mut inner = self.inner.lock();
        if !inner.feature_enabled {
            return Err(BrokerError::FeatureDisabled);
        }
        inner
            .consent
            .recheck_permit(permit, inner.feature_enabled, inner.feature_epoch)?;
        let current_member = inner.consent.scope_member_for_binding(
            binding,
            inner.feature_enabled,
            inner.feature_epoch,
        )?;
        if current_member.target_id != target.target_id
            || current_member.surface != SurfaceKind::WebView
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        {
            let run = inner
                .runs
                .get(&intent.run_id)
                .ok_or(BrokerError::RunNotFound)?;
            if run.generation != run_generation
                || run.stop != StopState::Running
                || run.paused
                || !run.in_flight.load(Ordering::SeqCst)
                || run.snapshot_id.as_deref() != Some(&intent.snapshot_id)
                || !run.element_refs.contains(&intent.element_ref)
                || !matches!(&run.consent, ConsentGate::Bound(current)
                    if current.target_id == binding.target_id
                        && current.request_id == binding.request_id
                        && current.grant_revision == binding.grant_revision)
                || !run.target.as_ref().is_some_and(|current| {
                    current.target_id == target.target_id
                        && current.target_generation == target.target_generation
                        && current.executor_generation == target.executor_generation
                })
            {
                return Err(BrokerError::StopRequested);
            }
        }

        let mut authority_commit = || {
            let run = inner
                .runs
                .get_mut(&intent.run_id)
                .ok_or(BrokerError::RunNotFound)?;
            super::file_action::invalidate_observation(run);
            run.last_unknown = false;
            run.unknown_idle_confirmed = false;
            Ok(())
        };
        staged.commit(&mut authority_commit)
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_webview_download(
        &self,
        run_generation: u64,
        binding: &crate::task_consent::ConsentBinding,
        fingerprint: [u8; 32],
        intent: &WebViewDownloadIntent,
        outcome: DownloadTerminal,
        dispatched: bool,
        activity: bool,
    ) {
        let mut inner = self.inner.lock();
        let late = inner
            .runs
            .get(&intent.run_id)
            .is_some_and(|run| run.generation != run_generation);
        if late {
            inner.dropped_late = inner.dropped_late.saturating_add(1);
        }
        let (kind, executed, reason, unknown) = match &outcome {
            DownloadTerminal::Success(_) => (OutcomeKind::Applied, true, "applied".into(), false),
            DownloadTerminal::Rejected(detail) => {
                (OutcomeKind::Rejected, false, detail.clone(), false)
            }
            DownloadTerminal::Unknown(detail) => {
                (OutcomeKind::Unknown, false, detail.clone(), true)
            }
        };
        if let Some(run) = inner.runs.get_mut(&intent.run_id) {
            if late {
                run.seen.insert(
                    intent.action_id.clone(),
                    ActionOutcome {
                        action_id: intent.action_id.clone(),
                        run_id: intent.run_id.clone(),
                        kind: OutcomeKind::Unknown,
                        executed: false,
                        reason: Some("download result after generation change".into()),
                        generation: run_generation,
                    },
                );
                return;
            }
            run.seen.insert(
                intent.action_id.clone(),
                ActionOutcome {
                    action_id: intent.action_id.clone(),
                    run_id: intent.run_id.clone(),
                    kind,
                    executed,
                    reason: Some(reason),
                    generation: run_generation,
                },
            );
            if dispatched && !matches!(outcome, DownloadTerminal::Success(_)) {
                super::file_action::invalidate_observation(run);
            }
            if unknown {
                run.last_unknown = true;
            }
            run.download_replay.insert(
                intent.action_id.clone(),
                DownloadReplay {
                    binding: binding.clone(),
                    fingerprint,
                    origin_target_id: intent.target_id.clone(),
                    origin_target_generation: intent.target_generation,
                    outcome,
                },
            );
        }
        if !late && activity && !unknown {
            super::note_locked_activity(&mut inner, &intent.run_id);
        }
    }

    pub(crate) fn replay_webview_download_for_permit(
        &self,
        permit: &crate::task_consent::ExecutionPermit,
        args: &serde_json::Value,
    ) -> Result<Option<WebViewDownloadCompletion>, BrokerError> {
        if permit.capability != "input" {
            return Err(BrokerError::TargetUnauthorized);
        }
        let action_id = args
            .as_object()
            .and_then(|options| options.get("actionId"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| BrokerError::Schema("actionId required".into()))?;
        crate::browser::validate_browser_action_id(action_id)?;

        let inner = self.inner.lock();
        Self::recheck_model_call_locked(&inner, permit)?;
        let run = inner
            .runs
            .get(&permit.run_id)
            .ok_or(BrokerError::RunNotFound)?;
        let Some(replay) = run.download_replay.get(action_id) else {
            return Ok(None);
        };
        if !same_download_owner(replay, permit)
            || run.fingerprints.get(action_id) != Some(&replay.fingerprint)
        {
            return Err(BrokerError::DuplicateAction);
        }
        let intent = crate::tools::parse_webview_download_intent(
            &permit.run_id,
            Some(&replay.origin_target_id),
            args,
        )?;
        if intent.target_id != replay.origin_target_id
            || intent.target_generation != replay.origin_target_generation
            || download_fingerprint(&intent) != replay.fingerprint
        {
            return Err(BrokerError::Schema("actionId fingerprint conflict".into()));
        }
        replay_download(replay.outcome.clone()).map(Some)
    }

    fn trace_webview_download(&self, intent: &WebViewDownloadIntent) {
        let mut inner = self.inner.lock();
        push_trace(
            &mut inner,
            "webview_download",
            &intent.run_id,
            &format!("download {} {}", intent.action_id, intent.filename),
            TraceAudience::Model,
        );
    }
}

fn validate_download_result(
    result: &DownloadResult,
    expected: &std::path::Path,
    staging_root: &std::path::Path,
) -> Result<u64, BrokerError> {
    let actual = std::fs::canonicalize(&result.path)
        .map_err(|error| BrokerError::Adapter(format!("cannot resolve staged file: {error}")))?;
    let expected = std::fs::canonicalize(expected).map_err(|error| {
        BrokerError::Adapter(format!("cannot resolve download result: {error}"))
    })?;
    if actual != expected {
        return Err(BrokerError::Schema(
            "download result did not use the Broker staging path".into(),
        ));
    }
    crate::staging::accept_staged_file(&actual, staging_root)
}

fn cleanup_undispatched_destination(destination: &std::path::Path) {
    let _ = std::fs::remove_file(destination);
    if let Some(directory) = destination.parent() {
        let _ = std::fs::remove_dir(directory);
        if let Some(downloads) = directory.parent() {
            let _ = std::fs::remove_dir(downloads);
        }
    }
}

fn download_fingerprint(intent: &WebViewDownloadIntent) -> [u8; 32] {
    action_ledger::fingerprint(&action_ledger::canonical_semantics(
        "download",
        &intent.target_id,
        intent.target_generation,
        &intent.snapshot_id,
        Some(&intent.element_ref),
        &serde_json::json!({ "filename": intent.filename }),
    ))
}

fn same_download_owner(
    replay: &DownloadReplay,
    permit: &crate::task_consent::ExecutionPermit,
) -> bool {
    let binding = &replay.binding;
    binding.app_session_id == permit.session_id
        && binding.agent_session_id == permit.agent_session_id
        && binding.owner_nonce == permit.owner_nonce
        && binding.incarnation == permit.incarnation
        && binding.connection_generation == permit.connection_generation
        && binding.task_id == permit.task_id
        && binding.feature_epoch == permit.feature_epoch
        && binding.run_id == permit.run_id
}

fn replay_download(terminal: DownloadTerminal) -> Result<WebViewDownloadCompletion, BrokerError> {
    match terminal {
        DownloadTerminal::Success(completion) => Ok(completion),
        DownloadTerminal::Rejected(detail) => Err(BrokerError::Adapter(detail)),
        DownloadTerminal::Unknown(detail) => {
            Err(BrokerError::Adapter(format!("{detail}; outcome unknown")))
        }
    }
}

fn download_error(error: crate::adapter::DownloadError) -> BrokerError {
    match error.disposition {
        DownloadDisposition::NotDispatched | DownloadDisposition::DispatchedRejected => {
            BrokerError::Adapter(error.detail)
        }
        DownloadDisposition::DispatchedUnknown => {
            BrokerError::Adapter(format!("{}; outcome unknown", error.detail))
        }
    }
}
