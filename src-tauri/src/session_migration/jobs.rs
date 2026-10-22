//! Recoverable import jobs: receipt → staging → publish → link → catalog commit.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(test)]
use std::sync::atomic::AtomicUsize;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::catalog::{
    find_entry, find_revision_by_snapshot, imports_dir, insert_revision, is_tombstoned,
    revision_dir, staging_dir, upsert_entry, upsert_source, CatalogEntry,
};
use super::errors::{io_storage_error, sqlite_storage_error, ErrorCode, ImportError};
use super::hash::{options_hash, sha256_bytes};
use super::ids::new_import_id;
use super::selection::{
    parse_live_selection_set, validate_prepared_selection, validate_published_selection,
    AllowedSelection, PreparedSelectionIdentity, SelectionSet,
};
use super::types::{AdapterOutput, ImportOutcome};

pub(crate) const DURABLE_SCHEMA_VERSION: u32 = 2;

#[path = "job_entry.rs"]
pub(super) mod entry;

#[path = "job_request_reader.rs"]
pub(super) mod request_reader;

#[path = "jobs_undo_ownership.rs"]
mod undo_ownership;

pub const EVENT_NAME: &str = "session-import://changed";

#[cfg(test)]
static PREVIEW_DIRECTORY_SCANS: AtomicUsize = AtomicUsize::new(0);

pub fn require_disk_space(available: u64, estimated_new: u64) -> Result<(), ImportError> {
    if super::budget::disk_sufficient(available, estimated_new) {
        Ok(())
    } else {
        Err(ImportError::new(
            ErrorCode::DiskSpaceLow,
            format!(
                "need {} bytes free, have {available}",
                estimated_new.saturating_add(1024 * 1024)
            ),
        ))
    }
}

