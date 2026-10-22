//! Durable job inputs and short state transactions, separate from publication.

use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::sync::Arc;

use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::catalog::{imports_dir, open_catalog};
use super::errors::{ErrorCode, ImportError};
use super::jobs::{self, CommitItem, JobItem, JobRecord, JobState, PreparedRevision, Receipt};
use super::selection::{validate_prepared_selection, PreparedSelectionIdentity, SelectionSet};
use super::types::AdapterOutput;

#[path = "job_association_acceptance.rs"]
pub(super) mod association_acceptance;
#[path = "job_association_context.rs"]
mod association_context;
#[path = "job_association_members.rs"]
pub(super) mod association_members;
#[path = "job_association_publication.rs"]
pub(super) mod association_publication;
#[path = "job_durable_reader.rs"]
mod durable_reader;
#[path = "job_input_reader.rs"]
pub(super) mod input_reader;
#[path = "job_input_wire.rs"]
mod input_wire;
#[path = "job_recovery_input.rs"]
mod recovery_input;
pub(super) use recovery_input::{with_stored_recovery_input, RecoveryInput};
#[path = "job_preview_retention.rs"]
pub(super) mod preview_retention;
#[path = "job_provenance_scope.rs"]
pub(super) mod provenance_scope;
#[path = "job_publication_acceptance.rs"]
pub(super) mod publication_acceptance;
#[path = "job_worker.rs"]
mod worker;

#[cfg(test)]
pub(super) fn test_preview_acceptance_owner(held: impl FnOnce()) -> Result<(), ImportError> {
    let _operation = super::preview_lifecycle::operation()?;
    let _owner = preview_retention::PendingInputs::acquire()?;
    held();
    Ok(())
}

static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

pub fn attach(app: tauri::AppHandle) {
    let _ = APP.set(app);
}

/// Notify catalog browsers after workspace metadata commits. Reader anchors use
/// their own lane and must not be invalidated by organization-only edits.
pub(crate) fn workspace_changed() {
    use tauri::Emitter;
    if let Some(app) = APP.get() {
        let _ = app.emit("session-import://workspace-changed", ());
    }
}

fn changed(job: &JobRecord) {
    changed_version(&job.job_id, job.state_version);
}

pub(super) fn changed_version(job_id: &str, state_version: u32) {
    use tauri::Emitter;
    if let Some(app) = APP.get() {
        let _ = app.emit(
            jobs::EVENT_NAME,
            serde_json::json!({
                "jobId": job_id, "stateVersion": state_version,
                "eventSeq": jobs::next_event_seq(),
            }),
        );
    }
}

fn failure(e: impl std::fmt::Display) -> ImportError {
    ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string())
}

/// OS lock lives in the import domain; never nests the app's project store lock.
pub struct ImportLock(File);

impl ImportLock {
    pub fn acquire(name: &str) -> Result<Self, ImportError> {
        Self::acquire_inner(name, false)
    }

    pub(super) fn try_acquire(name: &str) -> Result<Self, ImportError> {
        Self::acquire_inner(name, true)
    }

    fn acquire_inner(name: &str, nonblocking: bool) -> Result<Self, ImportError> {
        if !matches!(name, "publish.lock" | "worker.lock") {
            return Err(failure("invalid import lock name"));
        }
        let path = imports_dir().join(name);
        std::fs::create_dir_all(imports_dir()).map_err(failure)?;
        super::catalog::ensure_inside_imports(&path)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| failure(format!("lock {}: {e}", path.display())))?;
        use crate::native_soak_telemetry::{observe_import_os_lock, ImportOsLockKind};
        let kind = if name == "publish.lock" {
            ImportOsLockKind::Publish
        } else {
            ImportOsLockKind::Worker
        };
        observe_import_os_lock(kind, || {
            if nonblocking {
                fs2::FileExt::try_lock_exclusive(&file)
            } else {
                fs2::FileExt::lock_exclusive(&file)
            }
        })
        .map_err(|e| {
            if nonblocking
                && (e.kind() == std::io::ErrorKind::WouldBlock
                    || e.raw_os_error() == fs2::lock_contended_error().raw_os_error())
            {
                ImportError::new(
                    ErrorCode::SourceBusy,
                    "import publication is busy; retry the plan",
                )
            } else {
                failure(format!("exclusive lock {}: {e}", path.display()))
            }
        })?;
        Ok(Self(file))
    }
}

