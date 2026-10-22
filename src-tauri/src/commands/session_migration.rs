// Tauri commands for session/workspace import.

use crate::session_migration::detect::DiscoveredRoot;
use crate::session_migration::engine;
use crate::session_migration::errors::ImportError;
use crate::session_migration::ids::reject_imported_history_id;
use crate::session_migration::job_actions::{self, JobActionRequest};
use crate::session_migration::jobs::JobRecord;
use crate::session_migration::selection::SelectionSet;
use crate::session_migration::types::Provider;
use crate::session_migration::workspace::{SourceWorkspace, WorkspaceMapping};

fn map_err(e: ImportError) -> String {
    e.into_command_err()
}

#[tauri::command]
pub fn session_import_list_roots() -> Result<Vec<DiscoveredRoot>, String> {
    Ok(engine::list_default_roots())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStartReq {
    pub sources: Option<Vec<engine::ScanSourcePath>>,
}

#[tauri::command]
pub async fn session_import_scan_start(req: ScanStartReq) -> Result<engine::ScanState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::scan::start_background(req.sources.unwrap_or_default())
            .map_err(map_err)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn session_import_scan_page(
    scan_id: String,
    epoch: u64,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let (candidates, next, discovered, complete) =
        engine::scan_page(&scan_id, epoch, cursor.as_deref(), limit).map_err(map_err)?;
    Ok(serde_json::json!({
        "candidates": candidates,
        "nextCursor": next,
        "discoveredCount": discovered,
        "complete": complete
    }))
}

#[tauri::command]
pub async fn session_import_preview(
    candidate_id: String,
    expected_source_revision: Option<String>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (preview_id, adapter) =
            engine::preview(&candidate_id, expected_source_revision.as_deref()).map_err(map_err)?;
        Ok(serde_json::json!({"previewId":preview_id,"adapter":adapter}))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn session_import_preview_events(
    preview_id: String,
    cursor: Option<String>,
    limit: Option<usize>,
    leaf: Option<String>,
) -> Result<serde_json::Value, String> {
    let (events, next) =
        engine::preview_events_page(&preview_id, cursor.as_deref(), limit, leaf.as_deref())
            .map_err(map_err)?;
    Ok(serde_json::json!({
        "events": events,
        "nextCursor": next
    }))
}

#[tauri::command]
pub async fn session_import_prepare_start(
    scan_id: String,
    epoch: u64,
    selection_id: String,
    overrides: Vec<crate::session_migration::prepare::PrepareOverride>,
) -> Result<crate::session_migration::prepare::PrepareBatchStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::prepare::start(&scan_id, epoch, &selection_id, overrides)
            .map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

// Prepare polling/control also opens SQLite and can wait on its file locks.
// Keep that work off both the native IPC thread and async executor threads.
async fn run_prepare_command<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ImportError> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || work().map_err(map_err))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_prepare_get(
    batch_id: String,
) -> Result<crate::session_migration::prepare::PrepareBatchStatus, String> {
    run_prepare_command(move || crate::session_migration::prepare::status(&batch_id)).await
}

#[tauri::command]
pub async fn session_import_prepare_items(
    batch_id: String,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<crate::session_migration::prepare::PrepareBatchItemPage, String> {
    run_prepare_command(move || {
        crate::session_migration::prepare::items_page(&batch_id, cursor.as_deref(), limit)
    })
    .await
}

#[tauri::command]
pub async fn session_import_prepare_cancel(
    batch_id: String,
) -> Result<crate::session_migration::prepare::PrepareBatchStatus, String> {
    run_prepare_command(move || crate::session_migration::prepare::cancel(&batch_id)).await
}

#[tauri::command]
pub async fn session_import_prepare_retry(
    batch_id: String,
) -> Result<crate::session_migration::prepare::PrepareBatchStatus, String> {
    run_prepare_command(move || crate::session_migration::prepare::retry(&batch_id)).await
}

#[tauri::command]
pub async fn session_import_prepare_plan(
    batch_id: String,
    mappings: Vec<WorkspaceMapping>,
) -> Result<engine::DurablePlanRef, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let prepared =
            crate::session_migration::prepare::validated_plan_inputs(&batch_id).map_err(map_err)?;
        let plan = engine::plan_from_validated_prepare(prepared, mappings).map_err(map_err)?;
        Ok(engine::DurablePlanRef::from(&plan))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_prepare_mappings(
    batch_id: String,
    project_id: Option<String>,
) -> Result<Vec<WorkspaceMapping>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::prepare::mappings(&batch_id, project_id).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn session_import_plan(
    preview_ids: Vec<String>,
    mappings: Vec<WorkspaceMapping>,
    selection_set: SelectionSet,
) -> Result<engine::DurablePlanRef, String> {
    let plan =
        engine::plan_with_selection_set(preview_ids, mappings, selection_set).map_err(map_err)?;
    Ok(engine::DurablePlanRef::from(&plan))
}

#[tauri::command]
pub async fn session_import_commit(
    plan_id: String,
    plan_hash: String,
    request_id: String,
    explicit_reimport: Option<bool>,
) -> Result<JobRecord, String> {
    let explicit = explicit_reimport.unwrap_or(false);
    let job = tauri::async_runtime::spawn_blocking(move || {
        engine::commit_with_reimport(&plan_id, &plan_hash, &request_id, explicit).map_err(map_err)
    })
    .await
    .map_err(|e| e.to_string())??;
    crate::session_migration::job_control::wake();
    Ok(job)
}

#[tauri::command]
pub async fn session_import_job_history(
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<crate::session_migration::job_history::JobHistoryPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::job_history::page(cursor.as_deref(), limit).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_job_get(job_id: String) -> Result<JobRecord, String> {
    tauri::async_runtime::spawn_blocking(move || engine::job_get(&job_id).map_err(map_err))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_job_items(
    job_id: String,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<JobRecord, String> {
    tauri::async_runtime::spawn_blocking(move || {
        engine::job_items_page(&job_id, cursor.as_deref(), limit).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_cancel(
    job_id: String,
    expected_state_version: u32,
    request_id: String,
) -> Result<JobRecord, String> {
    tauri::async_runtime::spawn_blocking(move || {
        job_actions::cancel(&job_id, JobActionRequest { expected_state_version, request_id })
            .map_err(map_err)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_import_entry(
    import_id: String,
) -> Result<Option<crate::session_migration::catalog::CatalogEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        use crate::session_migration::catalog;
        if !catalog::catalog_exists().map_err(map_err)? {
            return Ok(None);
        }
        let conn = catalog::open_catalog_readonly().map_err(map_err)?;
        catalog::get_entry(&conn, &import_id).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_revisions(
    import_id: String,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<crate::session_migration::revision_history::RevisionPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::revision_history::page(&import_id, cursor.as_deref(), limit)
            .map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_entries(
    cursor: Option<String>,
    limit: Option<usize>,
    provider: Option<String>,
    query: Option<String>,
    sort: Option<String>,
    workspace: Option<crate::session_migration::workspace_catalog::WorkspaceFilter>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (entries, next, total) = engine::list_imported_page_in_workspace(
            cursor.as_deref(),
            limit,
            provider.as_deref(),
            query.as_deref(),
            sort.as_deref(),
            workspace.as_ref(),
        )
        .map_err(map_err)?;
        let providers = engine::list_imported_providers().map_err(map_err)?;
        Ok(serde_json::json!({
            "items": entries,
            "entries": entries.clone(),
            "nextCursor": next,
            "total": total,
            "providers": providers
        }))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn session_import_recovery() -> Result<engine::RecoverySnapshot, String> {
    engine::recovery().map_err(map_err)
}

#[tauri::command]
pub async fn session_import_catalog_rebuild(
) -> Result<crate::session_migration::catalog::CatalogRebuildReport, String> {
    tauri::async_runtime::spawn_blocking(|| {
        crate::session_migration::catalog::run_catalog_rebuild().map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn session_import_catalog_rebuild_status(
) -> crate::session_migration::catalog::CatalogRebuildStatus {
    crate::session_migration::catalog::catalog_rebuild_status()
}

#[tauri::command]
pub fn session_import_catalog_rebuild_cancel() -> bool {
    crate::session_migration::catalog::cancel_catalog_rebuild()
}

#[tauri::command]
pub async fn session_import_image_resolve(
    scope: crate::session_migration::image_reader::ImageScope,
    asset_id: String,
) -> Result<crate::session_migration::image_reader::ImageResolution, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::image_reader::resolve(&scope, &asset_id)
    })
    .await
    .map_err(|_| "imported image unavailable".to_string())
}

#[tauri::command]
pub fn session_import_events(
    import_id: String,
    revision: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
    leaf: Option<String>,
    direction: Option<String>,
) -> Result<serde_json::Value, String> {
    let page = engine::events_window(
        &import_id,
        revision.as_deref(),
        cursor.as_deref(),
        limit,
        leaf.as_deref(),
        direction.as_deref(),
    )
    .map_err(map_err)?;
    serde_json::to_value(page).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn session_import_event_context(
    import_id: String,
    revision: Option<String>,
    leaf: Option<String>,
    event_id: String,
    before: Option<usize>,
    after: Option<usize>,
    cursor: Option<String>,
) -> Result<crate::session_migration::reader::EventContextResponse, String> {
    let request = crate::session_migration::reader::EventContextRequest {
        event_id,
        before,
        after,
        cursor,
    };
    engine::event_context(&import_id, revision.as_deref(), leaf.as_deref(), &request)
        .map_err(map_err)
}

#[tauri::command]
pub fn session_import_search(
    import_id: String,
    revision: Option<String>,
    leaf: Option<String>,
    query: String,
    cursor: Option<String>,
    limit: Option<usize>,
    direction: Option<String>,
) -> Result<crate::session_migration::search::SearchWindow, String> {
    engine::search_imported_with_leaf_window(
        &import_id,
        revision.as_deref(),
        leaf.as_deref(),
        &query,
        cursor.as_deref(),
        limit,
        direction.as_deref(),
    )
    .map_err(map_err)
}

#[tauri::command]
pub fn session_import_export_markdown(
    import_id: String,
    revision: Option<String>,
    leaf: Option<String>,
) -> Result<String, String> {
    engine::export_markdown(&import_id, revision.as_deref(), leaf.as_deref()).map_err(map_err)
}

#[tauri::command]
pub async fn session_import_continue_prepare(
    import_id: String,
    revision: Option<String>,
    leaf: Option<String>,
    project_id: Option<String>,
    capability: Option<String>,
    selection: Option<crate::session_migration::continuation::AllowedSelection>,
) -> Result<crate::session_migration::continuation::HandoffDraft, String> {
    tauri::async_runtime::spawn_blocking(move || {
        engine::continue_prepare_with_selection(
            &import_id,
            revision.as_deref(),
            leaf.as_deref(),
            project_id,
            capability,
            selection,
        )
        .map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_continue_selection(
    draft_id: String,
    draft_hash: String,
    selection: crate::session_migration::continuation::AllowedSelection,
) -> Result<crate::session_migration::continuation::HandoffDraft, String> {
    tauri::async_runtime::spawn_blocking(move || {
        engine::continue_selection(&draft_id, &draft_hash, selection).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_continue_workspace(
    draft_id: String,
    draft_hash: String,
    workspace_id: Option<String>,
) -> Result<crate::session_migration::continuation::HandoffDraft, String> {
    tauri::async_runtime::spawn_blocking(move || {
        engine::continue_workspace(&draft_id, &draft_hash, workspace_id.as_deref()).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_continue_restore(
    draft_id: String,
    draft_hash: String,
) -> Result<crate::session_migration::continuation::HandoffDraft, String> {
    tauri::async_runtime::spawn_blocking(move || {
        engine::continue_restore(&draft_id, &draft_hash).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_continue_discard(
    draft_id: String,
    draft_hash: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        engine::continue_discard(&draft_id, &draft_hash).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn session_import_scan_select_current(
    scan_id: String,
    epoch: u64,
) -> Result<Vec<String>, String> {
    engine::scan_select_current(&scan_id, epoch).map_err(map_err)
}

#[tauri::command]
pub fn session_import_scan_selection_get(
    scan_id: String,
    epoch: u64,
) -> Result<crate::session_migration::scan_selection::CandidateSelection, String> {
    crate::session_migration::scan_selection::get(&scan_id, epoch).map_err(map_err)
}

#[tauri::command]
pub fn session_import_scan_selection_freeze(
    scan_id: String,
    epoch: u64,
) -> Result<crate::session_migration::scan_selection::CandidateSelection, String> {
    crate::session_migration::scan_selection::freeze_current(&scan_id, epoch).map_err(map_err)
}

#[tauri::command]
pub fn session_import_scan_selection_clear(
    scan_id: String,
    epoch: u64,
    selection_id: String,
) -> Result<crate::session_migration::scan_selection::CandidateSelection, String> {
    crate::session_migration::scan_selection::clear(&scan_id, epoch, &selection_id).map_err(map_err)
}

#[tauri::command]
pub fn session_import_scan_selection_set(
    scan_id: String,
    epoch: u64,
    selection_id: String,
    candidate_ids: Vec<String>,
    selected: bool,
) -> Result<crate::session_migration::scan_selection::CandidateSelection, String> {
    crate::session_migration::scan_selection::set_many(
        &scan_id,
        epoch,
        &selection_id,
        &candidate_ids,
        selected,
    )
    .map_err(map_err)
}

#[tauri::command]
pub fn session_import_scan_selection_invert(
    scan_id: String,
    epoch: u64,
    selection_id: String,
    candidate_ids: Vec<String>,
) -> Result<crate::session_migration::scan_selection::CandidateSelection, String> {
    crate::session_migration::scan_selection::invert_many(
        &scan_id,
        epoch,
        &selection_id,
        &candidate_ids,
    )
    .map_err(map_err)
}

#[tauri::command]
pub fn session_import_scan_selection_page(
    scan_id: String,
    epoch: u64,
    selection_id: String,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<crate::session_migration::scan_selection::CandidateSelectionPage, String> {
    crate::session_migration::scan_selection::page(
        &scan_id,
        epoch,
        &selection_id,
        cursor.as_deref(),
        limit,
    )
    .map_err(map_err)
}

#[tauri::command]
pub fn session_import_scan_selection_membership(
    scan_id: String,
    epoch: u64,
    selection_id: String,
    candidate_ids: Vec<String>,
) -> Result<crate::session_migration::scan_selection::CandidateSelectionMembership, String> {
    crate::session_migration::scan_selection::selected_ids_for_candidates(
        &scan_id,
        epoch,
        &selection_id,
        &candidate_ids,
    )
    .map_err(map_err)
}

#[tauri::command]
pub async fn session_import_scan_refresh(
    scan_id: String,
    epoch: u64,
) -> Result<engine::ScanState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::scan::refresh_background(&scan_id, epoch).map_err(map_err)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn session_import_scan_get(scan_id: String, epoch: u64) -> Result<engine::ScanState, String> {
    crate::session_migration::scan::state(&scan_id, epoch).map_err(map_err)
}

#[tauri::command]
pub fn session_import_scan_cancel(
    scan_id: String,
    epoch: u64,
) -> Result<engine::ScanState, String> {
    crate::session_migration::scan::cancel(&scan_id, epoch).map_err(map_err)
}

#[tauri::command]
pub fn session_import_scan_continue(
    scan_id: String,
    epoch: u64,
) -> Result<engine::ScanState, String> {
    crate::session_migration::scan::continue_background(&scan_id, epoch).map_err(map_err)
}

#[tauri::command]
pub fn session_import_sources(
) -> Result<Vec<crate::session_migration::sources::SourceRegistration>, String> {
    let conn = crate::session_migration::catalog::open_catalog().map_err(map_err)?;
    crate::session_migration::sources::list(&conn).map_err(map_err)
}

#[tauri::command]
pub async fn session_import_source_register(
    source: engine::ScanSourcePath,
) -> Result<crate::session_migration::sources::SourceRegistration, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::source_locations::validate_relocation_request(&source)
            .map_err(map_err)?;
        let provider = Provider::parse(&source.provider)
            .ok_or_else(|| "FORMAT_UNSUPPORTED: unknown provider".to_string())?;
        let conn = crate::session_migration::catalog::open_catalog().map_err(map_err)?;
        crate::session_migration::sources::register(
            &conn,
            provider,
            std::path::Path::new(&source.path),
            source.source_id.as_deref(),
            &source.profile,
        )
        .map_err(map_err)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_import_source_locations(
    source_id: String,
    provider: String,
    host: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<crate::session_migration::source_locations::SourceLocationPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let provider = Provider::parse(&provider)
            .ok_or_else(|| "FORMAT_UNSUPPORTED: unknown provider".to_string())?;
        let conn = crate::session_migration::catalog::open_catalog_readonly().map_err(map_err)?;
        crate::session_migration::source_locations::page(
            &conn, &source_id, provider, host.as_deref().unwrap_or("local"),
            cursor.as_deref(), limit,
        ).map_err(map_err)
    }).await.map_err(|error| error.to_string())?
}

/// Associate a distinct Cursor directory without relocating the primary.
#[tauri::command]
pub async fn session_import_source_associate(
    request: crate::session_migration::source_locations::SourceAssociationRequest,
) -> Result<crate::session_migration::source_locations::SourceLocation, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = crate::session_migration::catalog::open_catalog().map_err(map_err)?;
        crate::session_migration::source_locations::associate(&conn, &request).map_err(map_err)
    }).await.map_err(|error| error.to_string())?
}

/// Preview source roots without registering or scanning conversations.
#[tauri::command]
pub async fn session_import_workspace_preview(
    source: crate::session_migration::workspace_import::WorkspaceSource,
) -> Result<crate::session_migration::workspace_import::WorkspacePreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::workspace_import::preview(source).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_workspace_save(
    request: crate::session_migration::workspace_import::WorkspaceSaveRequest,
) -> Result<SourceWorkspace, String> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::workspace_import::save(request).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())??;
    crate::session_migration::job_control::workspace_changed();
    Ok(result)
}

#[tauri::command]
pub async fn session_import_workspaces(
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<crate::session_migration::workspace_import::WorkspacePage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::workspace_import::list(cursor.as_deref(), limit.unwrap_or(20))
            .map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_workspace_get(
    source_workspace_id: String,
) -> Result<crate::session_migration::workspace_edit::SavedWorkspace, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::workspace_edit::get(&source_workspace_id).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_workspace_apply(
    request: crate::session_migration::workspace_apply::WorkspaceApplyRequest,
) -> Result<crate::workspace_store::WorkspaceRecord, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::workspace_apply::apply(request).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Independent project action. Workspace mappings are still saved separately.
#[tauri::command]
pub async fn session_import_project_create(
    request_id: String,
    target_path: String,
) -> Result<crate::store::ImportProjectResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let request = uuid::Uuid::parse_str(&request_id)
            .map_err(|_| "MAPPING_CONFLICT: invalid project request identity".to_string())?;
        crate::store::ensure_import_project(&request.to_string(), &target_path)
            .map_err(|error| format!("MAPPING_CONFLICT: {error}"))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_workspace_update(
    request: crate::session_migration::workspace_edit::WorkspaceUpdateRequest,
) -> Result<crate::session_migration::workspace_edit::SavedWorkspace, String> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::workspace_edit::update(request).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())??;
    crate::session_migration::job_control::workspace_changed();
    Ok(result)
}

#[tauri::command]
pub async fn session_import_workspace_relocation_preview(
    request: crate::session_migration::workspace_relocation::PreviewRequest,
) -> Result<crate::session_migration::workspace_relocation::RelocationPreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::workspace_relocation::preview(request).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_workspace_relocation_save(
    request: crate::session_migration::workspace_relocation::SaveRequest,
) -> Result<crate::session_migration::workspace_edit::SavedWorkspace, String> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::workspace_relocation::save(request).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())??;
    crate::session_migration::job_control::workspace_changed();
    Ok(result)
}

#[tauri::command]
pub fn session_import_scan_pump(scan_id: String, epoch: u64) -> Result<engine::ScanState, String> {
    engine::scan_pump(&scan_id, epoch).map_err(map_err)
}

#[tauri::command]
pub async fn session_import_retry_failed(
    job_id: String,
    expected_state_version: u32,
    request_id: String,
) -> Result<JobRecord, String> {
    let job = tauri::async_runtime::spawn_blocking(move || {
        job_actions::retry(&job_id, JobActionRequest { expected_state_version, request_id })
            .map_err(map_err)
    })
    .await
    .map_err(|e| e.to_string())??;
    crate::session_migration::job_control::wake();
    Ok(job)
}

#[tauri::command]
pub async fn session_import_undo(
    job_id: String,
    expected_state_version: u32,
    request_id: String,
) -> Result<JobRecord, String> {
    tauri::async_runtime::spawn_blocking(move || {
        job_actions::undo(&job_id, JobActionRequest { expected_state_version, request_id })
            .map_err(map_err)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn session_import_delete(import_id: String) -> Result<(), String> {
    engine::delete_imported(&import_id).map_err(map_err)?;
    crate::session_migration::job_control::workspace_changed();
    Ok(())
}

#[tauri::command]
pub fn session_import_propose_mappings(
    preview_ids: Vec<String>,
    fallback_project: Option<String>,
) -> Result<Vec<WorkspaceMapping>, String> {
    engine::propose_mappings(&preview_ids, fallback_project).map_err(map_err)
}

#[tauri::command]
pub async fn session_import_continue_create(
    app: tauri::AppHandle,
    draft_id: String,
    draft_hash: String,
    request_id: String,
    edited_text: Option<String>,
) -> Result<serde_json::Value, String> {
    create_import_continuation(draft_id, draft_hash, request_id, edited_text, |id| {
        crate::mirror::notify_sessions_changed(Some(&app), "import-continuation", id);
    })
    .await
}

/// Notify attached catalogs only after the complete native row is published.
pub(crate) async fn create_import_continuation(
    draft_id: String,
    draft_hash: String,
    request_id: String,
    edited_text: Option<String>,
    on_published: impl FnOnce(&str),
) -> Result<serde_json::Value, String> {
    let app_session_id = tauri::async_runtime::spawn_blocking(move || {
        engine::continue_create(&draft_id, &draft_hash, &request_id, edited_text).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())??;
    on_published(&app_session_id);
    Ok(serde_json::json!({ "appSessionId": app_session_id }))
}

#[tauri::command]
pub async fn session_import_continuation_get(
    app_session_id: String,
) -> Result<Option<crate::session_migration::continuation_view::ContinuationDraftView>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::continuation_view::get(&app_session_id).map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn session_import_continuation_save(
    app_session_id: String,
    expected_text_hash: String,
    text: String,
) -> Result<crate::session_migration::continuation_view::ContinuationDraftView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::continuation_view::save(
            &app_session_id,
            &expected_text_hash,
            &text,
        )
        .map_err(map_err)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Shared guard used by native session commands.
#[allow(dead_code)]
pub fn guard_native_session(session_id: Option<&str>) -> Result<(), String> {
    reject_imported_history_id(session_id)
}

#[cfg(test)]
#[path = "session_migration_entry_tests.rs"]
mod session_migration_entry_tests;

#[cfg(test)]
#[path = "session_migration_prepare_command_tests.rs"]
mod session_migration_prepare_command_tests;
