use super::*;
#[path = "jobs_copy.rs"]
mod jobs_copy;
#[cfg(test)]
use crate::session_migration::publish_timing::{span as timing_span, Stage as TimingStage};
use jobs_copy::copy_dir;

#[cfg(test)]
use super::tests::maybe_pause_publish;

#[path = "jobs_publish_item.rs"]
mod jobs_publish_item;
#[cfg(test)]
pub use jobs_publish_item::publish_item;
pub(in crate::session_migration) use jobs_publish_item::{publish_for_worker, ItemProgress};

/// Display metadata may improve without rewriting an immutable archive or
/// creating another revision. Never replace an alias or a newer revision's name.
pub(in crate::session_migration) fn repair_fallback_title(
    conn: &Connection,
    item: &CommitItem,
    import_id: &str,
    revision_id: &str,
) -> Result<(), ImportError> {
    if !matches!(item.provider.as_str(), "codex" | "claude-code" | "cursor") {
        return Ok(());
    }
    let Some(title) =
        item.adapter.graph.title.as_deref().filter(|title| {
            !title.trim().is_empty() && *title != item.adapter.graph.native_session_id
        })
    else {
        return Ok(());
    };
    conn.execute(
        "UPDATE entries SET title=?1 WHERE import_id=?2 AND current_revision=?3 AND title=native_session_id",
        params![title, import_id, revision_id],
    ).map_err(sqlite_storage_error)?;
    Ok(())
}

pub fn latest_receipt_for_revision(
    conn: &Connection,
    import_id: &str,
    revision_id: &str,
) -> Result<Option<Receipt>, ImportError> {
    let json: Option<String> = conn
        .query_row(
            "SELECT json FROM receipts WHERE import_id=?1 AND revision_id=?2 ORDER BY rowid DESC LIMIT 1",
            params![import_id, revision_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    json.map(|j| {
        let receipt: Receipt = serde_json::from_str(&j).map_err(|e| {
            ImportError::new(
                ErrorCode::SchemaUnsupported,
                format!("unsupported legacy or invalid durable receipt; create a new preview: {e}"),
            )
        })?;
        validate_receipt_shape(&receipt)?;
        Ok(receipt)
    })
    .transpose()
}

pub fn latest_receipt(
    conn: &Connection,
    job_id: &str,
    candidate_id: &str,
) -> Result<Option<Receipt>, ImportError> {
    let json: Option<String> = conn.query_row(
        "SELECT json FROM receipts WHERE job_id=?1 AND candidate_id=?2 ORDER BY rowid DESC LIMIT 1",
        params![job_id, candidate_id], |r| r.get(0),
    ).optional().map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    json.map(|j| {
        let receipt: Receipt = serde_json::from_str(&j).map_err(|e| {
            ImportError::new(
                ErrorCode::SchemaUnsupported,
                format!("unsupported legacy or invalid durable receipt; create a new preview: {e}"),
            )
        })?;
        validate_receipt_shape(&receipt)?;
        Ok(receipt)
    })
    .transpose()
}

/// Restore a job result from an already committed receipt after its source
/// preview has been released. The durable input is matched by the caller; this
/// side verifies the catalog identity and every immutable published file.
pub(crate) fn recover_committed_receipt(
    conn: &Connection,
    receipt: &Receipt,
) -> Result<JobItem, ImportError> {
    validate_receipt_shape(receipt)?;
    if receipt.phase != "committed" {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "receipt is not committed",
        ));
    }
    let import_id = receipt.import_id.clone().ok_or_else(|| {
        ImportError::new(ErrorCode::SnapshotUnavailable, "receipt has no import ID")
    })?;
    let revision_id = receipt.revision_id.clone().ok_or_else(|| {
        ImportError::new(ErrorCode::SnapshotUnavailable, "receipt has no revision ID")
    })?;
    let entry = crate::session_migration::catalog::get_entry(conn, &import_id)?
        .ok_or_else(|| ImportError::new(ErrorCode::SnapshotUnavailable, "import not in catalog"))?;
    let revision = crate::session_migration::catalog::get_revision(conn, &import_id, &revision_id)?
        .ok_or_else(|| {
            ImportError::new(ErrorCode::SnapshotUnavailable, "revision not in catalog")
        })?;
    if entry.hidden || revision.source_snapshot_id != receipt.snapshot_hash {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "catalog, revision, and committed receipt identity do not match",
        ));
    }
    crate::session_migration::registration_provenance::authorize(conn, &entry, receipt)?;
    let dir = revision_dir(&import_id, &revision_id)?;
    verify_published_for_read(&dir, receipt)?;
    crate::session_migration::search::ensure_body_search_projection(conn, &dir)?;
    Ok(JobItem {
        candidate_id: receipt.candidate_id.clone(),
        title: Some(entry.title),
        phase: "committed".into(),
        import_id: Some(import_id),
        revision_id: Some(revision_id),
        outcome: Some(revision.outcome),
        error: None,
        mapping_project_id: receipt.mapping_project_id.clone(),
    })
}

