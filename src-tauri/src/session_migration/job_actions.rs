//! Caller request identity is checked in the actual job writer, not before it.
//! A journal row deduplicates an action; it never authorizes publication/Undo.
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use super::{
    cursor::InstallationAuthenticator,
    errors::{sqlite_storage_error, ErrorCode, ImportError},
    hash,
    jobs::JobRecord,
    publication_crypto::JobCrypto,
    publication_protocol::PublicationProtocol,
};

const DOMAIN: &str = "session-import-job-action-v1";
const RECORD_BYTES: usize = 2048;

#[path = "job_action_undo.rs"]
pub(super) mod undo;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobActionRequest {
    pub expected_state_version: u32,
    pub request_id: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Action {
    Cancel,
    Retry,
    Undo,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    version: u32,
    request_id: String,
    job_id: String,
    context_digest: String,
    action: Action,
    expected_state_version: u32,
    result_state_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cleanup: Option<undo::Manifest>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    body: Record,
    mac: String,
}

pub(super) struct PreparedAction {
    request: JobActionRequest,
    job_id: String,
    action: Action,
    crypto: JobCrypto,
    legacy_auth: Option<InstallationAuthenticator>,
}

fn unavailable() -> ImportError {
    ImportError::new(
        ErrorCode::HistoryAccessUnavailable,
        "job action journal is missing or inconsistent",
    )
    .job_local()
}

fn conflict() -> ImportError {
    ImportError::new(
        ErrorCode::JobActionConflict,
        "request ID belongs to another job action",
    )
    .job_local()
}

fn canonical(value: &impl Serialize) -> Result<String, ImportError> {
    Ok(hash::canonical_json(
        &serde_json::to_value(value).map_err(|_| unavailable())?,
    ))
}

fn context(job: &JobRecord) -> String {
    hash::options_hash(&serde_json::json!([
        job.job_id,
        job.request_id,
        job.plan_id,
        job.plan_hash,
        job.created_at
    ]))
}

impl PreparedAction {
    pub(super) fn prepare(
        conn: &Connection,
        job_id: &str,
        action: Action,
        request: JobActionRequest,
    ) -> Result<Self, ImportError> {
        let canonical_id = |raw: &str| {
            uuid::Uuid::parse_str(raw).is_ok_and(|id| !id.is_nil() && id.to_string() == raw)
        };
        if !conn.is_autocommit()
            || request.expected_state_version == 0
            || !canonical_id(&request.request_id)
            || !canonical_id(job_id)
        {
            return Err(conflict());
        }
        let crypto = JobCrypto::prepare(conn, job_id)?;
        if !super::publication_crypto::present(conn, job_id)? {
            return Err(ImportError::new(ErrorCode::SourceNotFound, "job not found"));
        }
        // Load once, outside the writer. Legacy journal seals do not upgrade a
        // legacy job to signed publication or manufacture an acceptance record.
        let legacy_auth = if PublicationProtocol::for_job_id(job_id)? == PublicationProtocol::Legacy
        {
            Some(InstallationAuthenticator::load()?)
        } else {
            None
        };
        Ok(Self {
            request,
            job_id: job_id.into(),
            action,
            crypto,
            legacy_auth,
        })
    }

    pub(super) fn crypto(&self) -> &JobCrypto {
        &self.crypto
    }

    fn auth(&self) -> Result<&InstallationAuthenticator, ImportError> {
        self.crypto
            .require(&self.job_id, PublicationProtocol::for_job_id(&self.job_id)?)?
            .or(self.legacy_auth.as_ref())
            .ok_or_else(unavailable)
    }

    fn key(&self) -> String {
        format!("job_action:v1:{}", self.request.request_id)
    }

    fn read(&self, tx: &Transaction<'_>) -> Result<Option<String>, ImportError> {
        Self::read_at(tx, &self.key())
    }

