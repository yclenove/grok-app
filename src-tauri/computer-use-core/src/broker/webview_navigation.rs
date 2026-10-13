//! Broker-owned App WebView navigation admission and atomic successor commit.

use super::*;
use crate::adapter::{NavigationDisposition, NavigationError, NavigationRequest, NavigationResult};
use crate::browser::action_ledger;
use crate::task_consent::{NativeScopeProof, ScopeMember};

#[derive(Clone)]
pub(crate) struct WebViewNavigationCompletion {
    pub(crate) result: NavigationResult,
    pub(crate) target_generation: u64,
}

#[derive(Clone)]
enum NavigationTerminal {
    Success(Box<WebViewNavigationCompletion>),
    Rejected(String),
    Unknown(String),
}

#[derive(Clone)]
pub(super) struct NavigationReplay {
    binding: crate::task_consent::ConsentBinding,
    fingerprint: [u8; 32],
    origin_target_id: String,
    origin_target_generation: u64,
    outcome: NavigationTerminal,
}

enum Admit {
    Replay(Box<NavigationReplay>),
    Execute(Box<NavigationExecution>),
}

struct NavigationExecution {
    binding: crate::task_consent::ConsentBinding,
    cancellation: crate::execution::ActionCancellation,
    executor: surface_executor::SurfaceExecutor,
    fingerprint: [u8; 32],
    inflight: Arc<AtomicBool>,
    member: ScopeMember,
    run_generation: u64,
    target: AuthorizedTarget,
}

impl ComputerUseBroker {
    pub(crate) fn dispatch_webview_navigation(
        &self,
        mut request: NavigationRequest,
        permit: &crate::task_consent::ExecutionPermit,
    ) -> Result<WebViewNavigationCompletion, BrokerError> {
        match self.admit_webview_navigation(&request, permit)? {
            Admit::Replay(replay) => replay_navigation(replay.outcome),
            Admit::Execute(execution) => {
                let NavigationExecution {
                    binding,
                    cancellation,
                    executor,
                    fingerprint,
                    inflight,
                    member,
                    run_generation,
                    target,
                } = *execution;
                let _guard = InFlightClear::new(inflight, &self.changes);
                request.cancellation = cancellation;
                if let Err(error) = self.recheck_model_call(permit) {
                    self.finish_webview_navigation(
                        run_generation,
                        &binding,
                        fingerprint,
                        &request,
                        NavigationTerminal::Rejected(error.to_string()),
                        false,
                    );
                    return Err(error);
                }
                self.trace_webview_navigation(&request);
                let transition = match executor.adapter.stage_navigation(&request) {
                    Ok(transition) => transition,
                    Err(error) => {
                        let unknown = error.disposition == NavigationDisposition::DispatchedUnknown;
                        let replay = if unknown {
                            NavigationTerminal::Unknown(error.detail.clone())
                        } else {
                            NavigationTerminal::Rejected(error.detail.clone())
                        };
                        self.finish_webview_navigation(
                            run_generation,
                            &binding,
                            fingerprint,
                            &request,
                            replay,
                            false,
                        );
                        return Err(navigation_error(error));
                    }
                };
                let result = transition.result().clone();
                let commit = self.commit_webview_navigation(
                    &request,
                    permit,
                    &binding,
                    &target,
                    &member,
                    &result,
                    transition,
                    run_generation,
                );
                match commit {
                    Ok(target_generation) => {
                        let completion = WebViewNavigationCompletion {
                            result,
                            target_generation,
                        };
                        self.finish_webview_navigation(
                            run_generation,
                            &binding,
                            fingerprint,
                            &request,
                            NavigationTerminal::Success(Box::new(completion.clone())),
                            true,
                        );
                        Ok(completion)
                    }
                    Err(error) => {
                        let detail = format!(
                            "WebView navigation completed but successor commit failed: {error}"
                        );
                        self.finish_webview_navigation(
                            run_generation,
                            &binding,
                            fingerprint,
                            &request,
                            NavigationTerminal::Unknown(detail.clone()),
                            false,
                        );
                        Err(BrokerError::Adapter(format!("{detail}; outcome unknown")))
                    }
                }
            }
        }
    }

