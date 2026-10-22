//! Exact accepted adoption reconciles catalog ownership with immutable receipts.
//! This is not a text alias or permission to rewrite historical provenance.
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::catalog::{self, CatalogEntry};
use super::errors::{sqlite_storage_error, ErrorCode, ImportError};
use super::jobs::{self, JobRecord, Receipt};
use super::{hash, registration_provenance_store as store, sources};

#[path = "registration_provenance_record.rs"]
mod record;
pub(super) use record::old_owner;
pub(super) use record::AssociationProof;
pub(super) use record::ProofRecord;

#[path = "registration_receipt.rs"]
pub(super) mod receipt_store;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct InputIdentity {
    pub candidate_id: String,
    pub source_id: String,
    pub provider: String,
    pub native_session_id: String,
    pub legacy_id: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Proof {
    version: u32,
    pub import_id: String,
    original_installation: String,
    original_native: String,
    original_locator: String,
    expected_revision: String,
    anchor_receipt: String,
    anchor_digest: String,
    receipt_cutoff: i64,
    host: String,
    job_id: String,
    request_id: String,
    plan_id: String,
    plan_hash: String,
    input: InputIdentity,
}

fn refusal() -> ImportError {
    ImportError::new(
        ErrorCode::HistoryAccessUnavailable,
        "historical registration provenance is missing or inconsistent",
    )
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max
}
fn digest_shape(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(super) fn receipt_digest(receipt: &Receipt) -> Result<String, ImportError> {
    Ok(hash::options_hash(
        &serde_json::to_value(receipt).map_err(|_| refusal())?,
    ))
}

impl Proof {
    pub(super) fn require_input(&self, input: &InputIdentity) -> Result<(), ImportError> {
        self.validate_shape()?;
        if &self.input != input {
            return Err(refusal());
        }
        Ok(())
    }

    /// Called after genuine adoption, inside the SAME acceptance writer. The
    /// detached preflight proof selects an old import but grants no SQL access.
    pub(super) fn accepted_allocation_target(
        &self,
        conn: &Connection,
        input: &InputIdentity,
    ) -> Result<CatalogEntry, ImportError> {
        if conn.is_autocommit() {
            return Err(refusal());
        }
        self.require_input(input)?;
        let Some(ProofRecord::Legacy(stored)) = store::get(conn, &self.import_id)? else {
            return Err(refusal());
        };
        if stored.digest()? != self.digest()? {
            return Err(refusal());
        }
        let entry = catalog::get_entry(conn, &self.import_id)?.ok_or_else(refusal)?;
        let receipt = anchor(conn, self)?;
        authorize(conn, &entry, &receipt)?;
        if entry.installation_id != input.source_id
            || entry.provider != input.provider
            || entry.native_session_id != input.native_session_id
            || entry.current_revision.as_deref() != Some(self.expected_revision.as_str())
        {
            return Err(refusal());
        }
        Ok(entry)
    }

    pub(super) fn digest(&self) -> Result<String, ImportError> {
        Ok(hash::options_hash(
            &serde_json::to_value(self).map_err(|_| refusal())?,
        ))
    }

    pub(super) fn validate_shape(&self) -> Result<(), ImportError> {
        if self.version != 1
            || self.host != "local"
            || self.receipt_cutoff <= 0
            || [
                self.import_id.as_str(),
                &self.original_native,
                &self.expected_revision,
                &self.anchor_receipt,
                &self.plan_id,
                &self.plan_hash,
                &self.request_id,
                &self.input.candidate_id,
                &self.input.native_session_id,
            ]
            .iter()
            .any(|s| !bounded(s, 4096))
            || [&self.original_locator, &self.input.legacy_id]
                .iter()
                .any(|s| !bounded(s, 8192))
            || [&self.input.source_id, &self.job_id]
                .iter()
                .any(|s| uuid::Uuid::parse_str(s).is_err())
            || super::types::Provider::parse(&self.input.provider).is_none()
            || !digest_shape(&self.anchor_digest)
            || !digest_shape(&self.input.digest)
            || self.original_installation
                != format!(
                    "local:{}",
                    hash::sha256_bytes(self.original_locator.as_bytes())
                )
            || !sources::locator_spellings(&self.input.legacy_id).contains(&self.original_locator)
        {
            return Err(refusal());
        }
        Ok(())
    }

    fn old_identity(&self) -> sources::LegacyIdentity {
        sources::LegacyIdentity {
            import_id: self.import_id.clone(),
            installation_id: self.original_installation.clone(),
            native_session_id: self.original_native.clone(),
            current_revision: Some(self.expected_revision.clone()),
            locator: self.original_locator.clone(),
        }
    }
}

fn registered(conn: &Connection, input: &InputIdentity) -> Result<(), ImportError> {
    let found: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sources WHERE installation_id=?1 AND provider=?2 AND host='local')",
        params![input.source_id, input.provider], |row| row.get(0),
    ).map_err(sqlite_storage_error)?;
    if !found || uuid::Uuid::parse_str(&input.source_id).is_err() {
        return Err(ImportError::new(
            ErrorCode::MappingConflict,
            "enqueue requires the registered local provider",
        ));
    }
    Ok(())
}