pub fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let meta = entry.metadata().ok();
            if meta.as_ref().is_some_and(|m| m.is_dir()) {
                dir_size(&entry.path())
            } else {
                meta.map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

/// Bytes that will be written. Existing revisions stay on disk (coexistence).
pub fn estimated_publish_bytes(item: &CommitItem) -> u64 {
    let mut bytes = if let Some(prepared) = &item.prepared {
        dir_size(&prepared.path)
    } else {
        serde_json::to_vec(&item.adapter)
            .map(|b| b.len() as u64)
            .unwrap_or(0)
    };
    for asset in &item.adapter.assets {
        bytes = bytes.saturating_add(asset.size);
    }
    bytes
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Cancelling,
    Completed,
    CompletedWithIssues,
    Cancelled,
    Failed,
    RecoveryNeeded,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Completed => "completed",
            Self::CompletedWithIssues => "completed_with_issues",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::RecoveryNeeded => "recovery_needed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    pub done: u32,
    pub retryable: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRecord {
    pub job_id: String,
    pub request_id: String,
    pub plan_hash: String,
    #[serde(default)]
    pub plan_id: String,
    pub state: JobState,
    pub state_version: u32,
    pub items: Vec<JobItem>,
    pub created_at: String,
    #[serde(default)]
    pub item_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_item_cursor: Option<String>,
    /// Derived from the full durable job in the same snapshot as its page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<JobProgress>,
}

#[cfg(test)]
thread_local! {
    static JOB_ITEM_DESERIALIZE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub fn take_job_item_deserializes() -> u32 {
    JOB_ITEM_DESERIALIZE.with(|c| c.replace(0))
}

fn deserialize_candidate_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    #[cfg(test)]
    JOB_ITEM_DESERIALIZE.with(|c| c.set(c.get() + 1));
    String::deserialize(d)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobItem {
    #[serde(deserialize_with = "deserialize_candidate_id")]
    pub candidate_id: String,
    /// Stable display metadata captured with the durable job input. Older jobs
    /// may omit it and the UI keeps its existing ID fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub phase: String,
    pub import_id: Option<String>,
    pub revision_id: Option<String>,
    pub outcome: Option<String>,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping_project_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_member: Option<super::source_member::SourceMemberPin>,
    pub receipt_id: String,
    pub job_id: String,
    pub candidate_id: String,
    pub installation_id: String,
    pub provider: String,
    pub native_session_id: String,
    pub source_id: String,
    pub root_path: String,
    pub legacy_id: String,
    pub phase: String,
    pub import_id: Option<String>,
    pub revision_id: Option<String>,
    pub snapshot_hash: String,
    pub options_hash: String,
    pub selection_set: SelectionSet,
    pub selection_hash: String,
    pub previous_revision: Option<String>,
    pub created_entry: bool,
    pub file_hashes: std::collections::BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mapping_project_id: Option<String>,
    pub created_project: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name_at_link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_path_at_link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_pinned_at_link: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_trusted_at_link: Option<bool>,
}

static PUBLISH_LOCK: Mutex<()> = Mutex::new(());
static EVENT_SEQ: AtomicU64 = AtomicU64::new(1);

/// Accepted identity adoption shares publication's existing process/OS owners.
/// Fields release the OS owner before the process owner; no writer/store lock
/// is held while acquiring either, and contention never waits on the UI path.
pub(super) struct AdoptionPublicationLock {
    _file: super::job_control::ImportLock,
    _process: std::sync::MutexGuard<'static, ()>,
}

pub(super) fn try_adoption_publication_lock() -> Result<AdoptionPublicationLock, ImportError> {
    let process = PUBLISH_LOCK.try_lock().map_err(|error| match error {
        std::sync::TryLockError::WouldBlock => ImportError::new(
            ErrorCode::SourceBusy,
            "import publication is busy; retry the plan",
        ),
        std::sync::TryLockError::Poisoned(_) => {
            ImportError::new(ErrorCode::SnapshotUnavailable, "publish lock poisoned")
        }
    })?;
    let file = super::job_control::ImportLock::try_acquire("publish.lock")?;
    Ok(AdoptionPublicationLock {
        _file: file,
        _process: process,
    })
}

pub fn next_event_seq() -> u64 {
    EVENT_SEQ.fetch_add(1, Ordering::SeqCst)
}

pub fn find_job_by_request(
    conn: &Connection,
    request_id: &str,
) -> Result<Option<JobRecord>, ImportError> {
    let prepared = if conn.is_autocommit() {
        request_reader::PreparedRequest::prepare(conn, request_id)?
    } else {
        request_reader::PreparedRequest::legacy_snapshot(conn, request_id)?
    };
    prepared.read(conn, request_id)
}

// Production operation owners prepare and pass crypto explicitly before SQL.
#[cfg(test)]
pub fn get_job(conn: &Connection, job_id: &str) -> Result<Option<JobRecord>, ImportError> {
    let crypto = if conn.is_autocommit() {
        crate::session_migration::publication_crypto::JobCrypto::prepare(conn, job_id)?
    } else {
        crate::session_migration::publication_crypto::JobCrypto::legacy_snapshot(conn, job_id)?
    };
    get_job_with_crypto(conn, job_id, &crypto)
}

pub(in crate::session_migration) fn get_job_for_writer<'snapshot, 'connection>(
    tx: &'snapshot rusqlite::Transaction<'connection>,
    job_id: &str,
    crypto: &'snapshot crate::session_migration::publication_crypto::JobCrypto,
) -> Result<
    (
        Option<JobRecord>,
        entry::AuthenticatedWriterEntry<'snapshot, 'connection>,
    ),
    ImportError,
> {
    let entry = entry::authenticate_writer(tx, job_id, crypto)?;
    Ok((read_job_body(tx, job_id)?, entry))
}

pub(in crate::session_migration) fn get_job_with_crypto(
    conn: &Connection,
    job_id: &str,
    crypto: &crate::session_migration::publication_crypto::JobCrypto,
) -> Result<Option<JobRecord>, ImportError> {
    entry::read_with_crypto(conn, job_id, crypto, |conn| read_job_body(conn, job_id))
}

fn read_job_body(conn: &Connection, job_id: &str) -> Result<Option<JobRecord>, ImportError> {
    let json: Option<String> = conn
        .query_row(
            "SELECT json FROM jobs WHERE job_id = ?1",
            params![job_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    json.map(|s| {
        serde_json::from_str(&s).map_err(|e| {
            ImportError::new(ErrorCode::SnapshotUnavailable, format!("invalid job: {e}"))
        })
    })
    .transpose()
}

pub fn save_job(conn: &Connection, job: &JobRecord) -> Result<(), ImportError> {
    let json = serde_json::to_string(job)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    conn.execute(
        "INSERT INTO jobs(job_id, request_id, plan_hash, state, state_version, json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(job_id) DO UPDATE SET
           state=excluded.state,
           state_version=excluded.state_version,
           json=excluded.json",
        params![
            job.job_id,
            job.request_id,
            job.plan_hash,
            job.state.as_str(),
            job.state_version,
            json,
            job.created_at
        ],
    )
    .map_err(sqlite_storage_error)?;
    for item in &job.items {
        conn.execute(
            "UPDATE job_items SET phase=?3 WHERE job_id=?1 AND candidate_id=?2",
            params![job.job_id, item.candidate_id, item.phase],
        )
        .map_err(sqlite_storage_error)?;
    }
    Ok(())
}

pub fn write_receipt(conn: &Connection, receipt: &Receipt) -> Result<(), ImportError> {
    validate_receipt_shape(receipt)?;
    let json = serde_json::to_string(receipt)
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    conn.execute(
        "INSERT INTO receipts(receipt_id, job_id, candidate_id, phase, import_id, revision_id, json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            receipt.receipt_id,
            receipt.job_id,
            receipt.candidate_id,
            receipt.phase,
            receipt.import_id,
            receipt.revision_id,
            json,
            chrono::Utc::now().to_rfc3339()
        ],
    )
    .map_err(sqlite_storage_error)?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct CommitItem {
    pub candidate_id: String,
    pub installation_id: String,
    pub provider: String,
    pub root_path: String,
    pub adapter: AdapterOutput,
    pub prepared: Option<Arc<PreparedRevision>>,
    pub options: serde_json::Value,
    pub mapping_project_id: Option<String>,
    pub explicit_reimport: bool,
    pub create_project: bool,
    pub import_unassigned_anyway: bool,
    pub explicit_target: Option<String>,
    pub selection_set: SelectionSet,
    pub(crate) identity_association: Option<super::job_input_association::AssociationInput>,
}

/// Normalized files created by the Host, never a source application's path.
/// An Arc keeps the preview alive while a queued/running job consumes it.
#[derive(Debug)]
pub struct PreparedRevision {
    pub path: PathBuf,
    pub digest: String,
    pub file_hashes: std::collections::BTreeMap<String, String>,
    owned_parent: PathBuf,
    retained_by_job: AtomicBool,
}

impl PreparedRevision {
    pub(crate) fn from_owned(
        path: PathBuf,
        digest: String,
        file_hashes: std::collections::BTreeMap<String, String>,
        owned_parent: PathBuf,
    ) -> Arc<Self> {
        Arc::new(Self {
            path,
            digest,
            file_hashes,
            owned_parent,
            retained_by_job: AtomicBool::new(false),
        })
    }

    pub fn retain_for_job(&self) {
        self.retained_by_job.store(true, Ordering::Release);
    }

    pub fn reopen(
        path: PathBuf,
        digest: String,
        file_hashes: std::collections::BTreeMap<String, String>,
    ) -> Result<Arc<Self>, ImportError> {
        super::catalog::ensure_inside_imports(&path)?;
        let parent = staging_dir().join("previews");
        if path.parent() != Some(parent.as_path())
            || !path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| Uuid::parse_str(n).is_ok())
        {
            return Err(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "invalid preview lease",
            ));
        }
        Ok(Arc::new(Self {
            path,
            digest,
            file_hashes,
            owned_parent: parent,
            retained_by_job: AtomicBool::new(true),
        }))
    }
}

impl Drop for PreparedRevision {
    fn drop(&mut self) {
        if !self.retained_by_job.load(Ordering::Acquire)
            && owned_preview_dir(&self.path)
            && self.path.parent() == Some(self.owned_parent.as_path())
            && self
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| Uuid::parse_str(s).is_ok())
        {
            let Ok(Some(_cleanup)) = super::preview_lifecycle::try_cleanup() else {
                return;
            };
            if preview_lease_is_live(&self.path) {
                return;
            }
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewLease {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_member: Option<super::source_member::SourceMemberPin>,
    pub preview_id: String,
    pub candidate_id: String,
    #[serde(default)]
    pub source_revision: String,
    #[serde(default)]
    pub prepare_batch_id: Option<String>,
    pub source_id: String,
    pub root_path: String,
    pub legacy_id: String,
    pub snapshot_digest: String,
    pub expires_unix: i64,
    pub retained_by_job: bool,
    #[serde(default)]
    pub file_hashes: std::collections::BTreeMap<String, String>,
}

pub(super) fn owned_preview_dir(path: &Path) -> bool {
    let parent = staging_dir().join("previews");
    super::catalog::ensure_inside_imports(path).is_ok()
        && path.parent() == Some(parent.as_path())
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| Uuid::parse_str(n).is_ok())
}

pub fn write_preview_lease(dir: &Path, lease: &PreviewLease) -> Result<(), ImportError> {
    super::preview_lease_io::write(dir, lease)
}

pub fn verify_disk_preview_integrity(
    dir: &Path,
    expected_preview_id: &str,
) -> Result<(PreviewLease, std::collections::BTreeMap<String, String>), ImportError> {
    let lease = read_preview_lease(dir)?
        .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "preview lease missing"))?;
    if lease.preview_id != expected_preview_id {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "preview lease identity mismatch",
        ));
    }
    if lease.file_hashes.is_empty() {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "preview lease missing file hashes",
        ));
    }
    let hashes = prepared_hashes(dir)?;
    if hashes != lease.file_hashes {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "preview file hash mismatch",
        ));
    }
    let manifest_raw = fs::read(dir.join("manifest.json"))
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_raw).map_err(|e| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            format!("preview manifest is not json: {e}"),
        )
    })?;
    let snapshot = manifest
        .get("sourceSnapshotId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if snapshot != lease.snapshot_digest {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "preview snapshot digest mismatch",
        ));
    }
    Ok((lease, hashes))
}

