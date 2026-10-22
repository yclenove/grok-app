//! Independent, metadata-only workspace import. Never scans project contents or
//! mutates native projects, workspace bindings, trust, settings or source files.

use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::{
    catalog,
    errors::{ErrorCode, ImportError},
    hash::sha256_bytes,
    workspace::{self, SourceWorkspace, WorkspaceRootChoice},
};

const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceSource {
    pub path: String,
    pub kind: WorkspaceSourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceSourceKind {
    File,
    Folder,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePreview {
    pub source: WorkspaceSource,
    pub workspace: SourceWorkspace,
    pub fingerprint: String,
    pub saved_revision: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceSaveRequest {
    pub source: WorkspaceSource,
    pub expected_fingerprint: String,
    pub expected_saved_revision: Option<String>,
    pub choices: Vec<WorkspaceRootChoice>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePage {
    pub items: Vec<SourceWorkspace>,
    pub next_cursor: Option<String>,
}

fn io_error(error: std::io::Error) -> ImportError {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => ErrorCode::SourceNotFound,
        std::io::ErrorKind::PermissionDenied => ErrorCode::SourcePermissionDenied,
        _ => ErrorCode::SnapshotUnavailable,
    };
    ImportError::new(code, error.to_string())
}

pub(super) fn db_error(error: impl std::fmt::Display) -> ImportError {
    ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
}

fn bounded_read(path: &Path, max: usize) -> Result<Vec<u8>, ImportError> {
    let file = File::open(path).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "expected a regular file",
        ));
    }
    if metadata.len() > max as u64 {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "file exceeds import limit",
        ));
    }
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > max {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "file grew beyond import limit",
        ));
    }
    Ok(bytes)
}

fn read_source(source: &WorkspaceSource) -> Result<(SourceWorkspace, String), ImportError> {
    read_source_inner(source, true)
}

pub(super) fn read_relocation_source(
    source: &WorkspaceSource,
) -> Result<(SourceWorkspace, String), ImportError> {
    read_source_inner(source, false)
}

fn read_source_inner(
    source: &WorkspaceSource,
    resolve_legacy: bool,
) -> Result<(SourceWorkspace, String), ImportError> {
    let selected = Path::new(&source.path);
    if !selected.is_absolute() {
        return Err(ImportError::new(
            ErrorCode::WorkspaceUnresolved,
            "select an absolute workspace path",
        ));
    }
    // Keep the selected path as the base for relative folder entries, including
    // workspace files reached through a symlink. Identity also records the target.
    let resolved = std::fs::canonicalize(selected).map_err(io_error)?;
    let (mut workspace, content_hash) = match source.kind {
        WorkspaceSourceKind::Folder => {
            if !resolved.is_dir() {
                return Err(ImportError::new(
                    ErrorCode::WorkspaceUnresolved,
                    "expected a workspace directory",
                ));
            }
            (
                workspace::source_workspace_from_folder(&source.path),
                "folder".to_string(),
            )
        }
        WorkspaceSourceKind::File => {
            if !selected
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("code-workspace"))
            {
                return Err(ImportError::new(
                    ErrorCode::FormatUnsupported,
                    "expected a .code-workspace file",
                ));
            }
            let bytes = bounded_read(selected, MAX_FILE_BYTES)?;
            let text = std::str::from_utf8(&bytes).map_err(|error| {
                ImportError::new(ErrorCode::FormatUnsupported, error.to_string())
            })?;
            let workspace =
                workspace::source_workspace_from_code_workspace_checked(text, &source.path)
                    .map_err(|error| ImportError::new(ErrorCode::FormatUnsupported, error))?;
            (workspace, sha256_bytes(&bytes))
        }
    };
    if let Some(source_id) = &source.source_id {
        super::workspace_identity::scope(
            &catalog::open_catalog_readonly()?,
            &mut workspace,
            source_id,
        )?;
    } else if resolve_legacy && catalog::catalog_path().try_exists().map_err(io_error)? {
        super::workspace_identity::resolve_unassigned(
            &catalog::open_catalog_readonly()?,
            &mut workspace,
        )?;
    }
    let base = format!("{}\0{}\0{content_hash}", source.path, resolved.display());
    let fingerprint = sha256_bytes(
        match &workspace.source_identity {
            Some(identity) => format!("{base}\0{}", identity.source_id),
            None => base,
        }
        .as_bytes(),
    );
    Ok((workspace, fingerprint))
}

pub(super) fn stored_json(conn: &Connection, id: &str) -> Result<Option<String>, ImportError> {
    conn.query_row(
        "SELECT json FROM workspaces WHERE source_workspace_id=?1",
        [id],
        |row| row.get(0),
    )
    .optional()
    .map_err(db_error)
}

/// This path is read-only, including when the catalog does not exist yet.
pub fn preview(mut source: WorkspaceSource) -> Result<WorkspacePreview, ImportError> {
    let (mut workspace, fingerprint) = read_source(&source)?;
    source.source_id = workspace
        .source_identity
        .as_ref()
        .map(|identity| identity.source_id.clone());
    let saved = if catalog::catalog_path().try_exists().map_err(io_error)? {
        stored_json(
            &catalog::open_catalog_readonly()?,
            &workspace.source_workspace_id,
        )?
    } else {
        None
    };
    if let Some(json) = &saved {
        let previous: SourceWorkspace = serde_json::from_str(json).map_err(db_error)?;
        if previous.source_workspace_id != workspace.source_workspace_id
            || previous.source_identity != workspace.source_identity
        {
            return Err(ImportError::new(
                ErrorCode::MappingConflict,
                "saved workspace source identity mismatch",
            ));
        }
        workspace.mapping_choices = previous
            .mapping_choices
            .into_iter()
            .filter(|choice| {
                workspace
                    .roots
                    .iter()
                    .any(|root| root.root_id == choice.root_id)
            })
            .collect();
    }
    Ok(WorkspacePreview {
        source,
        workspace,
        fingerprint,
        saved_revision: saved.map(|json| sha256_bytes(json.as_bytes())),
    })
}

