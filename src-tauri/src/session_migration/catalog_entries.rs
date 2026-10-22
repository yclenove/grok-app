use super::*;

pub fn search_imported_titles(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<CatalogEntry>, ImportError> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let cap = limit.clamp(1, crate::session_migration::budget::SEARCH_LIMIT_MAX);
    let mut stmt = conn
        .prepare(
            "SELECT import_id, installation_id, provider, native_session_id, title, alias, hidden, current_revision, created_at
             FROM entries
             WHERE hidden=0
               AND instr(lower(ifnull(title,'') || ' ' || ifnull(alias,'') || ' ' || native_session_id), ?1) > 0
             ORDER BY created_at DESC, import_id ASC
             LIMIT ?2",
        )
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let rows = stmt
        .query_map(params![q, cap as i64], row_entry)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?);
    }
    Ok(out)
}

pub(crate) const BODY_SEARCH_CHUNK_BYTES: usize = 64 * 1024;
pub(crate) const BODY_SEARCH_CHUNK_OVERLAP_BYTES: usize = 1024;

/// Builds one complete body-search generation inside a transaction. A failed
/// archive read rolls the whole projection back, so readers never observe a
/// partially rebuilt current revision.
pub(crate) struct BodySearchProjection<'conn> {
    tx: rusqlite::Transaction<'conn>,
    import_id: String,
    revision_id: String,
    buffer: String,
    next_ordinal: i64,
    has_text: bool,
    buffer_is_overlap_only: bool,
}

impl<'conn> BodySearchProjection<'conn> {
    pub(crate) fn begin(
        conn: &'conn Connection,
        import_id: &str,
        revision_id: &str,
    ) -> Result<Self, ImportError> {
        let tx =
            rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
                .map_err(sqlite_storage_error)?;
        tx.execute(
            "DELETE FROM body_search_chunks WHERE import_id=?1 AND revision_id=?2",
            params![import_id, revision_id],
        )
        .map_err(sqlite_storage_error)?;
        Ok(Self {
            tx,
            import_id: import_id.to_string(),
            revision_id: revision_id.to_string(),
            buffer: String::with_capacity(BODY_SEARCH_CHUNK_BYTES),
            next_ordinal: 0,
            has_text: false,
            buffer_is_overlap_only: false,
        })
    }

    pub(crate) fn push_text(&mut self, text: &str) -> Result<(), ImportError> {
        if text.is_empty() {
            return Ok(());
        }
        if self.has_text {
            self.append("\n")?;
        }
        self.has_text = true;
        self.append(text)
    }

    fn append(&mut self, mut text: &str) -> Result<(), ImportError> {
        while !text.is_empty() {
            let room = BODY_SEARCH_CHUNK_BYTES.saturating_sub(self.buffer.len());
            let mut take = room.min(text.len());
            while take > 0 && !text.is_char_boundary(take) {
                take -= 1;
            }
            if take == 0 {
                self.flush_with_overlap()?;
                continue;
            }
            self.buffer.push_str(&text[..take]);
            self.buffer_is_overlap_only = false;
            text = &text[take..];
            if self.buffer.len() == BODY_SEARCH_CHUNK_BYTES {
                self.flush_with_overlap()?;
            }
        }
        Ok(())
    }

    fn insert_buffer(&mut self) -> Result<(), ImportError> {
        self.tx
            .execute(
                "INSERT INTO body_search_chunks(import_id, revision_id, chunk_ordinal, text)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    self.import_id,
                    self.revision_id,
                    self.next_ordinal,
                    self.buffer
                ],
            )
            .map_err(sqlite_storage_error)?;
        self.next_ordinal += 1;
        Ok(())
    }

    fn flush_with_overlap(&mut self) -> Result<(), ImportError> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let mut overlap_start = self
            .buffer
            .len()
            .saturating_sub(BODY_SEARCH_CHUNK_OVERLAP_BYTES);
        while overlap_start < self.buffer.len() && !self.buffer.is_char_boundary(overlap_start) {
            overlap_start += 1;
        }
        let overlap = self.buffer[overlap_start..].to_string();
        self.insert_buffer()?;
        self.buffer.clear();
        self.buffer.push_str(&overlap);
        self.buffer_is_overlap_only = true;
        Ok(())
    }

    pub(crate) fn commit(mut self) -> Result<(), ImportError> {
        if self.next_ordinal == 0 || (!self.buffer.is_empty() && !self.buffer_is_overlap_only) {
            self.insert_buffer()?;
        }
        self.tx.commit().map_err(sqlite_storage_error)
    }
}

