//! Reuse the actual verified old revision without inventing a new receipt or
//! project. Preflight does archive I/O; the final writer uses only SQL/crypto.
use super::super::record::Deduplicated;
use super::*;
use crate::session_migration::{catalog, hash::options_hash, jobs::revision_identity};
use rusqlite::params;

struct Reuse {
    entry: catalog::CatalogEntry,
    receipt: Receipt,
    revision_digest: String,
    selection_hash: String,
    owner: super::super::prior::ReceiptOwner,
}

pub(in crate::session_migration::publication_outcome) fn revision_digest(
    conn: &Connection,
    import: &str,
    revision: &str,
) -> Result<String, ImportError> {
    let fields = conn.query_row(
        "SELECT revision_id,import_id,source_snapshot_id,normalizer_version,options_hash,outcome,inventory_json,dir_path,created_at FROM revisions WHERE import_id=?1 AND revision_id=?2",
        params![import, revision],
        |row| (0..9).map(|index| row.get::<_, String>(index)).collect::<Result<Vec<_>, _>>(),
    ).map_err(sqlite_storage_error)?;
    Ok(sha256_bytes(canonical(&fields)?.as_bytes()))
}

pub(in crate::session_migration::publication_outcome) fn identity_digest(
    entry: &catalog::CatalogEntry,
) -> Result<String, ImportError> {
    Ok(sha256_bytes(
        canonical(&(
            &entry.import_id,
            &entry.installation_id,
            &entry.provider,
            &entry.native_session_id,
        ))?
        .as_bytes(),
    ))
}

fn require_reuse_target(
    allocation: &Allocation,
    entry: &catalog::CatalogEntry,
) -> Result<(), ImportError> {
    if allocation.created_entry
        || allocation.import_id != entry.import_id
        || allocation.previous_revision != entry.current_revision
    {
        return Err(invalid());
    }
    Ok(())
}

impl Reuse {
    fn capture(
        conn: &Connection,
        item: &CommitItem,
        expected: &catalog::CatalogEntry,
        revision: &str,
    ) -> Result<Self, ImportError> {
        if !conn.is_autocommit() {
            return Err(invalid());
        }
        let (entry, receipt, revision_digest) = {
            let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
            let entry = catalog::get_entry(&tx, &expected.import_id)?.ok_or_else(invalid)?;
            if canonical(&entry)? != canonical(expected)?
                || entry.installation_id != item.installation_id
                || entry.provider != item.provider
                || entry.native_session_id != item.adapter.graph.native_session_id
            {
                return Err(invalid());
            }
            let old = jobs::latest_receipt_for_revision(&tx, &entry.import_id, revision)?
                .ok_or_else(invalid)?;
            let receipt = receipt::read(&tx, &old.receipt_id)?.ok_or_else(invalid)?;
            if receipt::digest(&receipt)? != receipt::digest(&old)? {
                return Err(invalid());
            }
            let options = options_hash(&item.options);
            let semantic = revision_identity::key(&options, &item.selection_set, item)?;
            if revision_identity::key(&options, &receipt.selection_set, item)? != semantic
                || revision_identity::find_duplicate_metadata(
                    &tx,
                    item,
                    Some(&entry),
                    &receipt.snapshot_hash,
                    &semantic,
                    &options,
                    &receipt.selection_set,
                )?
                .as_deref()
                    != Some(revision)
                || receipt.snapshot_hash != item.prepared.as_ref().ok_or_else(invalid)?.digest
                || receipt.selection_hash != receipt.selection_set.hash()
            {
                return Err(invalid());
            }
            let digest = revision_digest(&tx, &entry.import_id, revision)?;
            (entry, receipt, digest)
        };
        // Key loading is outside SQL. The prepared object has no SQL grant;
        // the receipt's actual owner is reselected in every current snapshot.
        let owner = super::super::prior::ReceiptOwner::prepare(conn, &receipt)?;
        let reuse = Self {
            revision_digest,
            selection_hash: receipt.selection_set.hash(),
            entry,
            receipt,
            owner,
        };
        {
            let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
            reuse.require(&tx)?;
        }
        let dir = catalog::revision_dir(&reuse.entry.import_id, revision)?;
        #[cfg(test)]
        tests::before_archive(conn, &reuse.receipt);
        let selection = jobs::verify_published_for_read(&dir, &reuse.receipt)?;
        if selection.canonicalized() != reuse.receipt.selection_set.canonicalized() {
            return Err(invalid());
        }
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        reuse.require(&tx)?;
        Ok(reuse)
    }

    fn require(&self, tx: &Transaction<'_>) -> Result<(), ImportError> {
        let current = catalog::get_entry(tx, &self.entry.import_id)?.ok_or_else(invalid)?;
        if canonical(&current)? != canonical(&self.entry)? {
            return Err(invalid());
        }
        self.require_archive_rows(tx)
    }

