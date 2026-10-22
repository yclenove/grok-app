//! Actual ToolCall/release consumer; native effects run after map guards drop.
use super::turn_event_owner::TurnEventOwner;
use super::*;
use crate::acp_client::AcpEvent;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

pub(super) struct ToolCallChunk {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub raw: Value,
}

pub(super) enum ToolEvent {
    Call(ToolCallChunk),
    Released(String),
}

pub(super) struct ToolCompletion {
    owner: TurnEventOwner,
}

pub(super) struct ToolUpdate {
    pub snapshot: SessionSnapshot,
    pub finished: bool,
    pub completion: Option<ToolCompletion>,
    empty_run: Option<(String, String, String)>,
    pub emits: Vec<StreamEmitPayload>,
    pub persists: Vec<PendingSessionPersist>,
    pub journal: Option<PendingCompletedToolJournal>,
    pub tool: Option<Value>,
    pub generated: Option<Value>,
    audit: Option<ToolAudit>,
}

struct ToolAudit {
    name: String,
    status: String,
    summary: String,
    open_changed: bool,
    already_terminal: bool,
}

struct ToolPresentation {
    basis: ToolIdentityBasis,
    identity: Option<ToolIdentity>,
    title: String,
    kind: String,
    name: String,
    input: Option<String>,
    detail: Option<String>,
    path: Option<String>,
    output: Option<String>,
    before: Option<String>,
    after: Option<String>,
    attachment: Option<crate::store::MessageAttachmentStored>,
}

#[derive(PartialEq, Eq)]
struct ToolIdentityBasis {
    identity: Option<ToolIdentity>,
    capacity: bool,
}

impl SessionManager {
    pub(super) fn handle_tool_event(
        &self,
        app: &AppHandle,
        process: &str,
        agent: Option<&str>,
        event: ToolEvent,
    ) {
        let Some(update) = self.prepare_tool_event(process, agent, event) else {
            return;
        };
        Self::emit_stream_payloads(app, update.emits);
        Self::commit_session_persists(Some(app), update.persists);
        Self::emit_empty_run_if_any(app, update.empty_run);
        if let Some(job) = update.journal {
            schedule_completed_tool_journal_persist(job);
        }
        if let Some(audit) = update.audit {
            Self::audit_tool_call(
                update.snapshot.session_id.as_deref().unwrap_or_default(),
                update.snapshot.project_path.as_deref(),
                &audit.name,
                &audit.status,
                Some(&audit.summary),
                audit.open_changed,
                audit.already_terminal,
            );
        }
        if let Some(payload) = update.generated {
            Self::grant_tool_media(&payload);
            let _ = app.emit("session://generated_image", payload);
        }
        if let Some(payload) = update.tool {
            let _ = app.emit("session://tool", payload);
        }
        if update.finished {
            if let Some(completion) = update.completion {
                if let Some(snapshot) = self.park_tool_completion(completion) {
                    // Runtime is the completed owner; state reflects actual focus.
                    Self::emit_runtime(app, &snapshot);
                    Self::emit_state(app, &self.snapshot());
                }
            }
        }
    }

    pub(super) fn grant_tool_media(payload: &Value) {
        if let Some(path) = payload.get("path").and_then(Value::as_str) {
            grant_media_attachment_path(path);
        }
    }

    pub(super) fn park_tool_completion(
        &self,
        completion: ToolCompletion,
    ) -> Option<SessionSnapshot> {
        let owner = completion.owner;
        self.promote_background_ready_to_parked_matching(&owner.id, |s| owner.matches(s));
        // Completion may now be live, background or already parked. A different
        // generation must receive neither this parking action nor stale Ready.
        let _transfer = self.runtime_transfer.lock();
        if let Some(snapshot) = self
            .inner
            .lock()
            .as_ref()
            .filter(|s| owner.matches(s))
            .map(Self::snapshot_from_live)
        {
            return Some(snapshot);
        }
        if let Some(snapshot) = self
            .background
            .lock()
            .get(&owner.id)
            .filter(|s| owner.matches(s))
            .map(Self::snapshot_from_live)
        {
            return Some(snapshot);
        }
        self.parked
            .lock()
            .get(&owner.id)
            .filter(|s| owner.matches_parked(s))
            .map(Self::snapshot_from_parked)
    }