pub(super) fn persist_receipt(conn: &Connection, rec: &mut Receipt) -> Result<(), ImportError> {
    rec.receipt_id = Uuid::new_v4().to_string();
    write_receipt(conn, rec)
}

pub(super) fn advance_receipt(
    conn: &Connection,
    rec: &mut Receipt,
    phase: &str,
) -> Result<(), ImportError> {
    rec.phase = phase.into();
    rec.receipt_id = Uuid::new_v4().to_string();
    write_receipt(conn, rec)
}

pub(crate) fn prepared_hashes(
    dir: &Path,
) -> Result<std::collections::BTreeMap<String, String>, ImportError> {
    #[cfg(test)]
    let _span = crate::session_migration::codec_profile::Span::start("archive-prepared-hashes", 0);
    // The SQLite index is a rebuildable projection, not immutable source data.
    let mut hashes = std::collections::BTreeMap::new();
    for name in ["manifest.json", "events.jsonl", "assets.json"] {
        let path = dir.join(name);
        crate::session_migration::catalog::ensure_inside_imports(&path)?;
        if !fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink()) {
            return Err(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "revision file missing or not regular",
            ));
        }
        hashes.insert(
            name.into(),
            crate::session_migration::hash::sha256_file(&path)
                .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e))?,
        );
    }
    let blobs = dir.join("blobs");
    if blobs.exists() {
        let metadata = fs::symlink_metadata(&blobs)
            .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "revision blobs path is not a regular directory",
            ));
        }
        let mut entries = Vec::new();
        for entry in fs::read_dir(&blobs)
            .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?
        {
            let entry = entry.map_err(|error| {
                ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
            })?;
            let file_type = entry.file_type().map_err(|error| {
                ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
            })?;
            if !file_type.is_file() || file_type.is_symlink() {
                return Err(ImportError::new(
                    ErrorCode::SnapshotUnavailable,
                    "revision blob is not a regular file",
                ));
            }
            entries.push(entry.file_name());
        }
        entries.sort();
        for name in entries {
            let path = blobs.join(&name);
            crate::session_migration::catalog::ensure_inside_imports(&path)?;
            hashes.insert(
                format!("blobs/{}", name.to_string_lossy()),
                crate::session_migration::hash::sha256_file(&path)
                    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e))?,
            );
        }
    }
    Ok(hashes)
}

pub(crate) fn published_hashes(
    dir: &Path,
) -> Result<std::collections::BTreeMap<String, String>, ImportError> {
    let mut hashes = prepared_hashes(dir)?;
    let selection_path = dir.join("selection.json");
    crate::session_migration::catalog::ensure_inside_imports(&selection_path)?;
    let metadata = fs::symlink_metadata(&selection_path)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "published selection is missing or not regular",
        ));
    }
    hashes.insert(
        "selection.json".into(),
        crate::session_migration::hash::sha256_file(&selection_path)
            .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error))?,
    );
    Ok(hashes)
}

pub(crate) fn verify_published(dir: &Path, rec: &Receipt) -> Result<SelectionSet, ImportError> {
    validate_receipt_shape(rec)?;
    if rec.file_hashes.is_empty() || published_hashes(dir)? != rec.file_hashes {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "published revision does not match its receipt",
        ));
    }
    let raw = fs::read(dir.join("selection.json"))
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    let value: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    let selection_set = parse_live_selection_set(&value)?;
    let entry = selection_set.single_entry()?;
    if selection_set != rec.selection_set.canonicalized()
        || selection_set.hash() != rec.selection_hash
    {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "published selection does not match its receipt",
        ));
    }
    let source_member = validate_published_selection(
        entry,
        dir,
        PreparedSelectionIdentity {
            preview_id: Some(&entry.preview_id),
            snapshot_digest: Some(&rec.snapshot_hash),
            provider: Some(&rec.provider),
            native_session_id: Some(&rec.native_session_id),
            ..PreparedSelectionIdentity::default()
        },
    )?;
    if source_member != rec.source_member {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "published source member does not match its receipt",
        ));
    }
    Ok(selection_set)
}

pub(super) fn outcome_str(o: ImportOutcome) -> &'static str {
    match o {
        ImportOutcome::Complete => "complete",
        ImportOutcome::Partial => "partial",
        ImportOutcome::Unsupported => "unsupported",
        ImportOutcome::Failed => "failed",
    }
}

