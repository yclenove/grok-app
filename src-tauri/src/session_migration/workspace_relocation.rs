//! Explicit metadata relocation. Read the new source, never require the old one.
use super::{
    catalog,
    errors::{ErrorCode, ImportError},
    hash::sha256_bytes,
    workspace::{SourceWorkspace, WorkspaceRoot, WorkspaceRootChoice},
    workspace_edit, workspace_identity,
    workspace_import::{self, db_error, stored_json, WorkspaceSource, WorkspaceSourceKind},
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

const MAX_RECEIPT: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewRequest {
    pub source_workspace_id: String,
    pub expected_revision: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelocationPreview {
    pub request: PreviewRequest,
    pub before: SourceWorkspace,
    pub after: SourceWorkspace,
    pub retained_root_ids: Vec<String>,
    pub added_root_ids: Vec<String>,
    pub removed_roots: Vec<WorkspaceRoot>,
    pub fingerprint: String,
    pub proposal_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveRequest {
    pub request_id: String,
    pub preview: PreviewRequest,
    pub fingerprint: String,
    pub proposal_hash: String,
}

fn stale(message: &str) -> ImportError {
    ImportError::new(ErrorCode::PlanStale, message)
}
fn digest(value: &impl Serialize) -> Result<String, ImportError> {
    let bytes = serde_json::to_vec(value).map_err(db_error)?;
    if bytes.len() > MAX_RECEIPT {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "workspace metadata exceeds 16 MiB",
        ));
    }
    Ok(sha256_bytes(&bytes))
}

fn validate(request: &PreviewRequest) -> Result<(), ImportError> {
    if request.source_workspace_id.is_empty()
        || request.source_workspace_id.len() > 256
        || request.expected_revision.len() != 64
        || !request
            .expected_revision
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
        || request.path.is_empty()
        || request.path.len() > 8192
        || request.path.contains('\0')
    {
        return Err(stale("invalid workspace relocation input"));
    }
    Ok(())
}

pub fn preview(request: PreviewRequest) -> Result<RelocationPreview, ImportError> {
    validate(&request)?;
    let saved = workspace_edit::get(&request.source_workspace_id)?;
    if saved.revision != request.expected_revision {
        return Err(stale("workspace changed before relocation"));
    }
    let before = saved.workspace;
    let source = WorkspaceSource {
        path: request.path.clone(),
        kind: if before.workspace_file.is_some() {
            WorkspaceSourceKind::File
        } else {
            WorkspaceSourceKind::Folder
        },
        source_id: before.source_identity.as_ref().map(|s| s.source_id.clone()),
    };
    let (mut after, fingerprint) = workspace_import::read_relocation_source(&source)?;
    let conn = catalog::open_catalog_readonly()?;
    if after.source_workspace_id != before.source_workspace_id
        && stored_json(&conn, &after.source_workspace_id)?.is_some()
    {
        return Err(ImportError::new(
            ErrorCode::MappingConflict,
            "destination already belongs to another saved workspace",
        ));
    }
    after
        .source_workspace_id
        .clone_from(&before.source_workspace_id);
    workspace_identity::validate_relocation(&conn, &before, &after)?;
    let retained_root_ids: Vec<_> = after
        .roots
        .iter()
        .filter(|next| before.roots.iter().any(|old| old.root_id == next.root_id))
        .map(|root| root.root_id.clone())
        .collect();
    let added_root_ids = after
        .roots
        .iter()
        .filter(|root| !retained_root_ids.contains(&root.root_id))
        .map(|root| root.root_id.clone())
        .collect();
    let removed_roots = before
        .roots
        .iter()
        .filter(|root| !retained_root_ids.contains(&root.root_id))
        .cloned()
        .collect();
    let primary = before
        .mapping_choices
        .iter()
        .find(|c| c.primary && retained_root_ids.contains(&c.root_id))
        .map(|c| c.root_id.clone())
        .or_else(|| {
            before
                .default_root_id
                .clone()
                .filter(|id| retained_root_ids.contains(id))
        })
        .or_else(|| after.roots.first().map(|r| r.root_id.clone()));
    after.mapping_choices = after
        .roots
        .iter()
        .map(|root| {
            let mut choice = before
                .mapping_choices
                .iter()
                .find(|c| c.root_id == root.root_id)
                .cloned()
                .unwrap_or(WorkspaceRootChoice {
                    root_id: root.root_id.clone(),
                    target_project_id: None,
                    explicit_target: None,
                    primary: false,
                });
            choice.primary = primary.as_deref() == Some(&root.root_id);
            choice
        })
        .collect();
    after.default_root_id = primary;
    after.source_fingerprint = Some(fingerprint.clone());
    let proposal_hash = digest(&(&request, &fingerprint, &after))?;
    Ok(RelocationPreview {
        request,
        before,
        after,
        retained_root_ids,
        added_root_ids,
        removed_roots,
        fingerprint,
        proposal_hash,
    })
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    version: u32,
    request_hash: String,
    workspace: SourceWorkspace,
    revision: String,
}

fn receipt_exists(conn: &Connection) -> Result<bool, ImportError> {
    // One statement gives both facts the same read snapshot while another
    // request may atomically publish the optional schema for the first time.
    let (table, version): (bool, Option<String>) = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='workspace_relocations' AND type='table'),
            (SELECT value FROM meta WHERE key='workspace_relocations_version')",
        [], |r| Ok((r.get(0)?, r.get(1)?)),
    ).map_err(db_error)?;
    match (table, version.as_deref()) {
        (false, None) => Ok(false),
        (true, Some("1")) => Ok(true),
        _ => Err(db_error(
            "relocation receipt schema is missing or unsupported",
        )),
    }
}