impl Drop for ImportLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DurableInput {
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_member: Option<super::source_member::SourceMemberPin>,
    candidate_id: String,
    installation_id: String,
    provider: String,
    source_id: String,
    root_path: String,
    legacy_id: String,
    // Only metadata: events have already been written to the leased preview.
    adapter: AdapterOutput,
    preview_path: PathBuf,
    preview_digest: String,
    preview_file_hashes: std::collections::BTreeMap<String, String>,
    options: serde_json::Value,
    mapping_project_id: Option<String>,
    explicit_reimport: bool,
    create_project: bool,
    import_unassigned_anyway: bool,
    explicit_target: Option<String>,
    selection_set: SelectionSet,
    selection_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    identity_association: Option<super::job_input_association::AssociationInput>,
}

impl DurableInput {
    fn parse(json: &str) -> Result<Self, ImportError> {
        input_wire::inspect(json)?;
        let input: Self = serde_json::from_str(json).map_err(input_wire::unsupported)?;
        input.validate_shape()?;
        Ok(input)
    }

    fn registration_identity(
        &self,
    ) -> Result<super::registration_provenance::InputIdentity, ImportError> {
        super::job_input_association::refuse_publication(self.identity_association.as_ref())?;
        self.input_identity()
    }

    fn input_identity(&self) -> Result<super::registration_provenance::InputIdentity, ImportError> {
        if self.installation_id != self.source_id {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "registered enqueue source identity mismatch",
            ));
        }
        Ok(super::registration_provenance::InputIdentity {
            candidate_id: self.candidate_id.clone(),
            source_id: self.source_id.clone(),
            provider: self.provider.clone(),
            native_session_id: self.adapter.graph.native_session_id.clone(),
            legacy_id: self.legacy_id.clone(),
            digest: super::hash::sha256_bytes(
                serde_json::to_string(self).map_err(failure)?.as_bytes(),
            ),
        })
    }

    fn from_item(item: &CommitItem) -> Result<(Self, Arc<PreparedRevision>), ImportError> {
        super::identity_association::reject_behavior_options(&item.options)?;
        item.selection_set.validate_shape()?;
        let entry = item.selection_set.single_entry()?;
        let prepared = item.prepared.clone().ok_or_else(|| {
            ImportError::new(
                ErrorCode::PlanStale,
                "durable input requires an immutable prepared preview",
            )
        })?;
        let actual_hashes = jobs::prepared_hashes(&prepared.path)?;
        if actual_hashes != prepared.file_hashes {
            return Err(ImportError::new(
                ErrorCode::SourceChanged,
                "fixed preview was modified before enqueue",
            ));
        }
        let verified = validate_prepared_selection(
            entry,
            &prepared.path,
            PreparedSelectionIdentity {
                preview_id: Some(&entry.preview_id),
                candidate_id: Some(&item.candidate_id),
                root_path: Some(&item.root_path),
                snapshot_digest: Some(&prepared.digest),
                provider: Some(&item.provider),
                native_session_id: Some(&item.adapter.graph.native_session_id),
                ..PreparedSelectionIdentity::default()
            },
        )?;
        super::mapping_scope::validate_options(
            &item.options,
            &super::mapping_scope::MappingSubject::new(
                verified.source_id.clone(),
                verified.provider,
                verified.cwd,
            ),
        )?;
        let selection_set = if item.identity_association.is_some() {
            item.selection_set.clone()
        } else {
            item.selection_set.canonicalized()
        };
        let selection_hash = selection_set.hash();
        let mut adapter = item.adapter.clone();
        adapter.graph.events.clear();
        let input = Self {
            version: if item.identity_association.is_some() {
                input_wire::ASSOCIATION_VERSION
            } else {
                input_wire::ORDINARY_VERSION
            },
            source_member: verified.source_member,
            candidate_id: item.candidate_id.clone(),
            installation_id: item.installation_id.clone(),
            provider: item.provider.clone(),
            source_id: verified.source_id,
            root_path: item.root_path.clone(),
            legacy_id: verified.legacy_id,
            adapter,
            preview_path: prepared.path.clone(),
            preview_digest: prepared.digest.clone(),
            preview_file_hashes: prepared.file_hashes.clone(),
            options: item.options.clone(),
            mapping_project_id: item.mapping_project_id.clone(),
            explicit_reimport: item.explicit_reimport,
            create_project: item.create_project,
            import_unassigned_anyway: item.import_unassigned_anyway,
            explicit_target: item.explicit_target.clone(),
            selection_set,
            selection_hash,
            identity_association: item.identity_association.clone(),
        };
        input.validate_shape()?;
        Ok((input, prepared))
    }

    fn validate_shape(&self) -> Result<(), ImportError> {
        input_wire::validate_version(self.version, self.identity_association.is_some())?;
        super::identity_association::reject_behavior_options(&self.options)?;
        self.selection_set.validate_shape()?;
        if let Some(association) = &self.identity_association {
            association.validate(&self.selection_set)?;
            if self.installation_id != self.source_id {
                return Err(ImportError::new(
                    ErrorCode::PlanStale,
                    "association source identity mismatch",
                ));
            }
        }
        if self.selection_set.hash() != self.selection_hash {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "durable selection hash mismatch",
            ));
        }
        super::source_member::validate_record(
            self.source_member.as_ref(),
            super::source_member::RecordIdentity {
                candidate_id: &self.candidate_id,
                source_id: &self.source_id,
                provider: &self.provider,
                root_path: &self.root_path,
                legacy_id: &self.legacy_id,
                native_session_id: &self.adapter.graph.native_session_id,
            },
        )?;
        super::mapping_scope::validate_options(
            &self.options,
            &super::mapping_scope::MappingSubject::new(
                self.source_id.clone(),
                self.provider.clone(),
                self.adapter.graph.cwd.clone(),
            ),
        )?;
        Ok(())
    }

    fn into_item(self) -> Result<CommitItem, ImportError> {
        self.validate_shape()?;
        let entry = self.selection_set.single_entry()?;
        let prepared = PreparedRevision::reopen(
            self.preview_path,
            self.preview_digest,
            self.preview_file_hashes,
        )?;
        let actual_hashes = jobs::prepared_hashes(&prepared.path)?;
        if actual_hashes != prepared.file_hashes {
            return Err(ImportError::new(
                ErrorCode::SourceChanged,
                "durable prepared preview was modified",
            ));
        }
        let verified = validate_prepared_selection(
            entry,
            &prepared.path,
            PreparedSelectionIdentity {
                preview_id: Some(&entry.preview_id),
                candidate_id: Some(&self.candidate_id),
                source_id: Some(&self.source_id),
                root_path: Some(&self.root_path),
                legacy_id: Some(&self.legacy_id),
                snapshot_digest: Some(&prepared.digest),
                provider: Some(&self.provider),
                native_session_id: Some(&self.adapter.graph.native_session_id),
            },
        )?;
        if verified.source_member != self.source_member {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "durable source member differs from the immutable preview",
            ));
        }
        super::mapping_scope::validate_options(
            &self.options,
            &super::mapping_scope::MappingSubject::new(
                verified.source_id,
                verified.provider,
                verified.cwd,
            ),
        )?;
        let selection_set = if self.identity_association.is_some() {
            self.selection_set.clone()
        } else {
            self.selection_set.canonicalized()
        };
        Ok(CommitItem {
            candidate_id: self.candidate_id,
            installation_id: self.installation_id,
            provider: self.provider,
            root_path: self.root_path,
            adapter: self.adapter,
            prepared: Some(prepared),
            options: self.options,
            mapping_project_id: self.mapping_project_id,
            explicit_reimport: self.explicit_reimport,
            create_project: self.create_project,
            import_unassigned_anyway: self.import_unassigned_anyway,
            explicit_target: self.explicit_target,
            selection_set,
            identity_association: self.identity_association,
        })
    }

    /// Intent metadata only. The accepted proof/input digest is still required
    /// before this could authorize anything; no TTL or retained-preview inspect.
    fn validate_context(&self, conn: &Connection, job: &JobRecord) -> Result<(), ImportError> {
        let Some(association) = &self.identity_association else {
            return Ok(());
        };
        association.require_job(job)?;
        let plan = super::engine::association_input_plan(
            conn,
            &association.plan_id,
            &association.plan_hash,
        )?;
        association_context::validate(self, job, &plan)
    }
}

