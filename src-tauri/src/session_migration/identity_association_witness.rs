//! Internal verified facts, not a serializable grant or an idle preview cache.
use super::{unsupported, AssociationReview, VERSION};
use crate::session_migration::{
    catalog::CatalogEntry,
    hash,
    jobs::Receipt,
    preview_lifecycle::OperationGuard,
    selection::{PreparedSelectionValidation, PreviewSelection, SelectionSet},
    sources::SourceRegistration,
    types,
};
use serde_json::json;
use std::collections::BTreeMap;

pub(in crate::session_migration) struct InspectionWitness {
    pub(in crate::session_migration) _operation: OperationGuard,
    pub(in crate::session_migration) entry: CatalogEntry,
    pub(in crate::session_migration) expected_revision: String,
    pub(in crate::session_migration) preview: PreviewSelection,
    pub(in crate::session_migration) validated: PreparedSelectionValidation,
    pub(in crate::session_migration) prepared_hashes: BTreeMap<String, String>,
    pub(in crate::session_migration) receipt: Receipt,
    pub(in crate::session_migration) anchor_row: i64,
    pub(in crate::session_migration) receipt_cutoff: i64,
    pub(in crate::session_migration) registration: SourceRegistration,
    pub(in crate::session_migration) registered_paths: Vec<String>,
    pub(in crate::session_migration) old_normalizer: String,
}

impl InspectionWitness {
    /// Complete SQL-only CAS against the detached authenticated facts. The
    /// caller holds the existing publication lock and immediate transaction.
    pub(in crate::session_migration) fn compare_catalog(
        &self,
        conn: &rusqlite::Connection,
    ) -> Result<(), crate::session_migration::errors::ImportError> {
        use crate::session_migration::{
            catalog, registration_provenance as provenance, registration_provenance_store as store,
            sources,
        };
        if conn.is_autocommit() {
            return Err(super::stale(
                "association CAS requires the accepted-job writer",
            ));
        }
        let entry = catalog::get_entry(conn, &self.entry.import_id)?;
        let registration = sources::get(conn, &self.validated.source_id)?;
        if serde_json::to_value(&entry).map_err(|_| unsupported())?
            != serde_json::to_value(Some(&self.entry)).map_err(|_| unsupported())?
            || serde_json::to_value(&registration).map_err(|_| unsupported())?
                != serde_json::to_value(&self.registration).map_err(|_| unsupported())?
            || sources::registered_paths(conn, &registration)? != self.registered_paths
        {
            return Err(super::stale(
                "association target or source registration changed after review",
            ));
        }
        let member = self
            .validated
            .source_member
            .as_ref()
            .ok_or_else(unsupported)?;
        crate::session_migration::source_locations::validate_registered_with(
            conn,
            &member.reference.location,
        )?;
        super::ensure_not_deleted(
            conn,
            &self.entry.installation_id,
            &self.entry.provider,
            &[
                &self.entry.native_session_id,
                &self.validated.native_session_id,
            ],
        )?;
        let conflict: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE installation_id=?1 AND provider=?2 AND native_session_id=?3)",
            rusqlite::params![self.entry.installation_id, self.entry.provider, self.validated.native_session_id],
            |row| row.get(0)).map_err(crate::session_migration::errors::sqlite_storage_error)?;
        if conflict {
            return Err(super::conflict(
                "current native identity already belongs to an import",
            ));
        }
        if store::verify_schema(conn)? {
            let present: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM registration_provenance WHERE import_id=?1)",
                    [&self.entry.import_id],
                    |row| row.get(0),
                )
                .map_err(crate::session_migration::errors::sqlite_storage_error)?;
            if present {
                return Err(super::conflict(
                    "association cannot overwrite any existing proof",
                ));
            }
        }
        let (row, receipt) = provenance::receipt_store::point(conn, &self.receipt.receipt_id)?;
        let latest = provenance::receipt_store::latest(
            conn,
            &self.entry.import_id,
            &self.expected_revision,
        )?;
        let normalizer: String = conn
            .query_row(
                "SELECT normalizer_version FROM revisions WHERE import_id=?1 AND revision_id=?2",
                rusqlite::params![self.entry.import_id, self.expected_revision],
                |row| row.get(0),
            )
            .map_err(crate::session_migration::errors::sqlite_storage_error)?;
        let revision = catalog::get_revision(conn, &self.entry.import_id, &self.expected_revision)?;
        if row != self.anchor_row
            || provenance::receipt_digest(&receipt)? != provenance::receipt_digest(&self.receipt)?
            || latest.as_ref().map(|receipt| receipt.receipt_id.as_str())
                != Some(self.receipt.receipt_id.as_str())
            || provenance::cutoff(conn, &self.entry.import_id)? != self.receipt_cutoff
            || normalizer != self.old_normalizer
            || !revision
                .is_some_and(|revision| revision.source_snapshot_id == self.receipt.snapshot_hash)
        {
            return Err(super::stale(
                "association revision/anchor/cutoff changed after authentication",
            ));
        }
        Ok(())
    }

    pub(in crate::session_migration) fn review(
        &self,
    ) -> Result<AssociationReview, crate::session_migration::errors::ImportError> {
        let pin = self
            .validated
            .source_member
            .as_ref()
            .ok_or_else(unsupported)?;
        let selection_hash = SelectionSet::single(self.preview.clone()).hash();
        // Keep the public review's exact canonical payload; the later accepted
        // transaction consumes these same facts rather than another inspector.
        let review_digest = hash::options_hash(&json!({
            "domain":"session-import-explicit-identity-association-v1", "entry":self.entry,
            "oldNormalizer":self.old_normalizer, "anchorRow":self.anchor_row, "anchor":self.receipt,
            "receiptCutoff":self.receipt_cutoff,
            "preview":self.preview, "selectionHash":selection_hash,
            "preparedHashes":self.prepared_hashes, "snapshotDigest":self.validated.snapshot_digest,
            "sourceMember":pin, "sourceRegistration":self.registration,
            "registeredPaths":self.registered_paths, "currentNormalizer":types::NORMALIZER_VERSION,
        }));
        Ok(AssociationReview {
            version: VERSION,
            import_id: self.entry.import_id.clone(),
            expected_revision: self.expected_revision.clone(),
            preview_id: self.preview.preview_id.clone(),
            source_id: self.validated.source_id.clone(),
            old_native_session_id: self.entry.native_session_id.clone(),
            current_native_session_id: self.validated.native_session_id.clone(),
            old_normalizer_version: self.old_normalizer.clone(),
            current_normalizer_version: types::NORMALIZER_VERSION.into(),
            selection_hash,
            review_digest,
        })
    }
}
