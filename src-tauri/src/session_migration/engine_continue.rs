//! Continuation preparation and recoverable native session creation.
use super::*;
use crate::session_migration::continuation_scope;

#[cfg(test)]
pub fn continue_prepare(
    import_id: &str,
    revision: Option<&str>,
    leaf: Option<&str>,
    project_id: Option<String>,
    capability: Option<String>,
) -> Result<HandoffDraft, ImportError> {
    continue_prepare_with_selection(import_id, revision, leaf, project_id, capability, None)
}

pub fn continue_prepare_with_selection(
    import_id: &str,
    revision: Option<&str>,
    leaf: Option<&str>,
    project_id: Option<String>,
    capability: Option<String>,
    selection: Option<AllowedSelection>,
) -> Result<HandoffDraft, ImportError> {
    let auth = continuation_scope::resolve(import_id, revision, leaf)?;
    let published = match selection.as_ref() {
        Some(selection) => continuation_scope::narrow(&auth.selection, selection)?,
        None => auth.selection,
    };
    let (rev, leaf) = (auth.revision_id, auth.leaf_id);
    if let Some(draft) = pending_continue_draft(import_id, &rev, &leaf)? {
        if selection.is_some() && published.hash() != draft.selection.hash() {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "creation already started with a different selection",
            ));
        }
        return Ok(draft);
    }
    let cap = capability.unwrap_or_else(|| "local-full".into());
    let draft = prepare_draft(
        import_id,
        &rev,
        &leaf,
        Some(published),
        project_id,
        &cap,
        None,
    )?;
    persist_continue_draft(&draft, None)?;
    Ok(draft)
}

fn pending_continue_draft(
    import_id: &str,
    revision: &str,
    leaf: &str,
) -> Result<Option<HandoffDraft>, ImportError> {
    let conn = open_catalog()?;
    let mut stmt = conn
        .prepare("SELECT json FROM drafts WHERE import_id=?1")
        .map_err(crate::session_migration::continuation_intent::error)?;
    let rows = stmt
        .query_map([import_id], |row| row.get::<_, String>(0))
        .map_err(crate::session_migration::continuation_intent::error)?;
    for row in rows {
        let raw = row.map_err(crate::session_migration::continuation_intent::error)?;
        let mut draft: HandoffDraft = serde_json::from_str(&raw)
            .map_err(crate::session_migration::continuation_intent::error)?;
        let intent =
            crate::session_migration::continuation_intent::for_draft(&conn, &draft.draft_id)?;
        if intent.is_none_or(|i| i.cancelled) {
            continue;
        }
        if draft.revision_id != revision || draft.leaf != leaf {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "retry or discard the pending continuation on its original branch first",
            ));
        }
        crate::session_migration::continuation_create::check_draft(&draft)?;
        crate::session_migration::continuation_intent::hydrate(&conn, &mut draft)?;
        return Ok(Some(draft));
    }
    Ok(None)
}

fn persist_continue_draft(draft: &HandoffDraft, previous: Option<&str>) -> Result<(), ImportError> {
    let conn = open_catalog()?;
    conn.busy_timeout(std::time::Duration::ZERO)
        .map_err(crate::session_migration::continuation_intent::error)?;
    let _lock = crate::store_lock::lock_exclusive(&crate::paths::sessions_index_file())
        .map_err(crate::session_migration::continuation_intent::error)?;
    if let Some(id) = previous {
        if load_draft_json(&conn, id)?.is_none() {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "draft was discarded or replaced",
            ));
        }
    }
    let mut stmt = conn
        .prepare("SELECT draft_id FROM drafts WHERE import_id=?1")
        .map_err(crate::session_migration::continuation_intent::error)?;
    let ids = stmt
        .query_map([&draft.import_id], |row| row.get::<_, String>(0))
        .map_err(crate::session_migration::continuation_intent::error)?;
    for id in ids {
        let id = id.map_err(crate::session_migration::continuation_intent::error)?;
        if crate::session_migration::continuation_intent::for_draft(&conn, &id)?
            .is_some_and(|i| !i.cancelled)
        {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "retry or discard the pending continuation first",
            ));
        }
    }
    let created = chrono::Utc::now();
    let expiry = created + chrono::Duration::seconds(PREVIEW_TTL_SECS as i64);
    let json = serde_json::to_string(&draft)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    crate::session_migration::catalog::write_under_store_lock(&conn, |conn| {
        persist_draft_json(
            conn,
            &draft.draft_id,
            &draft.draft_hash,
            &draft.import_id,
            &json,
            &created.to_rfc3339(),
            &expiry.to_rfc3339(),
        )
    })?;
    rt(|r| {
        r.drafts
            .retain(|_, existing| existing.import_id != draft.import_id);
        r.drafts.insert(draft.draft_id.clone(), draft.clone());
    });
    Ok(())
}