#[cfg(test)]
pub(in crate::session_migration) fn encode_input(item: &CommitItem) -> Result<String, ImportError> {
    let (input, _) = DurableInput::from_item(item)?;
    serde_json::to_string(&input).map_err(failure)
}

/// Metadata-only parser for primitive recovery tests; never an acceptance,
/// source/proof grant, authenticated owner, or a V3 rollout bypass.
#[cfg(test)]
pub(in crate::session_migration) fn inspect_recovery_input<T>(
    json: &str,
    inspect: impl FnOnce(RecoveryInput<'_>) -> T,
) -> Result<T, ImportError> {
    let input = DurableInput::parse(json)?;
    Ok(inspect(input.recovery_input()))
}

#[cfg(test)]
pub(in crate::session_migration) fn decode_input(
    json: &str,
    job: &JobRecord,
) -> Result<CommitItem, ImportError> {
    let input = DurableInput::parse(json)?;
    input.validate_context(&open_catalog()?, job)?;
    input.into_item()
}

#[cfg(test)]
pub(in crate::session_migration) fn inspect_input_metadata(
    json: &str,
    job: &JobRecord,
) -> Result<String, ImportError> {
    let input = DurableInput::parse(json)?;
    input.validate_context(&open_catalog()?, job)?;
    Ok(super::hash::sha256_bytes(json.as_bytes()))
}

/// Test-only protocol evidence; this never authorizes or accepts a job.
#[cfg(test)]
pub(in crate::session_migration) fn inspect_association_draft_input(
    json: &str,
    job: &JobRecord,
) -> Result<
    (
        super::job_input_association::AssociationInput,
        super::registration_provenance::InputIdentity,
    ),
    ImportError,
> {
    let input = DurableInput::parse(json)?;
    input.validate_context(&open_catalog()?, job)?;
    let association = input.identity_association.ok_or_else(|| {
        ImportError::new(
            ErrorCode::PlanStale,
            "protocol draft requires explicit association input",
        )
    })?;
    Ok((
        association,
        super::registration_provenance::InputIdentity {
            candidate_id: input.candidate_id,
            source_id: input.source_id,
            provider: input.provider,
            native_session_id: input.adapter.graph.native_session_id,
            legacy_id: input.legacy_id,
            digest: super::hash::sha256_bytes(json.as_bytes()),
        },
    ))
}

#[cfg(test)]
pub(in crate::session_migration) fn validate_committed_input_metadata(
    json: &str,
    job_id: &str,
    receipt: &Receipt,
) -> Result<(), ImportError> {
    DurableInput::parse(json)?.validate_committed_receipt(job_id, receipt)
}

fn state_tx(conn: &Connection) -> Result<Transaction<'_>, ImportError> {
    Transaction::new_unchecked(conn, TransactionBehavior::Immediate).map_err(failure)
}