    fn read_at(tx: &Transaction<'_>, key: &str) -> Result<Option<String>, ImportError> {
        tx.query_row(
            "SELECT CASE WHEN typeof(value)='text' AND length(CAST(value AS BLOB))<=?2
                         THEN value END FROM meta WHERE key=?1",
            params![key, RECORD_BYTES],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| match error {
            rusqlite::Error::FromSqlConversionFailure(_, _, _)
            | rusqlite::Error::InvalidColumnType(_, _, _) => unavailable(),
            error => sqlite_storage_error(error),
        })
    }

    fn saved(&self, tx: &Transaction<'_>, job: &JobRecord) -> Result<Option<Record>, ImportError> {
        let Some(encoded) = self.read(tx)? else {
            return Ok(None);
        };
        if encoded.as_bytes().starts_with(&[0xef, 0xbb, 0xbf]) {
            return Err(unavailable());
        }
        let envelope: Envelope = serde_json::from_str(&encoded).map_err(|_| unavailable())?;
        let mac = hex::decode(&envelope.mac).map_err(|_| unavailable())?;
        if envelope.body.version != 1
            || !self
                .auth()?
                .verify(DOMAIN, canonical(&envelope.body)?.as_bytes(), &mac)
        {
            return Err(unavailable());
        }
        let body = envelope.body;
        if body.request_id != self.request.request_id
            || body.job_id != job.job_id
            || body.context_digest != context(job)
            || body.action != self.action
            || body.expected_state_version != self.request.expected_state_version
        {
            return Err(conflict());
        }
        if body.result_state_version <= body.expected_state_version
            || job.state_version < body.result_state_version
            || (body.action == Action::Undo) != body.cleanup.is_some()
        {
            return Err(unavailable());
        }
        Ok(Some(body))
    }

    /// true means replay. The caller authenticates the actual current job with
    /// crypto() before calling this, including on a replay. No writes here.
    pub(super) fn replay(
        &self,
        tx: &Transaction<'_>,
        job: &JobRecord,
    ) -> Result<bool, ImportError> {
        if tx.is_autocommit() || job.job_id != self.job_id {
            return Err(unavailable());
        }
        self.require_header(tx, job)?;
        if let Some(auth) = self
            .crypto
            .require(&self.job_id, PublicationProtocol::for_job_id(&self.job_id)?)?
        {
            // Explicit controls, including lost-reply replays, must not accept
            // a damaged original input. This is not a progress-poll cache.
            super::job_control::publication_acceptance::journal::validate_snapshot(
                tx,
                auth,
                &self.job_id,
            )?;
        }
        if self.saved(tx, job)?.is_some() {
            return Ok(true);
        }
        if self.request.expected_state_version != job.state_version {
            return Err(ImportError::new(
                ErrorCode::JobStateChanged,
                "job state changed; review the current state before another action",
            )
            .job_local());
        }
        Ok(false)
    }

    /// Same transaction as the state write. This fixed metadata never stores
    /// the full job/items or transfers a writer witness to a later request.
    pub(super) fn record(
        &self,
        tx: &Transaction<'_>,
        job: &JobRecord,
        changes_before_write: u64,
        expected_changes: u64,
    ) -> Result<(), ImportError> {
        self.record_with_cleanup(tx, job, changes_before_write, expected_changes, None)
    }

    fn record_with_cleanup(
        &self,
        tx: &Transaction<'_>,
        job: &JobRecord,
        changes_before_write: u64,
        expected_changes: u64,
        cleanup: Option<undo::Manifest>,
    ) -> Result<(), ImportError> {
        if tx.is_autocommit()
            || job.job_id != self.job_id
            || job.state_version <= self.request.expected_state_version
            || self.read(tx)?.is_some()
            || tx.total_changes().checked_sub(changes_before_write) != Some(expected_changes)
            || (self.action == Action::Undo) != cleanup.is_some()
        {
            return Err(unavailable());
        }
        let body = Record {
            version: 1,
            request_id: self.request.request_id.clone(),
            job_id: job.job_id.clone(),
            context_digest: context(job),
            action: self.action,
            expected_state_version: self.request.expected_state_version,
            result_state_version: job.state_version,
            cleanup,
        };
        let mac = hex::encode(self.auth()?.sign(DOMAIN, canonical(&body)?.as_bytes()));
        let encoded = canonical(&Envelope { body, mac })?;
        if encoded.len() > RECORD_BYTES {
            return Err(unavailable());
        }
        self.require_header(tx, job)?;
        let changes = tx.total_changes();
        let inserted = tx
            .execute(
                "INSERT INTO meta(key,value) VALUES(?1,?2)",
                params![self.key(), encoded],
            )
            .map_err(sqlite_storage_error)?;
        // A journal trigger cannot smuggle a second mutation behind a recorded
        // reply. Refusal rolls the action and its journal back together.
        if inserted != 1
            || tx.total_changes().checked_sub(changes) != Some(1)
            || self.read(tx)?.as_deref() != Some(encoded.as_str())
        {
            return Err(unavailable());
        }
        Ok(())
    }

