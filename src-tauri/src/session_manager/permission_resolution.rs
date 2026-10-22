//! Actual tool-approval command core; transport and native effects stay separate.
use super::*;
use crate::acp_client::PermissionOutcome;
use crate::permission::{should_widen_session_allow, PermissionPolicy};
use serde_json::Value;

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct PermissionResponseKey {
    session: String,
    client: usize,
    epoch: u64,
    turn: Option<String>,
    rpc: u64,
    request_generation: Option<uuid::Uuid>,
}

pub(super) struct ResponseReservation<'a> {
    responses: &'a Mutex<HashSet<PermissionResponseKey>>,
    key: PermissionResponseKey,
}

impl PermissionResponseKey {
    pub(super) fn capture(
        session: &LiveSession,
        client: &Arc<crate::acp_client::AcpClient>,
        rpc: u64,
    ) -> Self {
        Self {
            session: session.app_session_id.clone(),
            client: Arc::as_ptr(client) as usize,
            epoch: session.turn_open_epoch,
            turn: session.active_turn_id.clone(),
            rpc,
            request_generation: session
                .pending_permission_ui
                .as_ref()
                .map(|request| request.request_generation),
        }
    }

    pub(super) fn matches_pending_gate(&self, session: &LiveSession) -> bool {
        // Delivery is a fact about this gate on this transport. A policy,
        // workspace or turn change cannot make the same RPC unsent again.
        self.session == session.app_session_id
            && session
                .acp
                .as_ref()
                .is_some_and(|client| self.client == Arc::as_ptr(client) as usize)
            && session.pending_permission_rpc_id == Some(self.rpc)
            && self.request_generation
                == session
                    .pending_permission_ui
                    .as_ref()
                    .map(|request| request.request_generation)
    }
}

impl Drop for ResponseReservation<'_> {
    fn drop(&mut self) {
        self.responses.lock().remove(&self.key);
    }
}

struct PermissionOwner {
    turn: turn_event_owner::TurnEventOwner,
    client: Arc<crate::acp_client::AcpClient>,
    key: PermissionResponseKey,
    binding: workspace_context::Binding,
    context: Option<workspace_context::RuntimeWorkspaceContext>,
    project_path: Option<String>,
    agent: Option<String>,
    options: Option<Value>,
    tool: Option<String>,
    ui: Option<UiPermissionRequest>,
    policy: PermissionPolicy,
    reply_sent: bool,
}

impl PermissionOwner {
    fn capture(
        session: &LiveSession,
        rpc: u64,
        request_generation: Option<uuid::Uuid>,
    ) -> Result<Self, String> {
        match session.pending_permission_rpc_id {
            Some(id) if id == rpc => {}
            Some(_) => {
                return Err(
                    "stale permission request — this approval no longer matches the pending gate"
                        .into(),
                )
            }
            None => {
                return Err(
                    "no pending permission on this chat — request expired or was cancelled".into(),
                )
            }
        }
        if session
            .pending_permission_ui
            .as_ref()
            .is_some_and(|ui| ui.rpc_id != rpc || ui.session_id != session.app_session_id)
        {
            return Err("pending permission card no longer matches this request".into());
        }
        match (&session.pending_permission_ui, request_generation) {
            (Some(card), Some(generation)) if card.request_generation == generation => {}
            // Legacy fallback is only for gates with no generated Host card.
            (None, None) => {}
            _ => return Err("stale permission request identity; refresh the pending card".into()),
        }
        let client = session
            .acp
            .clone()
            .ok_or("no agent process for this chat; permission request expired")?;
        if session
            .pending_permission_reply
            .as_ref()
            .is_some_and(|reply| reply.matches_pending_gate(session))
        {
            return Err("this permission response has already been sent".into());
        }
        Ok(Self {
            turn: turn_event_owner::TurnEventOwner::capture(session, false),
            key: PermissionResponseKey::capture(session, &client, rpc),
            client,
            binding: workspace_context::Binding::from(&session.meta),
            context: session.workspace_context.clone(),
            project_path: session.project_path.clone(),
            agent: session.meta.agent_session_id.clone(),
            options: session.pending_permission_options.clone(),
            tool: session.pending_permission_tool_name.clone(),
            ui: session.pending_permission_ui.clone(),
            policy: session.policy,
            reply_sent: false,
        })
    }