pub fn read_preview_lease(dir: &Path) -> Result<Option<PreviewLease>, ImportError> {
    super::preview_lease_io::read(dir)
}

fn preview_lease_is_live(dir: &Path) -> bool {
    // Drop cannot reconcile a receipt or report its indeterminate state. Leave
    // that work to explicit cleanup under lifecycle + acceptance protection.
    if super::job_control::preview_retention::pending() {
        return true;
    }
    match read_preview_lease(dir) {
        // A successfully prepared revision without a lease is still owned
        // only by this in-memory value and may be discarded. An unreadable
        // lease is an indeterminate ownership state; Drop cannot return the
        // I/O error, so keep the snapshot and let explicit cleanup report or
        // retry it instead of deleting it.
        Ok(None) => false,
        Err(_) => true,
        Ok(Some(lease)) => {
            lease.retained_by_job
                || lease.expires_unix > chrono::Utc::now().timestamp()
                || lease.prepare_batch_id.as_deref().is_some_and(|batch_id| {
                    super::prepare::preview_is_retained(batch_id, &lease.preview_id)
                })
        }
    }
}

pub fn renew_preview_lease(dir: &Path, ttl_secs: u64) -> Result<(), ImportError> {
    super::preview_lease_io::update(dir, |lease| {
        lease.expires_unix = chrono::Utc::now().timestamp() + ttl_secs as i64;
    })
}