pub(super) fn cutoff(conn: &Connection, import_id: &str) -> Result<i64, ImportError> {
    conn.query_row(
        "SELECT COALESCE(MAX(rowid),0) FROM receipts WHERE import_id=?1",
        [import_id],
        |row| row.get(0),
    )
    .map_err(sqlite_storage_error)
}

/// Exact receipt point read validates JSON against every duplicated SQL column.
pub(super) fn persisted_receipt(conn: &Connection, receipt: &Receipt) -> Result<i64, ImportError> {
    let (rowid, stored) = receipt_store::point(conn, &receipt.receipt_id)?;
    jobs::validate_receipt_shape(&stored)?;
    if stored.phase != "committed" || receipt_digest(&stored)? != receipt_digest(receipt)? {
        return Err(refusal());
    }
    Ok(rowid)
}

/// Archive authentication happens in this detached read snapshot, before writer.
pub(super) fn preflight(
    conn: &Connection,
    input: InputIdentity,
    job: &JobRecord,
) -> Result<Option<Proof>, ImportError> {
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    store::verify_schema(&tx)?;
    registered(&tx, &input)?;
    let Some(old) = sources::resolve_identity(
        &tx,
        &input.source_id,
        &input.legacy_id,
        &input.provider,
        &input.native_session_id,
    )?
    else {
        return Ok(None);
    };
    let revision = old.current_revision.clone().ok_or_else(refusal)?;
    let local: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sources WHERE installation_id=?1 AND provider=?2 AND host='local')",
        params![old.installation_id, input.provider], |row| row.get(0),
    ).map_err(sqlite_storage_error)?;
    if !local {
        return Err(refusal());
    }
    let entry = catalog::get_entry(&tx, &old.import_id)?.ok_or_else(refusal)?;
    jobs::published_auth_from_entry(&tx, &entry, &old.import_id, Some(&revision), None)?;
    let anchor =
        jobs::latest_receipt_for_revision(&tx, &old.import_id, &revision)?.ok_or_else(refusal)?;
    // The stored legacy installation must have originated from this exact locator.
    if !sources::locator_spellings(&anchor.legacy_id).contains(&old.locator) {
        return Err(refusal());
    }
    persisted_receipt(&tx, &anchor)?;
    let proof = Proof {
        version: 1,
        import_id: old.import_id,
        original_installation: old.installation_id,
        original_native: old.native_session_id,
        original_locator: old.locator,
        expected_revision: revision,
        anchor_receipt: anchor.receipt_id.clone(),
        anchor_digest: receipt_digest(&anchor)?,
        receipt_cutoff: cutoff(&tx, anchor.import_id.as_deref().ok_or_else(refusal)?)?,
        host: "local".into(),
        job_id: job.job_id.clone(),
        request_id: job.request_id.clone(),
        plan_id: job.plan_id.clone(),
        plan_hash: job.plan_hash.clone(),
        input,
    };
    proof.validate_shape()?;
    Ok(Some(proof))
}

