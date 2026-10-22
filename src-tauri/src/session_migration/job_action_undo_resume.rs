//! The content transaction has committed. Resume exact durable cleanup only;
//! persist its notes/completion together, never run content Undo a second time.
use super::*;
use rusqlite::TransactionBehavior;

fn changed_target() -> ImportError {
    ImportError::new(
        ErrorCode::MappingConflict,
        "Undo target changed after content rollback; project kept",
    )
}

fn signed_proof(
    tx: &Transaction<'_>,
    action: &PreparedAction,
    job: &JobRecord,
    intent: &Intent,
) -> Result<cleanup::Cleanup, ImportError> {
    let proof = cleanup::capture(tx, job, &intent.candidate_id, action.crypto())?;
    if proof.identity_digest != intent.identity_digest || intent.target_digest.is_none() {
        return Err(unavailable());
    }
    Ok(proof)
}

fn legacy_proof(tx: &Transaction<'_>, job: &JobRecord, intent: &Intent) -> Result<(), ImportError> {
    let item = job
        .items
        .iter()
        .find(|item| item.candidate_id == intent.candidate_id)
        .ok_or_else(unavailable)?;
    let receipt =
        jobs::latest_receipt(tx, &job.job_id, &intent.candidate_id)?.ok_or_else(unavailable)?;
    if item.phase != "undone"
        || !receipt.created_project
        || intent.target_digest.is_some()
        || legacy_identity(&receipt)? != intent.identity_digest
    {
        return Err(unavailable());
    }
    Ok(())
}

fn current(tx: &Transaction<'_>, action: &PreparedAction) -> Result<JobRecord, ImportError> {
    let (job, _entry) = jobs::get_job_for_writer(tx, &action.job_id, action.crypto())?;
    let job = job.ok_or_else(unavailable)?;
    if action.action != Action::Undo || !action.replay(tx, &job)? {
        return Err(unavailable());
    }
    Ok(job)
}

fn remove(
    conn: &Connection,
    action: &PreparedAction,
    pending: &Pending,
    proof: &cleanup::Cleanup,
) -> Result<(), ImportError> {
    if pending.intent.target_digest.as_deref() != Some(&proof.target_digest) {
        return Err(changed_target());
    }
    let creation = proof.allocation.project_creation(action.auth()?)?;
    crate::session_migration::project_link::undo_created_project_checked(
        conn,
        &proof.receipt,
        &creation,
        |tx| {
            let job = current(tx, action)?;
            let encoded =
                PreparedAction::read_at(tx, &address(action, pending.intent.index, false))?
                    .ok_or_else(unavailable)?;
            if hash::sha256_bytes(encoded.as_bytes()) != pending.digest {
                return Err(unavailable());
            }
            let actual = signed_proof(tx, action, &job, &pending.intent)?;
            if pending.intent.target_digest.as_deref() != Some(&actual.target_digest) {
                return Err(changed_target());
            }
            Ok(())
        },
    )
}