pub(crate) fn write_revision_metadata_files(
    dir: &Path,
    adapter: &AdapterOutput,
    snapshot_id: &str,
) -> Result<(), ImportError> {
    let manifest = serde_json::json!({
        "schemaVersion": crate::session_migration::types::SCHEMA_VERSION,
        "normalizerVersion": crate::session_migration::types::NORMALIZER_VERSION,
        "sourceSnapshotId": snapshot_id,
        "outcome": outcome_str(adapter.outcome),
        "inventory": adapter.inventory,
        "issues": adapter.issues,
        "evidence": adapter.evidence,
        "graph": {
            "nativeSessionId": adapter.graph.native_session_id,
            "title": adapter.graph.title,
            "cwd": adapter.graph.cwd,
            "activeLeaf": adapter.graph.active_leaf,
            "relations": adapter.graph.relations,
        }
    });
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    fs::write(dir.join("manifest.json"), manifest_bytes).map_err(io_storage_error)?;
    fs::OpenOptions::new()
        .write(true)
        .open(dir.join("manifest.json"))
        .and_then(|file| file.sync_all())
        .map_err(io_storage_error)?;
    let asset_bytes = serde_json::to_vec_pretty(&adapter.assets)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    fs::write(dir.join("assets.json"), asset_bytes).map_err(io_storage_error)?;
    fs::OpenOptions::new()
        .write(true)
        .open(dir.join("assets.json"))
        .and_then(|file| file.sync_all())
        .map_err(io_storage_error)?;
    Ok(())
}

/// Undo entries this job created. Does not resurrect tombstones or delete
/// revisions created by a later job.
#[cfg(test)]
pub fn undo_job(conn: &Connection, job_id: &str) -> Result<JobRecord, ImportError> {
    let crypto = crate::session_migration::publication_crypto::JobCrypto::prepare(conn, job_id)?;
    undo_job_inner(conn, job_id, &crypto, None)
}

pub(in crate::session_migration) fn undo_job_action(
    conn: &Connection,
    job_id: &str,
    action: &crate::session_migration::job_actions::PreparedAction,
) -> Result<JobRecord, ImportError> {
    undo_job_inner(conn, job_id, action.crypto(), Some(action))
}

/// Startup resumes only an already-recorded cleanup; it cannot start Undo.
pub(in crate::session_migration) fn resume_undo_action(
    conn: &Connection,
    action: &crate::session_migration::job_actions::PreparedAction,
) -> Result<JobRecord, ImportError> {
    let _pub = PUBLISH_LOCK
        .lock()
        .map_err(|_| ImportError::new(ErrorCode::SnapshotUnavailable, "publish lock poisoned"))?;
    let _file_lock = crate::session_migration::job_control::ImportLock::acquire("publish.lock")?;
    crate::session_migration::job_actions::undo::resume(conn, action)
}

