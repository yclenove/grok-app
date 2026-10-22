use super::*;
use rusqlite::OptionalExtension;

use crate::session_migration::errors::sqlite_storage_error;

#[cfg(test)]
pub fn commit(plan_id: &str, plan_hash: &str, request_id: &str) -> Result<JobRecord, ImportError> {
    commit_with_reimport(plan_id, plan_hash, request_id, false)
}
pub fn commit_with_reimport(
    plan_id: &str,
    plan_hash: &str,
    request_id: &str,
    explicit_reimport: bool,
) -> Result<JobRecord, ImportError> {
    let operation = super::super::preview_lifecycle::operation()?;
    let conn = open_catalog()?;
    if let Some(existing) = find_job_by_request(&conn, request_id)? {
        if existing.plan_hash != plan_hash || existing.plan_id != plan_id {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "request ID belongs to a different plan",
            ));
        }
        return job_get(&existing.job_id);
    }
    crate::session_migration::job_control::preview_retention::reconcile_operation(
        &conn, &operation,
    )?;
    let plan = super::engine_prepare_plan::load_durable_plan(plan_id, plan_hash)?;
    let has_associations = !plan.identity_associations.is_empty();
    if has_associations && explicit_reimport {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "a confirmed identity association cannot also be an explicit reimport",
        ));
    }
    let job = JobRecord {
        job_id: plan.publication_protocol.new_job_id(),
        request_id: request_id.into(),
        plan_hash: plan_hash.into(),
        plan_id: plan_id.into(),
        state: JobState::Queued,
        state_version: 1,
        items: Vec::new(),
        created_at: super::super::clock::now().to_rfc3339(),
        item_count: 0,
        next_item_cursor: None,
        progress: None,
    };
    let items = commit_items_for_plan(&plan, &job, explicit_reimport)?;
    // Only a durable plan with explicit review bindings may enter association
    // acceptance. Ordinary registered enqueue continues to refuse such inputs.
    let queued = if has_associations {
        crate::session_migration::job_control::enqueue_confirmed_associations(job, items)?
    } else {
        crate::session_migration::job_control::enqueue_registered(job, items)?
    };
    job_get(&queued.job_id)
}

pub(in crate::session_migration) fn commit_items_for_plan(
    plan: &PlanState,
    job: &JobRecord,
    explicit_reimport: bool,
) -> Result<Vec<CommitItem>, ImportError> {
    plan.publication_protocol.require_job_id(&job.job_id)?;
    crate::session_migration::identity_association::validate_bindings(
        &plan.selection_set,
        &plan.identity_associations,
    )?;
    let mut items = Vec::new();
    let selections = plan
        .selection_set
        .entries
        .iter()
        .map(|selection| (selection.preview_id.as_str(), selection))
        .collect::<std::collections::HashMap<_, _>>();
    for pid in &plan.preview_ids {
        let selection = (*selections.get(pid.as_str()).ok_or_else(|| {
            ImportError::new(
                ErrorCode::PlanStale,
                "persisted plan does not cover every preview",
            )
        })?)
        .clone();
        let snapshot = plan.snapshots.get(pid).ok_or_else(|| {
            ImportError::new(
                ErrorCode::PlanStale,
                "persisted plan snapshot binding missing",
            )
        })?;
        let raw_selection = SelectionSet::single(selection);
        let mut item = commit_item_from_preview_bound(
            pid,
            &plan.mappings,
            explicit_reimport,
            raw_selection.clone(),
            Some(snapshot),
        )?;
        if let Some(confirmation) = plan
            .identity_associations
            .iter()
            .find(|c| c.preview_id == *pid)
        {
            let association = crate::session_migration::job_input_association::AssociationInput {
                confirmation: confirmation.clone(),
                job_id: job.job_id.clone(),
                request_id: job.request_id.clone(),
                plan_id: plan.plan_id.clone(),
                plan_hash: plan.plan_hash.clone(),
            };
            association.require_job(job)?;
            association.require_plan(plan, &raw_selection)?;
            item.selection_set = raw_selection;
            item.identity_association = Some(association);
        }
        items.push(item);
    }
    Ok(items)
}