pub fn continue_create(
    draft_id: &str,
    draft_hash: &str,
    request_id: &str,
    edited_text: Option<String>,
) -> Result<String, ImportError> {
    let conn = open_catalog()?;
    let existing = crate::session_migration::continuation::load_ref_by_request(&conn, request_id)?;
    let draft = if existing.is_some() {
        None
    } else {
        match load_continue_draft(draft_id, draft_hash) {
            Ok(draft) => Some(draft),
            Err(_)
                if crate::session_migration::continuation::load_ref_by_request(
                    &conn, request_id,
                )?
                .is_some() =>
            {
                None
            }
            Err(error) => return Err(error),
        }
    };
    let side = draft
        .as_ref()
        .map(|draft| continuation_sidecar_for(draft, request_id));
    let id = crate::session_migration::continuation_create::run(
        draft.as_ref(),
        draft_id,
        draft_hash,
        request_id,
        edited_text,
        side,
    )?;
    rt(|r| {
        r.drafts.remove(draft_id);
    });
    Ok(id)
}

fn load_continue_draft(id: &str, hash: &str) -> Result<HandoffDraft, ImportError> {
    let json = load_draft_json(&open_catalog()?, id)?
        .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "continuation draft not found"))?;
    let draft: HandoffDraft = serde_json::from_str(&json)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    if draft.draft_id != id || draft.draft_hash != hash {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "continuation draft changed",
        ));
    }
    crate::session_migration::continuation_create::check_draft(&draft)?;
    Ok(draft)
}

pub fn continue_restore(id: &str, hash: &str) -> Result<HandoffDraft, ImportError> {
    let mut draft = load_continue_draft(id, hash)?;
    crate::session_migration::continuation_intent::hydrate(&open_catalog()?, &mut draft)?;
    Ok(draft)
}

pub fn continue_discard(id: &str, hash: &str) -> Result<(), ImportError> {
    let conn = open_catalog()?;
    if load_draft_json(&conn, id)?.is_none() {
        match crate::session_migration::continuation_intent::for_draft(&conn, id)? {
            None => return Ok(()),
            Some(intent) if (intent.cancelled || intent.published) && intent.draft_hash == hash => {
                return Ok(())
            }
            _ => {
                return Err(ImportError::new(
                    ErrorCode::PlanStale,
                    "continuation already exists; retry to open it",
                ))
            }
        }
    }
    let draft = load_continue_draft(id, hash)?;
    crate::session_migration::continuation_create::discard(&draft)?;
    rt(|r| {
        r.drafts.remove(id);
    });
    Ok(())
}

/// Explicitly review a native workspace; source membership never selects one.
pub fn continue_workspace(
    id: &str,
    hash: &str,
    workspace_id: Option<&str>,
) -> Result<HandoffDraft, ImportError> {
    let mut draft = load_continue_draft(id, hash)?;
    if crate::session_migration::continuation_intent::draft_claimed(&open_catalog()?, id)? {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "creation already started; retry the existing request",
        ));
    }
    if draft.creation_version.is_none() {
        draft.workspace_project_constraint = draft.project_id.clone();
    }
    draft.workspace = workspace_id
        .map(|wid| {
            crate::session_migration::continuation_target::select(
                wid,
                draft.workspace_project_constraint.as_deref(),
            )
        })
        .transpose()?;
    if let Some(workspace) = &draft.workspace {
        draft.project_id = Some(workspace.primary_project_id.clone());
    } else {
        draft.project_id = draft.workspace_project_constraint.clone();
    }
    draft.creation_version = Some(1);
    draft.draft_id = Uuid::new_v4().to_string();
    draft.draft_hash.clear();
    draft.draft_hash = crate::session_migration::hash::sha256_bytes(
        &serde_json::to_vec(&draft)
            .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?,
    );
    persist_continue_draft(&draft, Some(id))?;
    Ok(draft)
}

