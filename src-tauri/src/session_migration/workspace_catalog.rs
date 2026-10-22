//! Read-only history organization. Membership never grants execution access.
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{
    cursor::{decode_bound_cursor, encode_bound_cursor, CursorScope},
    errors::{ErrorCode, ImportError},
    workspace_association as association, workspace_association_store as storage,
    workspace_import::db_error,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum WorkspaceFilter {
    Unassigned,
    Workspace {
        #[serde(rename = "sourceWorkspaceId")]
        source_workspace_id: String,
    },
}

impl WorkspaceFilter {
    pub(super) fn sql_value(&self) -> Result<(u8, &str), ImportError> {
        match self {
            Self::Unassigned => Ok((1, "")),
            Self::Workspace {
                source_workspace_id,
            } if !source_workspace_id.is_empty() && source_workspace_id.len() <= 256 => {
                Ok((2, source_workspace_id))
            }
            _ => Err(ImportError::new(
                ErrorCode::MappingConflict,
                "invalid workspace filter",
            )),
        }
    }
}

/// A corrupt row must not silently become "unassigned" or vanish from a filter.
/// Stream compact association records; never retain all entries or read archives.
pub(super) fn validate_memberships(conn: &Connection) -> Result<bool, ImportError> {
    if !storage::available(conn)? {
        return Ok(false);
    }
    let mut stmt = conn.prepare("SELECT a.import_id, CASE WHEN length(CAST(a.json AS BLOB))<=65536 THEN a.json ELSE NULL END
        FROM entry_workspaces a JOIN entries e ON e.import_id=a.import_id
        WHERE e.hidden=0 AND NOT EXISTS (SELECT 1 FROM tombstones t
            WHERE t.installation_id=e.installation_id AND t.provider=e.provider AND t.native_session_id=e.native_session_id)").map_err(db_error)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(db_error)?;
    for row in rows {
        let (id, raw) = row.map_err(db_error)?;
        storage::decode(
            &id,
            &raw.ok_or_else(|| db_error("workspace association exceeds metadata limit"))?,
        )?;
    }
    Ok(true)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceGroup {
    pub source_workspace_id: String,
    pub display_name: String,
    pub source_id: Option<String>,
    pub provider: Option<String>,
    pub source_locator: Option<String>,
    pub missing: bool,
}

fn validate_group(group: &WorkspaceGroup) -> Result<(), ImportError> {
    if group.source_workspace_id.len() > 256
        || group.display_name.len() > 8192
        || group
            .source_id
            .as_ref()
            .is_some_and(|value| value.len() > 256)
        || group
            .provider
            .as_ref()
            .is_some_and(|value| value.len() > 64)
        || group
            .source_locator
            .as_ref()
            .is_some_and(|value| value.len() > 16384)
    {
        return Err(db_error("workspace group exceeds metadata limit"));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceGroupsPage {
    pub items: Vec<WorkspaceGroup>,
    pub next_cursor: Option<String>,
    pub selected: Option<WorkspaceGroup>,
}

fn group(
    conn: &Connection,
    id: &str,
    has_associations: bool,
) -> Result<Option<WorkspaceGroup>, ImportError> {
    if let Some((choice, workspace)) = association::workspace(conn, id)? {
        let source_locator = choice
            .source_locator
            .or_else(|| workspace.workspace_file.map(|file| format!("file:{file}")))
            .or_else(|| {
                workspace
                    .roots
                    .first()
                    .map(|root| format!("folder:{}", root.original_path))
            });
        let identity = workspace.source_identity;
        return Ok(Some(WorkspaceGroup {
            source_workspace_id: id.into(),
            display_name: choice.display_name,
            source_id: identity.as_ref().map(|value| value.source_id.clone()),
            provider: identity.map(|value| value.provider),
            source_locator,
            missing: false,
        }));
    }
    if !has_associations {
        return Ok(None);
    }
    let import_id: Option<String> = conn.query_row("SELECT a.import_id FROM entry_workspaces a JOIN entries e ON e.import_id=a.import_id
        WHERE e.hidden=0 AND json_extract(a.json,'$.choice.sourceWorkspaceId')=?1
        AND NOT EXISTS (SELECT 1 FROM tombstones t WHERE t.installation_id=e.installation_id AND t.provider=e.provider AND t.native_session_id=e.native_session_id)
        ORDER BY a.import_id LIMIT 1", [id], |row| row.get(0)).optional().map_err(db_error)?;
    let Some(import_id) = import_id else {
        return Ok(None);
    };
    let record = storage::load(conn, &import_id)?
        .ok_or_else(|| db_error("workspace membership disappeared"))?;
    let choice = record
        .choice
        .ok_or_else(|| db_error("workspace membership is unassigned"))?;
    Ok(Some(WorkspaceGroup {
        source_workspace_id: id.into(),
        display_name: choice.display_name,
        source_id: Some(record.source_id),
        provider: Some(record.provider),
        source_locator: choice.source_locator,
        missing: true,
    }))
}

pub fn groups(
    cursor: Option<&str>,
    limit: Option<usize>,
    selected_id: Option<&str>,
) -> Result<WorkspaceGroupsPage, ImportError> {
    if selected_id.is_some_and(|id| id.is_empty() || id.len() > 256) {
        return Err(db_error("invalid selected workspace"));
    }
    if !super::catalog::catalog_exists()? {
        return Ok(WorkspaceGroupsPage {
            items: vec![],
            next_cursor: None,
            selected: None,
        });
    }
    let conn = super::catalog::open_catalog_readonly()?;
    let tx = conn.unchecked_transaction().map_err(db_error)?;
    let has_associations = validate_memberships(&tx)?;
    let scope = CursorScope {
        owner: "catalog",
        snapshot: "workspace-groups-v1",
        leaf: "",
        filter: "all",
    };
    let after = cursor
        .map(|raw| decode_bound_cursor(raw, scope))
        .transpose()?
        .and_then(|c| c.mark)
        .unwrap_or_default();
    let limit = super::budget::page_limit(limit);
    // Include saved empty workspaces, and deleted workspace IDs that still organize history.
    let orphan_ids = if has_associations {
        "UNION SELECT json_extract(a.json,'$.choice.sourceWorkspaceId') AS id FROM entry_workspaces a
        JOIN entries e ON e.import_id=a.import_id WHERE e.hidden=0 AND json_type(a.json,'$.choice')='object'
        AND NOT EXISTS (SELECT 1 FROM tombstones t WHERE t.installation_id=e.installation_id AND t.provider=e.provider AND t.native_session_id=e.native_session_id)"
    } else {
        ""
    };
    let sql = format!("SELECT id FROM (SELECT source_workspace_id AS id FROM workspaces {orphan_ids}) WHERE id>?1 ORDER BY id LIMIT ?2");
    let mut stmt = tx.prepare(&sql).map_err(db_error)?;
    let ids = stmt
        .query_map(params![after, limit + 1], |r| r.get::<_, String>(0))
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    let more = ids.len() > limit;
    let mut items = Vec::new();
    for id in ids.into_iter().take(limit) {
        items.push(
            group(&tx, &id, has_associations)?
                .ok_or_else(|| db_error("workspace group disappeared"))?,
        );
    }
    for item in &items {
        validate_group(item)?;
    }
    let next_cursor = if more {
        Some(encode_bound_cursor(
            scope,
            0,
            items.last().map(|item| item.source_workspace_id.clone()),
        )?)
    } else {
        None
    };
    let selected = selected_id
        .map(|id| group(&tx, id, has_associations))
        .transpose()?
        .flatten();
    if let Some(selected) = &selected {
        validate_group(selected)?;
    }
    Ok(WorkspaceGroupsPage {
        items,
        next_cursor,
        selected,
    })
}

#[cfg(test)]
#[path = "workspace_catalog_tests.rs"]
mod tests;