#[cfg(test)]
pub fn mark_preview_retained(dir: &Path) -> Result<(), ImportError> {
    super::preview_lease_io::update(dir, |lease| lease.retained_by_job = true)
}

#[cfg(test)]
pub fn expire_preview_lease_file(dir: &Path) -> Result<(), ImportError> {
    super::preview_lease_io::update(dir, |lease| lease.expires_unix = 0)
}

pub(crate) fn adopt_preview_for_batch(dir: &Path, batch_id: &str) -> Result<(), ImportError> {
    super::preview_lease_io::update(dir, |lease| {
        lease.prepare_batch_id = Some(batch_id.to_string());
    })
}

fn referenced_preview_dirs(
    conn: &Connection,
) -> Result<std::collections::BTreeSet<PathBuf>, ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return referenced_preview_dirs(&tx);
    }
    // Only project the retention reference, not the full durable input. Invalid
    // or missing references cannot prove that any preview is safe to delete.
    let mut stmt = conn
        .prepare(
            "SELECT job_id,candidate_id FROM job_items
             WHERE phase IN ('queued','running','recovery_needed','failed','cancelled')",
        )
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
    let mut out = std::collections::BTreeSet::new();
    for row in rows {
        let (job_id, candidate_id) =
            row.map_err(|e| ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string()))?;
        out.insert(super::job_control::retained_preview_path(
            conn,
            &job_id,
            &candidate_id,
        )?);
    }
    Ok(out)
}

