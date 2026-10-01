use super::*;

pub(super) fn semantic_hit(
    nodes: &[crate::protocol::ObservationNode],
    action: crate::protocol::ActionKind,
    x: f64,
    y: f64,
) -> Option<&crate::protocol::ObservationNode> {
    use crate::protocol::ActionKind;
    let name = match action {
        ActionKind::Click => "click",
        ActionKind::SetValue => "set_value",
        ActionKind::TypeText => "type_text",
        ActionKind::Key => "key",
        ActionKind::Scroll => "scroll",
        ActionKind::Drag => "drag",
        ActionKind::Wait => "wait",
    };
    let mut hits: Vec<_> = nodes
        .iter()
        .filter(|node| node.contains_point(x, y) && node.actions.iter().any(|a| a == name))
        .filter_map(|node| {
            let area = node.width? * node.height?;
            (area.is_finite() && area > 0.0).then_some((area, node))
        })
        .collect();
    hits.sort_by(|a, b| a.0.total_cmp(&b.0));
    let first = hits.first()?;
    // Prefer the smallest actionable control, never an enclosing container or
    // an arbitrary tied sibling. An ambiguous hit keeps the directed coordinate.
    if hits.get(1).is_some_and(|next| next.0 == first.0) {
        return None;
    }
    Some(first.1)
}

impl ComputerUseBroker {
    pub fn act(&self, req: ActionRequest) -> ActionOutcome {
        match self.act_inner(&req) {
            Ok(o) => o,
            Err(e) => ActionOutcome {
                action_id: req.action_id,
                run_id: req.run_id,
                kind: OutcomeKind::Rejected,
                executed: false,
                reason: Some(e.to_string()),
                generation: 0,
            },
        }
    }