#[cfg(test)]
pub(super) fn commit_item_from_preview(
    preview_id: &str,
    mappings: &[WorkspaceMapping],
    explicit_reimport: bool,
    selection_set: SelectionSet,
) -> Result<CommitItem, ImportError> {
    commit_item_from_preview_bound(preview_id, mappings, explicit_reimport, selection_set, None)
}

fn commit_item_from_preview_bound(
    preview_id: &str,
    mappings: &[WorkspaceMapping],
    explicit_reimport: bool,
    selection_set: SelectionSet,
    snapshot: Option<&crate::session_migration::plan_snapshot::PlanSnapshot>,
) -> Result<CommitItem, ImportError> {
    if let Some(preview) = rt(|r| r.previews.get(preview_id).cloned()) {
        crate::session_migration::source_member::require_preview_pin(
            &preview.prepared.path,
            preview.source_member.as_ref(),
        )?;
        return commit_item_from_parts(
            preview.candidate_id,
            preview.source_id,
            preview.legacy_id,
            preview.root_path,
            preview.adapter,
            preview.prepared,
            mappings,
            explicit_reimport,
            selection_set,
            snapshot,
        );
    }
    let path = crate::session_migration::jobs::find_preview_dir(preview_id)?
        .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "preview missing"))?;
    let (lease, hashes) =
        crate::session_migration::jobs::verify_disk_preview_integrity(&path, preview_id)?;
    if !lease.retained_by_job && lease.expires_unix <= chrono::Utc::now().timestamp() {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "preview lease expired",
        ));
    }
    let prepared = PreparedRevision::reopen(path.clone(), lease.snapshot_digest.clone(), hashes)?;
    let adapter = adapter_from_prepared(&path)?;
    commit_item_from_parts(
        lease.candidate_id,
        lease.source_id,
        lease.legacy_id,
        lease.root_path,
        adapter,
        prepared,
        mappings,
        explicit_reimport,
        selection_set,
        snapshot,
    )
}

// This is the only reconstruction boundary shared by resident and reopened
// previews; explicit provenance fields are validated together below.
#[allow(clippy::too_many_arguments)]
pub(super) fn commit_item_from_parts(
    candidate_id: String,
    source_id: String,
    legacy_id: String,
    root_path: String,
    adapter: AdapterOutput,
    prepared: std::sync::Arc<PreparedRevision>,
    mappings: &[WorkspaceMapping],
    explicit_reimport: bool,
    selection_set: SelectionSet,
    snapshot: Option<&crate::session_migration::plan_snapshot::PlanSnapshot>,
) -> Result<CommitItem, ImportError> {
    let selection = selection_set.single_entry()?;
    let verified = validate_prepared_selection(
        selection,
        &prepared.path,
        PreparedSelectionIdentity {
            preview_id: Some(&selection.preview_id),
            candidate_id: Some(&candidate_id),
            source_id: Some(&source_id),
            root_path: Some(&root_path),
            legacy_id: Some(&legacy_id),
            snapshot_digest: Some(&prepared.digest),
            provider: Some(adapter.evidence.provider.as_str()),
            native_session_id: Some(&adapter.graph.native_session_id),
        },
    )?;
    if let Some(snapshot) = snapshot {
        snapshot.require(&verified)?;
    }
    let subject = crate::session_migration::mapping_scope::MappingSubject::new(
        verified.source_id,
        verified.provider,
        verified.cwd,
    );
    let row = crate::session_migration::mapping_scope::resolve(mappings, &subject)?;
    let mapping_project_id = row.and_then(|m| m.target_project_id.clone());
    Ok(CommitItem {
        candidate_id,
        installation_id: source_id,
        provider: adapter.evidence.provider.as_str().to_string(),
        root_path,
        adapter,
        prepared: Some(prepared),
        options: crate::session_migration::mapping_scope::options(row),
        mapping_project_id,
        explicit_reimport,
        create_project: row.is_some_and(|m| m.create_project),
        import_unassigned_anyway: row.is_some_and(|m| m.import_unassigned_anyway),
        explicit_target: row.and_then(|m| m.explicit_target.clone()),
        identity_association: None,
        selection_set: selection_set.canonicalized(),
    })
}

