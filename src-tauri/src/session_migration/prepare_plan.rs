//! A catalog plan is publishable only after its prepare-batch receipt commits.
//!
//! The two SQLite databases are not one atomic transaction. Hold the batch write
//! lock while persisting the catalog record, then commit the planned state and
//! receipt together. A crash/failure between them leaves an inert catalog row;
//! recovery and commit must verify the receipt rather than trusting that row.

use rusqlite::{params, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::engine::PlanState;
use super::errors::{ErrorCode, ImportError};
use super::prepare::{db_failure, open_existing, PREPARE_BATCH_VERSION, RETENTION_SECS};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PreparePlanOrigin {
    pub version: u32,
    pub batch_id: String,
    pub state_version: u64,
}

fn stale(message: &str) -> ImportError {
    ImportError::new(ErrorCode::PlanStale, message)
}

pub(crate) fn finalize(
    origin: &PreparePlanOrigin,
    persist: impl FnOnce() -> Result<PlanState, ImportError>,
) -> Result<PlanState, ImportError> {
    if origin.version != 1 {
        return Err(stale("unsupported prepare plan origin"));
    }
    let mut conn = open_existing(&origin.batch_id)?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(db_failure)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_failure)?;
    let ready: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM meta WHERE id=1 AND schema_version=?1
         AND batch_id=?2 AND state_version=?3 AND state IN ('completed','completed-with-issues')
         AND cancel_requested=0
         AND NOT EXISTS(SELECT 1 FROM items WHERE state IN ('queued','preparing')))",
            params![PREPARE_BATCH_VERSION, origin.batch_id, origin.state_version],
            |row| row.get(0),
        )
        .map_err(db_failure)?;
    if !ready {
        return Err(stale("prepare batch changed after plan validation"));
    }
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS plan_receipt(
         id INTEGER PRIMARY KEY CHECK(id=1), version INTEGER NOT NULL,
         plan_id TEXT NOT NULL, plan_hash TEXT NOT NULL, source_state_version INTEGER NOT NULL);",
    )
    .map_err(db_failure)?;
    let plan = persist()?;
    #[cfg(test)]
    super::preview_lifecycle_tests::after_publish_point("plan_catalog_persisted");
    tx.execute(
        "INSERT INTO plan_receipt(id,version,plan_id,plan_hash,source_state_version)
         VALUES(1,1,?1,?2,?3)",
        params![plan.plan_id, plan.plan_hash, origin.state_version],
    )
    .map_err(db_failure)?;
    let changed = tx
        .execute(
            "UPDATE meta SET state='planned',state_version=state_version+1,updated_at=?1,
         retention_expires_unix=MAX(retention_expires_unix,?3)
         WHERE id=1 AND state_version=?2",
            params![
                chrono::Utc::now().to_rfc3339(),
                origin.state_version,
                chrono::Utc::now().timestamp() + RETENTION_SECS
            ],
        )
        .map_err(db_failure)?;
    if changed != 1 {
        return Err(stale("prepare plan finalization lost its batch"));
    }
    tx.commit().map_err(db_failure)?;
    #[cfg(test)]
    super::preview_lifecycle_tests::after_publish_point("plan_finalized");
    Ok(plan)
}

pub(crate) fn verify_receipt(
    origin: &PreparePlanOrigin,
    plan_id: &str,
    plan_hash: &str,
) -> Result<(), ImportError> {
    if origin.version != 1 {
        return Err(stale("unsupported prepare plan origin"));
    }
    let conn = open_existing(&origin.batch_id)?;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='plan_receipt')",
            [],
            |row| row.get(0),
        )
        .map_err(db_failure)?;
    if !exists {
        return Err(stale("prepare plan was not finalized"));
    }
    let finalized: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM plan_receipt p JOIN meta m ON m.id=p.id
         WHERE p.id=1 AND p.version=1 AND p.plan_id=?1 AND p.plan_hash=?2
         AND p.source_state_version=?3 AND m.state_version=p.source_state_version+1
         AND m.batch_id=?4 AND m.schema_version=?5 AND m.state='planned')",
            params![
                plan_id,
                plan_hash,
                origin.state_version,
                origin.batch_id,
                PREPARE_BATCH_VERSION
            ],
            |row| row.get(0),
        )
        .map_err(db_failure)?;
    if !finalized {
        return Err(stale(
            "prepare plan receipt does not match the finalized batch",
        ));
    }
    Ok(())
}
