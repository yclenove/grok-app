//! Borrow original durable metadata, never reopen a disposed prepared preview.
//! This view is not authentication, a SQL grant, or a reconstructed CommitItem.
use super::{provenance_scope, Connection, DurableInput, ErrorCode, ImportError, JobItem, Receipt};
use crate::session_migration::{hash, jobs, selection::SelectionSet, types::ImportOutcome};

/// Invoke a SQL-only reader with the authenticated original input in its own
/// current snapshot. Neither the borrowed view nor the return is a writer grant.
pub(in crate::session_migration) fn with_stored_recovery_input<T>(
    conn: &Connection,
    job: &str,
    candidate: &str,
    crypto: &crate::session_migration::publication_crypto::JobCrypto,
    read: impl FnOnce(RecoveryInput<'_>) -> Result<T, ImportError>,
) -> Result<T, ImportError> {
    if conn.is_autocommit() {
        return Err(mismatch());
    }
    let input = super::durable_reader::load(conn, job, candidate, crypto)?;
    read(input.recovery_input())
}

pub(in crate::session_migration) struct RecoveryInput<'a> {
    input: &'a DurableInput,
}

impl DurableInput {
    pub(super) fn recovery_input(&self) -> RecoveryInput<'_> {
        RecoveryInput { input: self }
    }

    pub(super) fn validate_committed_receipt(
        &self,
        job: &str,
        receipt: &Receipt,
    ) -> Result<(), ImportError> {
        let original = self.recovery_input();
        original.validate_receipt(job, receipt)?;
        if receipt.phase != "committed" {
            return Err(mismatch());
        }
        Ok(())
    }
}

fn mismatch() -> ImportError {
    ImportError::new(
        ErrorCode::PlanStale,
        "committed receipt does not match the durable job input",
    )
}

impl RecoveryInput<'_> {
    pub(in crate::session_migration) fn candidate(&self) -> &str {
        &self.input.candidate_id
    }

    pub(in crate::session_migration) fn digest(&self) -> Result<String, ImportError> {
        Ok(hash::sha256_bytes(
            &serde_json::to_vec(self.input).map_err(super::failure)?,
        ))
    }

    pub(in crate::session_migration) fn owner<'crypto>(
        &self,
        conn: &Connection,
        job: &str,
        crypto: &'crypto super::super::publication_crypto::JobCrypto,
    ) -> Result<provenance_scope::publication::PublicationOwner<'crypto>, ImportError> {
        provenance_scope::publication::for_input(conn, job, self.input, crypto)
    }

    pub(in crate::session_migration) fn validate_receipt(
        &self,
        job: &str,
        receipt: &Receipt,
    ) -> Result<(), ImportError> {
        let input = self.input;
        input.validate_shape()?;
        if receipt.source_member != input.source_member
            || receipt.job_id != job
            || receipt.candidate_id != input.candidate_id
            || receipt.installation_id != input.installation_id
            || receipt.provider != input.provider
            || receipt.native_session_id != input.adapter.graph.native_session_id
            || receipt.source_id != input.source_id
            || receipt.root_path != input.root_path
            || receipt.legacy_id != input.legacy_id
            || receipt.snapshot_hash != input.preview_digest
            || receipt.options_hash != hash::options_hash(&input.options)
            || receipt.selection_hash != input.selection_hash
            || receipt.selection_set.canonicalized() != input.selection_set.canonicalized()
        {
            return Err(mismatch());
        }
        Ok(())
    }

    pub(in crate::session_migration) fn result(
        &self,
        import: &str,
        revision: &str,
        mapping: Option<String>,
    ) -> JobItem {
        let input = self.input;
        let outcome = match input.adapter.outcome {
            ImportOutcome::Complete => "complete",
            ImportOutcome::Partial => "partial",
            ImportOutcome::Unsupported => "unsupported",
            ImportOutcome::Failed => "failed",
        };
        JobItem {
            candidate_id: input.candidate_id.clone(),
            title: Some(
                input
                    .adapter
                    .graph
                    .title
                    .clone()
                    .unwrap_or_else(|| input.adapter.graph.native_session_id.clone()),
            ),
            phase: "committed".into(),
            import_id: Some(import.into()),
            revision_id: Some(revision.into()),
            outcome: Some(outcome.into()),
            error: None,
            mapping_project_id: mapping,
        }
    }

    pub(in crate::session_migration) fn dedup_result(
        &self,
        import: &str,
        revision: &str,
    ) -> JobItem {
        let mut result = self.result(import, revision, self.input.mapping_project_id.clone());
        result.outcome = Some("deduplicated".into());
        result
    }

    pub(in crate::session_migration) fn validate_reused_receipt(
        &self,
        receipt: &Receipt,
    ) -> Result<(), ImportError> {
        let input = self.input;
        input.validate_shape()?;
        let options = hash::options_hash(&input.options);
        let key = |selection: &SelectionSet| {
            jobs::revision_identity::key_for_intent(
                &options,
                selection,
                input.mapping_project_id.as_deref(),
                input.create_project,
                input.explicit_target.as_deref(),
                input.import_unassigned_anyway,
            )
        };
        if receipt.phase != "committed"
            || receipt.provider != input.provider
            || receipt.snapshot_hash != input.preview_digest
            || receipt.options_hash != options
            || key(&receipt.selection_set)? != key(&input.selection_set)?
        {
            return Err(mismatch());
        }
        Ok(())
    }

    pub(in crate::session_migration) fn require_published_revision(
        &self,
        conn: &Connection,
        import: &str,
        revision: &str,
    ) -> Result<(), ImportError> {
        let input = self.input;
        let options = hash::options_hash(&input.options);
        let semantic = jobs::revision_identity::key_for_intent(
            &options,
            &input.selection_set,
            input.mapping_project_id.as_deref(),
            input.create_project,
            input.explicit_target.as_deref(),
            input.import_unassigned_anyway,
        )?;
        let result = self.result(import, revision, None);
        let inventory = serde_json::to_string(&input.adapter.inventory).map_err(super::failure)?;
        // Pure path construction only; revision_dir's filesystem containment
        // verification belongs outside the catalog snapshot.
        let path = crate::session_migration::catalog::entries_dir()
            .join(crate::session_migration::ids::import_id_dir_component(
                import,
            ))
            .join(revision);
        let matches: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM revisions WHERE import_id=?1 AND revision_id=?2 AND source_snapshot_id=?3 AND normalizer_version=?4 AND options_hash=?5 AND outcome=?6 AND inventory_json=?7 AND dir_path=?8)",
            rusqlite::params![import, revision, input.preview_digest, crate::session_migration::types::NORMALIZER_VERSION, semantic, result.outcome, inventory, path.to_string_lossy()],
            |row| row.get(0),
        ).map_err(crate::session_migration::errors::sqlite_storage_error)?;
        if !matches {
            return Err(mismatch());
        }
        Ok(())
    }
}