pub(crate) fn body_search_projection_exists(
    conn: &Connection,
    import_id: &str,
    revision_id: &str,
) -> Result<bool, ImportError> {
    conn.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM body_search_chunks WHERE import_id=?1 AND revision_id=?2
         )",
        params![import_id, revision_id],
        |row| row.get(0),
    )
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))
}

pub fn search_imported_bodies(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<(CatalogEntry, String)>, ImportError> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let cap = limit.clamp(1, crate::session_migration::budget::SEARCH_LIMIT_MAX);
    let mut stmt = conn
        .prepare(
            "WITH first_matches AS (
               SELECT import_id, revision_id, MIN(chunk_ordinal) AS chunk_ordinal
               FROM body_search_chunks
               WHERE instr(lower(text), ?1) > 0
               GROUP BY import_id, revision_id
             )
             SELECT e.import_id, e.installation_id, e.provider, e.native_session_id,
                    e.title, e.alias, e.hidden, e.current_revision, e.created_at,
                    b.text
             FROM first_matches m
             INNER JOIN body_search_chunks b ON b.import_id = m.import_id
              AND b.revision_id = m.revision_id
              AND b.chunk_ordinal = m.chunk_ordinal
             INNER JOIN entries e ON e.import_id = m.import_id
              AND e.current_revision = m.revision_id
             WHERE e.hidden=0
             ORDER BY e.created_at DESC, e.import_id ASC
             LIMIT ?2",
        )
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let rows = stmt
        .query_map(params![q, cap as i64], |row| {
            Ok((row_entry(row)?, row.get::<_, String>(9)?))
        })
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?);
    }
    Ok(out)
}

pub fn is_tombstoned(
    conn: &Connection,
    installation_id: &str,
    provider: &str,
    native: &str,
) -> Result<bool, ImportError> {
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM tombstones WHERE installation_id=?1 AND provider=?2 AND native_session_id=?3",
            params![installation_id, provider, native],
            |r| r.get(0),
        )
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    Ok(n > 0)
}

pub fn hide_entry(conn: &Connection, import_id: &str) -> Result<(), ImportError> {
    let owned = conn
        .is_autocommit()
        .then(|| {
            rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
                .map_err(|error| {
                    ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
                })
        })
        .transpose()?;
    let conn = owned.as_deref().unwrap_or(conn);
    super::super::workspace_association_store::revoke(conn, import_id)?;
    conn.execute(
        "UPDATE entries SET hidden=1 WHERE import_id=?1",
        params![import_id],
    )
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    if let Some(tx) = owned {
        tx.commit()
            .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    }
    Ok(())
}

pub fn insert_tombstone(
    conn: &Connection,
    installation_id: &str,
    provider: &str,
    native: &str,
) -> Result<(), ImportError> {
    let owned = conn
        .is_autocommit()
        .then(|| {
            rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
                .map_err(|error| {
                    ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
                })
        })
        .transpose()?;
    let conn = owned.as_deref().unwrap_or(conn);
    if let Some(entry) = find_entry(conn, installation_id, provider, native)? {
        if !entry.hidden {
            super::super::workspace_association_store::revoke(conn, &entry.import_id)?;
        }
    }
    conn.execute(
        "INSERT OR REPLACE INTO tombstones(installation_id, provider, native_session_id, deleted_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            installation_id,
            provider,
            native,
            chrono::Utc::now().to_rfc3339()
        ],
    )
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    if let Some(tx) = owned {
        tx.commit()
            .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    }
    Ok(())
}

pub fn clear_tombstone(
    conn: &Connection,
    installation_id: &str,
    provider: &str,
    native: &str,
) -> Result<(), ImportError> {
    conn.execute(
        "DELETE FROM tombstones WHERE installation_id=?1 AND provider=?2 AND native_session_id=?3",
        params![installation_id, provider, native],
    )
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    Ok(())
}

pub fn upsert_source(
    conn: &Connection,
    installation_id: &str,
    provider: &str,
    root_path: &str,
) -> Result<(), ImportError> {
    conn.execute(
        "INSERT OR IGNORE INTO sources(installation_id, provider, profile, host, root_path, created_at)
         VALUES (?1, ?2, NULL, 'local', ?3, ?4)",
        params![
            installation_id,
            provider,
            root_path,
            chrono::Utc::now().to_rfc3339()
        ],
    )
    .map_err(sqlite_storage_error)?;
    Ok(())
}

