//! Index construction is separate from ownership and crash-safe publication.
use super::*;

pub(super) fn fill_sqlite_from_jsonl(
    revision_dir: &Path,
    sqlite_path: &Path,
    cancel: &dyn Fn() -> bool,
) -> Result<(), ImportError> {
    let _ = std::fs::remove_file(sqlite_path);
    let mut connection = sqlite_open::open(sqlite_path).map_err(index_error)?;
    let txn = connection.transaction().map_err(index_error)?;
    create_index_schema(&txn)?;
    let mut stream = open_jsonl(&archive_events_path(revision_dir))?;
    let mut n = 0u32;
    for rec in stream.by_ref() {
        if cancel() {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "index rebuild cancelled",
            ));
        }
        let rec = rec?;
        let event: ImportedEvent = serde_json::from_value(rec.value).map_err(|_| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "archived event_json is corrupt",
            )
        })?;
        let json_len = rec.byte_end.saturating_sub(rec.byte_start);
        insert_index_event(&txn, &event, rec.byte_start as i64, json_len as i64)?;
        n += 1;
        if n > 1_000_000 {
            return Err(ImportError::new(
                ErrorCode::ResourceLimit,
                "index rebuild exceeded event budget",
            ));
        }
    }
    let summary = stream.finish()?;
    if !summary.issues.is_empty() {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "archived event stream is incomplete",
        ));
    }
    txn.commit().map_err(index_error)?;
    let _ = connection.execute_batch("PRAGMA journal_mode=DELETE;");
    Ok(())
}

#[cfg(test)]
pub fn build_index(revision_dir: &Path, events: &[ImportedEvent]) -> Result<(), ImportError> {
    std::fs::create_dir_all(revision_dir).map_err(index_error)?;
    owner::with(revision_dir, &|| false, || {
        build_index_owned(revision_dir, events)
    })?;
    project_body_search(revision_dir, events)
}

#[cfg(test)]
fn build_index_owned(revision_dir: &Path, events: &[ImportedEvent]) -> Result<(), ImportError> {
    owner::validate_artifacts(revision_dir)?;
    let jsonl = archive_events_path(revision_dir);
    let mut off = 0i64;
    if !jsonl.exists() {
        let file = std::fs::File::create(&jsonl).map_err(index_error)?;
        let mut writer = BufWriter::new(file);
        for ev in events {
            serde_json::to_writer(&mut writer, ev).map_err(index_error)?;
            writer.write_all(b"\n").map_err(index_error)?;
        }
        writer.flush().map_err(index_error)?;
    }
    let live = index_path(revision_dir);
    let _ = std::fs::remove_file(&live);
    let mut connection = sqlite_open::open(&live).map_err(index_error)?;
    let txn = connection.transaction().map_err(index_error)?;
    create_index_schema(&txn)?;
    for ev in events {
        let mut line = Vec::new();
        serde_json::to_writer(&mut line, ev).map_err(index_error)?;
        insert_index_event(&txn, ev, off, line.len() as i64)?;
        off += line.len() as i64 + 1;
    }
    txn.commit().map_err(index_error)?;
    let _ = connection.execute_batch("PRAGMA journal_mode=DELETE;");
    Ok(())
}
