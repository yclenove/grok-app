//! Edits the saved workspace association without reopening the source. Source
//! identities and paths are immutable here; targets are separate user choices.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{
    catalog,
    errors::{ErrorCode, ImportError},
    hash::sha256_bytes,
    workspace::{SourceWorkspace, WorkspaceRootChoice},
    workspace_import::{db_error, persist_workspace, stored_json, validate_choices},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedWorkspace {
    pub workspace: SourceWorkspace,
    pub revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceUpdateRequest {
    pub source_workspace_id: String,
    pub expected_revision: String,
    pub choices: Vec<WorkspaceRootChoice>,
    #[serde(default)]
    pub source_id: Option<String>,
}

pub fn get(id: &str) -> Result<SavedWorkspace, ImportError> {
    if id.is_empty() || id.len() > 256 {
        return Err(ImportError::new(
            ErrorCode::SourceNotFound,
            "invalid workspace identity",
        ));
    }
    let conn = catalog::open_catalog_readonly()?;
    let raw = stored_json(&conn, id)?
        .ok_or_else(|| ImportError::new(ErrorCode::SourceNotFound, "saved workspace not found"))?;
    let workspace: SourceWorkspace = serde_json::from_str(&raw).map_err(db_error)?;
    if workspace.source_workspace_id != id {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "workspace identity mismatch",
        ));
    }
    Ok(SavedWorkspace {
        workspace,
        revision: sha256_bytes(raw.as_bytes()),
    })
}

pub(super) fn validate_new_targets(
    choices: &[WorkspaceRootChoice],
    previous: Option<&SourceWorkspace>,
) -> Result<(), ImportError> {
    for choice in choices {
        let Some(target) = &choice.explicit_target else {
            continue;
        };
        let unchanged = previous.is_some_and(|workspace| {
            workspace.mapping_choices.iter().any(|old| {
                old.root_id == choice.root_id && old.explicit_target.as_ref() == Some(target)
            })
        });
        if unchanged {
            continue;
        }
        let path = Path::new(target);
        let canonical = path
            .canonicalize()
            .map_err(|error| ImportError::new(ErrorCode::WorkspaceUnresolved, error.to_string()))?;
        if !canonical.is_dir() {
            return Err(ImportError::new(
                ErrorCode::WorkspaceUnresolved,
                "target directory is unavailable",
            ));
        }
        // Keep the selected spelling: this is metadata, not a permission grant.
        // Normalizing it here breaks lost-response retries once a drive is
        // offline (and exposes Windows verbatim paths in the UI). Execution
        // must revalidate the current target through the native workspace API.
    }
    Ok(())
}

pub fn update(request: WorkspaceUpdateRequest) -> Result<SavedWorkspace, ImportError> {
    let current = get(&request.source_workspace_id)?;
    validate_choices(&current.workspace, &request.choices)?;
    validate_new_targets(&request.choices, Some(&current.workspace))?;
    let mut workspace = current.workspace;
    if let Some(source_id) = &request.source_id {
        super::workspace_identity::adopt(
            &catalog::open_catalog_readonly()?,
            &mut workspace,
            source_id,
        )?;
    }
    workspace.default_root_id = request
        .choices
        .iter()
        .find(|row| row.primary)
        .map(|row| row.root_id.clone());
    workspace.mapping_choices = request.choices;
    // Never substitute current.revision: a stale caller cannot overwrite a
    // newer edit. Exact repeated requests remain idempotent after lost replies.
    let workspace = persist_workspace(workspace, Some(&request.expected_revision))?;
    let revision = sha256_bytes(
        serde_json::to_string(&workspace)
            .map_err(db_error)?
            .as_bytes(),
    );
    Ok(SavedWorkspace {
        workspace,
        revision,
    })
}

#[cfg(test)]
#[path = "workspace_edit_tests.rs"]
mod tests;