#[cfg(test)]
pub fn enqueue(job: JobRecord, items: Vec<CommitItem>) -> Result<JobRecord, ImportError> {
    enqueue_inner(job, items, association_acceptance::Lane::Ordinary)
}

pub(crate) fn enqueue_registered(
    job: JobRecord,
    items: Vec<CommitItem>,
) -> Result<JobRecord, ImportError> {
    enqueue_inner(job, items, association_acceptance::Lane::Registered)
}

/// Acceptance for an explicitly reviewed durable association plan. Rechecks
/// its full membership, target and review before the atomic identity/job write.
pub(in crate::session_migration) fn enqueue_confirmed_associations(
    job: JobRecord,
    items: Vec<CommitItem>,
) -> Result<JobRecord, ImportError> {
    enqueue_inner(
        job,
        items,
        association_acceptance::Lane::ConfirmedAssociations,
    )
}

fn enqueue_inner(
    mut job: JobRecord,
    items: Vec<CommitItem>,
    lane: association_acceptance::Lane,
) -> Result<JobRecord, ImportError> {
    let protocol = super::publication_protocol::PublicationProtocol::for_job_id(&job.job_id)?;
    for item in &items {
        lane.require_item(item)?;
    }
    let operation = super::preview_lifecycle::operation()?;
    let protection = super::preview_lifecycle::Protection::Operation(&operation);
    let conn = open_catalog()?;
    let mut retention = preview_retention::PendingInputs::acquire()?;
    if let Some(existing) = retention.existing(&conn, &job)? {
        return Ok(existing);
    }
    retention.reconcile(&conn, &protection)?;
    // Fix inputs before taking the short SQLite write transaction.
    let prepared: Vec<_> = items
        .iter()
        .map(DurableInput::from_item)
        .collect::<Result<_, _>>()?;
    // Authenticate old archives without holding SQLite's writer reservation.
    // The short accepted-job transaction below rechecks detached CAS evidence.
    let identities = association_acceptance::preflight(&conn, &job, &prepared, lane)?;
    let scope = provenance_scope::capture(&conn, &job, &prepared, &identities)?;
    if protocol == super::publication_protocol::PublicationProtocol::SignedOutcomeV1
        && scope.is_none()
    {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "signed publication requires its original registered acceptance",
        )
        .job_local());
    }
    let compatibility = provenance_scope::compatibility::capture(&conn)?;
    // Adoption cannot interleave with publication's external project effects.
    // Acquire outside SQLite/store locks; busy leaves the unaccepted plan intact.
    let _publication = identities
        .iter()
        .any(association_acceptance::Identity::requires_publication_lock)
        .then(jobs::try_adoption_publication_lock)
        .transpose()?;
    retention.begin(&conn, &job, &prepared)?;
    let result = (|| {
        retention.verify_catalog(&conn)?;
        let request = jobs::request_reader::PreparedRequest::prepare(&conn, &job.request_id)?;
        let tx = state_tx(&conn)?;
        if let Some(existing) = request.read(&tx, &job.request_id)? {
            if existing.plan_hash != job.plan_hash || existing.plan_id != job.plan_id {
                return Err(ImportError::new(
                    ErrorCode::PlanStale,
                    "request ID belongs to a different plan",
                ));
            }
            return Ok((existing, false));
        }
        if let Some(compatibility) = &compatibility {
            compatibility.record(&tx)?;
        }
        if let Some(scope) = &scope {
            scope.verify_before_acceptance(&tx)?;
        }
        for identity in &identities {
            identity.accept(&tx)?;
        }
        job.state = JobState::Queued;
        job.items = items
            .iter()
            .map(|i| JobItem {
                candidate_id: i.candidate_id.clone(),
                title: Some(
                    i.adapter
                        .graph
                        .title
                        .clone()
                        .unwrap_or_else(|| i.adapter.graph.native_session_id.clone()),
                ),
                phase: "queued".into(),
                import_id: None,
                revision_id: None,
                outcome: None,
                error: None,
                mapping_project_id: i.mapping_project_id.clone(),
            })
            .collect();
        job.item_count = job.items.len() as u32;
        jobs::save_job(&tx, &job)?;
        for (input, _) in &prepared {
            tx.execute("INSERT INTO job_items(job_id,candidate_id,phase,result_json) VALUES(?1,?2,'queued',?3)",
            params![job.job_id, input.candidate_id, serde_json::to_string(input).map_err(failure)?])
            .map_err(failure)?;
        }
        if let Some(scope) = &scope {
            scope.record(&tx, &job)?;
        }
        // Retention is established before commit so a drop cannot erase an input
        // after its durable reference becomes visible to another worker.
        retention.record_acceptance(&tx)?;
        for (_, lease) in &prepared {
            retention.retain(lease, &protection)?;
            #[cfg(test)]
            super::preview_lifecycle_tests::after_enqueue_retention();
        }
        #[cfg(test)]
        super::preview_lifecycle_tests::before_enqueue_commit();
        tx.commit().map_err(super::errors::sqlite_storage_error)?;
        #[cfg(test)]
        super::preview_lifecycle_tests::after_enqueue_commit();
        retention.accept(&conn, &protection);
        Ok((job, true))
    })();
    match result {
        Ok((job, newly_accepted)) => {
            if newly_accepted {
                changed(&job);
            }
            Ok(job)
        }
        Err(mut error) => {
            if let Err(repair) = retention.rollback(&conn, &protection) {
                error
                    .message
                    .push_str(&format!("; retention rollback deferred: {repair}"));
            }
            Err(error)
        }
    }
}

