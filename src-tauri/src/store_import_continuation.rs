//! Native continuation publication uses a prepared full row, never a later bind.
use super::*;

/// Caller holds the session index, project, workspace and sidecar locks. The
/// index is published last, after handoff/sidecar/messages are complete.
pub(crate) fn publish(meta: &SessionMeta) -> Result<(), String> {
    let path = sessions_index_file();
    let mut value =
        projects_io::read_value(&path, 64 * 1024 * 1024)?.unwrap_or_else(|| serde_json::json!([]));
    let list: Vec<SessionMeta> =
        serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    if let Some(existing) = list.iter().find(|row| row.id == meta.id) {
        if existing.project_id != meta.project_id
            || existing.workspace_id != meta.workspace_id
            || existing.workspace_root_snapshot != meta.workspace_root_snapshot
            || existing.workspace_capability != meta.workspace_capability
        {
            return Err("MAPPING_CONFLICT: existing continuation target changed".into());
        }
        return Ok(());
    }
    let mut next = list.clone();
    next.push(meta.clone());
    session_refs::validate(&session_refs::snapshot(&list), &next)?;
    value
        .as_array_mut()
        .ok_or("invalid sessions index")?
        .insert(0, serde_json::to_value(meta).map_err(|e| e.to_string())?);
    crate::store_lock::write_bytes_replace(
        &path,
        &serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
    )
}

pub(crate) fn new_meta(
    id: String,
    project_id: Option<String>,
    title: Option<String>,
    scheduled: bool,
) -> SessionMeta {
    let now = Utc::now();
    SessionMeta {
        id: id.clone(),
        project_id: normalize_move_project_id(project_id),
        title: title.unwrap_or_else(|| "New chat".into()),
        agent_session_id: None,
        created_at: now,
        updated_at: now,
        model_id: None,
        archived: false,
        pinned: false,
        provider_id: None,
        agent_execution: None,
        effort: None,
        mode: None,
        permission_policy: None,
        json_schema: None,
        scheduled,
        worktree_path: None,
        worktree_branch: None,
        is_worktree_session: false,
        plugin_dirs: Vec::new(),
        extra_rules: None,
        max_agent_turns: None,
        system_prompt_override: None,
        fork_agent_session: false,
        fork_rewind_prompt_index: None,
        no_ask_user: None,
        workspace_id: None,
        workspace_root_snapshot: None,
        workspace_capability: None,
    }
}

/// Validate a new binding before claiming a durable creation request.
pub(crate) fn validate(meta: &SessionMeta) -> Result<(), String> {
    session_refs::validate(&Default::default(), std::slice::from_ref(meta))
}
