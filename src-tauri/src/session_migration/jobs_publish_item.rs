use super::*;

/// Internal worker result; a recorded ACK must not be repeated after commit.
pub(in crate::session_migration) struct ItemProgress {
    item: JobItem,
    recorded_version: Option<u32>,
}

impl ItemProgress {
    pub(in crate::session_migration) fn pending(item: JobItem) -> Self {
        Self {
            item,
            recorded_version: None,
        }
    }

    /// Only a publisher or independently authenticated durable outcome reader
    /// may construct this after validating an already committed exact ACK.
    pub(in crate::session_migration) fn recorded(item: JobItem, version: u32) -> Self {
        Self {
            item,
            recorded_version: Some(version),
        }
    }

    /// The worker consumes this once; verified signed replays retain their
    /// already committed identity instead of repeating the progress setter.
    /// Legacy recovery and early outcomes still use the normal setter.
    pub(in crate::session_migration) fn finish(
        self,
        conn: &Connection,
        job_id: &str,
        index: usize,
        crypto: &crate::session_migration::publication_crypto::JobCrypto,
    ) -> Result<(), ImportError> {
        if self.recorded_version.is_none() {
            crate::session_migration::job_progress::set_item_with_crypto(
                conn, job_id, index, &self.item, crypto,
            )?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(in crate::session_migration) fn expect_recorded(self) -> JobItem {
        assert!(
            self.recorded_version.is_some(),
            "final publication missed its ACK"
        );
        self.item
    }
}

/// Publish one normalized graph into the import archive. Idempotent on
/// (installation, provider, native id, snapshot, options).
#[cfg(test)]
pub fn publish_item(
    conn: &Connection,
    job_id: &str,
    item: &CommitItem,
    cancel: &dyn Fn() -> bool,
) -> Result<JobItem, ImportError> {
    crate::session_migration::publication_protocol::PublicationProtocol::for_job_id(job_id)?;
    let crypto = if conn.is_autocommit() {
        crate::session_migration::publication_crypto::JobCrypto::prepare(conn, job_id)?
    } else {
        // Fault probes already inside SQL retain legacy metadata refusals;
        // they cannot load signed crypto or acquire a new writer grant here.
        crate::session_migration::publication_crypto::JobCrypto::legacy_snapshot(conn, job_id)?
    };
    publish(conn, job_id, item, cancel, None, &crypto).map(|progress| progress.item)
}

pub(in crate::session_migration) fn publish_for_worker(
    conn: &Connection,
    job_id: &str,
    index: usize,
    item: &CommitItem,
    cancel: &dyn Fn() -> bool,
    crypto: &crate::session_migration::publication_crypto::JobCrypto,
) -> Result<ItemProgress, ImportError> {
    #[cfg(test)]
    let _span = crate::session_migration::codec_profile::Span::start("worker-publish-item", 0);
    publish(conn, job_id, item, cancel, Some(index), crypto)
}

fn publish(
    conn: &Connection,
    job_id: &str,
    item: &CommitItem,
    cancel: &dyn Fn() -> bool,
    progress_slot: Option<usize>,
    crypto: &crate::session_migration::publication_crypto::JobCrypto,
) -> Result<ItemProgress, ImportError> {
    // Sequential phase spans are disjoint; lower-level spans remain inclusive.
    // Every guard is test-only and unwinds on the original error/return path.
    #[cfg(test)]
    let phase = crate::session_migration::codec_profile::Span::start("publication-preflight", 0);
    let protocol =
        crate::session_migration::publication_protocol::PublicationProtocol::for_job_id(job_id)?;
    crypto.require(job_id, protocol)?;
    crate::session_migration::identity_association::reject_behavior_options(&item.options)?;
    let _operation = crate::session_migration::preview_lifecycle::operation()?;
    let _pub = PUBLISH_LOCK
        .lock()
        .map_err(|_| ImportError::new(ErrorCode::SnapshotUnavailable, "publish lock poisoned"))?;
    let _file_lock = crate::session_migration::job_control::ImportLock::acquire("publish.lock")?;
    let prepared = item.prepared.as_ref().ok_or_else(|| {
        ImportError::new(
            ErrorCode::PlanStale,
            "commit item has no immutable prepared preview",
        )
    })?;
    item.selection_set.validate_shape()?;
    let raw_selection = item.selection_set.single_entry()?;
    let verified = validate_prepared_selection(
        raw_selection,
        &prepared.path,
        PreparedSelectionIdentity {
            preview_id: Some(&raw_selection.preview_id),
            candidate_id: Some(&item.candidate_id),
            root_path: Some(&item.root_path),
            snapshot_digest: Some(&prepared.digest),
            provider: Some(&item.provider),
            native_session_id: Some(&item.adapter.graph.native_session_id),
            ..PreparedSelectionIdentity::default()
        },
    )?;
    let actual_prepared_hashes = prepared_hashes(&prepared.path)?;
    if actual_prepared_hashes != prepared.file_hashes {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "fixed preview was modified",
        ));
    }
    #[cfg(test)]
    drop(phase);
    #[cfg(test)]
    let phase = crate::session_migration::codec_profile::Span::start("publication-identity", 0);
    let association = crate::session_migration::job_control::association_publication::AcceptedPublication::for_item(conn, job_id, item)?;
    let existing_for = |prior: Option<&Receipt>| match &association {
        Some(accepted) => accepted.target(conn, prior).map(Some),
        None => find_entry(
            conn,
            &item.installation_id,
            &item.provider,
            &item.adapter.graph.native_session_id,
        ),
    };
    // Preserve legacy receipt/target validation order and its error semantics.
    // The signed path, in contrast, authenticates before reading its receipts.
    let legacy =
        if crate::session_migration::publication_protocol::PublicationProtocol::for_job_id(job_id)?
            == crate::session_migration::publication_protocol::PublicationProtocol::Legacy
        {
            let prior =
                crate::session_migration::job_control::association_publication::latest_receipt(
                    conn,
                    job_id,
                    &item.candidate_id,
                    association.as_ref(),
                )?;
            let existing = existing_for(prior.as_ref())?;
            Some((prior, existing))
        } else {
            None
        };
    let publication_owner =
        crate::session_migration::job_control::provenance_scope::publication::for_item(
            conn, job_id, item, crypto,
        )?;
    let signed = crate::session_migration::publication_outcome::SignedPublication::capture(
        conn,
        job_id,
        item,
        &verified,
        crypto,
        &publication_owner,
    )?;
    if let Some(signed) = &signed {
        if let Some((item, version)) =
            signed.replay_deduplicated(conn, &publication_owner, progress_slot)?
        {
            return Ok(ItemProgress {
                item,
                recorded_version: Some(version),
            });
        }
    }
    let (prior, existing) = match (&signed, legacy) {
        (Some(signed), None) => {
            let prior = signed.prior(conn, &publication_owner)?;
            let existing = existing_for(prior.as_ref())?;
            (prior, existing)
        }
        (None, Some(legacy)) => legacy,
        _ => {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "publication protocol changed during preflight",
            ))
        }
    };
    #[cfg(test)]
    drop(phase);
    #[cfg(test)]
    let phase = crate::session_migration::codec_profile::Span::start("publication-resolution", 0);
    let selection_set = item.selection_set.canonicalized();
    let selection_hash = selection_set.hash();
    let estimated = estimated_publish_bytes(item);
    let probe = imports_dir();
    fs::create_dir_all(&probe).map_err(io_storage_error)?;
    let available = fs2::available_space(&probe).map_err(|e| {
        let error = io_storage_error(e);
        ImportError::new(
            error.code,
            format!("disk space {}: {}", probe.display(), error.message),
        )
    })?;
    require_disk_space(available, estimated)?;
    let result = |phase: &str, import_id, revision_id, outcome: &str| {
        ItemProgress::pending(JobItem {
            candidate_id: item.candidate_id.clone(),
            title: Some(
                item.adapter
                    .graph
                    .title
                    .clone()
                    .unwrap_or_else(|| item.adapter.graph.native_session_id.clone()),
            ),
            phase: phase.into(),
            import_id,
            revision_id,
            outcome: Some(outcome.into()),
            error: None,
            mapping_project_id: item.mapping_project_id.clone(),
        })
    };
    let native = &item.adapter.graph.native_session_id;
    let tombstoned = is_tombstoned(conn, &item.installation_id, &item.provider, native)?;
    if tombstoned && !item.explicit_reimport {
        return Ok(result("tombstoned", None, None, "tombstoned"));
    }
    let opt_hash = options_hash(&item.options);
    let semantic_key = revision_identity::key(&opt_hash, &selection_set, item)?;
    let snapshot_id = verified.snapshot_digest.clone();
    if let Some(rec) = &prior {
        if rec.job_id != job_id {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "receipt job identity changed",
            ));
        }
        validate_receipt_for_item(
            rec,
            item,
            &selection_set,
            &selection_hash,
            &opt_hash,
            &verified,
        )?;
    }
    // Recover the same preallocated IDs, including a rename whose next receipt
    // was never written. Do not deduplicate away this job's ownership information.
    #[cfg(test)]
    drop(phase);
    #[cfg(test)]
    let phase = crate::session_migration::codec_profile::Span::start("publication-receipt", 0);
    let mut rec = if let Some(rec) = prior {
        rec
    } else {
        if cancel() {
            return Ok(result("cancelled", None, None, "cancelled"));
        }
        let allocation = signed
            .as_ref()
            .map(|signed| signed.allocation())
            .transpose()?;
        let import_id = match allocation {
            Some(allocation) => allocation.import_id.clone(),
            None => existing
                .as_ref()
                .map(|e| e.import_id.clone())
                .unwrap_or_else(new_import_id),
        };
        // A signed publisher's terminal ACK is no longer committed after Undo.
        // An explicit restoration publishes its own accepted source into fresh
        // allocated IDs instead of weakening prior-owner/ACK authentication or
        // resurrecting the old job's project-creation authority. Legacy archive
        // reuse keeps its historical behavior; the final writer rechecks CAS
        // and the tombstone before atomically restoring the catalog entry.
        let duplicate = if association.is_some()
            || (signed.is_some() && item.explicit_reimport && tombstoned)
        {
            None
        } else {
            revision_identity::find_duplicate(
                conn,
                item,
                existing.as_ref(),
                &snapshot_id,
                &semantic_key,
                &opt_hash,
            )?
        };
        if let Some(rev) = duplicate {
            let Some(entry) = existing.as_ref() else {
                return Err(ImportError::new(
                    ErrorCode::SnapshotUnavailable,
                    "deduplicated revision has no catalog entry",
                ));
            };
            if let Some(signed) = &signed {
                let (item, version) =
                    signed.deduplicate(conn, &publication_owner, entry, &rev, progress_slot)?;
                return Ok(ItemProgress {
                    item,
                    recorded_version: Some(version),
                });
            }
            let published =
                super::published_auth_from_entry(conn, entry, &import_id, Some(&rev), None)?;
            if revision_identity::key(&opt_hash, &published.selection_set, item)? != semantic_key {
                return Err(ImportError::new(
                    ErrorCode::PlanStale,
                    "deduplicated revision selection does not match",
                ));
            }
            let published_dir = revision_dir(&import_id, &rev)?;
            crate::session_migration::search::ensure_body_search_projection(conn, &published_dir)?;
            if item.explicit_reimport {
                restore_explicit_reimport(conn, item, &import_id, &rev)?;
            }
            repair_fallback_title(conn, item, &import_id, &rev)?;
            return Ok(result(
                "committed",
                Some(import_id),
                Some(rev),
                "deduplicated",
            ));
        }
        let mut rec = Receipt {
            version: DURABLE_SCHEMA_VERSION,
            source_member: verified.source_member.clone(),
            receipt_id: allocation.map_or_else(
                || Uuid::new_v4().to_string(),
                |allocation| allocation.receipt_id.clone(),
            ),
            job_id: job_id.into(),
            candidate_id: item.candidate_id.clone(),
            installation_id: item.installation_id.clone(),
            provider: item.provider.clone(),
            native_session_id: item.adapter.graph.native_session_id.clone(),
            source_id: verified.source_id.clone(),
            root_path: verified.root_path.clone(),
            legacy_id: verified.legacy_id.clone(),
            phase: "prepared".into(),
            import_id: Some(import_id),
            revision_id: Some(allocation.map_or_else(
                || Uuid::new_v4().to_string(),
                |allocation| allocation.revision_id.clone(),
            )),
            snapshot_hash: snapshot_id.clone(),
            options_hash: opt_hash.clone(),
            selection_set: selection_set.clone(),
            selection_hash: selection_hash.clone(),
            previous_revision: allocation.map_or_else(
                || existing.as_ref().and_then(|e| e.current_revision.clone()),
                |allocation| allocation.previous_revision.clone(),
            ),
            created_entry: allocation
                .map_or_else(|| existing.is_none(), |allocation| allocation.created_entry),
            file_hashes: Default::default(),
            mapping_project_id: item.mapping_project_id.clone(),
            created_project: false,
            project_decision: None,
            project_name_at_link: None,
            project_path_at_link: None,
            project_pinned_at_link: None,
            project_trusted_at_link: None,
        };
        match &signed {
            Some(signed) => signed.prepare(conn, &publication_owner, &mut rec)?,
            None => write_receipt(conn, &rec)
                .map_err(|e| ImportError::new(e.code, format!("prepared receipt: {e}")))?,
        }
        #[cfg(test)]
        maybe_pause_publish("prepared");
        rec
    };
    #[cfg(test)]
    drop(phase);
    #[cfg(test)]
    let phase = crate::session_migration::codec_profile::Span::start("publication-archive", 0);
    let import_id = rec.import_id.clone().ok_or_else(|| {
        ImportError::new(ErrorCode::SnapshotUnavailable, "receipt has no import ID")
    })?;
    let revision_id = rec.revision_id.clone().ok_or_else(|| {
        ImportError::new(ErrorCode::SnapshotUnavailable, "receipt has no revision ID")
    })?;
    let dest = revision_dir(&import_id, &revision_id)?;
    if rec.phase == "committed" {
        verify_published_for_read(&dest, &rec)?;
        crate::session_migration::search::ensure_body_search_projection(conn, &dest)?;
        let committed = JobItem {
            candidate_id: item.candidate_id.clone(),
            title: Some(
                item.adapter
                    .graph
                    .title
                    .clone()
                    .unwrap_or_else(|| item.adapter.graph.native_session_id.clone()),
            ),
            phase: "committed".into(),
            import_id: Some(import_id),
            revision_id: Some(revision_id),
            outcome: Some(outcome_str(item.adapter.outcome).into()),
            error: None,
            mapping_project_id: rec.mapping_project_id.clone(),
        };
        let recorded_version = if let Some(signed) = &signed {
            let index = progress_slot.ok_or_else(|| {
                ImportError::new(
                    ErrorCode::PlanStale,
                    "signed publication requires its worker ACK slot",
                )
            })?;
            Some(signed.validate_committed(conn, &publication_owner, &rec, &committed, index)?)
        } else {
            None
        };
        return Ok(ItemProgress {
            item: committed,
            recorded_version,
        });
    }
    let staging = staging_dir()
        .join(sha256_bytes(job_id.as_bytes()))
        .join(sha256_bytes(item.candidate_id.as_bytes()));
    crate::session_migration::catalog::ensure_inside_imports(&staging)?;
    if dest.exists() {
        // Rename succeeded before the next receipt. Recover this dest only.
        verify_published_for_read(&dest, &rec)?;
    } else if rec.phase != "published" && rec.phase != "linked" {
        if cancel() {
            return Ok(result("cancelled", Some(import_id), None, "cancelled"));
        }
        fs::create_dir_all(&staging).map_err(io_storage_error)?;
        crate::session_migration::catalog::ensure_inside_imports(&prepared.path)?;
        if !copy_dir(&prepared.path, &staging, cancel)? {
            return Ok(result("cancelled", Some(import_id), None, "cancelled"));
        }
        if prepared.file_hashes != prepared_hashes(&staging)? {
            return Err(ImportError::new(
                ErrorCode::SourceChanged,
                "fixed preview was modified",
            ));
        }
        write_selection_file(&staging, &selection_set)?;
        let file_hashes = published_hashes(&staging)?;
        validate_staged_hashes(&rec, &file_hashes)?;
        if rec.phase != "staged" {
            rec.file_hashes = file_hashes;
            match &signed {
                Some(signed) => signed.advance(conn, &publication_owner, &mut rec, "staged")?,
                None => advance_receipt(conn, &mut rec, "staged")?,
            }
        }
        #[cfg(test)]
        maybe_pause_publish("staged");
        let cancelled = {
            #[cfg(test)]
            let _timing = timing_span(TimingStage::CancelCheck);
            cancel()
        };
        if cancelled {
            return Ok(result("cancelled", Some(import_id), None, "cancelled"));
        }
        {
            #[cfg(test)]
            let _timing = timing_span(TimingStage::DestinationDirectory);
            fs::create_dir_all(dest.parent().ok_or_else(|| {
                ImportError::new(ErrorCode::SnapshotUnavailable, "revision parent missing")
            })?)
            .map_err(io_storage_error)?;
        }
        {
            #[cfg(test)]
            let _timing = timing_span(TimingStage::RenameRevision);
            #[cfg(test)]
            super::tests::maybe_fail_publish_io("rename", &dest).map_err(io_storage_error)?;
            fs::rename(&staging, &dest).map_err(|e| {
                let error = io_storage_error(e);
                ImportError::new(error.code, format!("publish revision: {}", error.message))
            })?;
        }
        {
            #[cfg(test)]
            let _timing = timing_span(TimingStage::VerifyRevision);
            verify_published_for_read(&dest, &rec)?;
        }
        #[cfg(test)]
        maybe_pause_publish("after_rename");
        #[cfg(all(debug_assertions, feature = "native-import-audit"))]
        crate::session_migration::native_fault::pause(
            crate::session_migration::native_fault::Point::AfterRename,
            job_id,
        );
    } else {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "published revision directory is missing",
        ));
    }
    if rec.phase != "published" && rec.phase != "linked" {
        {
            #[cfg(test)]
            let _timing = timing_span(TimingStage::PublishedReceipt);
            match &signed {
                Some(signed) => signed.advance(conn, &publication_owner, &mut rec, "published")?,
                None => advance_receipt(conn, &mut rec, "published")?,
            }
        }
        #[cfg(test)]
        maybe_pause_publish("after_receipt_published");
        #[cfg(all(debug_assertions, feature = "native-import-audit"))]
        crate::session_migration::native_fault::pause(
            crate::session_migration::native_fault::Point::AfterPublishedReceipt,
            job_id,
        );
    }
    #[cfg(test)]
    drop(phase);
    #[cfg(test)]
    let phase = crate::session_migration::codec_profile::Span::start("publication-link", 0);
    if rec.phase == "published" {
        // Freeze project identity, then touch the app store outside any catalog
        // transaction. Persist the planned id before create so crash recovery
        // reuses the same record.
        match &signed {
            Some(signed) => signed.link(conn, &publication_owner, &mut rec)?,
            None => {
                if crate::session_migration::project_link::prepare_link(item, &mut rec) {
                    persist_receipt(conn, &mut rec)?;
                }
                crate::session_migration::project_link::apply_store_link(item, &mut rec)?;
                crate::session_migration::project_link::remember_entry_mapping(conn, &rec)?;
                advance_receipt(conn, &mut rec, "linked")?;
            }
        }
        #[cfg(test)]
        maybe_pause_publish("linked");
    }
    #[cfg(test)]
    drop(phase);
    #[cfg(test)]
    let phase = crate::session_migration::codec_profile::Span::start("publication-search", 0);
    crate::session_migration::search::ensure_body_search_projection(conn, &dest)?;
    #[cfg(test)]
    drop(phase);
    #[cfg(test)]
    let _final_phase = crate::session_migration::codec_profile::Span::start("publication-final", 0);
    // No project/store locks inside this short catalog transaction.
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(sqlite_storage_error)?;
    if let Some(accepted) = &association {
        accepted.target(&tx, Some(&rec))?;
    }
    let entry_auth = publication_owner.verify_for_progress(&tx)?;
    let tombstoned = is_tombstoned(&tx, &item.installation_id, &item.provider, native)?;
    if tombstoned && !item.explicit_reimport {
        return Ok(result("tombstoned", None, None, "tombstoned"));
    }
    let current = find_entry(&tx, &item.installation_id, &item.provider, native)?;
    if current.as_ref().is_some_and(|e| e.import_id != import_id)
        || current.as_ref().and_then(|e| e.current_revision.as_ref())
            != rec.previous_revision.as_ref()
    {
        return Err(ImportError::new(
            ErrorCode::MappingConflict,
            "a later import revision was kept",
        ));
    }
    let committed = JobItem {
        candidate_id: item.candidate_id.clone(),
        title: Some(
            item.adapter
                .graph
                .title
                .clone()
                .unwrap_or_else(|| native.clone()),
        ),
        phase: "committed".into(),
        import_id: Some(import_id.clone()),
        revision_id: Some(revision_id.clone()),
        outcome: Some(outcome_str(item.adapter.outcome).into()),
        error: None,
        mapping_project_id: rec.mapping_project_id.clone(),
    };
    // Consume the same-snapshot witness before any SQL mutation. ACK and final
    // receipt/catalog publication become visible together, or all roll back.
    let prepared_final = if let Some(signed) = &signed {
        let (Some(auth), Some(_)) = (entry_auth.as_ref(), progress_slot) else {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "signed publication requires authenticated worker ACK",
            ));
        };
        Some(signed.before_final(&tx, auth, &rec, &committed)?)
    } else {
        None
    };
    let progress_version = if let Some(index) = progress_slot {
        let auth = match entry_auth {
            Some(auth) => auth,
            None => super::super::entry::authenticate_writer(&tx, job_id, crypto)?,
        };
        Some(
            crate::session_migration::job_progress::set_item_authenticated(
                &tx, job_id, index, &committed, auth,
            )?,
        )
    } else {
        drop(entry_auth);
        None
    };
    if tombstoned {
        crate::session_migration::catalog::clear_tombstone(
            &tx,
            &item.installation_id,
            &item.provider,
            native,
        )?;
    }
    upsert_source(&tx, &item.installation_id, &item.provider, &item.root_path)?;
    let entry = CatalogEntry {
        import_id: import_id.clone(),
        installation_id: item.installation_id.clone(),
        provider: item.provider.clone(),
        native_session_id: native.clone(),
        title: item
            .adapter
            .graph
            .title
            .clone()
            .unwrap_or_else(|| native.clone()),
        alias: current.as_ref().and_then(|e| e.alias.clone()),
        hidden: current.as_ref().is_some_and(|e| e.hidden) && !item.explicit_reimport,
        current_revision: Some(revision_id.clone()),
        created_at: current
            .map(|e| e.created_at)
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
    };
    upsert_entry(&tx, &entry)?;
    let inv = serde_json::to_string(&item.adapter.inventory)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    insert_revision(
        &tx,
        &revision_id,
        &import_id,
        &snapshot_id,
        &semantic_key,
        outcome_str(item.adapter.outcome),
        &inv,
        &dest.to_string_lossy(),
    )?;
    match &signed {
        Some(signed) => {
            let ack = progress_slot.zip(progress_version).ok_or_else(|| {
                ImportError::new(ErrorCode::PlanStale, "signed publication has no final ACK")
            })?;
            signed.finish(
                &tx,
                &publication_owner,
                prepared_final.ok_or_else(|| {
                    ImportError::new(
                        ErrorCode::PlanStale,
                        "signed final transition was not prepared",
                    )
                })?,
                &committed,
                ack,
            )?;
            let current_entry = crate::session_migration::catalog::get_entry(&tx, &import_id)?;
            let encode_error = |error: serde_json::Error| {
                ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
            };
            let revision_matches: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM revisions WHERE revision_id=?1 AND import_id=?2 AND source_snapshot_id=?3 AND normalizer_version=?4 AND options_hash=?5 AND outcome=?6 AND inventory_json=?7 AND dir_path=?8)",
                params![revision_id, import_id, snapshot_id, crate::session_migration::types::NORMALIZER_VERSION, semantic_key, outcome_str(item.adapter.outcome), inv, dest.to_string_lossy()], |row| row.get(0),
            ).map_err(sqlite_storage_error)?;
            if serde_json::to_value(current_entry.as_ref()).map_err(encode_error)?
                != serde_json::to_value(Some(&entry)).map_err(encode_error)?
                || !revision_matches
            {
                return Err(ImportError::new(
                    ErrorCode::PlanStale,
                    "final catalog publication changed before commit",
                ));
            }
        }
        None => advance_receipt(&tx, &mut rec, "committed")?,
    }
    #[cfg(test)]
    maybe_pause_publish("before_catalog_commit");
    tx.commit().map_err(sqlite_storage_error)?;
    if let Some(version) = progress_version {
        crate::session_migration::job_control::changed_version(job_id, version);
    }
    #[cfg(test)]
    maybe_pause_publish("after_catalog_commit");
    #[cfg(all(debug_assertions, feature = "native-import-audit"))]
    crate::session_migration::native_fault::pause(
        crate::session_migration::native_fault::Point::AfterCatalogCommit,
        job_id,
    );
    Ok(ItemProgress {
        item: committed,
        recorded_version: progress_version,
    })
}

#[cfg(test)]
#[path = "jobs_publish_progress_tests.rs"]
mod progress_tests;