pub(super) fn adapter_from_prepared(path: &Path) -> Result<AdapterOutput, ImportError> {
    let raw = std::fs::read(path.join("manifest.json"))
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let manifest: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let evidence: crate::session_migration::types::SourceEvidence = serde_json::from_value(
        manifest
            .get("evidence")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    )
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let graph = manifest
        .get("graph")
        .cloned()
        .unwrap_or(serde_json::json!({}));
    Ok(AdapterOutput {
        evidence,
        graph: crate::session_migration::types::NormalizedGraph {
            events: Vec::new(),
            relations: serde_json::from_value(
                graph
                    .get("relations")
                    .cloned()
                    .unwrap_or(serde_json::json!([])),
            )
            .unwrap_or_default(),
            active_leaf: graph
                .get("activeLeaf")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            native_session_id: graph
                .get("nativeSessionId")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            title: graph
                .get("title")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            cwd: graph
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        },
        inventory: serde_json::from_value(
            manifest
                .get("inventory")
                .cloned()
                .unwrap_or(serde_json::json!({})),
        )
        .unwrap_or_default(),
        issues: serde_json::from_value(
            manifest
                .get("issues")
                .cloned()
                .unwrap_or(serde_json::json!([])),
        )
        .unwrap_or_default(),
        assets: Vec::new(),
        outcome: match manifest.get("outcome").and_then(|v| v.as_str()) {
            Some("partial") => crate::session_migration::types::ImportOutcome::Partial,
            Some("unsupported") => crate::session_migration::types::ImportOutcome::Unsupported,
            Some("failed") => crate::session_migration::types::ImportOutcome::Failed,
            _ => crate::session_migration::types::ImportOutcome::Complete,
        },
    })
}

pub fn job_get(job_id: &str) -> Result<JobRecord, ImportError> {
    job_items_page(job_id, None, None)
}