    fn require_archive_rows(&self, tx: &Transaction<'_>) -> Result<(), ImportError> {
        receipt::require_point(tx, &self.receipt)?;
        let revision = self.receipt.revision_id.as_deref().ok_or_else(invalid)?;
        let latest = jobs::latest_receipt_for_revision(tx, &self.entry.import_id, revision)?
            .ok_or_else(invalid)?;
        if receipt::digest(&latest)? != receipt::digest(&self.receipt)?
            || revision_digest(tx, &self.entry.import_id, revision)? != self.revision_digest
        {
            return Err(invalid());
        }
        self.owner.require(tx, &self.receipt)
    }

    fn data(
        &self,
        allocation: &VerifiedRecord,
        item: &JobItem,
    ) -> Result<Deduplicated, ImportError> {
        Ok(Deduplicated {
            allocation_digest: allocation.digest.clone(),
            import_id: self.entry.import_id.clone(),
            revision_id: self.receipt.revision_id.clone().ok_or_else(invalid)?,
            receipt_id: self.receipt.receipt_id.clone(),
            receipt_digest: receipt::digest(&self.receipt)?,
            revision_digest: self.revision_digest.clone(),
            selection_hash: self.selection_hash.clone(),
            entry_identity_digest: identity_digest(&self.entry)?,
            item_digest: sha256_bytes(canonical(item)?.as_bytes()),
        })
    }

    fn projected_entry(&self, item: &CommitItem, revision: &str) -> catalog::CatalogEntry {
        Self::projected_from(&self.entry, item, revision)
    }

    fn projected_from(
        entry: &catalog::CatalogEntry,
        item: &CommitItem,
        revision: &str,
    ) -> catalog::CatalogEntry {
        let mut expected = entry.clone();
        if item.explicit_reimport {
            expected.hidden = false;
            expected.current_revision = Some(revision.into());
        }
        if expected.current_revision.as_deref() == Some(revision)
            && expected.title == expected.native_session_id
            && matches!(item.provider.as_str(), "codex" | "claude-code" | "cursor")
        {
            if let Some(title) = item.adapter.graph.title.as_ref().filter(|title| {
                !title.trim().is_empty() && **title != item.adapter.graph.native_session_id
            }) {
                expected.title.clone_from(title);
            }
        }
        expected
    }
}