#[cfg(test)]
pub fn change_job(
    conn: &Connection,
    id: &str,
    update: impl FnOnce(&mut JobRecord) -> Result<(), ImportError>,
) -> Result<JobRecord, ImportError> {
    let crypto = super::publication_crypto::JobCrypto::prepare(conn, id)?;
    change_job_in_tx(state_tx(conn)?, id, &crypto, None, update)
}

fn change_job_in_tx(
    tx: rusqlite::Transaction<'_>,
    id: &str,
    crypto: &super::publication_crypto::JobCrypto,
    action: Option<&super::job_actions::PreparedAction>,
    update: impl FnOnce(&mut JobRecord) -> Result<(), ImportError>,
) -> Result<JobRecord, ImportError> {
    let (job, entry) = jobs::get_job_for_writer(&tx, id, crypto)?;
    let mut job =
        job.ok_or_else(|| ImportError::new(ErrorCode::SourceNotFound, "job not found"))?;
    if let Some(action) = action {
        if action.replay(&tx, &job)? {
            return Ok(job);
        }
    }
    update(&mut job)?;
    job.state_version = job
        .state_version
        .checked_add(1)
        .ok_or_else(|| failure("job state version overflow"))?;
    entry.require_writer(&tx, id)?;
    let changes = tx.total_changes();
    jobs::save_job(&tx, &job)?;
    if let Some(action) = action {
        action.record(&tx, &job, changes, 1 + job.items.len() as u64)?;
    }
    #[cfg(test)]
    if action.is_some() {
        super::preview_lifecycle_tests::after_publish_point("before_retry_commit");
    }
    tx.commit().map_err(failure)?;
    #[cfg(test)]
    if action.is_some() {
        super::preview_lifecycle_tests::after_publish_point("after_retry_commit");
    }
    #[cfg(all(debug_assertions, feature = "native-import-audit"))]
    if matches!(
        job.state,
        JobState::Completed | JobState::CompletedWithIssues
    ) {
        super::native_fault::pause(super::native_fault::Point::BeforeCompletionEvent, id);
    }
    changed(&job);
    Ok(job)
}