pub(in crate::session_migration) fn resume(
    conn: &Connection,
    action: &PreparedAction,
) -> Result<JobRecord, ImportError> {
    let signed =
        PublicationProtocol::for_job_id(&action.job_id)? == PublicationProtocol::SignedOutcomeV1;
    let prepared = {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        let job = current(&tx, action)?;
        let work = pending(&tx, action, &job)?;
        // Authenticate the complete intent set and every original receipt
        // before any native effect, including when some completions exist.
        work.into_iter()
            .map(|item| {
                let proof = if signed {
                    Some(signed_proof(&tx, action, &job, &item.intent)?)
                } else {
                    legacy_proof(&tx, &job, &item.intent)?;
                    None
                };
                Ok((item, proof))
            })
            .collect::<Result<Vec<_>, ImportError>>()?
    };
    let mut results = Vec::new();
    for (work, proof) in prepared {
        if work.done.is_some() {
            continue;
        }
        let effect = match proof {
            Some(proof) => remove(conn, action, &work, &proof),
            None => Err(ImportError::new(
                ErrorCode::MappingConflict,
                "project creation provenance unavailable; project kept",
            )),
        };
        #[cfg(test)]
        crate::session_migration::preview_lifecycle_tests::after_publish_point("after_undo_native");
        if effect.is_ok() {
            // Native removal may survive a later completion-record failure.
            // Startup recovery has no successful command reply to emit this.
            crate::session_migration::job_control::workspace_changed();
        }
        // Corrupt original evidence is a refusal, not a completed cleanup note.
        // Already performed effects stay recoverable from their pending intents.
        if effect.as_ref().is_err_and(|error| {
            matches!(
                error.code,
                ErrorCode::HistoryAccessUnavailable | ErrorCode::PlanStale
            )
        }) {
            return Err(effect.unwrap_err());
        }
        let error = effect
            .err()
            .map(|error| error.into_command_err().chars().take(512).collect());
        results.push((
            work.intent.index,
            work.digest,
            work.intent.candidate_id,
            error,
        ));
    }
    if results.is_empty() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return current(&tx, action);
    }
    finish(conn, action, results)
}

type ResultRow = (usize, String, String, Option<String>);

fn finish(
    conn: &Connection,
    action: &PreparedAction,
    results: Vec<ResultRow>,
) -> Result<JobRecord, ImportError> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(sqlite_storage_error)?;
    let (job, entry) = jobs::get_job_for_writer(&tx, &action.job_id, action.crypto())?;
    let mut job = job.ok_or_else(unavailable)?;
    if !action.replay(&tx, &job)? {
        return Err(unavailable());
    }
    let actual = pending(&tx, action, &job)?;
    let mut notes_changed = false;
    for (index, digest, candidate, error) in &results {
        let pending = actual.get(*index).ok_or_else(unavailable)?;
        if pending.digest != *digest
            || pending.intent.candidate_id != *candidate
            || pending.done.is_some()
        {
            return Err(unavailable());
        }
        let item = job
            .items
            .iter_mut()
            .find(|item| item.candidate_id == *candidate)
            .ok_or_else(unavailable)?;
        if item.phase != "undone" {
            return Err(unavailable());
        }
        if error.is_some() && item.error != *error {
            item.error.clone_from(error);
            notes_changed = true;
        }
    }
    entry.require_writer(&tx, &action.job_id)?;
    let changes = tx.total_changes();
    let expected = if notes_changed {
        job.state_version = job.state_version.checked_add(1).ok_or_else(unavailable)?;
        jobs::save_job(&tx, &job)?;
        1 + job.items.len() as u64
    } else {
        0
    };
    if tx.total_changes().checked_sub(changes) != Some(expected) {
        return Err(unavailable());
    }
    action.require_header(&tx, &job)?;
    for (index, digest, _, error) in results {
        let encoded = seal(
            action,
            DONE_DOMAIN,
            Done {
                version: 1,
                intent_digest: digest,
                error,
            },
        )?;
        insert(&tx, &address(action, index, true), &encoded)?;
    }
    let changes = tx.total_changes();
    let removed = tx
        .execute("DELETE FROM meta WHERE key=?1", [pending_key(action)])
        .map_err(sqlite_storage_error)?;
    if removed != 1
        || tx.total_changes().checked_sub(changes) != Some(1)
        || PreparedAction::read_at(&tx, &pending_key(action))?.is_some()
    {
        return Err(unavailable());
    }
    #[cfg(test)]
    crate::session_migration::preview_lifecycle_tests::after_publish_point(
        "before_undo_cleanup_commit",
    );
    tx.commit().map_err(sqlite_storage_error)?;
    #[cfg(test)]
    crate::session_migration::preview_lifecycle_tests::after_publish_point(
        "after_undo_cleanup_commit",
    );
    if notes_changed {
        crate::session_migration::job_control::changed_version(&job.job_id, job.state_version);
    }
    Ok(job)
}