pub(super) fn validate_choices(
    workspace: &SourceWorkspace,
    choices: &[WorkspaceRootChoice],
) -> Result<(), ImportError> {
    let roots: BTreeSet<_> = workspace
        .roots
        .iter()
        .map(|root| root.root_id.as_str())
        .collect();
    let chosen: BTreeSet<_> = choices
        .iter()
        .map(|choice| choice.root_id.as_str())
        .collect();
    if roots != chosen
        || choices.len() != chosen.len()
        || choices.iter().filter(|choice| choice.primary).count() != usize::from(!roots.is_empty())
    {
        return Err(ImportError::new(
            ErrorCode::MappingConflict,
            "confirm each root once and choose exactly one primary root",
        ));
    }
    for choice in choices {
        if let Some(target) = &choice.explicit_target {
            if choice.target_project_id.is_some()
                || target.len() > 8192
                || target.contains('\0')
                || !Path::new(target).is_absolute()
            {
                return Err(ImportError::new(
                    ErrorCode::MappingConflict,
                    "choose one existing project or an absolute local target directory",
                ));
            }
        }
    }
    let targets: BTreeSet<_> = choices
        .iter()
        .filter_map(|choice| choice.target_project_id.as_deref())
        .collect();
    if targets.is_empty() {
        return Ok(());
    }
    // load_projects() runs migrations and can rewrite trust/pin metadata. Import
    // only needs to check IDs, so it deliberately uses a read-only snapshot.
    let projects = crate::store::load_projects_snapshot()
        .map_err(|error| ImportError::new(ErrorCode::MappingConflict, error))?;
    if targets
        .iter()
        .any(|id| !projects.iter().any(|project| &project.id == id))
    {
        return Err(ImportError::new(
            ErrorCode::MappingConflict,
            "selected project no longer exists",
        ));
    }
    Ok(())
}

pub fn save(request: WorkspaceSaveRequest) -> Result<SourceWorkspace, ImportError> {
    let (mut workspace, fingerprint) = read_source(&request.source)?;
    if fingerprint != request.expected_fingerprint {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "workspace changed after preview; preview again",
        ));
    }
    validate_choices(&workspace, &request.choices)?;
    // Previously selected targets may be temporarily offline; only newly chosen
    // directories must exist now. Source roots remain unchanged provenance.
    let previous = if catalog::catalog_path().try_exists().map_err(io_error)? {
        stored_json(
            &catalog::open_catalog_readonly()?,
            &workspace.source_workspace_id,
        )?
        .map(|json| {
            serde_json::from_str::<SourceWorkspace>(&json).map_err(|error| {
                ImportError::new(
                    ErrorCode::PlanStale,
                    format!("saved workspace is invalid: {error}"),
                )
            })
        })
        .transpose()?
    } else {
        None
    };
    super::workspace_edit::validate_new_targets(&request.choices, previous.as_ref())?;
    workspace.default_root_id = request
        .choices
        .iter()
        .find(|choice| choice.primary)
        .map(|choice| choice.root_id.clone());
    workspace.mapping_choices = request.choices;
    workspace.source_fingerprint = Some(fingerprint);
    persist_workspace(workspace, request.expected_saved_revision.as_deref())
}

pub(super) fn persist_workspace(
    workspace: SourceWorkspace,
    expected_revision: Option<&str>,
) -> Result<SourceWorkspace, ImportError> {
    let json = serde_json::to_string(&workspace).map_err(db_error)?;
    let mut conn = catalog::open_catalog()?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    let previous = stored_json(&tx, &workspace.source_workspace_id)?;
    if previous.as_deref() == Some(json.as_str()) {
        return Ok(workspace); // Exact retry after lost response is idempotent.
    }
    if previous
        .map(|value| sha256_bytes(value.as_bytes()))
        .as_deref()
        != expected_revision
    {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "workspace mappings changed; preview again",
        ));
    }
    // Roots and scoped choices are a single catalog record, never global cwd
    // mappings: two workspaces sharing a folder cannot overwrite each other.
    catalog::upsert_workspace(&tx, &workspace)?;
    tx.commit().map_err(db_error)?;
    Ok(workspace)
}

pub fn list(after: Option<&str>, limit: usize) -> Result<WorkspacePage, ImportError> {
    if !catalog::catalog_path().try_exists().map_err(io_error)? {
        return Ok(WorkspacePage {
            items: Vec::new(),
            next_cursor: None,
        });
    }
    let conn = catalog::open_catalog_readonly()?;
    let cap = limit.clamp(1, 50);
    let mut stmt = conn.prepare("SELECT json FROM workspaces WHERE source_workspace_id > ?1 ORDER BY source_workspace_id LIMIT ?2").map_err(db_error)?;
    let rows = stmt
        .query_map(params![after.unwrap_or(""), (cap + 1) as i64], |row| {
            row.get::<_, String>(0)
        })
        .map_err(db_error)?;
    let mut items = Vec::new();
    for row in rows {
        items.push(
            serde_json::from_str::<SourceWorkspace>(&row.map_err(db_error)?).map_err(db_error)?,
        );
    }
    let next_cursor = if items.len() > cap {
        items.truncate(cap);
        items
            .last()
            .map(|workspace| workspace.source_workspace_id.clone())
    } else {
        None
    };
    Ok(WorkspacePage { items, next_cursor })
}

#[cfg(test)]
#[path = "workspace_import_tests.rs"]
mod tests;