fn undo_job_inner(
    conn: &Connection,
    job_id: &str,
    crypto: &crate::session_migration::publication_crypto::JobCrypto,
    action: Option<&crate::session_migration::job_actions::PreparedAction>,
) -> Result<JobRecord, ImportError> {
    use crate::session_migration::catalog::{get_entry, hide_entry, insert_tombstone};
    let _pub = PUBLISH_LOCK
        .lock()
        .map_err(|_| ImportError::new(ErrorCode::SnapshotUnavailable, "publish lock poisoned"))?;
    let _file_lock = crate::session_migration::job_control::ImportLock::acquire("publish.lock")?;
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let (job, entry_auth) = super::get_job_for_writer(&tx, job_id, crypto)?;
    let mut job =
        job.ok_or_else(|| ImportError::new(ErrorCode::SourceNotFound, "job not found"))?;
    if let Some(action) = action {
        if action.replay(&tx, &job)? {
            drop(entry_auth);
            drop(tx);
            return crate::session_migration::job_actions::undo::resume(conn, action);
        }
    }
    if !crate::session_migration::job_control::terminal(job.state) {
        return Err(ImportError::new(
            ErrorCode::SourceBusy,
            "cancel or finish the job before undo",
        ));
    }
    let prepared_undo = entry_auth.undo_receipts(&tx, &job)?;
    let changes_before = tx.total_changes();
    let mut expected_changes = super::item_store::save_changes(&tx, &job)?;
    let mut created_links: Vec<Receipt> = Vec::new();
    for (item, receipt) in job.items.iter_mut().zip(prepared_undo.receipts) {
        if item.phase != "committed" || item.outcome.as_deref() == Some("deduplicated") {
            continue;
        }
        let Some(import_id) = item.import_id.as_ref() else {
            continue;
        };
        let Some(entry) = get_entry(&tx, import_id)? else {
            continue;
        };
        let Some(receipt) = receipt.read(&tx, job_id, &item.candidate_id)? else {
            item.phase = "conflict".into();
            item.error = Some("ownership receipt missing".into());
            continue;
        };
        if is_tombstoned(
            &tx,
            &entry.installation_id,
            &entry.provider,
            &entry.native_session_id,
        )? {
            item.phase = "undone".into();
            continue;
        }
        if entry.current_revision != item.revision_id {
            item.phase = "conflict".into();
            item.error = Some("later revision kept".into());
            continue;
        }
        // Shared ordinary/association result checks remain inside the same
        // writer and precede every target or post-COMMIT project side effect.
        super::undo_ownership::require_target(&tx, job_id, item, &entry, &receipt)?;
        if let Some(previous) = receipt.previous_revision.clone() {
            // Restoring the previous display revision does not remove the newer
            // immutable snapshot; existing continuation references remain valid.
            tx.execute(
                "UPDATE entries SET current_revision=?1 WHERE import_id=?2",
                params![previous, import_id],
            )
            .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
            expected_changes += 1;
        } else if receipt.created_entry {
            let refs =
                crate::session_migration::continuation::list_refs_for_import(&tx, import_id)?;
            if !refs.is_empty() {
                item.phase = "conflict".into();
                item.error = Some(
                    ImportError::new(
                        ErrorCode::MappingConflict,
                        "continuation sessions still reference this import",
                    )
                    .into_command_err(),
                );
                continue;
            }
            // hide_entry revokes the saved workspace association (one row,
            // plus one schema-version row on its first use), then hides entry.
            expected_changes += 3 + u64::from(
                !crate::session_migration::workspace_association_store::available(&tx)?,
            );
            hide_entry(&tx, import_id)?;
            insert_tombstone(
                &tx,
                &entry.installation_id,
                &entry.provider,
                &entry.native_session_id,
            )?;
        } else {
            item.phase = "conflict".into();
            item.error = Some("previous revision is unknown".into());
            continue;
        }
        item.phase = "undone".into();
        if receipt.created_project {
            created_links.push(receipt);
        }
    }
    job.state_version = job.state_version.checked_add(1).ok_or_else(|| {
        ImportError::new(ErrorCode::SnapshotUnavailable, "job state version overflow")
    })?;
    save_job(&tx, &job)?;
    let creations = match prepared_undo.signed {
        Some(signed) => signed.require_after(&tx, &job)?,
        None => vec![],
    };
    if let Some(action) = action {
        crate::session_migration::job_actions::undo::record(
            &tx,
            action,
            &job,
            &created_links,
            changes_before,
            expected_changes,
        )?;
    }
    #[cfg(test)]
    crate::session_migration::preview_lifecycle_tests::after_publish_point("before_undo_commit");
    tx.commit()
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    #[cfg(test)]
    crate::session_migration::preview_lifecycle_tests::after_publish_point("after_undo_commit");
    if let Some(action) = action {
        crate::session_migration::job_control::changed_version(job_id, job.state_version);
        // The content is already committed even if later native cleanup or its
        // completion record fails. Catalog views must refresh on that path too.
        crate::session_migration::job_control::workspace_changed();
        return crate::session_migration::job_actions::undo::resume(conn, action);
    }
    let mut project_notes = Vec::new();
    for rec in created_links {
        if !job
            .items
            .iter()
            .any(|i| i.candidate_id == rec.candidate_id && i.phase == "undone")
        {
            continue;
        }
        // Legacy mutable results cannot prove native creation. Keep that
        // project with an explicit conflict; content Undo remains available.
        let result = match creations
            .iter()
            .find(|(candidate, _)| candidate == &rec.candidate_id)
        {
            Some((_, allocated)) => crypto
                .require(job_id, crate::session_migration::publication_protocol::PublicationProtocol::SignedOutcomeV1)
                .and_then(|auth| auth.ok_or_else(|| ImportError::new(ErrorCode::HistoryAccessUnavailable, "native cleanup crypto unavailable")))
                .and_then(|auth| allocated.project_creation(auth))
                .and_then(|creation| crate::session_migration::project_link::undo_created_project(conn, &rec, &creation)),
            None => Err(ImportError::new(
                ErrorCode::MappingConflict,
                "project creation provenance unavailable; project kept",
            )),
        };
        if let Err(e) = result {
            project_notes.push((rec.candidate_id.clone(), e.into_command_err()));
        }
    }
    if !project_notes.is_empty() {
        for (cid, err) in project_notes {
            if let Some(item) = job.items.iter_mut().find(|i| i.candidate_id == cid) {
                item.error = Some(err);
            }
        }
        job.state_version = job.state_version.checked_add(1).ok_or_else(|| {
            ImportError::new(ErrorCode::SnapshotUnavailable, "job state version overflow")
        })?;
        save_job(conn, &job)?;
    }
    Ok(job)
}