pub fn terminal(state: JobState) -> bool {
    matches!(
        state,
        JobState::Completed
            | JobState::CompletedWithIssues
            | JobState::Cancelled
            | JobState::Failed
    )
}

#[cfg(test)]
pub fn cancel(id: &str) -> Result<JobRecord, ImportError> {
    super::job_progress::request_cancel(&open_catalog()?, id)
}

#[cfg(test)]
pub fn retry(id: &str) -> Result<JobRecord, ImportError> {
    change_job(&open_catalog()?, id, requeue_retryable)
}

pub(super) fn retry_action(
    conn: &Connection,
    id: &str,
    action: &super::job_actions::PreparedAction,
) -> Result<JobRecord, ImportError> {
    change_job_in_tx(
        state_tx(conn)?,
        id,
        action.crypto(),
        Some(action),
        requeue_retryable,
    )
}

fn requeue_retryable(job: &mut JobRecord) -> Result<(), ImportError> {
    if !terminal(job.state) && job.state != JobState::RecoveryNeeded {
        return Err(ImportError::new(
            ErrorCode::SourceBusy,
            "job is already active",
        ));
    }
    for item in &mut job.items {
        // Partial source content requires a new preview, not a replay of
        // the same immutable input. Completed/undone/conflicting items stay.
        // Also drop import/revision ids. Cancel after a prepared receipt
        // leaves that receipt's import id on the item, and signed recovery
        // refuses any queued item that still has one. The receipt, not this
        // ACK, remains the identity the next publish resumes.
        if matches!(
            item.phase.as_str(),
            "failed" | "cancelled" | "recovery_needed"
        ) {
            item.phase = "queued".into();
            item.outcome = None;
            item.error = None;
            item.import_id = None;
            item.revision_id = None;
        }
    }
    if job.items.iter().any(|i| i.phase == "queued") {
        job.state = JobState::Queued;
    }
    Ok(())
}