pub fn job_items_page(
    job_id: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<JobRecord, ImportError> {
    if !crate::session_migration::catalog::catalog_exists()? {
        return Err(ImportError::new(ErrorCode::SourceNotFound, "job not found"));
    }
    // Progress polling must not initialize optional tables or compete with the
    // import worker for a writer reservation.
    let conn = crate::session_migration::catalog::open_catalog_readonly()?;
    let crypto = super::super::publication_crypto::JobCrypto::prepare(&conn, job_id)?;
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    let _entry = super::super::jobs::entry::authenticate_with_crypto(&tx, job_id, &crypto)?;
    // Header, aggregate counts and visible items must describe one version,
    // even when the worker commits another item while this read is in flight.
    let header: String = tx
        .query_row(
            "SELECT json_set(json_remove(json, '$.items'), '$.items', json('[]'))
             FROM jobs WHERE job_id=?1",
            rusqlite::params![job_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(sqlite_storage_error)?
        .ok_or_else(|| ImportError::new(ErrorCode::SourceNotFound, "job not found"))?;
    let (total, done, retryable): (u32, u32, u32) = tx
        .query_row(
            "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN json_extract(value,'$.phase') IN ('committed','failed','cancelled')
                    OR COALESCE(json_extract(value,'$.importId'),'') != '' THEN 1 ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN json_extract(value,'$.phase') IN ('failed','cancelled','recovery_needed')
                    THEN 1 ELSE 0 END),0)
             FROM json_each((SELECT json FROM jobs WHERE job_id=?1), '$.items')",
            rusqlite::params![job_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(sqlite_storage_error)?;
    let mut job: JobRecord = serde_json::from_str(&header)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let cap = crate::session_migration::budget::page_limit(limit);
    let job_scope = CursorScope {
        owner: job_id,
        snapshot: job.plan_hash.as_str(),
        leaf: "",
        filter: "job-items",
    };
    let start = match cursor.filter(|s| !s.is_empty()) {
        None => 0usize,
        Some(raw) => decode_bound_cursor(raw, job_scope)?.offset as usize,
    };
    if start > total as usize || (total > 0 && start == total as usize) {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "job item cursor past end",
        ));
    }
    let mut stmt = tx
        .prepare(
            "SELECT value FROM json_each((SELECT json FROM jobs WHERE job_id=?1), '$.items')
             WHERE CAST(key AS INTEGER) >= ?2
             ORDER BY CAST(key AS INTEGER)
             LIMIT ?3",
        )
        .map_err(sqlite_storage_error)?;
    let rows = stmt
        .query_map(
            rusqlite::params![job_id, start as i64, (cap + 1) as i64],
            |r| r.get::<_, String>(0),
        )
        .map_err(sqlite_storage_error)?;
    let mut page: Vec<crate::session_migration::jobs::JobItem> = Vec::new();
    for row in rows {
        page.push(
            serde_json::from_str(&row.map_err(sqlite_storage_error)?)
                .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?,
        );
    }
    job.item_count = total;
    job.progress = Some(crate::session_migration::jobs::JobProgress { done, retryable });
    job.next_item_cursor = if page.len() > cap {
        page.pop();
        Some(encode_bound_cursor(
            job_scope,
            (start + page.len()) as u32,
            None,
        )?)
    } else {
        None
    };

    // Older durable jobs predate the optional title projection in JobItem. Read
    // only the bounded page's durable input metadata so reopening such a job
    // does not fall back to opaque `imp:` IDs or scan the whole batch.
    super::super::job_title_projection::hydrate(&tx, job_id, &mut page)?;
    job.items = page;
    Ok(job)
}

#[cfg(test)]
pub fn undo_import_job(job_id: &str) -> Result<JobRecord, ImportError> {
    let conn = open_catalog()?;
    undo_job(&conn, job_id)?;
    job_get(job_id)
}

pub fn delete_imported(import_id: &str) -> Result<(), ImportError> {
    let conn = open_catalog()?;
    crate::session_migration::jobs::delete_imported(&conn, import_id)
}

pub fn propose_mappings(
    preview_ids: &[String],
    fallback_project: Option<String>,
) -> Result<Vec<WorkspaceMapping>, ImportError> {
    let _operation = crate::session_migration::preview_lifecycle::operation()?;
    let mut subjects = Vec::new();
    for id in preview_ids {
        let (source_id, adapter) = if let Some(preview) = rt(|r| r.previews.get(id).cloned()) {
            (preview.source_id, preview.adapter)
        } else {
            let path = crate::session_migration::preview_store::disk_snapshot_path(id)?;
            let (lease, _) =
                crate::session_migration::jobs::verify_disk_preview_integrity(&path, id)?;
            (lease.source_id, adapter_from_prepared(&path)?)
        };
        subjects.push(
            crate::session_migration::mapping_scope::MappingSubject::new(
                source_id,
                adapter.evidence.provider.as_str().to_string(),
                adapter.graph.cwd,
            ),
        );
    }
    Ok(crate::session_migration::mapping_scope::propose(
        &subjects,
        fallback_project,
    ))
}

#[cfg(test)]
pub fn cancel_job(job_id: &str) -> Result<JobRecord, ImportError> {
    crate::session_migration::job_control::cancel(job_id)?;
    job_get(job_id)
}

#[cfg(test)]
pub fn list_imported() -> Result<Vec<CatalogEntry>, ImportError> {
    let conn = open_catalog()?;
    list_entries(&conn)
}

#[cfg(test)]
pub fn list_imported_page(
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<(Vec<CatalogEntry>, Option<String>, u32), ImportError> {
    list_imported_page_filtered(cursor, limit, None, None, None)
}

#[cfg(test)]
pub fn list_imported_page_filtered(
    cursor: Option<&str>,
    limit: Option<usize>,
    provider: Option<&str>,
    query: Option<&str>,
    sort: Option<&str>,
) -> Result<(Vec<CatalogEntry>, Option<String>, u32), ImportError> {
    list_imported_page_in_workspace(cursor, limit, provider, query, sort, None)
}

pub fn list_imported_page_in_workspace(
    cursor: Option<&str>,
    limit: Option<usize>,
    provider: Option<&str>,
    query: Option<&str>,
    sort: Option<&str>,
    workspace: Option<&crate::session_migration::workspace_catalog::WorkspaceFilter>,
) -> Result<(Vec<CatalogEntry>, Option<String>, u32), ImportError> {
    if !crate::session_migration::catalog::catalog_exists()? {
        return Ok((vec![], None, 0));
    }
    let conn = crate::session_migration::catalog::open_catalog_readonly()?;
    list_entries_page_filtered(
        &conn,
        cursor,
        limit,
        EntryPageFilter {
            provider,
            query,
            sort: CatalogSort::parse(sort),
            workspace,
        },
    )
}

pub fn list_imported_providers() -> Result<Vec<String>, ImportError> {
    if !crate::session_migration::catalog::catalog_exists()? {
        return Ok(vec![]);
    }
    let conn = crate::session_migration::catalog::open_catalog_readonly()?;
    list_entry_providers(&conn)
}

#[cfg(test)]
pub fn list_import_revisions(
    import_id: &str,
) -> Result<Vec<crate::session_migration::catalog::RevisionRecord>, ImportError> {
    if !is_import_id(import_id) {
        return Err(ImportError::new(
            ErrorCode::ImportedHistoryReadOnly,
            "not an import id",
        ));
    }
    let conn = open_catalog()?;
    crate::session_migration::catalog::list_revisions(&conn, import_id)
}

#[cfg(test)]
pub fn events_page(
    import_id: &str,
    revision: Option<&str>,
    cursor: Option<&str>,
    limit: Option<usize>,
    leaf: Option<&str>,
) -> Result<(Vec<serde_json::Value>, Option<String>, u32), ImportError> {
    let page = events_window(import_id, revision, cursor, limit, leaf, None)?;
    Ok((page.events, page.next_cursor, page.total))
}

pub fn events_window(
    import_id: &str,
    revision: Option<&str>,
    cursor: Option<&str>,
    limit: Option<usize>,
    leaf: Option<&str>,
    direction: Option<&str>,
) -> Result<crate::session_migration::reader::EventPageResponse, ImportError> {
    if !is_import_id(import_id) {
        return Err(ImportError::new(
            ErrorCode::ImportedHistoryReadOnly,
            "not an import id",
        ));
    }
    let conn = open_catalog()?;
    let entry = get_entry(&conn, import_id)?
        .ok_or_else(|| ImportError::new(ErrorCode::SourceNotFound, "import not found"))?;
    if entry.hidden {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "imported history is hidden",
        ));
    }
    let rev = revision
        .map(|s| s.to_string())
        .or(entry.current_revision)
        .ok_or_else(|| ImportError::new(ErrorCode::SnapshotUnavailable, "no revision"))?;
    let dir = crate::session_migration::catalog::revision_dir(import_id, &rev)?;
    let auth = load_published_auth(import_id, Some(&rev), None)?;
    if auth.revision_id != rev {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "reader revision does not match the published receipt",
        ));
    }
    crate::session_migration::reader::event_page(
        &dir,
        import_id,
        &rev,
        Some(
            leaf.filter(|id| !id.trim().is_empty())
                .unwrap_or(&auth.leaf_id),
        ),
        cursor,
        limit,
        crate::session_migration::reader::PageDirection::parse(direction)?,
    )
}

