//! Explicitly copy saved mappings into the native workspace store. This is a
//! separate user action, not part of metadata import, trust, or session binding.

use rusqlite::TransactionBehavior;
use serde::Deserialize;

use crate::workspace_store::{
    self as native, WorkspaceCapability, WorkspaceRecord, WorkspaceRootAccess, WorkspaceRootRole,
};

use super::{
    catalog,
    errors::{ErrorCode, ImportError},
    hash::sha256_bytes,
    workspace::SourceWorkspace,
    workspace_import::{db_error, stored_json, validate_choices},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceApplyRequest {
    pub source_workspace_id: String,
    pub expected_revision: String,
}

fn mapping_error(message: impl Into<String>) -> ImportError {
    ImportError::new(ErrorCode::MappingConflict, message)
}

pub fn apply(request: WorkspaceApplyRequest) -> Result<WorkspaceRecord, ImportError> {
    if request.source_workspace_id.is_empty()
        || request.source_workspace_id.len() > 256
        || request.expected_revision.len() != 64
    {
        return Err(mapping_error(
            "invalid saved workspace identity or revision",
        ));
    }
    if !catalog::catalog_path().try_exists().map_err(db_error)? {
        return Err(ImportError::new(
            ErrorCode::SourceNotFound,
            "saved workspace not found",
        ));
    }
    let mut conn = catalog::open_catalog()?;
    // Keep the confirmed mapping revision stable until its native copy is
    // published. No catalog write follows, so no cross-store commit is claimed.
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    let json = stored_json(&tx, &request.source_workspace_id)?
        .ok_or_else(|| ImportError::new(ErrorCode::SourceNotFound, "saved workspace not found"))?;
    if sha256_bytes(json.as_bytes()) != request.expected_revision {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "workspace mappings changed; reopen before creating a native copy",
        ));
    }
    let source: SourceWorkspace = serde_json::from_str(&json).map_err(db_error)?;
    if source.source_workspace_id != request.source_workspace_id {
        return Err(mapping_error("saved workspace identity mismatch"));
    }
    if source.roots.is_empty() || source.roots.len() > native::MAX_EXTRA_ROOTS + 1 {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "native workspace requires one to nine mapped roots",
        ));
    }
    validate_choices(&source, &source.mapping_choices)?;
    let projects = native::load_projects_snapshot().map_err(mapping_error)?;
    let primary = source
        .mapping_choices
        .iter()
        .find(|choice| choice.primary)
        .and_then(|choice| choice.target_project_id.as_ref())
        .ok_or_else(|| mapping_error("map the primary root to an existing local project"))?;
    let mut roots = Vec::<native::WorkspaceRoot>::new();
    let mut project_targets = Vec::new();
    for root in &source.roots {
        let choice = source
            .mapping_choices
            .iter()
            .find(|choice| choice.root_id == root.root_id)
            .ok_or_else(|| mapping_error("root mapping missing"))?;
        let target = if let Some(id) = &choice.target_project_id {
            let project = projects
                .iter()
                .find(|project| &project.id == id)
                .ok_or_else(|| mapping_error("mapped project no longer exists"))?;
            if project.is_ssh_remote() {
                return Err(mapping_error(
                    "map remote roots to local projects or local directories",
                ));
            }
            project_targets.push((project.id.clone(), project.path.clone()));
            &project.path
        } else {
            choice
                .explicit_target
                .as_ref()
                .ok_or_else(|| mapping_error("map every root before creating a native workspace"))?
        };
        // Only deliberate mappings are considered; never probe a source URI,
        // original root, guessed cwd, network share, or WSL installation.
        if target.starts_with("//")
            || (target.starts_with(r"\\") && !target.starts_with(r"\\?\"))
            || target
                .get(..8)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(r"\\?\UNC\"))
        {
            return Err(mapping_error(
                "native import requires local target directories",
            ));
        }
        let (path, available) =
            native::canonicalize_workspace_root(target).map_err(mapping_error)?;
        if !available {
            return Err(ImportError::new(
                ErrorCode::WorkspaceUnresolved,
                "mapped directory is unavailable",
            ));
        }
        if roots.iter().any(|previous| previous.path == path) {
            return Err(mapping_error(
                "multiple source roots map to the same local directory",
            ));
        }
        roots.push(native::WorkspaceRoot {
            path,
            role: if choice.primary {
                WorkspaceRootRole::Primary
            } else {
                WorkspaceRootRole::Extra
            },
            access: if choice.primary {
                WorkspaceRootAccess::Write
            } else {
                WorkspaceRootAccess::Read
            },
            path_ok: Some(true),
        });
    }
    // Full source revision + target snapshot gives retries a stable identity.
    // A later remap makes a new native copy instead of changing an active one.
    let identity = serde_json::to_vec(&(
        &request.source_workspace_id,
        &request.expected_revision,
        primary,
        &roots,
    ))
    .map_err(db_error)?;
    let record = WorkspaceRecord {
        id: format!("ws_import_{}", sha256_bytes(&identity)),
        name: source.display_name,
        primary_project_id: primary.clone(),
        roots,
        profile_ref: None,
        capability: WorkspaceCapability::ContextOnly,
        capability_reason: Some("imported workspace: extra roots are context only".into()),
        updated_at: chrono::Utc::now(),
    };
    native::insert_imported(record, &project_targets).map_err(|message| {
        if message.starts_with("MAPPING_CONFLICT:") {
            mapping_error(message)
        } else {
            db_error(message)
        }
    })
}

#[cfg(test)]
#[path = "workspace_apply_tests.rs"]
mod tests;