impl SignedPublication<'_> {
    fn dedup_item(&self, import: &str, revision: &str) -> JobItem {
        JobItem {
            candidate_id: self.item.candidate_id.clone(),
            title: Some(
                self.item
                    .adapter
                    .graph
                    .title
                    .clone()
                    .unwrap_or_else(|| self.item.adapter.graph.native_session_id.clone()),
            ),
            phase: "committed".into(),
            import_id: Some(import.into()),
            revision_id: Some(revision.into()),
            outcome: Some("deduplicated".into()),
            error: None,
            mapping_project_id: self.item.mapping_project_id.clone(),
        }
    }

    pub(super) fn deduplicated_record(
        &self,
        tx: &Transaction<'_>,
    ) -> Result<VerifiedRecord, ImportError> {
        let result = journal::load_deduplicated(tx, self.auth, &self.allocated.body.binding)?;
        if !receipt::prefix(
            tx,
            self.allocation()?,
            &self.allocated.body.binding,
            &self.item.candidate_id,
        )?
        .is_empty()
        {
            return Err(invalid());
        }
        result.follows(&self.allocated)?;
        Ok(result)
    }

    pub(in crate::session_migration) fn replay_deduplicated(
        &self,
        conn: &Connection,
        owner: &PublicationOwner<'_>,
        slot: Option<usize>,
    ) -> Result<Option<(JobItem, u32)>, ImportError> {
        let (record, entry, item) = {
            let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
            self.verify(&tx, owner)?;
            if !journal::present(&tx, &self.allocated.body.binding, StageKind::Deduplicated)? {
                if self.current(&tx)?.is_empty() {
                    require_unacknowledged(
                        &tx,
                        self.job(),
                        &self.item.candidate_id,
                        slot.ok_or_else(invalid)?,
                    )?;
                }
                return Ok(None);
            }
            let record = self.deduplicated_record(&tx)?;
            let Stage::Deduplicated(data) = &record.body.stage else {
                return Err(invalid());
            };
            let entry = catalog::get_entry(&tx, &data.import_id)?.ok_or_else(invalid)?;
            let item = self.dedup_item(&data.import_id, &data.revision_id);
            require_ack(&tx, self.job(), &item, slot.ok_or_else(invalid)?, None)?;
            (record, entry, item)
        };
        let Stage::Deduplicated(data) = &record.body.stage else {
            return Err(invalid());
        };
        let reuse = Reuse::capture(conn, self.item, &entry, &data.revision_id)?;
        if reuse.data(&self.allocated, &item)? != *data {
            return Err(invalid());
        }
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        self.verify(&tx, owner)?;
        reuse.require(&tx)?;
        if self.deduplicated_record(&tx)?.encoded != record.encoded {
            return Err(invalid());
        }
        let version = require_ack(&tx, self.job(), &item, slot.ok_or_else(invalid)?, None)?;
        Ok(Some((item, version)))
    }

    pub(in crate::session_migration) fn deduplicate(
        &self,
        conn: &Connection,
        owner: &PublicationOwner<'_>,
        entry: &catalog::CatalogEntry,
        revision: &str,
        slot: Option<usize>,
    ) -> Result<(JobItem, u32), ImportError> {
        let index = slot.ok_or_else(invalid)?;
        // Dedup may select an older revision, but must not silently adopt a
        // catalog current that advanced after this job's acceptance. Replay
        // deliberately does not reapply this pre-publication CAS after commit.
        require_reuse_target(self.allocation()?, entry)?;
        let reuse = Reuse::capture(conn, self.item, entry, revision)?;
        let item = self.dedup_item(&entry.import_id, revision);
        let sealed = seal(
            self.auth,
            self.allocated.body.binding.clone(),
            Stage::Deduplicated(reuse.data(&self.allocated, &item)?),
        )?;
        let candidate = verify(
            self.auth,
            sealed.encoded.as_bytes(),
            &sealed.body.binding,
            StageKind::Deduplicated,
        )?;
        candidate.follows(&self.allocated)?;
        let dir = catalog::revision_dir(&entry.import_id, revision)?;
        crate::session_migration::search::ensure_body_search_projection(conn, &dir)?;
        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
            .map_err(sqlite_storage_error)?;
        let auth = owner.verify_for_progress(&tx)?.ok_or_else(invalid)?;
        self.require_writer_allocation(&tx, &auth)?;
        reuse.require(&tx)?;
        if !self.current(&tx)?.is_empty() {
            return Err(invalid());
        }
        require_unacknowledged(&tx, self.job(), &self.item.candidate_id, index)?;
        let tombstoned = catalog::is_tombstoned(
            &tx,
            &self.item.installation_id,
            &self.item.provider,
            &self.item.adapter.graph.native_session_id,
        )?;
        if tombstoned && !self.item.explicit_reimport {
            return Err(invalid());
        }
        let expected = reuse.projected_entry(self.item, revision);
        // The concrete writer witness is consumed before the first SQL write.
        let version = crate::session_migration::job_progress::set_item_authenticated(
            &tx,
            self.job(),
            index,
            &item,
            auth,
        )?;
        if self.item.explicit_reimport {
            catalog::clear_tombstone(
                &tx,
                &self.item.installation_id,
                &self.item.provider,
                &self.item.adapter.graph.native_session_id,
            )?;
            tx.execute(
                "UPDATE entries SET hidden=0,current_revision=?1 WHERE import_id=?2",
                params![revision, entry.import_id],
            )
            .map_err(sqlite_storage_error)?;
        }
        jobs::repair_deduplicated_title(&tx, self.item, &entry.import_id, revision)?;
        journal::insert(&tx, self.auth, &sealed)?;
        self.verify(&tx, owner)?;
        reuse.require_archive_rows(&tx)?;
        if canonical(&catalog::get_entry(&tx, &entry.import_id)?.ok_or_else(invalid)?)?
            != canonical(&expected)?
            || catalog::is_tombstoned(
                &tx,
                &self.item.installation_id,
                &self.item.provider,
                &self.item.adapter.graph.native_session_id,
            )? != (tombstoned && !self.item.explicit_reimport)
            || self.deduplicated_record(&tx)?.encoded != sealed.encoded
        {
            return Err(invalid());
        }
        require_ack(&tx, self.job(), &item, index, Some(version))?;
        tx.commit().map_err(sqlite_storage_error)?;
        crate::session_migration::job_control::changed_version(self.job(), version);
        Ok((item, version))
    }
}

pub(in crate::session_migration::publication_outcome) fn require_unacknowledged(
    tx: &Transaction<'_>,
    job: &str,
    candidate: &str,
    index: usize,
) -> Result<(), ImportError> {
    let (json, projected): (String, String) = tx.query_row(
        "SELECT json_extract(j.json,?3),i.phase FROM jobs j JOIN job_items i ON i.job_id=j.job_id AND i.candidate_id=?2 WHERE j.job_id=?1",
        params![job, candidate, format!("$.items[{index}]")], |r| Ok((r.get(0)?, r.get(1)?)),
    ).map_err(sqlite_storage_error)?;
    let item: JobItem = serde_json::from_str(&json).map_err(|_| invalid())?;
    let original: serde_json::Value = serde_json::from_str(&json).map_err(|_| invalid())?;
    if item.candidate_id != candidate
        || item.phase != projected
        || !matches!(
            item.phase.as_str(),
            "queued" | "running" | "recovery_needed"
        )
        || item.import_id.is_some()
        || item.revision_id.is_some()
        || item.outcome.is_some()
        || canonical(&original)? != canonical(&item)?
    {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
#[path = "publication_dedup_tests.rs"]
mod tests;