/// Hide an import and write a tombstone. Does not delete project directories
/// or immutable revision snapshots. Continuation refs block deletion.
pub fn delete_imported(conn: &Connection, import_id: &str) -> Result<(), ImportError> {
    use crate::session_migration::catalog::{get_entry, hide_entry, insert_tombstone};
    #[cfg(test)]
    crate::session_migration::preview_lifecycle_tests::after_publish_point("before_delete_lock");
    let _pub = PUBLISH_LOCK
        .lock()
        .map_err(|_| ImportError::new(ErrorCode::SnapshotUnavailable, "publish lock poisoned"))?;
    let _file_lock = crate::session_migration::job_control::ImportLock::acquire("publish.lock")?;
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let conn = &tx;
    let entry = get_entry(conn, import_id)?
        .ok_or_else(|| ImportError::new(ErrorCode::SourceNotFound, "import not found"))?;
    let refs = crate::session_migration::continuation::list_refs_for_import(conn, import_id)?;
    if !refs.is_empty() {
        return Err(ImportError::new(
            ErrorCode::MappingConflict,
            "continuation sessions still reference this import",
        ));
    }
    hide_entry(conn, import_id)?;
    insert_tombstone(
        conn,
        &entry.installation_id,
        &entry.provider,
        &entry.native_session_id,
    )?;
    tx.commit()
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    Ok(())
}

pub(super) fn restore_explicit_reimport(
    conn: &Connection,
    item: &CommitItem,
    import_id: &str,
    revision_id: &str,
) -> Result<(), ImportError> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    crate::session_migration::catalog::clear_tombstone(
        &tx,
        &item.installation_id,
        &item.provider,
        &item.adapter.graph.native_session_id,
    )?;
    tx.execute(
        "UPDATE entries SET hidden=0, current_revision=?1 WHERE import_id=?2",
        params![revision_id, import_id],
    )
    .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    tx.commit()
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    Ok(())
}

/// Recover authorized durable jobs; never rescan the external applications.
#[cfg(test)]
pub fn recover_incomplete(_conn: &Connection) -> Result<u32, ImportError> {
    crate::session_migration::job_control::run_pending()
}

#[cfg(test)]
pub fn prepare_test_commit_item(mut item: CommitItem) -> CommitItem {
    let leaf_id = item
        .adapter
        .graph
        .active_leaf
        .clone()
        .or_else(|| {
            item.adapter
                .graph
                .events
                .last()
                .map(|event| event.id().to_string())
        })
        .expect("test commit item needs a real leaf");
    let preview_id = Uuid::new_v4().to_string();
    if item.selection_set.entries.is_empty() {
        let selection = crate::session_migration::continuation::default_selection(
            &item.adapter.graph.events,
            &item.adapter.graph.relations,
            &leaf_id,
        );
        item.selection_set =
            SelectionSet::single(crate::session_migration::selection::PreviewSelection {
                preview_id: preview_id.clone(),
                leaf_id,
                selection,
            });
    } else {
        let mut selection = item
            .selection_set
            .single_entry()
            .expect("custom test selection")
            .clone();
        selection.preview_id = preview_id.clone();
        item.selection_set = SelectionSet::single(selection);
    }
    let mut prepared_adapter = item.adapter.clone();
    let prepared = prepare_revision(
        &mut prepared_adapter,
        &item.options,
        None,
        Some(&preview_id),
    )
    .expect("prepare test revision");
    write_preview_lease(
        &prepared.path,
        &PreviewLease {
            source_member: None,
            preview_id,
            candidate_id: item.candidate_id.clone(),
            source_revision: "test-source-revision".into(),
            prepare_batch_id: None,
            source_id: format!("test-source:{}", item.installation_id),
            root_path: item.root_path.clone(),
            legacy_id: format!("test-legacy:{}", item.candidate_id),
            snapshot_digest: prepared.digest.clone(),
            expires_unix: chrono::Utc::now().timestamp() + 3600,
            retained_by_job: false,
            file_hashes: prepared.file_hashes.clone(),
        },
    )
    .expect("write test preview lease");
    item.adapter = prepared_adapter;
    item.prepared = Some(prepared);
    item
}
