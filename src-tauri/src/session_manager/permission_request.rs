//! Actual native permission-request consumer and deferred native effects.
use super::*;
use crate::acp_client::{AcpEvent, PermissionOutcome};
use crate::permission::{
    coerce_wire_option_id_for_tool, extract_shell_command, may_auto_allow, may_auto_deny,
    permission_path_target, permission_preview_text, resolve_reject_option_id, scope_key,
};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

pub(super) struct PermissionRequestFailure {
    message: String,
    recovery: Option<Box<PermissionRequestRecovery>>,
}

impl std::fmt::Display for PermissionRequestFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::fmt::Debug for PermissionRequestFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

impl From<String> for PermissionRequestFailure {
    fn from(error: String) -> Self {
        Self {
            message: error,
            recovery: None,
        }
    }
}

impl From<&str> for PermissionRequestFailure {
    fn from(error: &str) -> Self {
        error.to_string().into()
    }
}

struct PermissionRequestRecovery {
    owner: turn_event_owner::TurnEventOwner,
    binding: workspace_context::Binding,
    context: Option<workspace_context::RuntimeWorkspaceContext>,
    project_path: Option<String>,
    agent: Option<String>,
    policy: crate::permission::PermissionPolicy,
    update: PermissionRequestUpdate,
}

impl PermissionRequestRecovery {
    fn prepare(
        mut self,
        manager: &SessionManager,
        message: &str,
    ) -> Option<PermissionRequestUpdate> {
        // A file-backed rebind can happen without a runtime event. Check it
        // outside runtime guards, then compare the original runtime owner.
        let row = workspace_context::session(&self.owner.id).ok()?;
        if workspace_context::Binding::from(&row) != self.binding
            || row.agent_session_id != self.agent
        {
            return None;
        }
        if self
            .context
            .as_ref()
            .is_some_and(|context| context.validate(&self.owner.id).is_err())
        {
            return None;
        }
        let mut card = self.update.request.clone()?;
        let owner = self.owner.clone();
        manager
            .with_turn_event_owner_mut(&owner, |session, background| {
                if self.binding != workspace_context::Binding::from(&session.meta)
                    || self.context != session.workspace_context
                    || self.project_path != session.project_path
                    || self.policy != session.policy
                    || session.pending_permission_rpc_id != Some(card.rpc_id)
                    || session.pending_permission_ui.as_ref() != Some(&card)
                    || session.pending_permission_options.as_ref() != Some(&card.options)
                    || session.pending_permission_tool_name.as_ref() != Some(&card.tool_name)
                {
                    return None;
                }
                card.response_error = Some(message.to_string());
                session.pending_permission_ui = Some(card.clone());
                self.update.request = Some(card);
                self.update.background = background;
                self.update.snapshot = SessionManager::snapshot_from_live(session);
                Some(self.update)
            })
            .flatten()
    }
}

pub(super) struct PermissionRequestEvent {
    pub(super) rpc_id: u64,
    pub(super) tool_call_id: String,
    pub(super) tool_name: String,
    pub(super) title: String,
    pub(super) options: Value,
    pub(super) raw: Value,
}

impl PermissionRequestEvent {
    pub(super) fn from_acp(event: AcpEvent) -> Option<Self> {
        match event {
            AcpEvent::PermissionRequest {
                rpc_id,
                tool_call_id,
                tool_name,
                title,
                options,
                raw,
            } => Some(Self {
                rpc_id,
                tool_call_id,
                tool_name,
                title,
                options,
                raw,
            }),
            _ => None,
        }
    }

    fn card(&self, session_id: String) -> UiPermissionRequest {
        let target = permission_path_target(&self.raw, &self.tool_name);
        UiPermissionRequest {
            request_generation: uuid::Uuid::new_v4(),
            rpc_id: self.rpc_id,
            session_id,
            tool_call_id: self.tool_call_id.clone(),
            tool_name: self.tool_name.clone(),
            title: self.title.clone(),
            preview: permission_preview_text(&self.raw, &self.title)
                .chars()
                .take(2000)
                .collect(),
            scope_key: scope_key(
                &self.tool_name,
                if target.is_empty() {
                    &self.title
                } else {
                    &target
                },
            ),
            options: self.options.clone(),
            response_error: None,
        }
    }
}