#[path = "job_preview_cleanup.rs"]
mod preview_cleanup;
pub use preview_cleanup::cleanup_unreferenced_preview_dirs;

pub fn find_preview_dir(preview_id: &str) -> Result<Option<PathBuf>, ImportError> {
    let parsed = Uuid::parse_str(preview_id)
        .map_err(|_| ImportError::new(ErrorCode::PlanStale, "invalid preview identity"))?;
    if parsed.to_string() != preview_id {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "non-canonical preview identity",
        ));
    }
    let parent = staging_dir().join("previews");
    if !parent.is_dir() {
        return Ok(None);
    }
    super::catalog::ensure_inside_imports(&parent)?;
    let path = parent.join(preview_id);
    super::catalog::ensure_inside_imports(&path)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                error.to_string(),
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() || !owned_preview_dir(&path) {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "preview path is not an owned directory",
        ));
    }
    Ok(read_preview_lease(&path)?
        .is_some_and(|lease| lease.preview_id == preview_id)
        .then_some(path))
}

#[cfg(test)]
pub(crate) fn reset_preview_directory_scans() {
    PREVIEW_DIRECTORY_SCANS.store(0, Ordering::SeqCst);
}

#[cfg(test)]
pub(crate) fn preview_directory_scans() -> usize {
    PREVIEW_DIRECTORY_SCANS.load(Ordering::SeqCst)
}

pub fn prepare_revision(
    adapter: &mut AdapterOutput,
    options: &serde_json::Value,
    source_root: Option<&Path>,
    preview_directory: Option<&str>,
) -> Result<Arc<PreparedRevision>, ImportError> {
    let mut builder = match preview_directory {
        Some(directory_id) => {
            super::prepared_builder::PreparedRevisionBuilder::new_in_preview_dir(directory_id)?
        }
        None => super::prepared_builder::PreparedRevisionBuilder::new()?,
    };
    if let Some(root) = source_root {
        super::assets::materialize(adapter, root, &builder.blobs_path(), &|| false)?;
    } else {
        // Even without a copy root, pending references cannot be published as
        // complete. Graph validation deliberately leaves them pending.
        adapter.recount();
    }
    for event in &adapter.graph.events {
        builder.push_event(event)?;
    }
    Ok(builder.finalize(adapter, options)?.prepared)
}

pub(super) fn revision_semantic_key(options_digest: &str, selection_hash: &str) -> String {
    options_hash(&serde_json::json!({
        "normalizerVersion": super::types::NORMALIZER_VERSION,
        "optionsHash": options_digest,
        "selectionHash": selection_hash,
    }))
}

#[path = "jobs_revision_identity.rs"]
pub(super) mod revision_identity;