    fn act_inner(&self, req: &ActionRequest) -> Result<ActionOutcome, BrokerError> {
        if let Err(s) = req.validate_schema() {
            return Err(BrokerError::Schema(s));
        }
        let (authorized, executor) = self.authorized_binding(&req.run_id)?;
        if authorized.target_id != req.target_id {
            return Err(BrokerError::IdentityMismatch("targetId"));
        }
        let surface = authorized.surface;
        let adapter = executor.adapter;
        let capabilities = adapter.capabilities();
        if !capabilities.allows(req.action, &req.target) {
            return Err(BrokerError::Schema(format!(
                "unsupported action {:?} for backend {}; not dispatched",
                req.action,
                adapter.backend_id()
            )));
        }
        let primary_click = req.action != crate::protocol::ActionKind::Click
            || (crate::protocol::click_count(&req.parameters) == 1
                && crate::protocol::click_button(&req.parameters) == "left");
        let prefer_semantic = capabilities.support_for(req.action).semantic && primary_click;
        // YOLO / acceptEdits in parameters never grant desktop control.
        let t0 = Instant::now();
        {
            let g = self.inner.lock();
            if let Some(run) = g.runs.get(&req.run_id) {
                if let Some(t) = run.target.as_ref() {
                    if t.target_id != req.target_id {
                        return Err(BrokerError::IdentityMismatch("targetId"));
                    }
                }
            }
        }
        if !adapter.input_available() {
            let _ = self.pause_due_to(&req.run_id, "session locked");
            return Err(BrokerError::Schema("session locked; run paused".into()));
        }
        if !adapter.foreground_input_available(&req.target_id) {
            let _ = self.pause_due_to(&req.run_id, "focus drifted");
            return Err(BrokerError::Schema("focus drifted; run paused".into()));
        }
        if adapter.user_input_active() {
            let _ = self.pause_due_to(&req.run_id, "user input");
            return Err(BrokerError::Schema("user input; run paused".into()));
        }
        {
            let epoch = {
                let g = self.inner.lock();
                g.runs
                    .get(&req.run_id)
                    .map(|run| run.geometry_epoch)
                    .unwrap_or(0)
            };
            if epoch != 0 && adapter.current_geometry_revision_for(&req.target_id) != epoch {
                return Err(BrokerError::IdentityMismatch("geometryRevision"));
            }
        }
        let (generation, cancellation, inflight, dispatch_target, managed_request) = {
            let mut g = self.inner.lock();
            if !g.feature_enabled {
                return Err(BrokerError::FeatureDisabled);
            }
            let run = g
                .runs
                .get_mut(&req.run_id)
                .ok_or(BrokerError::RunNotFound)?;
            if run.stop != StopState::Running {
                return Err(BrokerError::StopRequested);
            }
            if run.paused {
                return Err(BrokerError::Schema("run is paused".into()));
            }
            if let Some(prev) = run.seen.get(&req.action_id) {
                return Ok(prev.clone());
            }
            if run.seen.len() >= self.action_budget {
                return Err(BrokerError::Schema(
                    "action budget exhausted; start a new run".into(),
                ));
            }
            if run.last_unknown {
                return Err(BrokerError::Schema(
                    "unknown result must observe before the next action".into(),
                ));
            }
            if (matches!(req.target, crate::protocol::ActionTarget::Coord { .. })
                || req.action == crate::protocol::ActionKind::Drag)
                && !run.last_image_ok
            {
                return Err(BrokerError::Schema(
                    "coordinate action requires a visual observation".into(),
                ));
            }
            let t = run.target.as_ref().ok_or(BrokerError::TargetUnauthorized)?;
            if t.target_id != req.target_id {
                return Err(BrokerError::IdentityMismatch("targetId"));
            }
            if t.surface != surface || t.executor_generation != executor.generation {
                return Err(BrokerError::IdentityMismatch("surfaceExecutorGeneration"));
            }
            if t.target_generation != req.target_generation {
                return Err(BrokerError::IdentityMismatch("targetGeneration"));
            }
            match run.snapshot_id.as_deref() {
                Some(s) if s == req.snapshot_id => {}
                _ => return Err(BrokerError::IdentityMismatch("snapshotId")),
            }
            if run.geometry_revision != req.geometry_revision {
                return Err(BrokerError::IdentityMismatch("geometryRevision"));
            }
            match &req.target {
                crate::protocol::ActionTarget::Element { element_ref }
                    if !run.element_refs.contains(element_ref) =>
                {
                    return Err(BrokerError::IdentityMismatch("elementRef"));
                }
                crate::protocol::ActionTarget::Coord { x, y }
                    if *x < 0.0
                        || *y < 0.0
                        || *x >= run.image_size.0 as f64
                        || *y >= run.image_size.1 as f64 =>
                {
                    return Err(BrokerError::Schema(
                        "coordinates outside observed image".into(),
                    ));
                }
                _ => {}
            }
            if req.action == crate::protocol::ActionKind::Drag {
                let (x, y) = crate::protocol::drag_destination(&req.parameters)
                    .map_err(BrokerError::Schema)?;
                if x >= run.image_size.0 as f64 || y >= run.image_size.1 as f64 {
                    return Err(BrokerError::Schema(
                        "drag destination outside observed image".into(),
                    ));
                }
            }
            let mut dispatch_target = req.target.clone();
            if prefer_semantic {
                if let crate::protocol::ActionTarget::Coord { x, y } = &req.target {
                    if let Some(node) = semantic_hit(&run.element_boxes, req.action, *x, *y) {
                        if run.element_refs.contains(&node.node_ref) {
                            dispatch_target = crate::protocol::ActionTarget::Element {
                                element_ref: node.node_ref.clone(),
                            };
                        }
                    }
                }
            }
            if run.in_flight.load(Ordering::SeqCst) {
                return Err(BrokerError::LeaseHeld {
                    run_id: req.run_id.clone(),
                });
            }
            if req.action == crate::protocol::ActionKind::Wait {
                if run.observe_count >= self.observe_budget {
                    return Err(BrokerError::Schema(
                        "observe budget exhausted; start a new run".into(),
                    ));
                }
                // A bounded wait is one model observation attempt, including
                // misses. Its internal polling is not UI preview.
                run.observe_count = run.observe_count.saturating_add(1);
            }
            let managed_request = if surface == SurfaceKind::ManagedBrowser {
                Some(
                    self.tabs
                        .admit_managed_request(&req.run_id, run.cancellation.clone())?,
                )
            } else {
                None
            };
            run.in_flight.store(true, Ordering::SeqCst);
            // Reserve before releasing admission; concurrent retries cannot dispatch twice.
            run.seen.insert(
                req.action_id.clone(),
                ActionOutcome {
                    action_id: req.action_id.clone(),
                    run_id: req.run_id.clone(),
                    kind: OutcomeKind::Unknown,
                    executed: false,
                    reason: Some("action pending".into()),
                    generation: run.generation,
                },
            );
            (
                run.generation,
                run.cancellation.clone(),
                run.in_flight.clone(),
                dispatch_target,
                managed_request,
            )
        };
        // Both the caller (through outcome publication) and the worker (through
        // physical completion, even after timeout) must release admission.
        let inflight_guard = Arc::new(InFlightClear(inflight));
        self.trace_stage(
            "prepare",
            req,
            &format!(
                "surface={} backend={} admitted",
                surface.as_wire(),
                adapter.backend_id()
            ),
        );

        if !adapter.target_alive(&req.target_id) {
            return self.record(
                req,
                generation,
                OutcomeKind::Rejected,
                false,
                Some(BrokerError::DeadTarget.to_string()),
                0,
            );
        }

        let dispatch = DispatchRequest {
            managed_request,
            cancellation: cancellation.clone(),
            run_id: req.run_id.clone(),
            action_id: req.action_id.clone(),
            generation,
            target_id: req.target_id.clone(),
            target_generation: req.target_generation,
            snapshot_id: req.snapshot_id.clone(),
            geometry_revision: req.geometry_revision,
            action: req.action,
            target: dispatch_target,
            parameters: req.parameters.clone(),
            scope: ActionScope::Directed,
        };
        if req.action == crate::protocol::ActionKind::Wait {
            let wait_result =
                self.wait_for_element(req, &dispatch, &inflight_guard, surface, adapter.as_ref());
            return match wait_result {
                Ok((kind, executed, detail)) => {
                    self.trace_stage("verify", req, kind.as_str());
                    self.record(
                        req,
                        generation,
                        kind,
                        executed,
                        Some(detail),
                        t0.elapsed().as_millis() as u64,
                    )
                }
                Err(error) => {
                    let text = error.to_string();
                    if text.contains("cancel") {
                        self.trace_stage("cancel", req, "revoked");
                    }
                    self.trace_stage("verify", req, "rejected");
                    self.record(
                        req,
                        generation,
                        OutcomeKind::Rejected,
                        false,
                        Some(text),
                        t0.elapsed().as_millis() as u64,
                    )
                }
            };
        }

        // Desktop native input is globally exclusive. Browser and WebView
        // executors use their own target ownership and never contend for it.
        if surface == SurfaceKind::Desktop {
            if let Err(error) = self
                .lease
                .try_acquire(&req.run_id, &self.instance_id)
                .map_err(|e| match e {
                    crate::error::LeaseError::Held { run_id, .. } => {
                        BrokerError::LeaseHeld { run_id }
                    }
                    crate::error::LeaseError::Io(s) => BrokerError::Adapter(s),
                })
            {
                return self.record(
                    req,
                    generation,
                    OutcomeKind::Rejected,
                    false,
                    Some(error.to_string()),
                    0,
                );
            }
        }

        self.trace_stage("dispatch", req, "directed");

        let (tx, rx) = mpsc::channel();
        let worker_guard = inflight_guard.clone();
        let worker = std::thread::Builder::new()
            .name("computer-use-action".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    cancellation.check()?;
                    adapter.act(&dispatch)
                }))
                .unwrap_or_else(|_| Err("adapter worker panicked; outcome unknown".into()));
                // Physical work has ended. Release our share before waking the
                // caller; its share still fences admission through publication.
                // Otherwise a returned outcome can race the worker's epilogue.
                drop(worker_guard);
                let _ = tx.send(result);
            });
        if let Err(error) = worker {
            return self.record(
                req,
                generation,
                OutcomeKind::Rejected,
                false,
                Some(format!("could not start action worker: {error}")),
                0,
            );
        }

        let adapter_result = match rx.recv_timeout(self.action_timeout) {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                if e.contains("cancel") {
                    self.trace_stage("cancel", req, "revoked");
                }
                self.trace_stage("verify", req, "unknown");
                return self.record(
                    req,
                    generation,
                    OutcomeKind::Unknown,
                    true,
                    Some(e),
                    t0.elapsed().as_millis() as u64,
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.trace_stage("verify", req, "unknown");
                return self.record(
                    req,
                    generation,
                    OutcomeKind::Unknown,
                    true,
                    Some("timeout; not auto-replayed".into()),
                    t0.elapsed().as_millis() as u64,
                );
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.trace_stage("verify", req, "unknown");
                return self.record(
                    req,
                    generation,
                    OutcomeKind::Unknown,
                    true,
                    Some("adapter worker dropped".into()),
                    t0.elapsed().as_millis() as u64,
                );
            }
        };

        if adapter_result.applied {
            self.trace_stage("apply", req, "applied");
        }
        let kind = adapter_result.outcome.unwrap_or({
            if adapter_result.applied
                && adapter_result.verifiable
                && adapter_result.postcondition_ok
            {
                OutcomeKind::Verified
            } else {
                OutcomeKind::Applied
            }
        });
        self.trace_stage("verify", req, kind.as_str());
        self.record(
            req,
            generation,
            kind,
            adapter_result.applied,
            Some(adapter_result.detail),
            t0.elapsed().as_millis() as u64,
        )
    }

    fn trace_stage(&self, stage: &str, req: &ActionRequest, extra: &str) {
        let mut g = self.inner.lock();
        push_trace(
            &mut g,
            stage,
            &req.run_id,
            &format!("{} {:?} {extra}", req.action_id, req.action),
            TraceAudience::Model,
        );
    }

    fn wait_for_element(
        &self,
        req: &ActionRequest,
        dispatch: &DispatchRequest,
        permit: &InFlightClear,
        surface: SurfaceKind,
        adapter: &dyn ComputerUseAdapter,
    ) -> Result<(OutcomeKind, bool, String), BrokerError> {
        if surface != SurfaceKind::Desktop || adapter.wait_uses_retained_reference() {
            // Browser and opted-in desktop refs are snapshot-scoped handles.
            // Re-observing and comparing strings cannot prove native identity.
            let result = adapter.act(dispatch).map_err(BrokerError::Adapter)?;
            dispatch.cancellation.check().map_err(BrokerError::Schema)?;
            if result.verifiable && result.postcondition_ok {
                return Ok((OutcomeKind::Verified, true, "wait matched".into()));
            }
            return Err(BrokerError::Adapter(
                "wait condition was not verified".into(),
            ));
        }
        let element_ref = match &req.target {
            crate::protocol::ActionTarget::Element { element_ref } => element_ref.clone(),
            crate::protocol::ActionTarget::Coord { .. } => {
                return Err(BrokerError::Schema("wait requires elementRef".into()))
            }
        };
        let expected = req
            .parameters
            .get("nameEquals")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let timeout = req
            .parameters
            .get("timeoutMs")
            .and_then(|v| v.as_u64())
            .unwrap_or(2000);
        let start = Instant::now();
        loop {
            dispatch.cancellation.check().map_err(BrokerError::Schema)?;
            if self.stop_state(&req.run_id)? != StopState::Running {
                return Err(BrokerError::StopRequested);
            }
            let observation = self.capture(&req.run_id, false, false, Some(permit))?;
            if observation
                .nodes
                .iter()
                .any(|n| n.node_ref == element_ref && n.name == expected)
            {
                return Ok((OutcomeKind::Verified, true, "wait matched".into()));
            }
            if start.elapsed() >= Duration::from_millis(timeout) {
                return Ok((OutcomeKind::Rejected, false, "wait timed out".into()));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn record(
        &self,
        req: &ActionRequest,
        generation: u64,
        kind: OutcomeKind,
        executed: bool,
        reason: Option<String>,
        act_ms: u64,
    ) -> Result<ActionOutcome, BrokerError> {
        let outcome = ActionOutcome {
            action_id: req.action_id.clone(),
            run_id: req.run_id.clone(),
            kind,
            executed,
            reason,
            generation,
        };
        let mut g = self.inner.lock();
        let late = if let Some(run) = g.runs.get_mut(&req.run_id) {
            if run.generation != generation {
                run.seen.insert(
                    req.action_id.clone(),
                    ActionOutcome {
                        kind: if executed {
                            OutcomeKind::Unknown
                        } else {
                            OutcomeKind::Rejected
                        },
                        reason: Some("result after generation change; do not replay".into()),
                        ..outcome.clone()
                    },
                );
                true
            } else {
                run.timings.act_ms = act_ms;
                if kind == OutcomeKind::Unknown {
                    run.last_unknown = true;
                }
                run.seen.insert(req.action_id.clone(), outcome.clone());
                false
            }
        } else {
            false
        };
        if late {
            g.dropped_late += 1;
            push_trace(
                &mut g,
                "late",
                &req.run_id,
                &req.action_id,
                TraceAudience::Model,
            );
            return Ok(ActionOutcome {
                kind: if executed {
                    OutcomeKind::Unknown
                } else {
                    OutcomeKind::Rejected
                },
                executed,
                reason: Some("late result after generation change".into()),
                ..outcome
            });
        }
        push_trace(
            &mut g,
            "act",
            &req.run_id,
            &format!("{} {:?} executed={}", req.action_id, kind, executed),
            TraceAudience::Model,
        );
        Ok(outcome)
    }
}
