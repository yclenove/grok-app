//! Immutable point-addressed records in catalog metadata. A loaded record is
//! not acceptance authorization; the caller must re-check that in this writer.
//! No key loading, archive, source, preview or native-store I/O in these methods.
use rusqlite::{OptionalExtension, Transaction};

use super::{
    invalid,
    record::{Binding, StageKind},
    verify, ImportError, InstallationAuthenticator, SealedRecord, VerifiedRecord, RECORD_BYTES,
};
use crate::session_migration::errors::sqlite_storage_error;

fn key(binding: &Binding, kind: StageKind) -> Result<String, ImportError> {
    binding.validate()?;
    Ok(format!(
        "publication_outcome:v1:{}:{}:{}",
        binding.job_id,
        binding.candidate_digest,
        kind.key(),
    ))
}

fn raw(tx: &Transaction<'_>, key: &str) -> Result<Option<String>, ImportError> {
    // Inspect bytes/type before bringing a possibly corrupt large row into
    // Rust. Both reads use the same transaction snapshot, with a PK lookup.
    let metadata = tx
        .query_row(
            "SELECT typeof(value),length(CAST(value AS BLOB)) FROM meta WHERE key=?1",
            [key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    let Some((storage_type, bytes)) = metadata else {
        return Ok(None);
    };
    if storage_type != "text" || bytes < 0 || bytes as u64 > RECORD_BYTES as u64 {
        return Err(invalid());
    }
    tx.query_row("SELECT value FROM meta WHERE key=?1", [key], |row| {
        row.get(0)
    })
    .map(Some)
    .map_err(sqlite_storage_error)
}

pub(super) fn present(
    tx: &Transaction<'_>,
    binding: &Binding,
    kind: StageKind,
) -> Result<bool, ImportError> {
    Ok(raw(tx, &key(binding, kind)?)?.is_some())
}

/// Missing signed records are errors here, never permission to try legacy.
pub(super) fn load_required(
    tx: &Transaction<'_>,
    authenticator: &InstallationAuthenticator,
    binding: &Binding,
    kind: StageKind,
) -> Result<VerifiedRecord, ImportError> {
    let encoded = raw(tx, &key(binding, kind)?)?.ok_or_else(invalid)?;
    verify(authenticator, encoded.as_bytes(), binding, kind)
}

/// Insert once. The identical producer retry is idempotent; no upsert, repair,
/// replacement or reader-issued re-signing can change an existing phase.
pub(super) fn insert(
    tx: &Transaction<'_>,
    authenticator: &InstallationAuthenticator,
    record: &SealedRecord,
) -> Result<(), ImportError> {
    let binding = &record.body.binding;
    let kind = record.body.stage.kind();
    // Validate the exact envelope even for a private, producer-built object.
    let current = verify(authenticator, record.encoded.as_bytes(), binding, kind)?;
    require_exclusive(tx, binding, kind)?;
    match kind {
        StageKind::Allocated => {}
        StageKind::Stored => {
            let allocation = load_required(tx, authenticator, binding, StageKind::Allocated)?;
            current.follows(&allocation)?;
        }
        StageKind::Committed => {
            let allocation = load_required(tx, authenticator, binding, StageKind::Allocated)?;
            let stored = load_required(tx, authenticator, binding, StageKind::Stored)?;
            stored.follows(&allocation)?;
            current.follows(&stored)?;
        }
        StageKind::Deduplicated => {
            let allocation = load_required(tx, authenticator, binding, StageKind::Allocated)?;
            current.follows(&allocation)?;
        }
    }
    let address = key(binding, kind)?;
    if let Some(existing) = raw(tx, &address)? {
        verify(authenticator, existing.as_bytes(), binding, kind)?;
        if existing == record.encoded {
            return Ok(());
        }
        return Err(invalid());
    }
    tx.execute(
        "INSERT INTO meta(key,value) VALUES(?1,?2)",
        [&address, &record.encoded],
    )
    .map_err(sqlite_storage_error)?;
    require_exclusive(tx, binding, kind)?;
    let saved = load_required(tx, authenticator, binding, kind)?;
    if saved.encoded != record.encoded {
        return Err(invalid());
    }
    Ok(())
}

fn require_exclusive(
    tx: &Transaction<'_>,
    binding: &Binding,
    kind: StageKind,
) -> Result<(), ImportError> {
    let conflicts: &[StageKind] = match kind {
        StageKind::Allocated => &[],
        StageKind::Stored | StageKind::Committed => &[StageKind::Deduplicated],
        StageKind::Deduplicated => &[StageKind::Stored, StageKind::Committed],
    };
    for conflict in conflicts {
        if present(tx, binding, *conflict)? {
            return Err(invalid());
        }
    }
    Ok(())
}

pub(super) fn load_deduplicated(
    tx: &Transaction<'_>,
    authenticator: &InstallationAuthenticator,
    binding: &Binding,
) -> Result<VerifiedRecord, ImportError> {
    require_exclusive(tx, binding, StageKind::Deduplicated)?;
    let allocation = load_required(tx, authenticator, binding, StageKind::Allocated)?;
    let result = load_required(tx, authenticator, binding, StageKind::Deduplicated)?;
    result.follows(&allocation)?;
    Ok(result)
}

/// Verify every point record before exposing a committed result. This does not
/// grant Undo: current accepted/source/member/input/fence/target checks are still
/// required in this same transaction before the first target write.
pub(super) fn load_committed(
    tx: &Transaction<'_>,
    authenticator: &InstallationAuthenticator,
    binding: &Binding,
) -> Result<VerifiedRecord, ImportError> {
    require_exclusive(tx, binding, StageKind::Committed)?;
    let allocation = load_required(tx, authenticator, binding, StageKind::Allocated)?;
    let stored = load_required(tx, authenticator, binding, StageKind::Stored)?;
    let committed = load_required(tx, authenticator, binding, StageKind::Committed)?;
    stored.follows(&allocation)?;
    committed.follows(&stored)?;
    Ok(committed)
}