pub(super) fn validate_receipt_shape(receipt: &Receipt) -> Result<(), ImportError> {
    if receipt.version != DURABLE_SCHEMA_VERSION {
        return Err(ImportError::new(
            ErrorCode::SchemaUnsupported,
            "unsupported receipt schema; create a new import preview",
        ));
    }
    let entry = receipt.selection_set.single_entry()?;
    super::source_member::validate_record(
        receipt.source_member.as_ref(),
        super::source_member::RecordIdentity {
            candidate_id: &receipt.candidate_id,
            source_id: &receipt.source_id,
            provider: &receipt.provider,
            root_path: &receipt.root_path,
            legacy_id: &receipt.legacy_id,
            native_session_id: &receipt.native_session_id,
        },
    )?;
    if receipt.selection_hash != receipt.selection_set.hash()
        || receipt.snapshot_hash.trim().is_empty()
        || receipt.options_hash.trim().is_empty()
        || receipt.candidate_id.trim().is_empty()
        || receipt.installation_id.trim().is_empty()
        || receipt.provider.trim().is_empty()
        || receipt.native_session_id.trim().is_empty()
        || receipt.source_id.trim().is_empty()
        || receipt.root_path.trim().is_empty()
        || receipt.legacy_id.trim().is_empty()
        || entry.preview_id.trim().is_empty()
    {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "receipt identity or selection hash is invalid",
        ));
    }
    Ok(())
}

pub(in crate::session_migration) fn validate_receipt_for_item(
    receipt: &Receipt,
    item: &CommitItem,
    selection_set: &SelectionSet,
    selection_hash: &str,
    options_hash: &str,
    verified: &super::selection::PreparedSelectionValidation,
) -> Result<(), ImportError> {
    validate_receipt_shape(receipt)?;
    if receipt.job_id.trim().is_empty()
        || receipt.source_member != verified.source_member
        || receipt.candidate_id != item.candidate_id
        || receipt.installation_id != item.installation_id
        || receipt.provider != item.provider
        || receipt.native_session_id != item.adapter.graph.native_session_id
        || receipt.source_id != verified.source_id
        || receipt.root_path != verified.root_path
        || receipt.legacy_id != verified.legacy_id
        || receipt.snapshot_hash != verified.snapshot_digest
        || receipt.options_hash != options_hash
        || receipt.selection_hash != selection_hash
        || receipt.selection_set.canonicalized() != *selection_set
    {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "receipt does not match the immutable job input",
        ));
    }
    Ok(())
}

// Rebuilding staging must not replace an already durable authorization map.
fn validate_staged_hashes(
    receipt: &Receipt,
    file_hashes: &std::collections::BTreeMap<String, String>,
) -> Result<(), ImportError> {
    if receipt.phase == "staged" && &receipt.file_hashes != file_hashes {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "rebuilt staging does not match its durable receipt",
        ));
    }
    Ok(())
}

/// Published revision SelectionSet is the only model-facing authorization.
#[derive(Debug, Clone)]
pub struct PublishedAuth {
    // Retain the verified owner identity in the authorization result even
    // though current production consumers already hold the same import id.
    #[allow(dead_code)]
    pub import_id: String,
    pub revision_id: String,
    pub leaf_id: String,
    pub selection_set: SelectionSet,
    pub selection: AllowedSelection,
}

pub fn load_published_auth(
    import_id: &str,
    revision: Option<&str>,
    leaf: Option<&str>,
) -> Result<PublishedAuth, ImportError> {
    let conn = super::catalog::open_catalog()?;
    load_published_auth_with(&conn, import_id, revision, leaf)
}

pub fn load_published_auth_with(
    conn: &Connection,
    import_id: &str,
    revision: Option<&str>,
    leaf: Option<&str>,
) -> Result<PublishedAuth, ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return load_published_auth_with(&tx, import_id, revision, leaf);
    }
    let entry = super::catalog::get_entry(conn, import_id)?
        .ok_or_else(|| ImportError::new(ErrorCode::SourceNotFound, "import not found"))?;
    if entry.hidden {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "imported history is hidden",
        ));
    }
    published_auth_from_entry(conn, &entry, import_id, revision, leaf)
}

