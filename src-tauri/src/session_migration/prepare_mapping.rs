//! Optional normalized mapping summaries; fixed previews remain the plan authority.
use std::collections::HashMap;

use rusqlite::{params, Connection};

use super::errors::{ErrorCode, ImportError};
use super::mapping_scope::MappingSubject;
use super::prepare::db_failure;
use super::scan::CandidateRef;
use super::selection::{PreparedSelectionIdentity, PreviewSelection};

pub(crate) type Summaries = HashMap<String, (String, MappingSubject)>;

pub(crate) fn persist_success(
    batch_id: &str,
    seq: u32,
    preview_id: &str,
    selection: &PreviewSelection,
    candidate_id: &str,
    subject: &MappingSubject,
) -> Result<(), ImportError> {
    let mut conn = super::prepare::open_existing(batch_id)?;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(db_failure)?;
    let mut selection = selection.clone();
    selection.selection = selection.selection.canonicalized();
    let changed = tx
        .execute(
            "UPDATE items SET state='prepared',preview_id=?2,selection_json=?3,error_json=NULL
         WHERE seq=?1 AND state='preparing'",
            params![
                seq,
                preview_id,
                serde_json::to_string(&selection).map_err(db_failure)?
            ],
        )
        .map_err(db_failure)?;
    if changed != 1 {
        return Err(db_failure("prepare success lost its state transition"));
    }
    store(&tx, candidate_id, preview_id, subject)?;
    tx.execute(
        "UPDATE meta SET state_version=state_version+1,updated_at=?1 WHERE id=1",
        params![chrono::Utc::now().to_rfc3339()],
    )
    .map_err(db_failure)?;
    tx.commit().map_err(db_failure)
}

pub(crate) fn create_schema(conn: &Connection) -> Result<(), ImportError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mapping_subjects(
           candidate_id TEXT PRIMARY KEY, preview_id TEXT NOT NULL, json TEXT NOT NULL
         );",
    )
    .map_err(db_failure)
}

/// Caller owns the transaction that also publishes the prepared item state.
pub(crate) fn store(
    conn: &Connection,
    candidate_id: &str,
    preview_id: &str,
    subject: &MappingSubject,
) -> Result<(), ImportError> {
    create_schema(conn)?;
    conn.execute(
        "INSERT INTO mapping_subjects(candidate_id,preview_id,json) VALUES(?1,?2,?3)
         ON CONFLICT(candidate_id) DO UPDATE SET preview_id=excluded.preview_id,json=excluded.json",
        params![
            candidate_id,
            preview_id,
            serde_json::to_string(subject).map_err(db_failure)?
        ],
    )
    .map_err(db_failure)?;
    Ok(())
}

pub(crate) fn load(conn: &Connection) -> Result<Summaries, ImportError> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='mapping_subjects')",
        [], |row| row.get(0),
    ).map_err(db_failure)?;
    if !exists {
        return Ok(HashMap::new());
    }
    let mut stmt = conn
        .prepare(
            "SELECT m.candidate_id,m.preview_id,m.json FROM mapping_subjects m
         JOIN items i ON i.candidate_id=m.candidate_id WHERE i.state='prepared'",
        )
        .map_err(db_failure)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(db_failure)?;
    let mut summaries = HashMap::new();
    for row in rows {
        let (candidate, preview, json) = row.map_err(db_failure)?;
        summaries.insert(
            candidate,
            (preview, serde_json::from_str(&json).map_err(db_failure)?),
        );
    }
    Ok(summaries)
}

pub(crate) fn subject_for(
    summaries: &Summaries,
    batch_id: &str,
    reference: &CandidateRef,
    preview_id: &str,
    selection: &str,
) -> Result<MappingSubject, ImportError> {
    super::source_member::validate_reference(reference)?;
    if let Some((cached_preview, subject)) = summaries.get(&reference.candidate.candidate_id) {
        if cached_preview != preview_id
            || subject.source.source_id != reference.source_id
            || subject.source.provider != reference.candidate.provider.as_str()
        {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "prepared mapping summary identity changed",
            ));
        }
        let path = super::jobs::find_preview_dir(preview_id)?
            .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "prepared preview is missing"))?;
        let lease = super::jobs::read_preview_lease(&path)?
            .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "prepared lease is missing"))?;
        super::source_member::validate_reference_lease(reference, &lease)?;
        if lease.prepare_batch_id.as_deref() != Some(batch_id) {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "prepared mapping batch changed",
            ));
        }
        return Ok(subject.clone());
    }
    // Older batches have no summary. Read only their fixed preview, never the provider.
    let path = super::jobs::find_preview_dir(preview_id)?
        .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "prepared preview is missing"))?;
    let (lease, _) = super::jobs::verify_disk_preview_integrity(&path, preview_id)?;
    super::source_member::validate_reference_lease(reference, &lease)?;
    if lease.prepare_batch_id.as_deref() != Some(batch_id)
        || lease.source_revision != reference.candidate.source_revision
    {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "prepared mapping preview identity changed",
        ));
    }
    let selection: PreviewSelection = serde_json::from_str(selection).map_err(db_failure)?;
    let verified = super::selection::validate_prepared_selection(
        &selection,
        &path,
        PreparedSelectionIdentity {
            preview_id: Some(preview_id),
            candidate_id: Some(&reference.candidate.candidate_id),
            source_id: Some(&reference.source_id),
            root_path: Some(&reference.root_path),
            legacy_id: Some(&reference.legacy_id),
            provider: Some(reference.candidate.provider.as_str()),
            ..PreparedSelectionIdentity::default()
        },
    )?;
    Ok(MappingSubject::new(
        verified.source_id,
        verified.provider,
        verified.cwd,
    ))
}
