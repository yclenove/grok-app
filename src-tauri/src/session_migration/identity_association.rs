//! Read-only inspection for an explicitly chosen historical import target.
//! A review digest is not a grant or a commit: acceptance must reauthenticate
//! these facts in the durable plan/job transaction before changing any identity.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{io::Read, path::Path};

use super::errors::{sqlite_storage_error, ErrorCode, ImportError};
use super::selection::{PreparedSelectionIdentity, PreviewSelection};
use super::{adapters, catalog, hash, jobs, registration_provenance, sources};

const VERSION: u32 = 1;
const MANIFEST_BYTES: u64 = 8 * 1024 * 1024;

#[path = "identity_association_plan.rs"]
mod plan;
pub(super) use plan::{
    bind_payload, parse_record, reject_behavior_options, review_confirmations, validate_bindings,
};
pub use plan::{AssociationConfirmation, AssociationPlanRequest};

#[path = "identity_association_witness.rs"]
mod witness;
pub(super) use witness::InspectionWitness;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssociationRequest {
    pub version: u32,
    pub import_id: String,
    pub expected_revision: String,
    pub preview: PreviewSelection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssociationReview {
    pub version: u32,
    pub import_id: String,
    pub expected_revision: String,
    pub preview_id: String,
    pub source_id: String,
    pub old_native_session_id: String,
    pub current_native_session_id: String,
    pub old_normalizer_version: String,
    pub current_normalizer_version: String,
    pub selection_hash: String,
    pub review_digest: String,
}

fn stale(message: &str) -> ImportError {
    ImportError::new(ErrorCode::PlanStale, message)
}
fn conflict(message: &str) -> ImportError {
    ImportError::new(ErrorCode::MappingConflict, message)
}
fn unsupported() -> ImportError {
    ImportError::new(
        ErrorCode::FormatUnsupported,
        "this historical identity pair requires a supported explicit association",
    )
}

impl AssociationRequest {
    fn validate(&self) -> Result<(), ImportError> {
        if self.version != VERSION {
            return Err(ImportError::new(
                if self.version > VERSION {
                    ErrorCode::ImportSchemaTooNew
                } else {
                    ErrorCode::SchemaUnsupported
                },
                "unsupported identity association request version",
            ));
        }
        let import_uuid = super::ids::parse_import_uuid(&self.import_id)?;
        if self.import_id != format!("imp:{import_uuid}")
            || !uuid::Uuid::parse_str(&self.expected_revision)
                .is_ok_and(|id| id.to_string() == self.expected_revision)
            || !uuid::Uuid::parse_str(&self.preview.preview_id)
                .is_ok_and(|id| id.to_string() == self.preview.preview_id)
        {
            return Err(stale(
                "association requires canonical import/revision/preview IDs",
            ));
        }
        super::selection::SelectionSet::single(self.preview.clone()).validate_shape()
    }
}

fn manifest(dir: &Path, expected_hash: &str) -> Result<Value, ImportError> {
    let path = dir.join("manifest.json");
    catalog::ensure_inside_imports(&path)?;
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .map_err(super::errors::io_storage_error)?
        .take(MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(super::errors::io_storage_error)?;
    if bytes.len() as u64 > MANIFEST_BYTES {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "association manifest exceeds byte limit",
        ));
    }
    if hash::sha256_bytes(&bytes) != expected_hash {
        return Err(stale(
            "association manifest differs from its authenticated hash",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|_| stale("association manifest is not valid JSON"))
}

fn manifest_version(value: &Value, normalizer: &str) -> Result<(), ImportError> {
    if value.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        || value.get("normalizerVersion").and_then(Value::as_str) != Some(normalizer)
        || value
            .pointer("/evidence/adapterVersion")
            .and_then(Value::as_str)
            != Some(normalizer)
        || value.pointer("/evidence/provider").and_then(Value::as_str) != Some("cursor")
        || value
            .pointer("/evidence/schemaFingerprint")
            .and_then(Value::as_str)
            != Some("cursor-transcript-jsonl-v1")
    {
        return Err(unsupported());
    }
    Ok(())
}

fn old_locator<'a>(receipt: &'a jobs::Receipt, old: &Value) -> Result<&'a str, ImportError> {
    let locator = receipt
        .legacy_id
        .strip_prefix("cursor-transcript:")
        .ok_or_else(unsupported)?;
    if Path::new(locator).file_stem().and_then(|s| s.to_str())
        != Some(receipt.native_session_id.as_str())
    {
        return Err(unsupported());
    }
    let artifacts = old
        .pointer("/evidence/artifacts")
        .and_then(Value::as_array)
        .ok_or_else(unsupported)?;
    let matching = artifacts
        .iter()
        .filter(|artifact| {
            artifact.get("kind").and_then(Value::as_str) == Some("cursor-transcript-jsonl-v1")
                && artifact
                    .get("artifactId")
                    .and_then(Value::as_str)
                    .is_some_and(|path| sources::same_location(path, locator))
        })
        .count();
    if matching != 1 {
        return Err(unsupported());
    }
    Ok(locator)
}