pub(super) struct PermissionRequestUpdate {
    pub(super) snapshot: SessionSnapshot,
    pub(super) request: Option<UiPermissionRequest>,
    command: String,
    pub(super) background: bool,
    pub(super) emits: Vec<StreamEmitPayload>,
    pub(super) persists: Vec<PendingSessionPersist>,
    empty: Option<(String, String, String)>,
}

impl PermissionRequestUpdate {
    pub(super) fn persist(&self) {
        if let Some(request) = &self.request {
            crate::turn_lease::update_active(
                &request.session_id,
                "permission_prompt",
                true,
                Some(crate::turn_lease::PendingTool {
                    tool_call_id: request.tool_call_id.clone(),
                    tool_name: request.tool_name.clone(),
                    title: request.title.clone(),
                    command: self.command.clone(),
                }),
            );
        }
    }

    fn emit(self, app: &AppHandle) {
        if let Some(request) = &self.request {
            crate::mirror::fanout_event(app, "session://permission", request);
            if self.background {
                let _ = app.emit(
                    "session://background_permission",
                    serde_json::json!({ "sessionId": request.session_id }),
                );
            }
        }
        SessionManager::emit_stream_payloads(app, self.emits);
        SessionManager::commit_session_persists(Some(app), self.persists);
        SessionManager::emit_runtime(app, &self.snapshot);
        SessionManager::emit_empty_run_if_any(app, self.empty);
    }
}

impl SessionManager {
    pub(super) async fn handle_permission_request(
        &self,
        app: &AppHandle,
        process: &str,
        agent: Option<&str>,
        event: PermissionRequestEvent,
    ) {
        let result = self.prepare_permission_request(process, agent, event).await;
        if let Some(update) = self.prepare_permission_request_effect(result) {
            update.persist();
            update.emit(app);
        }
    }

    pub(super) fn prepare_permission_request_effect(
        &self,
        result: Result<Option<PermissionRequestUpdate>, PermissionRequestFailure>,
    ) -> Option<PermissionRequestUpdate> {
        match result {
            Ok(update) => update,
            Err(error) => {
                tracing::warn!(%error, "permission request response failed");
                error
                    .recovery
                    .and_then(|recovery| recovery.prepare(self, &error.message))
            }
        }
    }

