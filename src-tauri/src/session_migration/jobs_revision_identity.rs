//! Archive identity is independent of the transient preview that authorized it.
//! Plan/receipt hashes still bind that exact preview; publication dedup verifies
//! both archives before comparing content and all material project decisions.

use super::*;

pub(in crate::session_migration) fn key(
    options_digest: &str,
    selections: &SelectionSet,
    item: &CommitItem,
) -> Result<String, ImportError> {
    key_for_intent(
        options_digest,
        selections,
        item.mapping_project_id.as_deref(),
        item.create_project,
        item.explicit_target.as_deref(),
        item.import_unassigned_anyway,
    )
}

/// Pure durable metadata variant. Recovery must not reconstruct a CommitItem
/// whose prepared preview has already been disposed.
pub(in crate::session_migration) fn key_for_intent(
    options_digest: &str,
    selections: &SelectionSet,
    mapping: Option<&str>,
    create: bool,
    target: Option<&str>,
    unassigned: bool,
) -> Result<String, ImportError> {
    let selected = selections.single_entry()?;
    Ok(options_hash(&serde_json::json!({
        "normalizerVersion": super::super::types::NORMALIZER_VERSION,
        "optionsHash": options_digest,
        "selectionContent": {
            "version": selections.version,
            "leafId": selected.leaf_id,
            "selection": selected.selection.canonicalized(),
        },
        "publication": {
            "mappingProjectId": mapping,
            "createProject": create,
            "explicitTarget": target,
            "importUnassignedAnyway": unassigned,
        },
    })))
}

pub(in crate::session_migration) fn find_duplicate(
    conn: &Connection,
    item: &CommitItem,
    existing: Option<&CatalogEntry>,
    snapshot_id: &str,
    semantic_key: &str,
    options_digest: &str,
) -> Result<Option<String>, ImportError> {
    find_duplicate_using(
        conn,
        item,
        existing,
        snapshot_id,
        semantic_key,
        options_digest,
        |entry, revision| {
            published_auth_from_entry(conn, entry, &entry.import_id, Some(revision), None)
                .map(|published| published.selection_set)
        },
    )
}

/// SQL metadata comparison only, not archive/source authentication or a grant.
/// The outcome preflight independently verifies the exact receipt's immutable
/// selection outside SQL, then rechecks these rows in its fresh snapshot.
pub(in crate::session_migration) fn find_duplicate_metadata(
    conn: &Connection,
    item: &CommitItem,
    existing: Option<&CatalogEntry>,
    snapshot_id: &str,
    semantic_key: &str,
    options_digest: &str,
    selection: &SelectionSet,
) -> Result<Option<String>, ImportError> {
    find_duplicate_using(
        conn,
        item,
        existing,
        snapshot_id,
        semantic_key,
        options_digest,
        |_, _| Ok(selection.clone()),
    )
}

fn find_duplicate_using(
    conn: &Connection,
    item: &CommitItem,
    existing: Option<&CatalogEntry>,
    snapshot_id: &str,
    semantic_key: &str,
    options_digest: &str,
    selection: impl FnOnce(&CatalogEntry, &str) -> Result<SelectionSet, ImportError>,
) -> Result<Option<String>, ImportError> {
    let Some(entry) = existing else {
        return Ok(None);
    };
    if let Some(revision) =
        find_revision_by_snapshot(conn, &entry.import_id, snapshot_id, semantic_key)?
    {
        return Ok(Some(revision));
    }
    // Recognize the current legacy history-only key after authenticating its
    // receipt and immutable files. Do not rewrite the old archive or receipt.
    // Legacy project decisions are not fully encoded in the old key; those
    // imports receive a new revision rather than guessing their original intent.
    if item.mapping_project_id.is_some()
        || item.create_project
        || item.import_unassigned_anyway
        || item.explicit_target.is_some()
    {
        return Ok(None);
    }
    let Some(revision) = entry.current_revision.as_deref() else {
        return Ok(None);
    };
    let stored: Option<String> = conn.query_row(
        "SELECT options_hash FROM revisions WHERE import_id=?1 AND revision_id=?2 AND source_snapshot_id=?3 AND normalizer_version=?4",
        params![entry.import_id, revision, snapshot_id, super::super::types::NORMALIZER_VERSION],
        |row| row.get(0),
    ).optional().map_err(sqlite_storage_error)?;
    let Some(stored) = stored else {
        return Ok(None);
    };
    let receipt =
        latest_receipt_for_revision(conn, &entry.import_id, revision)?.ok_or_else(|| {
            ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "published receipt missing",
            )
        })?;
    if receipt.options_hash != options_digest
        || receipt.mapping_project_id.is_some()
        || receipt.created_project
        || receipt.project_decision.as_deref() != Some("unassigned")
    {
        return Ok(None);
    }
    let selection = selection(entry, revision)?;
    if stored != revision_semantic_key(options_digest, &selection.hash())
        || key(options_digest, &selection, item)? != semantic_key
    {
        return Ok(None);
    }
    Ok(Some(revision.to_string()))
}