fn ensure_not_deleted(
    conn: &Connection,
    source: &str,
    provider: &str,
    natives: &[&str],
) -> Result<(), ImportError> {
    for native in natives {
        let deleted: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tombstones WHERE installation_id=?1 AND provider=?2 AND native_session_id=?3)",
            params![source, provider, native], |row| row.get(0),
        ).map_err(sqlite_storage_error)?;
        if deleted {
            return Err(conflict(
                "deleted identities require the explicit reimport workflow",
            ));
        }
    }
    Ok(())
}

/// Uses a detached read-only catalog snapshot, existing fixed-preview validation,
/// and existing published-history authorization. No identity/proof/job/lease write.
/// Normal derived index/verification caches and operation-lock files may be used.
pub fn inspect(request: &AssociationRequest) -> Result<AssociationReview, ImportError> {
    inspect_witness(request)?.review()
}

/// Detached authentication shared by public review and future atomic acceptance.
/// The witness retains operation protection, not a catalog transaction or grant.
pub(super) fn inspect_witness(
    request: &AssociationRequest,
) -> Result<InspectionWitness, ImportError> {
    request.validate()?;
    let _operation = super::preview_lifecycle::operation()?;
    let path = jobs::find_preview_dir(&request.preview.preview_id)?
        .ok_or_else(|| stale("association preview is unavailable"))?;
    let (lease, hashes) = jobs::verify_disk_preview_integrity(&path, &request.preview.preview_id)?;
    if lease.retained_by_job {
        return Err(ImportError::new(
            ErrorCode::SourceBusy,
            "association preview is already accepted by a job",
        ));
    }
    if lease.expires_unix <= chrono::Utc::now().timestamp() {
        return Err(stale("association preview expired"));
    }
    let verified = super::selection::validate_prepared_selection(
        &request.preview,
        &path,
        PreparedSelectionIdentity {
            preview_id: Some(&lease.preview_id),
            candidate_id: Some(&lease.candidate_id),
            source_id: Some(&lease.source_id),
            root_path: Some(&lease.root_path),
            legacy_id: Some(&lease.legacy_id),
            snapshot_digest: Some(&lease.snapshot_digest),
            provider: Some("cursor"),
            ..Default::default()
        },
    )?;
    let pin = verified.source_member.as_ref().ok_or_else(unsupported)?;
    pin.verify()?;
    let member = &pin.reference;
    if member.host != "local" || member.location.kind != "primary" || pin.proof != "artifact" {
        return Err(unsupported());
    }
    let fallback = adapters::cursor::TranscriptIdentity::new_scoped(
        Path::new(&member.locator),
        Some(Path::new(&verified.root_path)),
        member.location.identity_scope(),
    )?
    .finish();
    if verified.native_session_id != fallback {
        return Err(unsupported());
    }
    if super::types::NORMALIZER_VERSION != "1.0.6" {
        return Err(unsupported());
    }
    let current_manifest = manifest(&path, hashes.get("manifest.json").ok_or_else(unsupported)?)?;
    manifest_version(&current_manifest, super::types::NORMALIZER_VERSION)?;

    let conn = catalog::open_catalog_readonly()?;
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    super::source_locations::validate_registered_with(&tx, &member.location)?;
    let registration = sources::get(&tx, &verified.source_id)?;
    let registered_paths = sources::registered_paths(&tx, &registration)?;
    let entry = catalog::get_entry(&tx, &request.import_id)?
        .ok_or_else(|| conflict("association target is unavailable"))?;
    if entry.hidden {
        return Err(conflict("hidden history cannot be silently associated"));
    }
    if entry.current_revision.as_deref() != Some(&request.expected_revision) {
        return Err(stale("association target revision changed"));
    }
    if entry.provider != "cursor" || entry.installation_id != verified.source_id {
        return Err(conflict(
            "association requires the same registered Cursor installation",
        ));
    }
    let registered: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sources WHERE installation_id=?1 AND provider='cursor' AND host='local')",
        [&verified.source_id], |row| row.get(0),
    ).map_err(sqlite_storage_error)?;
    if !registered || uuid::Uuid::parse_str(&entry.installation_id).is_err() {
        return Err(conflict("association source host/provider is inconsistent"));
    }
    if super::registration_provenance_store::verify_schema(&tx)? {
        // Existence itself blocks another association, including a corrupt row
        // the strict proof reader refuses. Never replace an unreadable proof
        // or infer permission to create a new association chain.
        let present: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM registration_provenance WHERE import_id=?1)",
                [&entry.import_id],
                |row| row.get(0),
            )
            .map_err(sqlite_storage_error)?;
        if present {
            return Err(conflict("association chains require separate review"));
        }
    }
    ensure_not_deleted(
        &tx,
        &entry.installation_id,
        &entry.provider,
        &[&entry.native_session_id, &verified.native_session_id],
    )?;
    let existing: Option<String> = tx.query_row(
        "SELECT import_id FROM entries WHERE installation_id=?1 AND provider=?2 AND native_session_id=?3",
        params![entry.installation_id, entry.provider, verified.native_session_id], |row| row.get(0),
    ).optional().map_err(sqlite_storage_error)?;
    if existing.is_some() {
        return Err(conflict(
            "current native identity already belongs to an import",
        ));
    }
    let old_version: String = tx
        .query_row(
            "SELECT normalizer_version FROM revisions WHERE import_id=?1 AND revision_id=?2",
            params![entry.import_id, request.expected_revision],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    if old_version != "1.0.0" {
        return Err(unsupported());
    }
    let receipt = registration_provenance::receipt_store::latest(
        &tx,
        &entry.import_id,
        &request.expected_revision,
    )?
    .ok_or_else(|| stale("association target receipt is missing"))?;
    let anchor_row = registration_provenance::persisted_receipt(&tx, &receipt)?;
    jobs::load_published_auth_with(
        &tx,
        &entry.import_id,
        Some(&request.expected_revision),
        None,
    )?;
    let dir = catalog::revision_dir(&entry.import_id, &request.expected_revision)?;
    let old_manifest = manifest(
        &dir,
        receipt
            .file_hashes
            .get("manifest.json")
            .ok_or_else(unsupported)?,
    )?;
    manifest_version(&old_manifest, &old_version)?;
    old_locator(&receipt, &old_manifest)?;
    if !sources::locator_spellings(&receipt.legacy_id)
        .iter()
        .any(|old| sources::locator_spellings(&verified.legacy_id).contains(old))
        || receipt.source_id != verified.source_id
        || !sources::same_location(&receipt.root_path, &verified.root_path)
    {
        return Err(conflict(
            "historical and current source locators require explicit lineage evidence",
        ));
    }
    let receipt_cutoff = registration_provenance::cutoff(&tx, &entry.import_id)?;
    Ok(InspectionWitness {
        _operation,
        entry,
        expected_revision: request.expected_revision.clone(),
        preview: request.preview.clone(),
        validated: verified,
        prepared_hashes: hashes,
        receipt,
        anchor_row,
        receipt_cutoff,
        registration,
        registered_paths,
        old_normalizer: old_version,
    })
}

#[cfg(test)]
#[path = "identity_association_tests.rs"]
mod legacy_identity_tests;