pub(super) fn retained_preview_path(
    conn: &Connection,
    job_id: &str,
    candidate_id: &str,
) -> Result<PathBuf, ImportError> {
    input_reader::retained_preview_path(conn, job_id, candidate_id)
}

/// Metadata-only point verification survives disposal of completed previews.
/// Never opens provider sources or re-runs the prepared-input filesystem path.
pub(super) fn registration_input(
    conn: &Connection,
    job_id: &str,
    candidate_id: &str,
) -> Result<super::registration_provenance::InputIdentity, ImportError> {
    let json = input_reader::load_registration(conn, job_id, candidate_id)?;
    let input = DurableInput::parse(&json)?;
    let identity = input.registration_identity()?;
    if identity.candidate_id != candidate_id
        || identity.digest != super::hash::sha256_bytes(json.as_bytes())
    {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "accepted identity input is inconsistent",
        ));
    }
    Ok(identity)
}

#[cfg(test)]
pub(crate) fn load_input(
    conn: &Connection,
    job_id: &str,
    candidate_id: &str,
) -> Result<CommitItem, ImportError> {
    let crypto = if conn.is_autocommit() {
        super::publication_crypto::JobCrypto::prepare(conn, job_id)?
    } else {
        super::publication_crypto::JobCrypto::legacy_snapshot(conn, job_id)?
    };
    durable_reader::load(conn, job_id, candidate_id, &crypto)?.into_item()
}

#[cfg(test)]
pub(super) fn recover_test_input(
    conn: &Connection,
    job_id: &str,
    previous: &JobItem,
) -> Result<Option<JobItem>, ImportError> {
    let crypto = if conn.is_autocommit() {
        super::publication_crypto::JobCrypto::prepare(conn, job_id)?
    } else {
        super::publication_crypto::JobCrypto::legacy_snapshot(conn, job_id)?
    };
    let input = durable_reader::load(conn, job_id, &previous.candidate_id, &crypto)?;
    durable_reader::recover(conn, job_id, previous, &input, &crypto)
}

/// Replay authorized local inputs only. No discovery or source reads on restart.
/// Each invocation takes the OS worker lock before opening its own connection.
pub fn run_pending() -> Result<u32, ImportError> {
    worker::run_pending()
}

/// A contended catalog reader holds the writer out for about one busy
/// timeout. Retry that case a few times. History-fence and disk-space
/// failures stay terminal so a real conflict is not published over.
pub(super) const SOURCE_BUSY_ATTEMPTS: u32 = 8;

#[cfg(test)]
#[path = "job_control_cancel_fault.rs"]
mod cancel_fault;
#[path = "job_control_item_attempt.rs"]
mod item_attempt;
#[cfg(test)]
use cancel_fault::take_cancel_check_fault;
#[cfg(test)]
pub(super) use cancel_fault::CancelCheckFaultGuard;