fn receipt(
    conn: &Connection,
    request: &SaveRequest,
) -> Result<Option<workspace_edit::SavedWorkspace>, ImportError> {
    if !receipt_exists(conn)? {
        return Ok(None);
    }
    let raw: Option<(String, String)> = conn.query_row("SELECT CASE WHEN length(CAST(json AS BLOB))<=16777216 THEN json ELSE NULL END, digest FROM workspace_relocations WHERE request_id=?1",
        [&request.request_id], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(db_error)?;
    raw.map(|(json, checksum)| {
        if sha256_bytes(json.as_bytes()) != checksum {
            return Err(db_error("relocation receipt digest mismatch"));
        }
        let stored: Receipt = serde_json::from_str(&json).map_err(db_error)?;
        if stored.version != 1
            || stored.request_hash != digest(request)?
            || stored.workspace.source_workspace_id != request.preview.source_workspace_id
            || stored.revision != digest(&stored.workspace)?
        {
            return Err(stale("relocation request identity changed"));
        }
        // Return current metadata so an old retry cannot show stale mappings as
        // newly saved. The receipt proves completion, not permission to reapply.
        let current = stored_json(conn, &stored.workspace.source_workspace_id)?
            .ok_or_else(|| stale("relocated workspace was removed"))?;
        let workspace: SourceWorkspace = serde_json::from_str(&current).map_err(db_error)?;
        if workspace.source_workspace_id != stored.workspace.source_workspace_id {
            return Err(db_error("workspace identity mismatch"));
        }
        Ok(workspace_edit::SavedWorkspace {
            workspace,
            revision: sha256_bytes(current.as_bytes()),
        })
    })
    .transpose()
}

pub fn save(request: SaveRequest) -> Result<workspace_edit::SavedWorkspace, ImportError> {
    validate(&request.preview)?;
    if request.request_id.is_empty()
        || request.request_id.len() > 256
        || request.fingerprint.len() != 64
        || request.proposal_hash.len() != 64
        || !request.fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
        || !request.proposal_hash.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(stale("invalid relocation confirmation"));
    }
    // Lost-reply retries do not reopen either source, including after restart.
    if let Some(saved) = receipt(&catalog::open_catalog_readonly()?, &request)? {
        return Ok(saved);
    }
    let prepared = match preview(request.preview.clone()) {
        Ok(prepared) => prepared,
        Err(error) => {
            if let Some(saved) = receipt(&catalog::open_catalog_readonly()?, &request)? {
                return Ok(saved);
            }
            return Err(error);
        }
    };
    if prepared.fingerprint != request.fingerprint
        || prepared.proposal_hash != request.proposal_hash
    {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "relocation source or roots changed; preview again",
        ));
    }
    let next = prepared.after;
    let mut conn = catalog::open_catalog()?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(db_error)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    if let Some(saved) = receipt(&tx, &request)? {
        return Ok(saved);
    }
    let raw = stored_json(&tx, &next.source_workspace_id)?
        .ok_or_else(|| stale("workspace disappeared"))?;
    if sha256_bytes(raw.as_bytes()) != request.preview.expected_revision {
        return Err(stale("workspace changed after relocation preview"));
    }
    let previous: SourceWorkspace = serde_json::from_str(&raw).map_err(db_error)?;
    workspace_identity::validate_relocation(&tx, &previous, &next)?;
    let json = serde_json::to_string(&next).map_err(db_error)?;
    let revision = digest(&next)?;
    let targets: Vec<_> = next
        .mapping_choices
        .iter()
        .filter_map(|c| c.target_project_id.as_deref())
        .collect();
    super::project_references::with_targets(&tx, &targets, |conn| {
        workspace_import::validate_choices(&next, &next.mapping_choices)?;
        // This dedicated write is the only exception to ordinary locator
        // immutability; expected revision and identity were checked in this tx.
        conn.execute(
            "UPDATE workspaces SET display_name=?1,json=?2 WHERE source_workspace_id=?3",
            params![next.display_name, json, next.source_workspace_id],
        )
        .map_err(db_error)?;
        let receipt = serde_json::to_string(&Receipt {
            version: 1,
            request_hash: digest(&request)?,
            workspace: next.clone(),
            revision: revision.clone(),
        })
        .map_err(db_error)?;
        if receipt.len() > MAX_RECEIPT {
            return Err(db_error("relocation receipt exceeds 16 MiB"));
        }
        if !receipt_exists(conn)? {
            conn.execute_batch("CREATE TABLE workspace_relocations(request_id TEXT PRIMARY KEY, json TEXT NOT NULL, digest TEXT NOT NULL);
                INSERT INTO meta(key,value) VALUES('workspace_relocations_version','1');").map_err(db_error)?;
        }
        conn.execute(
            "INSERT INTO workspace_relocations(request_id,json,digest) VALUES(?1,?2,?3)",
            params![
                request.request_id,
                receipt,
                sha256_bytes(receipt.as_bytes())
            ],
        )
        .map_err(db_error)?;
        Ok(())
    })?;
    tx.commit().map_err(db_error)?;
    Ok(workspace_edit::SavedWorkspace {
        workspace: next,
        revision,
    })
}

#[cfg(test)]
#[path = "workspace_relocation_tests.rs"]
mod tests;