    fn matches(&self, session: &LiveSession) -> bool {
        self.turn.matches(session)
            && self.binding == workspace_context::Binding::from(&session.meta)
            && self.context == session.workspace_context
            && self.project_path == session.project_path
            && session.pending_permission_rpc_id == Some(self.key.rpc)
            && session.pending_permission_options == self.options
            && session.pending_permission_tool_name == self.tool
            && session.pending_permission_ui == self.ui
            && session.policy == self.policy
    }

    fn check(&self, manager: &SessionManager) -> Result<(), String> {
        if !self.client.is_alive() {
            return Err(
                "agent process is no longer running; permission request expired — reopen the chat"
                    .into(),
            );
        }
        // Strict store/context reads can acquire file locks; keep them outside
        // runtime-map guards. The final mutation rechecks the captured owner.
        let row = workspace_context::session(&self.turn.id)?;
        if workspace_context::Binding::from(&row) != self.binding
            || row.agent_session_id != self.agent
        {
            return Err("permission session or workspace binding changed".into());
        }
        if let Some(context) = &self.context {
            context.validate(&self.turn.id)?;
        }
        if !manager
            .with_session_mut(&self.turn.id, |session| {
                self.matches(session)
                    && (self.reply_sent
                        || !session
                            .pending_permission_reply
                            .as_ref()
                            .is_some_and(|reply| reply.matches_pending_gate(session)))
            })
            .unwrap_or(false)
        {
            return Err("permission request owner changed; the pending gate was preserved".into());
        }
        Ok(())
    }

    fn record_delivery(&mut self, manager: &SessionManager) {
        // Record immediately after successful write+flush, before store/audit
        // work or any test checkpoint. No await or borrowed map guard escapes.
        manager.with_session_mut(&self.turn.id, |session| {
            if self.key.matches_pending_gate(session) {
                session.pending_permission_reply = Some(self.key.clone());
            }
        });
        self.reply_sent = true;
    }

    fn reserve<'a>(&self, manager: &'a SessionManager) -> Result<ResponseReservation<'a>, String> {
        manager.reserve_permission_response(self.key.clone())
    }
}

pub(super) struct PermissionAnswer {
    pub(super) rpc_id: u64,
    pub(super) request_generation: Option<uuid::Uuid>,
    pub(super) decision: String,
    pub(super) option_id: Option<String>,
    pub(super) scope: Option<String>,
    pub(super) session_id: Option<String>,
    pub(super) client_options: Option<Value>,
    pub(super) client_tool_name: Option<String>,
}

pub(super) struct AutomaticPermissionOwner<'a> {
    pub(super) decision: &'a str,
    pub(super) turn: &'a turn_event_owner::TurnEventOwner,
    pub(super) policy: PermissionPolicy,
    pub(super) binding: &'a workspace_context::Binding,
    pub(super) context: Option<&'a workspace_context::RuntimeWorkspaceContext>,
    pub(super) project_path: Option<&'a str>,
    pub(super) request: &'a UiPermissionRequest,
}

impl AutomaticPermissionOwner<'_> {
    fn matches(&self, session: &LiveSession) -> bool {
        self.turn.matches(session)
            && self.policy == session.policy
            && *self.binding == workspace_context::Binding::from(&session.meta)
            && self.context == session.workspace_context.as_ref()
            && self.project_path == session.project_path.as_deref()
            && session.pending_permission_rpc_id == Some(self.request.rpc_id)
            && session.pending_permission_options.as_ref() == Some(&self.request.options)
            && session.pending_permission_tool_name.as_ref() == Some(&self.request.tool_name)
            && session.pending_permission_ui.as_ref() == Some(self.request)
    }
}