pub fn event_context(
    import_id: &str,
    revision: Option<&str>,
    leaf: Option<&str>,
    request: &crate::session_migration::reader::EventContextRequest,
) -> Result<crate::session_migration::reader::EventContextResponse, ImportError> {
    if !is_import_id(import_id) {
        return Err(ImportError::new(
            ErrorCode::ImportedHistoryReadOnly,
            "not an import id",
        ));
    }
    let conn = open_catalog()?;
    let entry = get_entry(&conn, import_id)?
        .ok_or_else(|| ImportError::new(ErrorCode::SourceNotFound, "import not found"))?;
    if entry.hidden {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "imported history is hidden",
        ));
    }
    let rev = revision
        .map(str::to_owned)
        .or(entry.current_revision)
        .ok_or_else(|| ImportError::new(ErrorCode::SnapshotUnavailable, "no revision"))?;
    let dir = crate::session_migration::catalog::revision_dir(import_id, &rev)?;
    let auth = load_published_auth(import_id, Some(&rev), None)?;
    if auth.revision_id != rev {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "reader revision does not match the published receipt",
        ));
    }
    crate::session_migration::reader::event_context(
        &dir,
        import_id,
        &rev,
        Some(
            leaf.filter(|id| !id.trim().is_empty())
                .unwrap_or(&auth.leaf_id),
        ),
        request,
    )
}