    fn require_header(&self, tx: &Transaction<'_>, job: &JobRecord) -> Result<(), ImportError> {
        let rows = super::jobs::item_store::uses_rows(tx, &job.job_id)?;
        let json = super::jobs::item_store::encode_header(job, rows)?;
        let current: bool = tx
            .query_row(
                "SELECT state_version=?2 AND state=?3 AND json=?4 AND request_id=?5
                        AND plan_hash=?6 AND created_at=?7 FROM jobs WHERE job_id=?1",
                params![
                    job.job_id,
                    job.state_version,
                    job.state.as_str(),
                    json,
                    job.request_id,
                    job.plan_hash,
                    job.created_at,
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(sqlite_storage_error)?
            .unwrap_or(false);
        if !current {
            return Err(unavailable());
        }
        if rows {
            // The compact header alone does not bind the item state observed
            // by this action. Compare every ordered row's exact saved bytes.
            let mut statement = tx
                .prepare(
                    "SELECT item_index,candidate_id,json FROM job_item_states
                     WHERE job_id=?1 ORDER BY item_index",
                )
                .map_err(sqlite_storage_error)?;
            let mut stored = statement
                .query([&job.job_id])
                .map_err(sqlite_storage_error)?;
            for (index, item) in job.items.iter().enumerate() {
                let row = stored
                    .next()
                    .map_err(sqlite_storage_error)?
                    .ok_or_else(unavailable)?;
                let json = serde_json::to_string(item).map_err(|_| unavailable())?;
                if row.get::<_, usize>(0).map_err(sqlite_storage_error)? != index
                    || row.get_ref(1).map_err(sqlite_storage_error)?
                        != rusqlite::types::ValueRef::Text(item.candidate_id.as_bytes())
                    || row.get_ref(2).map_err(sqlite_storage_error)?
                        != rusqlite::types::ValueRef::Text(json.as_bytes())
                {
                    return Err(unavailable());
                }
            }
            if stored.next().map_err(sqlite_storage_error)?.is_some() {
                return Err(unavailable());
            }
        }
        Ok(())
    }
}

pub fn cancel(job_id: &str, request: JobActionRequest) -> Result<JobRecord, ImportError> {
    let conn = super::catalog::open_catalog()?;
    let action = PreparedAction::prepare(&conn, job_id, Action::Cancel, request)?;
    super::job_progress::request_cancel_action(&conn, job_id, &action)?;
    super::engine::job_get(job_id)
}

pub fn retry(job_id: &str, request: JobActionRequest) -> Result<JobRecord, ImportError> {
    let conn = super::catalog::open_catalog()?;
    let action = PreparedAction::prepare(&conn, job_id, Action::Retry, request)?;
    super::job_control::retry_action(&conn, job_id, &action)?;
    super::engine::job_get(job_id)
}

pub fn undo(job_id: &str, request: JobActionRequest) -> Result<JobRecord, ImportError> {
    let conn = super::catalog::open_catalog()?;
    let action = PreparedAction::prepare(&conn, job_id, Action::Undo, request)?;
    super::jobs::undo_job_action(&conn, job_id, &action)?;
    super::engine::job_get(job_id)
}