pub(super) struct PermissionResolution {
    pub(super) snapshot: SessionSnapshot,
    pub(super) emits: Vec<StreamEmitPayload>,
    pub(super) persists: Vec<PendingSessionPersist>,
    pub(super) empty_run: Option<(String, String, String)>,
}

pub(super) struct PermissionAnswerFailure {
    pub(super) message: String,
    pub(super) reply_started: bool,
}

impl From<String> for PermissionAnswerFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            reply_started: false,
        }
    }
}

impl From<&str> for PermissionAnswerFailure {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}

impl PermissionAnswerFailure {
    fn after_write(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            reply_started: true,
        }
    }
}

impl SessionManager {
    pub(super) fn reserve_permission_response(
        &self,
        key: PermissionResponseKey,
    ) -> Result<ResponseReservation<'_>, String> {
        if !self.permission_responses.lock().insert(key.clone()) {
            return Err("this permission request is already being answered".into());
        }
        Ok(ResponseReservation {
            responses: &self.permission_responses,
            key,
        })
    }

    pub(super) async fn resolve_permission_core(
        &self,
        answer: PermissionAnswer,
    ) -> Result<PermissionResolution, String> {
        self.resolve_permission_answer(answer, None)
            .await
            .map_err(|error| error.message)
    }

    pub(super) async fn resolve_permission_answer(
        &self,
        answer: PermissionAnswer,
        automatic: Option<AutomaticPermissionOwner<'_>>,
    ) -> Result<PermissionResolution, PermissionAnswerFailure> {
        let PermissionAnswer {
            rpc_id,
            request_generation,
            decision,
            option_id,
            scope,
            session_id,
            client_options,
            client_tool_name,
        } = answer;
        let target = self.resolve_target_session(session_id)?;
        let scope_to_cache = if decision == "allow_session" || decision == "allow_for_session" {
            scope.filter(|s| !s.trim().is_empty())
        } else {
            None
        };
        // Capture the gate and its generation together; observation has no
        // activity, FSM or authorization side effects on expired requests.
        let mut owner = self
            .with_session_mut(&target, |session| {
                if automatic
                    .as_ref()
                    .is_some_and(|owner| !owner.matches(session))
                {
                    return Err(
                        "automatic permission request owner changed; the pending gate was preserved"
                            .into(),
                    );
                }
                PermissionOwner::capture(session, rpc_id, request_generation)
            })
            .ok_or("no session")??;
        if decision != "deny"
            && decision != "cancel"
            && owner.ui.as_ref().is_some_and(|ui| {
                ui.options.as_array().is_some_and(|options| {
                    !options.is_empty()
                        && options.iter().all(|option| {
                            matches!(
                                option.get("kind").and_then(Value::as_str),
                                Some(
                                    "reject_once"
                                        | "reject_always"
                                        | "reject-once"
                                        | "reject-always"
                                )
                            )
                        })
                })
            })
        {
            return Err("this pending permission only allows rejection".into());
        }
        if let (Some(scope), Some(ui)) = (&scope_to_cache, &owner.ui) {
            if scope != &ui.scope_key {
                return Err("permission scope no longer matches the pending request".into());
            }
        }
        let _reservation = owner.reserve(self)?;
        owner.check(self)?;
        let pending_options = owner
            .options
            .clone()
            .unwrap_or_else(|| serde_json::json!([]));
        let tool_name = owner
            .tool
            .clone()
            .filter(|tool| !tool.is_empty())
            .or_else(|| {
                client_tool_name
                    .as_ref()
                    .map(|tool| tool.trim().to_string())
                    .filter(|tool| !tool.is_empty())
            })
            .unwrap_or_default();
        let options = if pending_options.as_array().is_some_and(|a| !a.is_empty()) {
            pending_options
        } else if let Some(options) =
            client_options.filter(|v| v.as_array().is_some_and(|a| !a.is_empty()))
        {
            options
        } else {
            pending_options
        };
        let outcome = match decision.as_str() {
            "cancel" => PermissionOutcome::Cancelled,
            other => {
                let wire = crate::permission::coerce_wire_option_id_for_tool(
                    other,
                    option_id.as_deref(),
                    &options,
                    &tool_name,
                );
                if option_id.as_deref() != Some(wire.as_str()) {
                    tracing::info!(decision = %other, client = ?option_id, wire = %wire, tool = %tool_name,
                        "permission optionId coerced to CLI wire id");
                }
                PermissionOutcome::Selected { option_id: wire }
            }
        };
        #[cfg(test)]
        super::command_spawn_options_tests::permission_resolution_tests::pause(
            &target,
            "before_write",
        )
        .await;
        #[cfg(test)]
        super::command_spawn_options_tests::permission_request_tests::pause(
            &target,
            "before_write",
        )
        .await;
        owner.check(self)?;
        owner
            .client
            .respond_permission_tracked(rpc_id, outcome)
            .await
            .map_err(|failure| PermissionAnswerFailure {
                message: failure.message,
                reply_started: failure.may_have_written,
            })?;
        owner.record_delivery(self);
        #[cfg(test)]
        super::command_spawn_options_tests::permission_resolution_tests::pause(
            &target,
            "before_audit",
        )
        .await;
        // The wire response is already sent to the original child. Keep that
        // audit even if a newer gate appeared before runtime finalization.
        if let Some(automatic) = automatic {
            crate::audit_ledger::record_permission(
                Some(&target),
                owner.project_path.as_deref(),
                owner.tool.as_deref().unwrap_or(&tool_name),
                automatic.decision,
                owner.ui.as_ref().map(|ui| ui.title.as_str()),
            );
        } else {
            crate::audit_ledger::record_permission(
                Some(&target),
                owner.project_path.as_deref(),
                &tool_name,
                &decision,
                owner.ui.as_ref().map(|ui| ui.title.as_str()),
            );
        }
        #[cfg(test)]
        super::command_spawn_options_tests::permission_resolution_tests::pause(
            &target,
            "after_write",
        )
        .await;
        #[cfg(test)]
        super::command_spawn_options_tests::permission_request_tests::pause(&target, "after_write")
            .await;
        owner
            .check(self)
            .map_err(PermissionAnswerFailure::after_write)?;
        let mut emits = Vec::new();
        let mut persists = Vec::new();
        let (snapshot, empty_run) = self
            .with_turn_event_owner_mut(&owner.turn, |s, _| {
                if !owner.matches(s) || !owner.client.is_alive() {
                    return None;
                }
                Self::touch_activity_locked(s);
                s.pending_permission_rpc_id = None;
                s.pending_permission_reply = None;
                s.pending_permission_options = None;
                s.pending_permission_tool_name = None;
                s.pending_permission_ui = None;
                if let Some(scope) = scope_to_cache {
                    s.allow_cache.allow(scope);
                    if should_widen_session_allow(s.policy) {
                        s.allow_cache.allow_tool_family(&tool_name);
                    }
                }
                if s.fsm.state() == SessionState::AwaitingPermission {
                    let _ = s.fsm.permission_resolved_continue();
                }
                let empty_run = Self::try_finish_deferred_prompt_complete(
                    s,
                    Some(&mut emits),
                    Some(&mut persists),
                )
                .flatten();
                Some((Self::snapshot_from_live(s), empty_run))
            })
            .flatten()
            .ok_or_else(|| {
                PermissionAnswerFailure::after_write(
                    "permission request owner changed; the pending gate was preserved",
                )
            })?;
        Ok(PermissionResolution {
            snapshot,
            emits,
            persists,
            empty_run,
        })
    }
}
