//! A reviewed native execution target is independent of source-history membership.
use super::errors::{ErrorCode, ImportError};
use crate::workspace_store::{self, WorkspaceCapability, WorkspaceRecord};

fn conflict(message: impl std::fmt::Display) -> ImportError {
    ImportError::new(ErrorCode::MappingConflict, message.to_string())
}

pub(super) fn select(id: &str, project_id: Option<&str>) -> Result<WorkspaceRecord, ImportError> {
    let _locks = crate::store_lock::lock_exclusive_many(&[
        crate::paths::projects_file(),
        crate::paths::workspaces_file(),
    ])
    .map_err(conflict)?;
    let target = workspace_store::get_workspace(id)
        .map_err(conflict)?
        .ok_or_else(|| conflict("selected workspace is missing"))?;
    if project_id.is_some_and(|project| project != target.primary_project_id) {
        return Err(conflict("workspace belongs to a different project"));
    }
    validate(&target)?;
    Ok(target)
}

/// Caller holds the project/workspace writer locks while publishing a session.
pub(crate) fn validate(target: &WorkspaceRecord) -> Result<(), ImportError> {
    let current = workspace_store::get_workspace(&target.id)
        .map_err(conflict)?
        .ok_or_else(|| conflict("selected workspace is missing"))?;
    if current != *target {
        return Err(conflict(
            "workspace changed; review the execution target again",
        ));
    }
    if target.capability == WorkspaceCapability::Blocked {
        return Err(conflict("workspace execution is blocked"));
    }
    workspace_store::validate_primary_location(target).map_err(conflict)?;
    if target.roots.len() > workspace_store::MAX_EXTRA_ROOTS + 1 {
        return Err(conflict("too many workspace roots"));
    }
    for root in &target.roots {
        let (canonical, available) =
            workspace_store::canonicalize_workspace_root(&root.path).map_err(conflict)?;
        if !available || canonical != root.path {
            return Err(conflict("workspace folder moved or is unavailable"));
        }
    }
    Ok(())
}

pub(crate) fn capability(target: &WorkspaceRecord) -> String {
    match target.capability {
        WorkspaceCapability::None => "none",
        WorkspaceCapability::ContextOnly => "context_only",
        WorkspaceCapability::EnforcedRead => "enforced_read",
        WorkspaceCapability::ExtraWriteActive => "extra_write_active",
        WorkspaceCapability::Blocked => "blocked",
    }
    .into()
}