    pub(super) fn prepare_tool_event(
        &self,
        process: &str,
        agent: Option<&str>,
        event: ToolEvent,
    ) -> Option<ToolUpdate> {
        let route = self.capture_turn_event_owner(process, agent)?;
        #[cfg(test)]
        super::command_spawn_options_tests::tool_owner_tests::pause_after_route(&route.id);
        let (id, project) = self
            .with_turn_event_owner_mut(&route, |s, _| {
                (!Self::is_session_load_replay(s))
                    .then(|| (s.app_session_id.clone(), s.project_path.clone()))
            })
            .flatten()?;
        loop {
            let presentation = match &event {
                ToolEvent::Call(call) => {
                    Some(self.prepare_tool_presentation(&id, project.as_deref(), call))
                }
                ToolEvent::Released(_) => None,
            };
            #[cfg(test)]
            super::command_spawn_options_tests::tool_owner_tests::pause_before_mutation(&route.id);
            let result = self.with_turn_event_owner_mut(&route, |s, background| {
            if Self::is_session_load_replay(s) {
                return None;
            }
            let mut emits = Vec::new();
            let mut persists = Vec::new();
            let (tool, generated, journal, audit) = match &event {
                ToolEvent::Call(call) => {
                    let fields = presentation?;
                    // Preparation may have raced another accepted update. Rebuild
                    // outside all runtime-map guards before consuming this event.
                    if !self.publish_tool_identity(&s.app_session_id, &call.id, &fields) {
                        return Some(Err(()));
                    }
                    let generated = fields.attachment.as_ref().map(|att| {
                        if !s.stream_attachments.iter().any(|a| a.path == att.path) {
                            s.stream_attachments.push(att.clone());
                        }
                        json!({
                            "sessionId": s.app_session_id,
                            "messageId": s.streaming_message_id.clone().unwrap_or_default(),
                            "path": att.path, "name": att.name, "toolCallId": call.id,
                            "kind": if is_video_fs_path(&att.path) { "video" } else { "image" },
                        })
                    });
                    Self::touch_stream_progress_locked(s);
                    let already_terminal =
                        !call.id.is_empty() && s.terminal_tool_ids.contains(&call.id);
                    let open_changed = !call.id.is_empty()
                        && Self::note_tool_status_on_session(s, &call.id, &call.status);
                    s.tools_this_turn = s.tools_this_turn.saturating_add(1);
                    let status = if call.status.is_empty() {
                        "in_progress"
                    } else {
                        &call.status
                    };
                    let journal = fields.journal(&s.app_session_id, &call.id, status);
                    let tool = json!({
                        "sessionId": s.app_session_id, "toolCallId": call.id,
                        "title": fields.title, "kind": fields.kind, "status": status,
                        "path": fields.path, "detail": fields.detail, "input": fields.input,
                        "output": fields.output, "before": fields.before, "after": fields.after,
                    });
                    let name = if !call.kind.is_empty() {
                        call.kind.clone()
                    } else if background {
                        fields.title.clone()
                    } else if !call.title.is_empty() {
                        call.title.clone()
                    } else {
                        "tool".into()
                    };
                    let audit = ToolAudit {
                        name,
                        status: call.status.clone(),
                        summary: fields.title,
                        open_changed,
                        already_terminal,
                    };
                    (Some(tool), generated, journal, Some(audit))
                }
                ToolEvent::Released(id) => {
                    Self::release_tool_open_on_session(s, id);
                    Self::touch_stream_progress_locked(s);
                    (None, None, None, None)
                }
            };
            let finish =
                Self::try_finish_deferred_prompt_complete(s, Some(&mut emits), Some(&mut persists));
            Some(Ok(ToolUpdate {
                snapshot: Self::snapshot_from_live(s),
                finished: finish.is_some(),
                completion: finish.is_some().then(|| ToolCompletion {
                    owner: TurnEventOwner::capture(s, background),
                }),
                empty_run: finish.flatten(),
                emits,
                persists,
                journal,
                tool,
                generated,
                audit,
            }))
        })
        .flatten()?;
            match result {
                Ok(update) => return Some(update),
                Err(()) => continue,
            }
        }
    }

    fn tool_identity_basis(&self, id: &str, tool: &str) -> ToolIdentityBasis {
        let cache = self
            .tool_identities
            .lock()
            .expect("tool identity map poisoned");
        let session = cache.get(id);
        let identity = session.and_then(|map| map.get(tool)).cloned();
        let capacity = identity.is_some() || session.is_none_or(|map| map.len() < 2000);
        ToolIdentityBasis { identity, capacity }
    }

    fn publish_tool_identity(&self, id: &str, tool: &str, fields: &ToolPresentation) -> bool {
        let mut cache = self
            .tool_identities
            .lock()
            .expect("tool identity map poisoned");
        let session = cache.get(id);
        let identity = session.and_then(|map| map.get(tool));
        let capacity = identity.is_some() || session.is_none_or(|map| map.len() < 2000);
        if identity != fields.basis.identity.as_ref() || capacity != fields.basis.capacity {
            return false;
        }
        // Comparison and publication share one cache guard. All native effects
        // still follow the accepted mutation after the runtime-map guard drops.
        if let Some(identity) = &fields.identity {
            cache
                .entry(id.to_owned())
                .or_default()
                .insert(tool.to_owned(), identity.clone());
        }
        true
    }

