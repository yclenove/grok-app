//! Serialized continuation persistence. The catalog is the write-ahead dispatch
//! receipt; the per-session JSON is a projection, never proof that sending is safe.

use rusqlite::OptionalExtension;

use super::catalog;
use super::continuation::{
    read_sidecar_strict, save_continuation_ref, sidecar_path, ContinuationSidecar, DispatchState,
};
use super::errors::{sqlite_storage_error, ErrorCode, ImportError};

fn storage_error(error: impl std::fmt::Display) -> ImportError {
    super::errors::storage_error(error)
}

pub(super) fn lock(sid: &str) -> Result<crate::store_lock::ExclusiveLock, ImportError> {
    crate::store_lock::lock_exclusive(&sidecar_path(sid)).map_err(storage_error)
}

#[derive(serde::Deserialize)]
struct Receipt {
    #[serde(flatten)]
    side: ContinuationSidecar,
    #[serde(default, rename = "dispatchReceiptVersion")]
    version: u32,
}

/// Read-only lookup: ordinary sends must not create an import catalog. An
/// unreadable existing catalog is not evidence that this is an ordinary chat.
fn reference(sid: &str) -> Result<Option<Receipt>, ImportError> {
    if !catalog::catalog_path()
        .try_exists()
        .map_err(storage_error)?
    {
        return Ok(None);
    }
    let conn = catalog::open_catalog_readonly()?;
    let json: Option<String> = conn
        .query_row(
            "SELECT json FROM continuation_refs WHERE app_session_id=?1",
            [sid],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    let receipt: Option<Receipt> = json
        .map(|json| serde_json::from_str(&json).map_err(storage_error))
        .transpose()?;
    if receipt.as_ref().is_some_and(|receipt| receipt.version > 1) {
        return Err(storage_error(
            "unsupported continuation dispatch receipt version",
        ));
    }
    Ok(receipt)
}

fn same_identity(a: &ContinuationSidecar, b: &ContinuationSidecar) -> bool {
    a.request_id == b.request_id
        && a.import_id == b.import_id
        && a.revision_id == b.revision_id
        && a.branch_leaf == b.branch_leaf
        && a.allowed_selection_hash == b.allowed_selection_hash
        && a.handoff_hash == b.handoff_hash
        && a.grant_token == b.grant_token
        && a.runtime_capability == b.runtime_capability
}

/// Caller holds the sidecar lock when using this value for a mutation. Preserve
/// current tool exposure from the file, but never roll back a committed dispatch.
fn reconcile(
    mut side: ContinuationSidecar,
    receipt: Option<&Receipt>,
) -> Result<ContinuationSidecar, ImportError> {
    if let Some(receipt) = receipt {
        if !same_identity(&side, &receipt.side) {
            return Err(storage_error(
                "continuation sidecar/receipt identity mismatch",
            ));
        }
        // Older versions only saved the initial Prepared reference. Preserve
        // their non-Prepared sidecars; subsequent transitions upgrade the receipt.
        if receipt.version == 1
            || receipt.side.dispatch_id.is_some()
            || receipt.side.state != DispatchState::Prepared
        {
            side.state = receipt.side.state;
            side.dispatch_id.clone_from(&receipt.side.dispatch_id);
            side.prompt_hash.clone_from(&receipt.side.prompt_hash);
            side.agent_session_id
                .clone_from(&receipt.side.agent_session_id);
        }
    }
    Ok(side)
}

pub(super) fn read_effective(sid: &str) -> Result<Option<ContinuationSidecar>, ImportError> {
    let side = read_sidecar_strict(sid)?;
    let receipt = reference(sid)?;
    match side {
        Some(side) => reconcile(side, receipt.as_ref()).map(Some),
        None => {
            let draft = sidecar_path(sid).with_file_name("handoff_draft.txt");
            if receipt.is_some() || draft.try_exists().map_err(storage_error)? {
                return Err(ImportError::new(
                    ErrorCode::HistoryAccessUnavailable,
                    "continuation sidecar is missing; recover the existing continuation first",
                ));
            }
            Ok(None)
        }
    }
}

pub(super) fn write_file(path: &std::path::Path, bytes: &[u8]) -> Result<(), ImportError> {
    write_file_in(path, bytes, true)
}

/// Draft edits use this. The atomic rename must not `create_dir_all` a session
/// directory that `delete_session` has already removed.
pub(super) fn write_file_existing(path: &std::path::Path, bytes: &[u8]) -> Result<(), ImportError> {
    write_file_in(path, bytes, false)
}

fn write_file_in(
    path: &std::path::Path,
    bytes: &[u8],
    create_parent: bool,
) -> Result<(), ImportError> {
    // rename on Unix can replace a read-only destination. Respect that flag on
    // every platform rather than silently bypassing the user's write protection.
    match std::fs::metadata(path) {
        Ok(meta) if meta.permissions().readonly() => {
            return Err(storage_error("continuation file is read-only"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(storage_error(error)),
    }
    let write = if create_parent {
        crate::store_lock::write_bytes_replace(path, bytes)
    } else {
        crate::store_lock::write_bytes_replace_existing(path, bytes)
    };
    write.map_err(|error| {
        // A missing session directory is a deleted chat, not a generic IO fault.
        if !create_parent && error.starts_with("parent directory is gone:") {
            ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "continuation session no longer exists",
            )
        } else {
            storage_error(error)
        }
    })
}

pub(super) fn write_sidecar_unlocked(
    sid: &str,
    side: &ContinuationSidecar,
) -> Result<(), ImportError> {
    write_file(
        &sidecar_path(sid),
        &serde_json::to_vec_pretty(side).map_err(storage_error)?,
    )
}

/// All lifecycle writes commit to SQLite BEFORE replacing the sidecar. If the
/// process dies between writes, the receipt wins at the next gate/recovery. A
/// commit error aborts dispatch; this is not remote exactly-once execution.
fn persist_transition(sid: &str, side: &ContinuationSidecar) -> Result<(), ImportError> {
    let conn = catalog::open_catalog_under_store_lock()?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(sqlite_storage_error)?;
    catalog::write_under_store_lock(&conn, |conn| save_continuation_ref(conn, sid, side))?;
    write_sidecar_unlocked(sid, side)
}

pub(super) fn mutate(
    sid: &str,
    required: bool,
    lifecycle: bool,
    update: impl FnOnce(&mut ContinuationSidecar) -> Result<bool, ImportError>,
) -> Result<(), ImportError> {
    // Do not create a session directory/lock for ordinary-session callbacks.
    if read_effective(sid)?.is_none() && !required {
        return Ok(());
    }
    if lifecycle {
        catalog::open_catalog()?;
    }
    let _lock = lock(sid)?;
    let Some(mut side) = read_effective(sid)? else {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "no continuation sidecar",
        ));
    };
    if update(&mut side)? {
        if lifecycle {
            persist_transition(sid, &side)?;
        } else {
            write_sidecar_unlocked(sid, &side)?;
        }
    }
    Ok(())
}

fn allow_edit(side: &ContinuationSidecar) -> Result<(), ImportError> {
    if !matches!(
        side.state,
        DispatchState::Prepared | DispatchState::FailedBeforeDispatch
    ) {
        return Err(ImportError::new(
            ErrorCode::DispatchUnknown,
            "cannot edit an already dispatched handoff",
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn write_draft(sid: &str, text: &str) -> Result<(), ImportError> {
    let _lock = lock(sid)?;
    let receipt = reference(sid)?;
    // Initial creation has a catalog reference before the sidecar is published.
    let side = match read_sidecar_strict(sid)? {
        Some(side) => Some(reconcile(side, receipt.as_ref())?),
        None => receipt.map(|receipt| receipt.side),
    };
    if let Some(side) = side {
        allow_edit(&side)?;
    }
    write_file(
        &sidecar_path(sid).with_file_name("handoff_draft.txt"),
        text.as_bytes(),
    )
}

/// Caller holds the sidecar lock together with native store writer locks.
pub(super) fn recover_unlocked(
    sid: &str,
    expected: &ContinuationSidecar,
    edited_text: Option<String>,
) -> Result<(), ImportError> {
    // Re-read under the lock: the caller's reference may predate a dispatch.
    let receipt =
        reference(sid)?.ok_or_else(|| storage_error("continuation receipt is missing"))?;
    if !same_identity(expected, &receipt.side) {
        return Err(storage_error("continuation receipt identity changed"));
    }
    let side = match read_sidecar_strict(sid)? {
        Some(side) => reconcile(side, Some(&receipt))?,
        None if receipt.version == 1 => {
            let mut side = receipt.side;
            // A historical receipt is dispatch evidence, not current CLI tool
            // exposure. Missing-file recovery must obtain fresh exposure.
            side.advertised_tool_names.clear();
            side.probe_evidence = Some(super::capability::ProbeEvidence::SelfToolsList);
            side
        }
        None => {
            return Err(ImportError::new(
                ErrorCode::DispatchUnknown,
                "legacy continuation has no durable dispatch receipt; cannot prove safe recovery",
            ));
        }
    };
    if let Some(text) = edited_text.filter(|s| !s.is_empty()) {
        allow_edit(&side)?;
        write_file(
            &sidecar_path(sid).with_file_name("handoff_draft.txt"),
            text.as_bytes(),
        )?;
    }
    write_sidecar_unlocked(sid, &side)
}