pub fn upsert_workspace(conn: &Connection, workspace: &SourceWorkspace) -> Result<(), ImportError> {
    let targets: Vec<_> = workspace
        .mapping_choices
        .iter()
        .filter_map(|choice| choice.target_project_id.as_deref())
        .collect();
    // Even zero-project/empty workspaces need a reservation for identity claims.
    // For project references, with_targets owns the transaction so it keeps the
    // project lock until commit. Do not move that commit outside its lock scope.
    let owned = (conn.is_autocommit() && targets.is_empty())
        .then(|| {
            rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
                .map_err(|error| {
                    ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
                })
        })
        .transpose()?;
    let conn = owned.as_deref().unwrap_or(conn);
    super::super::project_references::with_targets(conn, &targets, |conn| {
        super::super::workspace_identity::validate_write(conn, workspace)?;
        let json = serde_json::to_string(workspace)
            .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
        conn.execute(
            "INSERT INTO workspaces(source_workspace_id, display_name, json)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(source_workspace_id) DO UPDATE SET
           display_name=excluded.display_name,
           json=excluded.json",
            params![workspace.source_workspace_id, workspace.display_name, json],
        )
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
        Ok(())
    })?;
    if let Some(tx) = owned {
        tx.commit()
            .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
pub fn list_workspaces(conn: &Connection) -> Result<Vec<SourceWorkspace>, ImportError> {
    let mut stmt = conn
        .prepare("SELECT json FROM workspaces ORDER BY display_name, source_workspace_id")
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let mut out = Vec::new();
    for row in rows {
        let json =
            row.map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
        out.push(
            serde_json::from_str(&json)
                .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?,
        );
    }
    Ok(out)
}

pub fn upsert_mapping(conn: &Connection, mapping: &WorkspaceMapping) -> Result<(), ImportError> {
    let targets: Vec<_> = mapping.target_project_id.as_deref().into_iter().collect();
    super::super::project_references::with_targets(conn, &targets, |conn| {
        let json = serde_json::to_string(mapping)
            .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
        conn.execute(
            "INSERT INTO mappings(root_id, json)
         VALUES (?1, ?2)
         ON CONFLICT(root_id) DO UPDATE SET json=excluded.json",
            params![mapping.root_id, json],
        )
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
        Ok(())
    })
}

#[cfg(test)]
pub fn list_mappings(conn: &Connection) -> Result<Vec<WorkspaceMapping>, ImportError> {
    let mut stmt = conn
        .prepare("SELECT json FROM mappings ORDER BY root_id")
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let mut out = Vec::new();
    for row in rows {
        let json =
            row.map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
        out.push(
            serde_json::from_str(&json)
                .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?,
        );
    }
    Ok(out)
}

pub fn upsert_entry(conn: &Connection, entry: &CatalogEntry) -> Result<(), ImportError> {
    conn.execute(
        "INSERT INTO entries(import_id, installation_id, provider, native_session_id, title, alias, hidden, current_revision, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(installation_id, provider, native_session_id) DO UPDATE SET
           title=excluded.title,
           hidden=excluded.hidden,
           current_revision=excluded.current_revision",
        params![
            entry.import_id,
            entry.installation_id,
            entry.provider,
            entry.native_session_id,
            entry.title,
            entry.alias,
            entry.hidden as i64,
            entry.current_revision,
            entry.created_at
        ],
    )
    .map_err(sqlite_storage_error)?;
    Ok(())
}

// Parameters mirror the immutable revision row one-for-one. Keeping that
// mapping explicit makes accidental column/order drift visible at this boundary.
#[allow(clippy::too_many_arguments)]
pub fn insert_revision(
    conn: &Connection,
    revision_id: &str,
    import_id: &str,
    snapshot_id: &str,
    revision_semantic_key: &str,
    outcome: &str,
    inventory_json: &str,
    dir_path: &str,
) -> Result<(), ImportError> {
    conn.execute(
        "INSERT OR IGNORE INTO revisions(revision_id, import_id, source_snapshot_id, normalizer_version, options_hash, outcome, inventory_json, dir_path, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            revision_id,
            import_id,
            snapshot_id,
            crate::session_migration::types::NORMALIZER_VERSION,
            revision_semantic_key,
            outcome,
            inventory_json,
            dir_path,
            chrono::Utc::now().to_rfc3339()
        ],
    )
    .map_err(sqlite_storage_error)?;
    Ok(())
}

#[cfg(test)]
pub fn revision_inventory(
    conn: &Connection,
    import_id: &str,
    revision_id: &str,
) -> Result<crate::session_migration::types::Inventory, ImportError> {
    let json: String = conn
        .query_row(
            "SELECT inventory_json FROM revisions WHERE import_id=?1 AND revision_id=?2",
            params![import_id, revision_id],
            |r| r.get(0),
        )
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    serde_json::from_str(&json).map_err(|e| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            format!("invalid inventory: {e}"),
        )
    })
}