/// Receipt + identity + file-hash check used by publish dedup.
/// Hidden catalog rows are allowed so explicit reimport can restore after
/// verification; model-facing callers must go through `load_published_auth_with`.
pub(super) fn published_auth_from_entry(
    conn: &Connection,
    entry: &CatalogEntry,
    import_id: &str,
    revision: Option<&str>,
    leaf: Option<&str>,
) -> Result<PublishedAuth, ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return published_auth_from_entry(&tx, entry, import_id, revision, leaf);
    }
    let revision_id = revision
        .map(str::to_string)
        .or(entry.current_revision.clone())
        .ok_or_else(|| ImportError::new(ErrorCode::SnapshotUnavailable, "no revision"))?;
    let revision_row =
        super::catalog::get_revision(conn, import_id, &revision_id)?.ok_or_else(|| {
            ImportError::new(ErrorCode::SnapshotUnavailable, "revision not in catalog")
        })?;
    // Only the new accepted association lane gets the scoped bounded point.
    // Ordinary/v1 historical capacity remains its existing reader contract.
    let receipt = if matches!(
        super::registration_provenance_store::get(conn, import_id)?,
        Some(super::registration_provenance::ProofRecord::Association(_))
    ) {
        super::registration_provenance::receipt_store::latest(conn, import_id, &revision_id)?
    } else {
        latest_receipt_for_revision(conn, import_id, &revision_id)?
    }
    .ok_or_else(|| {
        ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "published receipt missing",
        )
    })?;
    validate_receipt_shape(&receipt)?;
    if receipt.import_id.as_deref() != Some(import_id)
        || receipt.revision_id.as_deref() != Some(revision_id.as_str())
        || receipt.snapshot_hash != revision_row.source_snapshot_id
    {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "catalog, revision, and receipt identity do not match",
        ));
    }
    super::registration_provenance::authorize(conn, entry, &receipt)?;
    if !receipt.file_hashes.contains_key("selection.json")
        || !receipt.file_hashes.contains_key("events.jsonl")
        || !receipt.file_hashes.contains_key("manifest.json")
    {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "published receipt is missing required file hashes",
        ));
    }
    let dir = revision_dir(import_id, &revision_id)?;
    let selection_set = verify_published_for_read(&dir, &receipt)?;
    let (leaf_id, selection) = if let Some(leaf) = leaf.filter(|value| !value.is_empty()) {
        let preview = selection_set
            .entries
            .iter()
            .find(|entry| entry.leaf_id == leaf)
            .ok_or_else(|| {
                ImportError::new(
                    ErrorCode::HistoryAccessUnavailable,
                    "published selection does not include this leaf",
                )
            })?;
        (preview.leaf_id.clone(), preview.selection.clone())
    } else {
        let preview = selection_set.single_entry()?;
        (preview.leaf_id.clone(), preview.selection.clone())
    };
    Ok(PublishedAuth {
        import_id: import_id.to_string(),
        revision_id,
        leaf_id,
        selection,
        selection_set,
    })
}

fn write_selection_file(dir: &Path, selection_set: &SelectionSet) -> Result<(), ImportError> {
    selection_set.single_entry()?;
    let path = dir.join("selection.json");
    super::catalog::ensure_inside_imports(&path)?;
    let bytes = serde_json::to_vec_pretty(selection_set)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    let mut file = fs::File::create(&path).map_err(io_storage_error)?;
    #[cfg(test)]
    tests::maybe_fail_publish_io("selection_write", &path).map_err(io_storage_error)?;
    file.write_all(&bytes).map_err(io_storage_error)?;
    #[cfg(test)]
    tests::maybe_fail_publish_io("selection_sync", &path).map_err(io_storage_error)?;
    file.sync_all().map_err(io_storage_error)?;
    Ok(())
}

#[path = "jobs_publish.rs"]
mod jobs_publish;
pub(crate) use super::published_verification::verify_published_for_read;
#[cfg(test)]
pub use super::published_verification::{
    clear_published_read_verification_cache, clear_published_read_verification_proofs,
    take_full_published_read_verifications,
};
pub(in crate::session_migration) use jobs_publish::repair_fallback_title as repair_deduplicated_title;
pub use jobs_publish::*;

#[cfg(test)]
mod tests {
    use super::*;
    include!("jobs_tests.rs");
    include!("jobs_staged_retry_tests.rs");
    include!("jobs_io_fault_tests.rs");
}