pub(super) fn continuation_sidecar_for(
    draft: &HandoffDraft,
    request_id: &str,
) -> ContinuationSidecar {
    ContinuationSidecar {
        import_id: draft.import_id.clone(),
        revision_id: draft.revision_id.clone(),
        branch_leaf: draft.leaf.clone(),
        allowed_selection_hash: draft.selection.hash(),
        allowed_event_ids: Vec::new(),
        allowed_selection: Some(draft.selection.clone()),
        handoff_hash: draft.draft_hash.clone(),
        request_id: request_id.into(),
        dispatch_id: None,
        prompt_hash: None,
        agent_session_id: None,
        state: DispatchState::Prepared,
        grant_token: Uuid::new_v4().to_string(),
        runtime_capability: draft.runtime_capability.clone(),
        advertised_tool_names: Vec::new(),
        probe_evidence: None,
    }
}

/// Atomically retire the old review only after its replacement is complete.
pub fn continue_selection(
    id: &str,
    hash: &str,
    selection: AllowedSelection,
) -> Result<HandoffDraft, ImportError> {
    selection.validate_shape()?;
    let identity = crate::session_migration::hash::sha256_bytes(
        format!("selection-review-v1:{id}:{hash}:{}", selection.hash()).as_bytes(),
    );
    let review_id = Uuid::parse_str(&identity[..32])
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?
        .to_string();
    if let Some(draft) = restored_selection_review(&review_id)? {
        return Ok(draft);
    }
    let previous = load_continue_draft(id, hash)?;
    if crate::session_migration::continuation_intent::draft_claimed(&open_catalog()?, id)? {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "creation already started; retry the existing request",
        ));
    }
    let requested = continuation_scope::narrow(&previous.selection, &selection)?;
    let auth = continuation_scope::resolve(
        &previous.import_id,
        Some(&previous.revision_id),
        Some(&previous.leaf),
    )?;
    let mut requested = continuation_scope::narrow(&auth.selection, &requested)?;
    if !requested.include_subagents {
        let relations = crate::session_migration::continuation::load_relations(
            &previous.import_id,
            &previous.revision_id,
        )?;
        let members = crate::session_migration::selection::subagent_member_ids(&relations);
        requested
            .event_ids
            .retain(|id| !members.contains(id.as_str()));
        requested
            .pinned_event_ids
            .retain(|id| !members.contains(id.as_str()));
    }
    let constraint = if previous.creation_version.is_some() {
        previous.workspace_project_constraint.clone()
    } else {
        previous.project_id.clone()
    };
    let mut draft = prepare_draft(
        &previous.import_id,
        &previous.revision_id,
        &previous.leaf,
        Some(requested),
        constraint.clone(),
        &previous.runtime_capability,
        None,
    )?;
    draft.workspace = previous
        .workspace
        .as_ref()
        .map(|workspace| {
            crate::session_migration::continuation_target::select(
                &workspace.id,
                constraint.as_deref(),
            )
        })
        .transpose()?;
    if let Some(workspace) = &draft.workspace {
        draft.project_id = Some(workspace.primary_project_id.clone());
    }
    draft.draft_id = review_id.clone();
    draft.draft_hash.clear();
    draft.draft_hash = crate::session_migration::hash::sha256_bytes(
        &serde_json::to_vec(&draft)
            .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?,
    );
    if let Err(error) = persist_continue_draft(&draft, Some(id)) {
        if error.code == ErrorCode::PlanStale {
            if let Some(existing) = restored_selection_review(&review_id)? {
                return Ok(existing);
            }
        }
        return Err(error);
    }
    Ok(draft)
}

fn restored_selection_review(id: &str) -> Result<Option<HandoffDraft>, ImportError> {
    let conn = open_catalog()?;
    let Some(raw) = load_draft_json(&conn, id)? else {
        return Ok(None);
    };
    let mut draft: HandoffDraft = serde_json::from_str(&raw)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    if draft.draft_id != id {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "selection review identity changed",
        ));
    }
    crate::session_migration::continuation_create::check_draft(&draft)?;
    continuation_scope::bound_selection(
        &draft.import_id,
        &draft.revision_id,
        &draft.leaf,
        Some(&draft.selection),
        &draft.selection.hash(),
    )?;
    crate::session_migration::continuation_intent::hydrate(&conn, &mut draft)?;
    Ok(Some(draft))
}