pub(super) fn resolve_published_selection(
    import_id: &str,
    revision: Option<&str>,
    leaf: Option<&str>,
    caller: Option<&AllowedSelection>,
) -> Result<(String, String, AllowedSelection), ImportError> {
    let auth = crate::session_migration::continuation_scope::resolve(import_id, revision, leaf)?;
    let selection = match caller {
        Some(caller) => {
            crate::session_migration::continuation_scope::narrow(&auth.selection, caller)?
        }
        None => auth.selection,
    };
    Ok((auth.revision_id, auth.leaf_id, selection))
}

#[cfg(test)]
pub fn search_imported_with_leaf(
    import_id: &str,
    revision: Option<&str>,
    leaf: Option<&str>,
    query: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<(Vec<SearchHit>, Option<String>), ImportError> {
    let conn = open_catalog()?;
    let entry = get_entry(&conn, import_id)?
        .ok_or_else(|| ImportError::new(ErrorCode::SourceNotFound, "import not found"))?;
    if entry.hidden {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "imported history is hidden",
        ));
    }
    let rev = revision
        .map(str::to_owned)
        .or(entry.current_revision)
        .ok_or_else(|| ImportError::new(ErrorCode::SnapshotUnavailable, "no revision"))?;
    let dir = crate::session_migration::catalog::revision_dir(import_id, &rev)?;
    let auth = load_published_auth(import_id, Some(&rev), None)?;
    if auth.revision_id != rev {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "search revision does not match the published receipt",
        ));
    }
    let (resolved_leaf, chain) = crate::session_migration::reader::resolve_branch(
        &dir,
        Some(
            leaf.filter(|id| !id.trim().is_empty())
                .unwrap_or(&auth.leaf_id),
        ),
    )?;
    search_branch_ids(&dir, query, cursor, limit, &resolved_leaf, chain.as_slice())
}