    fn prepare_tool_presentation(
        &self,
        id: &str,
        project: Option<&str>,
        call: &ToolCallChunk,
    ) -> ToolPresentation {
        // Preview only this identity. Publication waits for the actual mutation.
        let basis = self.tool_identity_basis(id, &call.id);
        let identities = std::sync::Mutex::new(HashMap::new());
        if let Some(previous) = &basis.identity {
            identities.lock().unwrap().insert(
                id.to_owned(),
                HashMap::from([(call.id.clone(), previous.clone())]),
            );
        }
        if basis.capacity {
            remember_tool_identity(
                &identities,
                id,
                &call.id,
                &call.title,
                &call.kind,
                &call.raw,
            );
        }
        let (title, kind, name, input) =
            resolve_tool_identity(&identities, id, &call.id, &call.title, &call.kind);
        let identity = identities
            .lock()
            .unwrap()
            .get(id)
            .and_then(|map| map.get(&call.id))
            .cloned();
        let (kind, title) = enrich_tool_identity_from_raw(&call.raw, &title, &kind);
        let (detail, hint) = extract_tool_ui_fields(&call.raw);
        let structured = (call.status == "completed")
            .then(|| extract_structured_media_path(&call.raw))
            .flatten();
        let freeform = (call.status == "completed"
            && structured.is_none()
            && tool_is_media_capable(&name, &kind, &title, &call.raw))
        .then(|| extract_freeform_media_path(&call.raw))
        .flatten();
        let path = structured.or(freeform).or(hint).filter(|p| !p.is_empty());
        let attachment = preview_completed_tool_media_attachment(
            &call.raw,
            &name,
            &kind,
            &title,
            project,
            &call.status,
        )
        .map(|p| attachment_from_path(&p));
        let normalized = normalize_tool_kind_for_journal(&kind, &title);
        let kind = if normalized.is_empty() {
            kind
        } else {
            normalized
        };
        let name = if !name.is_empty() && !name.eq_ignore_ascii_case("tool") {
            name
        } else {
            kind.clone()
        };
        let label = tool_journal_label(&title, &name, &detail, &path);
        let title = if !label.is_empty() && !label.eq_ignore_ascii_case("tool") {
            label
        } else if !title.is_empty() {
            title
        } else {
            detail
                .clone()
                .or_else(|| path.clone())
                .unwrap_or_else(|| "tool".into())
        };
        let (before, after) = extract_tool_content_snippets(&call.raw);
        ToolPresentation {
            basis,
            identity,
            title,
            kind,
            name,
            input,
            detail,
            path,
            before,
            after,
            attachment,
            output: extract_tool_output(&call.raw),
        }
    }
}

impl ToolPresentation {
    fn journal(&self, id: &str, tool: &str, status: &str) -> Option<PendingCompletedToolJournal> {
        if tool.is_empty() || !matches!(status, "completed" | "failed" | "error" | "cancelled") {
            return None;
        }
        let title = tool_journal_one_line(&self.title, 240);
        let mut content = format!("tool_step|{status}|{}|{title}", self.name);
        if let Some(input) = self.input.as_deref().filter(|s| !s.trim().is_empty()) {
            content.push_str("\ninput:");
            content.push_str(&tool_journal_one_line(input, 400));
        }
        if let Some(detail) = &self.detail {
            content.push('\n');
            content.push_str(&detail.chars().take(400).collect::<String>());
        }
        if let Some(path) = &self.path {
            content.push('\n');
            content.push_str(path);
        }
        if let Some(output) = &self.output {
            content.push('\n');
            content.push_str(TOOL_OUTPUT_SENTINEL);
            content.push('\n');
            content.push_str(output);
        }
        Some(PendingCompletedToolJournal {
            app_sid: id.into(),
            tool_call_id: tool.into(),
            mid: format!("tool-{tool}"),
            content,
            is_error: matches!(status, "failed" | "error"),
        })
    }
}

impl ToolEvent {
    pub(super) fn from_acp(event: AcpEvent) -> Option<Self> {
        match event {
            AcpEvent::ToolCall {
                tool_call_id,
                title,
                kind,
                status,
                raw,
            } => Some(Self::Call(ToolCallChunk {
                id: tool_call_id,
                title,
                kind,
                status,
                raw,
            })),
            AcpEvent::ToolOpenReleased { tool_call_id } => Some(Self::Released(tool_call_id)),
            _ => None,
        }
    }
}