fn anchor(conn: &Connection, proof: &Proof) -> Result<Receipt, ImportError> {
    let json: String = conn
        .query_row(
            "SELECT json FROM receipts WHERE receipt_id=?1 AND phase='committed'
         AND import_id=?2 AND revision_id=?3 AND length(CAST(json AS BLOB))<=16777216",
            params![
                proof.anchor_receipt,
                proof.import_id,
                proof.expected_revision
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_storage_error)?
        .ok_or_else(refusal)?;
    let receipt: Receipt = serde_json::from_str(&json).map_err(|_| refusal())?;
    if receipt.receipt_id != proof.anchor_receipt
        || receipt.import_id.as_deref() != Some(&proof.import_id)
        || receipt.revision_id.as_deref() != Some(&proof.expected_revision)
        || receipt.installation_id != proof.original_installation
        || receipt.native_session_id != proof.original_native
        || receipt.provider != proof.input.provider
        || receipt_digest(&receipt)? != proof.anchor_digest
        || !sources::locator_spellings(&receipt.legacy_id).contains(&proof.original_locator)
        || persisted_receipt(conn, &receipt)? > proof.receipt_cutoff
    {
        return Err(refusal());
    }
    let revision = catalog::get_revision(conn, &proof.import_id, &proof.expected_revision)?
        .ok_or_else(refusal)?;
    if revision.source_snapshot_id != receipt.snapshot_hash {
        return Err(refusal());
    }
    Ok(receipt)
}

/// Short CAS; no filesystem/hash work. DDL/proof/catalog/job share one commit.
pub(super) fn accept(
    conn: &Connection,
    input: &InputIdentity,
    proof: Option<&Proof>,
) -> Result<(), ImportError> {
    registered(conn, input)?;
    let actual = sources::resolve_identity(
        conn,
        &input.source_id,
        &input.legacy_id,
        &input.provider,
        &input.native_session_id,
    )?;
    if actual != proof.map(Proof::old_identity) {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "legacy identity changed after preflight",
        ));
    }
    if let Some(proof) = proof {
        proof.validate_shape()?;
        if &proof.input != input || cutoff(conn, &proof.import_id)? != proof.receipt_cutoff {
            return Err(refusal());
        }
        let receipt = anchor(conn, proof)?;
        let current_anchor =
            jobs::latest_receipt_for_revision(conn, &proof.import_id, &proof.expected_revision)?
                .ok_or_else(refusal)?;
        if current_anchor.receipt_id != receipt.receipt_id {
            return Err(refusal());
        }
        store::insert(conn, proof)?;
    }
    sources::commit_identity_in_transaction(
        conn,
        &input.source_id,
        &input.legacy_id,
        &input.provider,
        &input.native_session_id,
    )?;
    Ok(())
}

/// Reconcile identity only. Existing caller still verifies snapshot/selection/
/// sourceMember and every immutable file using the original receipt identity.
pub(super) fn authorize(
    conn: &Connection,
    entry: &CatalogEntry,
    receipt: &Receipt,
) -> Result<(), ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return authorize(&tx, entry, receipt);
    }
    let record = store::get(conn, &entry.import_id)?;
    // A parsed protocol draft is not accepted authority. In particular it may
    // not bypass verification when current entry and receipt identities match.
    if let Some(ProofRecord::Association(proof)) = &record {
        return proof.authorize(conn, entry, receipt);
    }
    if entry.installation_id == receipt.installation_id
        && entry.provider == receipt.provider
        && entry.native_session_id == receipt.native_session_id
    {
        return Ok(());
    }
    let Some(ProofRecord::Legacy(proof)) = record else {
        return Err(refusal());
    };
    if receipt.import_id.as_deref() != Some(&entry.import_id)
        || entry.provider != proof.input.provider
        || receipt.provider != proof.input.provider
        || entry.installation_id != proof.input.source_id
        || entry.native_session_id != proof.input.native_session_id
        || receipt.installation_id != proof.original_installation
        || receipt.native_session_id != proof.original_native
        || !sources::locator_spellings(&receipt.legacy_id).contains(&proof.original_locator)
        || persisted_receipt(conn, receipt)? > proof.receipt_cutoff
    {
        return Err(refusal());
    }
    registered(conn, &proof.input)?;
    anchor(conn, &proof)?;
    let accepted: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM jobs WHERE job_id=?1 AND request_id=?2 AND plan_hash=?3
         AND json_extract(json,'$.jobId')=?1 AND json_extract(json,'$.requestId')=?2
         AND json_extract(json,'$.planHash')=?3 AND json_extract(json,'$.planId')=?4)",
            params![
                proof.job_id,
                proof.request_id,
                proof.plan_hash,
                proof.plan_id
            ],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    if !accepted
        || super::job_control::registration_input(conn, &proof.job_id, &proof.input.candidate_id)?
            != proof.input
    {
        return Err(refusal());
    }
    Ok(())
}