    pub(super) async fn prepare_permission_request(
        &self,
        process: &str,
        agent: Option<&str>,
        event: PermissionRequestEvent,
    ) -> Result<Option<PermissionRequestUpdate>, PermissionRequestFailure> {
        use super::permission_resolution::{
            AutomaticPermissionOwner, PermissionAnswer, PermissionResponseKey,
        };
        let Some(owner) = self.capture_turn_event_owner(process, agent) else {
            return Ok(None);
        };
        let Some((meta, context, project_path)) =
            self.with_turn_event_owner_mut(&owner, |session, _| {
                (
                    session.meta.clone(),
                    session.workspace_context.clone(),
                    session.project_path.clone(),
                )
            })
        else {
            return Ok(None);
        };
        #[cfg(test)]
        super::command_spawn_options_tests::permission_request_tests::pause(
            &owner.id,
            "after_route",
        )
        .await;
        // File-backed validation is outside runtime guards. The mutation below
        // rechecks the captured runtime binding as well as ACP/turn identity.
        let binding = workspace_context::Binding::from(&meta);
        let row = workspace_context::session(&owner.id)?;
        if workspace_context::Binding::from(&row) != binding
            || row.agent_session_id != meta.agent_session_id
        {
            return Err("permission request session or workspace binding changed".into());
        }
        if let Some(context) = &context {
            context.validate(&owner.id)?;
        }
        let mut request = event.card(owner.id.clone());
        let target = permission_path_target(&event.raw, &event.tool_name);
        let shell = extract_shell_command(&event.raw);
        let observation = self.with_turn_event_owner_mut(&owner, |session, background| {
            if binding != workspace_context::Binding::from(&session.meta)
                || context != session.workspace_context
                || project_path != session.project_path
            {
                return Err("permission request execution context changed".to_string());
            }
            let client = session
                .acp
                .clone()
                .filter(|client| client.is_alive())
                .ok_or("permission request client is unavailable")?;
            if Self::is_session_load_replay(session) {
                return Ok((
                    true,
                    false,
                    false,
                    session.policy,
                    background,
                    Some((
                        client.clone(),
                        PermissionResponseKey::capture(session, &client, event.rpc_id),
                    )),
                    Self::snapshot_from_live(session),
                ));
            }
            if session.pending_permission_rpc_id.is_some()
                && !session
                    .pending_permission_reply
                    .as_ref()
                    .is_some_and(|reply| reply.matches_pending_gate(session))
            {
                return Err("a permission request is already pending on this chat".into());
            }
            let root = session.project_path.as_ref().map(std::path::PathBuf::from);
            let auto = may_auto_allow(
                session.policy,
                &session.allow_cache,
                &request.scope_key,
                root.as_deref(),
                &target,
                &event.tool_name,
                &shell,
            );
            let deny = !auto && may_auto_deny(session.policy);
            if deny {
                let option_id = resolve_reject_option_id(&request.options);
                request.options =
                    serde_json::json!([{"optionId": option_id, "kind": "reject_once"}]);
            }
            Self::touch_activity_locked(session);
            let _ = session.fsm.await_permission();
            session.pending_permission_rpc_id = Some(request.rpc_id);
            session.pending_permission_reply = None;
            session.pending_permission_options = Some(request.options.clone());
            session.pending_permission_tool_name = Some(request.tool_name.clone());
            session.pending_permission_ui = Some(request.clone());
            Ok((
                false,
                auto,
                deny,
                session.policy,
                background,
                None,
                Self::snapshot_from_live(session),
            ))
        });
        let Some(observation) = observation else {
            return Ok(None);
        };
        let (replay, auto, deny, policy, background, client, snapshot) = observation?;
        if replay {
            let (client, key) = client.ok_or("replay permission client missing")?;
            let _reservation = self.reserve_permission_response(key)?;
            if !self
                .with_turn_event_owner_mut(&owner, |session, _| {
                    Self::is_session_load_replay(session)
                })
                .unwrap_or(false)
            {
                return Err("replay permission owner changed".into());
            }
            let option_id = coerce_wire_option_id_for_tool(
                "allow_once",
                None,
                &event.options,
                &event.tool_name,
            );
            client
                .respond_permission(event.rpc_id, PermissionOutcome::Selected { option_id })
                .await?;
            return Ok(None);
        }
        let mut update = PermissionRequestUpdate {
            snapshot,
            request: Some(request.clone()),
            command: extract_tool_input(&event.raw).unwrap_or_default(),
            background,
            emits: Vec::new(),
            persists: Vec::new(),
            empty: None,
        };
        if auto || deny {
            #[cfg(test)]
            super::command_spawn_options_tests::permission_request_tests::pause(
                &owner.id,
                "before_answer_capture",
            )
            .await;
            // Shared transport reservation, wire coercion, strict owner checks,
            // deferred finish and audit: do not maintain a second answer path.
            let answer = PermissionAnswer {
                rpc_id: event.rpc_id,
                request_generation: Some(request.request_generation),
                decision: if auto {
                    "allow_once".into()
                } else {
                    "deny".into()
                },
                option_id: deny.then(|| resolve_reject_option_id(&event.options)),
                scope: None,
                session_id: Some(owner.id.clone()),
                client_options: None,
                client_tool_name: None,
            };
            let result = self
                .resolve_permission_answer(
                    answer,
                    Some(AutomaticPermissionOwner {
                        decision: if auto { "auto_allow" } else { "auto_deny" },
                        turn: &owner,
                        policy,
                        binding: &binding,
                        context: context.as_ref(),
                        project_path: project_path.as_deref(),
                        request: &request,
                    }),
                )
                .await;
            let resolution = match result {
                Ok(resolution) => resolution,
                Err(failure) => {
                    return Err(PermissionRequestFailure {
                        message: failure.message,
                        // Partial/full writes, flush failures and post-write
                        // owner errors must never invite a duplicate response.
                        recovery: (!failure.reply_started).then(|| {
                            Box::new(PermissionRequestRecovery {
                                owner,
                                binding,
                                context,
                                project_path,
                                agent: meta.agent_session_id,
                                policy,
                                update,
                            })
                        }),
                    });
                }
            };
            update.snapshot = resolution.snapshot;
            update.request = None;
            update.emits = resolution.emits;
            update.persists = resolution.persists;
            update.empty = resolution.empty_run;
        }
        Ok(Some(update))
    }
}