fn run_job(conn: &Connection, id: &str) -> Result<worker::JobRun, ImportError> {
    #[cfg(test)]
    let _span = super::codec_profile::Span::start("worker-run-job", 0);
    #[cfg(test)]
    let start_span = super::codec_profile::Span::start("worker-start-job", 0);
    let crypto = match super::publication_crypto::JobCrypto::prepare(conn, id) {
        Ok(crypto) => crypto,
        Err(error) if error.is_job_local() => return Ok(worker::JobRun::Refused(error)),
        Err(error) => return Err(error),
    };
    let start = worker::start_job(conn, id, &crypto);
    let job = match start {
        Ok(job) => job,
        // ONLY the before-start entry refusal is untouched. Later errors must
        // never be mistaken for a scheduling refusal after publication effects.
        Err(error) if error.is_job_local() => return Ok(worker::JobRun::Refused(error)),
        Err(error) => return Err(error),
    };
    #[cfg(test)]
    drop(start_span);
    #[cfg(all(debug_assertions, feature = "native-import-audit"))]
    if job.state == JobState::Running {
        super::native_fault::pause(super::native_fault::Point::Running, id);
    }
    let mut cancellation_batched = false;
    let mut batch_cancelled = std::collections::HashSet::new();
    for (index, previous) in job.items.into_iter().enumerate() {
        if !matches!(
            previous.phase.as_str(),
            "queued" | "running" | "recovery_needed"
        ) {
            continue;
        }
        let candidate = previous.candidate_id.clone();
        if !cancellation_batched && super::job_progress::cancelling(conn, id)? {
            batch_cancelled = super::job_progress::cancel_unstarted_with_crypto(conn, id, &crypto)?;
            cancellation_batched = true;
        }
        if batch_cancelled.contains(&candidate) {
            continue;
        }
        // A busy read is not a user cancel, and a busy write is not a
        // permanent item failure. Only SourceBusy is retried. The bound is
        // a few busy timeouts, not an unbounded spin.
        let mut attempts = 0u32;
        loop {
            let result = item_attempt::attempt(conn, id, index, &candidate, &previous, &crypto);
            let retry = matches!(&result, Err(error) if error.code == ErrorCode::SourceBusy)
                && attempts + 1 < SOURCE_BUSY_ATTEMPTS
                && conn.is_autocommit();
            if retry {
                // Do not finish() this attempt. Persisting cancelled or failed
                // progress would skip the item, and any receipt already written
                // is what the next pass resumes.
                attempts += 1;
                continue;
            }
            let progress = match result {
                Ok(progress) => progress,
                Err(error) => jobs::ItemProgress::pending(JobItem {
                    phase: "failed".into(),
                    outcome: Some("failed".into()),
                    error: Some(error.into_command_err()),
                    ..previous.clone()
                }),
            };
            #[cfg(test)]
            let finish_span = super::codec_profile::Span::start("worker-finish-progress", 0);
            progress.finish(conn, id, index, &crypto)?;
            #[cfg(test)]
            drop(finish_span);
            break;
        }
    }
    #[cfg(test)]
    let terminal_span = super::codec_profile::Span::start("worker-terminal", 0);
    #[cfg(test)]
    super::preview_lifecycle_tests::after_publish_point("before_job_terminal");
    finalize_job(conn, id, &crypto)?;
    #[cfg(test)]
    super::preview_lifecycle_tests::after_publish_point("after_job_terminal");
    #[cfg(test)]
    drop(terminal_span);
    #[cfg(test)]
    let _cleanup_span = super::codec_profile::Span::start("worker-preview-cleanup", 0);
    if let Err(error) = jobs::cleanup_unreferenced_preview_dirs() {
        // The durable result is already recorded. Indeterminate GC remains
        // visible on explicit cleanup but cannot block a healthy later job.
        tracing::warn!(
            code = error.code.as_str(),
            "preview cleanup deferred after job completion; indeterminate inputs retained"
        );
    }
    Ok(worker::JobRun::Completed)
}

/// Per-item steps authenticate their own signed member; the complete accepted
/// member set and every original input are proved again in the writer that
/// makes the job terminal, as at start.
pub(super) fn finalize_job(
    conn: &Connection,
    id: &str,
    crypto: &super::publication_crypto::JobCrypto,
) -> Result<JobRecord, ImportError> {
    let tx = state_tx(conn)?;
    let protocol =
        crate::session_migration::publication_protocol::PublicationProtocol::for_job_id(id)?;
    if let Some(auth) = crypto.require(id, protocol)? {
        publication_acceptance::journal::validate_snapshot(&tx, auth, id)?;
    }
    change_job_in_tx(tx, id, crypto, None, |j| {
        j.state = if j.state == JobState::Cancelling {
            JobState::Cancelled
        } else if j.items.iter().any(|i| {
            i.error.is_some()
                || matches!(
                    i.outcome.as_deref(),
                    Some("partial" | "unsupported" | "failed" | "tombstoned")
                )
        }) {
            JobState::CompletedWithIssues
        } else {
            JobState::Completed
        };
        Ok(())
    })
}

pub fn wake() {
    tauri::async_runtime::spawn_blocking(|| {
        if let Err(error) = run_pending() {
            // Do not log source paths, conversations, or serialized job inputs.
            eprintln!("session import recovery: {}", error.code.as_str());
        }
    });
}