    fn admit_webview_navigation(
        &self,
        request: &NavigationRequest,
        permit: &crate::task_consent::ExecutionPermit,
    ) -> Result<Admit, BrokerError> {
        crate::browser::validate_browser_action_id(&request.action_id)?;
        if request.run_id != permit.run_id || permit.capability != "input" {
            return Err(BrokerError::TargetUnauthorized);
        }
        if request.target_generation == 0 {
            return Err(BrokerError::IdentityMismatch("targetGeneration"));
        }
        match request.kind {
            crate::adapter::NavigationKind::Url => {
                let url = request
                    .url
                    .as_deref()
                    .ok_or_else(|| BrokerError::Schema("url required for kind=url".into()))?;
                crate::browser::is_allowed_navigate_url(url)?;
            }
            _ if request.url.is_some() => {
                return Err(BrokerError::Schema("url is only valid for kind=url".into()));
            }
            _ => {}
        }
        let fingerprint = action_ledger::fingerprint(&action_ledger::canonical_semantics(
            request.kind.as_wire(),
            &request.target_id,
            request.target_generation,
            "",
            None,
            &serde_json::json!({ "url": request.url }),
        ));

        let mut inner = self.inner.lock();
        if !inner.feature_enabled {
            return Err(BrokerError::FeatureDisabled);
        }
        let binding = {
            let run = inner
                .runs
                .get(&request.run_id)
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
                .get_mut(&request.run_id)
                .ok_or(BrokerError::RunNotFound)?;
            if run.stop != StopState::Running {
                return Err(BrokerError::StopRequested);
            }
            if run.paused {
                return Err(BrokerError::Schema("run is paused".into()));
            }
            if let Some(existing) = run.fingerprints.get(&request.action_id) {
                if existing != &fingerprint {
                    return Err(BrokerError::Schema("actionId fingerprint conflict".into()));
                }
                if let Some(replay) = run.navigation_replay.get(&request.action_id) {
                    if !same_navigation_owner(replay, permit) {
                        return Err(BrokerError::DuplicateAction);
                    }
                    return Ok(Admit::Replay(Box::new(replay.clone())));
                }
                if run.in_flight.load(Ordering::SeqCst) {
                    return Err(BrokerError::LeaseHeld {
                        run_id: request.run_id.clone(),
                    });
                }
                return Err(BrokerError::DuplicateAction);
            }
        }
        if request.target_id != permit.target_id {
            return Err(BrokerError::TargetUnauthorized);
        }
        let member = inner.consent.scope_member_for_binding(
            &binding,
            inner.feature_enabled,
            inner.feature_epoch,
        )?;
        let target = inner
            .runs
            .get(&request.run_id)
            .and_then(|run| run.target.as_ref())
            .filter(|target| {
                target.target_id == request.target_id
                    && target.target_generation == request.target_generation
                    && target.surface == SurfaceKind::WebView
                    && !target.closed
            })
            .cloned()
            .ok_or(BrokerError::IdentityMismatch("targetGeneration"))?;
        let executor = self.executor_for_target(&target)?;
        let run = inner
            .runs
            .get_mut(&request.run_id)
            .ok_or(BrokerError::RunNotFound)?;
        if run.last_unknown {
            return Err(BrokerError::Schema(
                "unknown result must observe before the next action".into(),
            ));
        }
        if run.seen.contains_key(&request.action_id) {
            return Err(BrokerError::DuplicateAction);
        }
        if run.seen.len() >= self.action_budget {
            return Err(BrokerError::Schema(
                "action budget exhausted; start a new run".into(),
            ));
        }
        if run.in_flight.load(Ordering::SeqCst) {
            return Err(BrokerError::LeaseHeld {
                run_id: request.run_id.clone(),
            });
        }
        request.cancellation.check().map_err(BrokerError::Adapter)?;
        run.in_flight.store(true, Ordering::SeqCst);
        run.unknown_idle_confirmed = false;
        if let Some(activity) = &run.activity {
            activity.begin();
        }
        run.fingerprints
            .insert(request.action_id.clone(), fingerprint);
        run.seen.insert(
            request.action_id.clone(),
            ActionOutcome {
                action_id: request.action_id.clone(),
                run_id: request.run_id.clone(),
                kind: OutcomeKind::Unknown,
                executed: false,
                reason: Some("navigation pending".into()),
                generation: run.generation,
            },
        );
        Ok(Admit::Execute(Box::new(NavigationExecution {
            binding,
            cancellation: run.cancellation.clone(),
            executor,
            fingerprint,
            inflight: run.in_flight.clone(),
            member,
            run_generation: run.generation,
            target,
        })))
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_webview_navigation(
        &self,
        request: &NavigationRequest,
        permit: &crate::task_consent::ExecutionPermit,
        binding: &crate::task_consent::ConsentBinding,
        previous_target: &AuthorizedTarget,
        previous_member: &ScopeMember,
        result: &NavigationResult,
        transition: Box<dyn crate::adapter::StagedNavigation>,
        run_generation: u64,
    ) -> Result<u64, BrokerError> {
        if result.kind != request.kind
            || result.target.target_id == previous_target.target_id
            || result.target.kind != "webview"
            || result.identity.target_id().as_deref() != Some(&result.target.target_id)
        {
            return Err(BrokerError::IdentityMismatch("navigationSuccessor"));
        }
        let next_target_generation = previous_target
            .target_generation
            .checked_add(1)
            .ok_or_else(|| BrokerError::Schema("target generation exhausted".into()))?;
        let successor_member = ScopeMember {
            target_id: result.target.target_id.clone(),
            surface: SurfaceKind::WebView,
            native: Some(NativeScopeProof {
                executor_generation: previous_target.executor_generation,
                identity: result.identity.clone(),
            }),
            display_name: Some(result.target.title.clone()),
        };

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
        if &current_member != previous_member {
            return Err(BrokerError::TargetUnauthorized);
        }
        {
            let run = inner
                .runs
                .get(&request.run_id)
                .ok_or(BrokerError::RunNotFound)?;
            if run.generation != run_generation
                || run.stop != StopState::Running
                || run.paused
                || !run.in_flight.load(Ordering::SeqCst)
                || !matches!(&run.consent, ConsentGate::Bound(current)
                    if current.target_id == binding.target_id
                        && current.request_id == binding.request_id
                        && current.grant_revision == binding.grant_revision)
                || !run.target.as_ref().is_some_and(|target| {
                    target.target_id == previous_target.target_id
                        && target.target_generation == previous_target.target_generation
                        && target.executor_generation == previous_target.executor_generation
                })
                || run
                    .webview_targets
                    .iter()
                    .filter(|target| *target == &previous_target.target_id)
                    .count()
                    != 1
            {
                return Err(BrokerError::StopRequested);
            }
        }

        let mut authority_commit = || {
            let enabled = inner.feature_enabled;
            let epoch = inner.feature_epoch;
            let successor_binding = inner.consent.replace_active_member(
                binding,
                previous_member,
                successor_member.clone(),
                enabled,
                epoch,
            )?;
            let run = inner
                .runs
                .get_mut(&request.run_id)
                .ok_or(BrokerError::RunNotFound)?;
            let member = run
                .webview_targets
                .iter_mut()
                .find(|target| **target == previous_target.target_id)
                .ok_or(BrokerError::TargetUnauthorized)?;
            *member = result.target.target_id.clone();
            run.target = Some(AuthorizedTarget {
                target_id: result.target.target_id.clone(),
                title: result.target.title.clone(),
                backend: result.target.backend.clone(),
                closed: false,
                target_generation: next_target_generation,
                surface: SurfaceKind::WebView,
                executor_generation: previous_target.executor_generation,
            });
            run.consent = ConsentGate::Bound(successor_binding);
            run.snapshot_id = None;
            run.geometry_revision = 0;
            run.geometry_epoch = 0;
            run.last_image_ok = false;
            run.image_size = (0, 0);
            run.element_refs.clear();
            run.element_boxes.clear();
            run.last_unknown = false;
            run.unknown_idle_confirmed = false;
            Ok(())
        };
        transition.commit(&mut authority_commit)?;
        Ok(next_target_generation)
    }

    fn finish_webview_navigation(
        &self,
        run_generation: u64,
        binding: &crate::task_consent::ConsentBinding,
        fingerprint: [u8; 32],
        request: &NavigationRequest,
        outcome: NavigationTerminal,
        activity: bool,
    ) {
        let run_id = &request.run_id;
        let action_id = &request.action_id;
        let mut inner = self.inner.lock();
        let late = inner
            .runs
            .get(run_id)
            .is_some_and(|run| run.generation != run_generation);
        if late {
            inner.dropped_late = inner.dropped_late.saturating_add(1);
        }
        let (kind, executed, reason, unknown) = match &outcome {
            NavigationTerminal::Success(_) => (OutcomeKind::Applied, true, "applied".into(), false),
            NavigationTerminal::Rejected(detail) => {
                (OutcomeKind::Rejected, false, detail.clone(), false)
            }
            NavigationTerminal::Unknown(detail) => {
                (OutcomeKind::Unknown, false, detail.clone(), true)
            }
        };
        if let Some(run) = inner.runs.get_mut(run_id) {
            if late {
                run.seen.insert(
                    action_id.to_string(),
                    ActionOutcome {
                        action_id: action_id.to_string(),
                        run_id: run_id.to_string(),
                        kind: OutcomeKind::Unknown,
                        executed: false,
                        reason: Some("navigation result after generation change".into()),
                        generation: run_generation,
                    },
                );
                return;
            }
            run.seen.insert(
                action_id.to_string(),
                ActionOutcome {
                    action_id: action_id.to_string(),
                    run_id: run_id.to_string(),
                    kind,
                    executed,
                    reason: Some(reason),
                    generation: run_generation,
                },
            );
            if unknown {
                run.last_unknown = true;
            }
            run.navigation_replay.insert(
                action_id.to_string(),
                NavigationReplay {
                    binding: binding.clone(),
                    fingerprint,
                    origin_target_id: request.target_id.clone(),
                    origin_target_generation: request.target_generation,
                    outcome,
                },
            );
        }
        if !late && activity && !unknown {
            super::note_locked_activity(&mut inner, run_id);
        }
    }

    /// Return a terminal result for the exact historical App WebView action
    /// without consulting the retired native document or dispatching again.
    /// The current permit proves a live owner/task/run; the replay record proves
    /// the predecessor document and exact request fingerprint.
    pub(crate) fn replay_webview_navigation_for_permit(
        &self,
        permit: &crate::task_consent::ExecutionPermit,
        args: &serde_json::Value,
    ) -> Result<Option<WebViewNavigationCompletion>, BrokerError> {
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
        let Some(replay) = run.navigation_replay.get(action_id) else {
            return Ok(None);
        };
        if !same_navigation_owner(replay, permit)
            || run.fingerprints.get(action_id) != Some(&replay.fingerprint)
        {
            return Err(BrokerError::DuplicateAction);
        }
        let request = crate::tools::parse_webview_navigation_request(
            &permit.run_id,
            Some(&replay.origin_target_id),
            args,
        )?;
        if request.target_id != replay.origin_target_id
            || request.target_generation != replay.origin_target_generation
        {
            return Err(BrokerError::Schema("actionId fingerprint conflict".into()));
        }
        let fingerprint = navigation_fingerprint(&request);
        if fingerprint != replay.fingerprint {
            return Err(BrokerError::Schema("actionId fingerprint conflict".into()));
        }
        replay_navigation(replay.outcome.clone()).map(Some)
    }

    fn trace_webview_navigation(&self, request: &NavigationRequest) {
        let detail = request.url.as_deref().map_or_else(
            || format!("{} {}", request.kind.as_wire(), request.action_id),
            |url| {
                let redacted = url.split(['?', '#']).next().unwrap_or(url);
                format!(
                    "{} {} {redacted}",
                    request.kind.as_wire(),
                    request.action_id
                )
            },
        );
        let mut inner = self.inner.lock();
        push_trace(
            &mut inner,
            "webview_navigation",
            &request.run_id,
            &detail,
            TraceAudience::Model,
        );
    }
}

fn navigation_fingerprint(request: &NavigationRequest) -> [u8; 32] {
    action_ledger::fingerprint(&action_ledger::canonical_semantics(
        request.kind.as_wire(),
        &request.target_id,
        request.target_generation,
        "",
        None,
        &serde_json::json!({ "url": request.url }),
    ))
}

fn same_navigation_owner(
    replay: &NavigationReplay,
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

fn replay_navigation(
    replay: NavigationTerminal,
) -> Result<WebViewNavigationCompletion, BrokerError> {
    match replay {
        NavigationTerminal::Success(result) => Ok(*result),
        NavigationTerminal::Rejected(detail) => Err(BrokerError::Adapter(detail)),
        NavigationTerminal::Unknown(detail) => {
            Err(BrokerError::Adapter(format!("{detail}; outcome unknown")))
        }
    }
}

fn navigation_error(error: NavigationError) -> BrokerError {
    match error.disposition {
        NavigationDisposition::NotDispatched => BrokerError::Adapter(error.detail),
        NavigationDisposition::DispatchedUnknown => {
            BrokerError::Adapter(format!("{}; outcome unknown", error.detail))
        }
    }
}