pub fn search_imported_with_leaf_window(
    import_id: &str,
    revision: Option<&str>,
    leaf: Option<&str>,
    query: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
    direction: Option<&str>,
) -> Result<crate::session_migration::search::SearchWindow, ImportError> {
    let conn = open_catalog()?;
    let entry = get_entry(&conn, import_id)?
        .ok_or_else(|| ImportError::new(ErrorCode::SourceNotFound, "import not found"))?;
    if entry.hidden {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "imported history is hidden",
        ));
    }
    let rev = revision
        .map(str::to_owned)
        .or(entry.current_revision)
        .ok_or_else(|| ImportError::new(ErrorCode::SnapshotUnavailable, "no revision"))?;
    let dir = crate::session_migration::catalog::revision_dir(import_id, &rev)?;
    let auth = load_published_auth(import_id, Some(&rev), None)?;
    if auth.revision_id != rev {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "search revision does not match the published receipt",
        ));
    }
    let (resolved_leaf, chain) = crate::session_migration::reader::resolve_branch(
        &dir,
        Some(
            leaf.filter(|id| !id.trim().is_empty())
                .unwrap_or(&auth.leaf_id),
        ),
    )?;
    crate::session_migration::search::search_branch_window(
        &dir,
        query,
        cursor,
        limit,
        &resolved_leaf,
        chain.as_slice(),
        crate::session_migration::search::SearchDirection::parse(direction)?,
    )
}

pub fn export_markdown(
    import_id: &str,
    revision: Option<&str>,
    leaf: Option<&str>,
) -> Result<String, ImportError> {
    let (rev, leaf, selection) = resolve_published_selection(import_id, revision, leaf, None)?;
    let dir = crate::session_migration::catalog::revision_dir(import_id, &rev)?;
    let relations = crate::session_migration::continuation::load_relations(import_id, &rev)?;
    let events = crate::session_migration::search::indexed_events_in_source_order(
        &dir,
        &selection.event_ids,
    )?;
    selection.validate_for_leaf(&relations, &events, &leaf)?;
    let mut md = format!(
        "# Imported history\n\n- import: `{import_id}`\n- revision: `{rev}`\n- leaf: `{leaf}`\n- platform: {}\n\n",
        current_platform()
    );
    for event in &events {
        if let Some(text) = selection.project_event(event, &relations) {
            md.push_str(&text);
            md.push_str("\n\n");
        }
    }
    Ok(md)
}

#[cfg(test)]
pub fn export_for_model(
    import_id: &str,
    revision: Option<&str>,
    leaf: Option<&str>,
    selection: &AllowedSelection,
) -> Result<String, ImportError> {
    let (rev, leaf, published) =
        resolve_published_selection(import_id, revision, leaf, Some(selection))?;
    let dir = crate::session_migration::catalog::revision_dir(import_id, &rev)?;
    let relations = crate::session_migration::continuation::load_relations(import_id, &rev)?;
    let events = crate::session_migration::search::indexed_events_in_source_order(
        &dir,
        &published.event_ids,
    )?;
    published.validate_for_leaf(&relations, &events, &leaf)?;
    let mut md = String::new();
    for event in &events {
        if let Some(text) = published.project_event(event, &relations) {
            md.push_str(&text);
            md.push_str("\n\n");
        }
    }
    Ok(md)
}

pub fn grant_for_session(
    app_session_id: &str,
) -> Option<crate::session_migration::mcp_stdio::ImportHistoryGrant> {
    grant_with_selection(app_session_id)
}

pub fn grant_with_selection(
    app_session_id: &str,
) -> Option<crate::session_migration::mcp_stdio::ImportHistoryGrant> {
    let side = crate::session_migration::continuation::read_sidecar_strict(app_session_id)
        .ok()
        .flatten()?;
    let selection = crate::session_migration::continuation_scope::bound_selection(
        &side.import_id,
        &side.revision_id,
        &side.branch_leaf,
        side.allowed_selection.as_ref(),
        &side.allowed_selection_hash,
    )
    .ok()?;
    Some(crate::session_migration::mcp_stdio::ImportHistoryGrant {
        token: side.grant_token,
        app_session_id: app_session_id.into(),
        import_id: side.import_id,
        revision_id: side.revision_id,
        branch_leaf: side.branch_leaf,
        allowed_event_ids: selection.event_ids.clone(),
        allowed_selection_hash: side.allowed_selection_hash,
        selection: Some(selection),
    })
}