pub fn find_revision_by_snapshot(
    conn: &Connection,
    import_id: &str,
    snapshot_id: &str,
    revision_semantic_key: &str,
) -> Result<Option<String>, ImportError> {
    // `options_hash` is the legacy physical column name. New rows store a
    // composite revision key over options + selection + normalizer. Old WIP
    // rows therefore cannot falsely deduplicate against the stricter key.
    conn.query_row(
        "SELECT revision_id FROM revisions WHERE import_id=?1 AND source_snapshot_id=?2 AND options_hash=?3",
        params![import_id, snapshot_id, revision_semantic_key],
        |r| r.get(0),
    )
    .optional()
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RevisionRecord {
    pub revision_id: String,
    pub import_id: String,
    pub source_snapshot_id: String,
    pub outcome: String,
    pub created_at: String,
}

pub fn get_revision(
    conn: &Connection,
    import_id: &str,
    revision_id: &str,
) -> Result<Option<RevisionRecord>, ImportError> {
    conn.query_row(
        "SELECT revision_id, import_id, source_snapshot_id, outcome, created_at
         FROM revisions WHERE import_id=?1 AND revision_id=?2",
        params![import_id, revision_id],
        |row| {
            Ok(RevisionRecord {
                revision_id: row.get(0)?,
                import_id: row.get(1)?,
                source_snapshot_id: row.get(2)?,
                outcome: row.get(3)?,
                created_at: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))
}

/// All snapshots for one import, oldest first. Callers pick a revision; bodies stay separate.
#[cfg(test)]
pub fn list_revisions(
    conn: &Connection,
    import_id: &str,
) -> Result<Vec<RevisionRecord>, ImportError> {
    Ok(list_revisions_page(
        conn,
        import_id,
        None,
        Some(crate::session_migration::budget::PAGE_SIZE_MAX),
    )?
    .0)
}

#[cfg(test)]
pub fn list_revisions_page(
    conn: &Connection,
    import_id: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<(Vec<RevisionRecord>, Option<String>, u32), ImportError> {
    let cap = crate::session_migration::budget::page_limit(limit);
    let total: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM revisions WHERE import_id=?1",
            params![import_id],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?
        .try_into()
        .unwrap_or(0);
    let scope = CursorScope {
        owner: import_id,
        snapshot: "revisions",
        leaf: "",
        filter: "revisions",
    };
    let start = match cursor.filter(|s| !s.is_empty()) {
        None => 0u32,
        Some(raw) => decode_bound_cursor(raw, scope)?.offset,
    };
    if total > 0 && start >= total {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "revision cursor past end",
        ));
    }
    let mut stmt = conn
        .prepare(
            "SELECT revision_id, import_id, source_snapshot_id, outcome, created_at
             FROM revisions WHERE import_id=?1
             ORDER BY created_at ASC, revision_id ASC
             LIMIT ?2 OFFSET ?3",
        )
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let rows = stmt
        .query_map(params![import_id, (cap + 1) as i64, start as i64], |row| {
            Ok(RevisionRecord {
                revision_id: row.get(0)?,
                import_id: row.get(1)?,
                source_snapshot_id: row.get(2)?,
                outcome: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?);
    }
    let next = if out.len() > cap {
        out.pop();
        Some(encode_bound_cursor(scope, start + out.len() as u32, None)?)
    } else {
        None
    };
    Ok((out, next, total))
}

pub fn revision_dir(import_id: &str, revision_id: &str) -> Result<PathBuf, ImportError> {
    crate::session_migration::ids::parse_import_uuid(import_id)?;
    uuid::Uuid::parse_str(revision_id).map_err(|_| {
        ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "invalid import revision id",
        )
    })?;
    let path = entries_dir()
        .join(crate::session_migration::ids::import_id_dir_component(
            import_id,
        ))
        .join(revision_id);
    ensure_inside_imports(&path)?;
    Ok(path)
}

pub fn ensure_inside_imports(path: &Path) -> Result<(), ImportError> {
    let root = imports_dir();
    let root_c = root.canonicalize().unwrap_or(root);
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(ImportError::new(
            ErrorCode::SourcePermissionDenied,
            "parent traversal is not allowed",
        ));
    }
    let mut ancestor = path.to_path_buf();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        if let Some(name) = ancestor.file_name() {
            suffix.push(name.to_owned());
        }
        if !ancestor.pop() {
            break;
        }
    }
    let mut path_c = ancestor
        .canonicalize()
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    for part in suffix.into_iter().rev() {
        path_c.push(part);
    }
    if !path_c.starts_with(&root_c) {
        return Err(ImportError::new(
            ErrorCode::SourcePermissionDenied,
            "path is outside the imports directory",
        ));
    }
    Ok(())
}
