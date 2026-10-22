//! Reclamation follows exact durable recovery, never absence of a retryable row.
use super::super::{
    catalog,
    errors::{ErrorCode, ImportError},
    job_control::preview_retention,
    prepare, preview_lifecycle,
};
use super::{fs, owned_preview_dir, read_preview_lease, referenced_preview_dirs};

/// Delete expired, unreferenced preview snapshots owned by this app.
pub fn cleanup_unreferenced_preview_dirs() -> Result<u32, ImportError> {
    let parent = catalog::staging_dir().join("previews");
    if !parent.is_dir() && !preview_retention::pending() {
        return Ok(0);
    }
    let Some(cleanup) = preview_lifecycle::try_cleanup()? else {
        return Ok(0);
    };
    catalog::ensure_inside_imports(&parent)?;
    // Never create a catalog, or treat a failed read as an empty ownership set.
    let conn = catalog::open_catalog_readonly()?;
    preview_retention::reconcile_cleanup(&conn, &cleanup)?;
    let referenced = referenced_preview_dirs(&conn)?;
    let now = chrono::Utc::now().timestamp();
    let mut removed = 0u32;
    let mut candidates = Vec::new();
    let mut retained_leases = Vec::new();
    let mut deferred = Vec::new();
    let mut prepare_batch_ids = std::collections::BTreeSet::new();
    let failure =
        |error: std::io::Error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string());
    for entry in fs::read_dir(&parent).map_err(failure)? {
        let path = entry.map_err(failure)?.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(failure(error)),
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || !owned_preview_dir(&path)
            || referenced.contains(&path)
        {
            continue;
        }
        let lease = match read_preview_lease(&path) {
            Ok(lease) => lease,
            Err(error) => {
                deferred.push(error);
                continue;
            }
        };
        if lease.as_ref().is_some_and(|lease| {
            path.file_name().and_then(|name| name.to_str()) != Some(lease.preview_id.as_str())
        }) {
            deferred.push(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "preview cleanup lease identity mismatch",
            ));
            continue;
        }
        if let Some(lease) = lease.as_ref().filter(|lease| lease.retained_by_job) {
            // Authenticate immutable files outside any SQL transaction; then
            // authenticate the entire exact accepted job/input set, not title,
            // candidate id or one projected path from a possibly corrupt row.
            if let Err(error) = super::verify_disk_preview_integrity(&path, &lease.preview_id) {
                deferred.push(error);
                continue;
            }
            retained_leases.push((path.clone(), lease.clone()));
        }
        if let Some(batch_id) = lease
            .as_ref()
            .and_then(|lease| lease.prepare_batch_id.as_ref())
        {
            prepare_batch_ids.insert(batch_id.clone());
        }
        candidates.push((path, lease));
    }
    let completed: std::collections::BTreeMap<_, _> = retained_leases
        .iter()
        .map(|(path, _)| path)
        .zip(preview_retention::completed_owners(
            &conn,
            &retained_leases,
        )?)
        .collect();
    let retained_previews = prepare::retained_preview_ids(&prepare_batch_ids)?;
    for (path, lease) in candidates {
        let completed = match completed.get(&path) {
            Some(Ok(completed)) => *completed,
            Some(Err(error)) => {
                deferred.push(error.clone());
                continue;
            }
            None => false,
        };
        let live = match lease {
            Some(lease) => {
                (!completed && (lease.retained_by_job || lease.expires_unix > now))
                    || lease.prepare_batch_id.as_ref().is_some_and(|batch_id| {
                        retained_previews
                            .get(batch_id)
                            .is_some_and(|ids| ids.contains(&lease.preview_id))
                    })
            }
            None => false,
        };
        if live {
            continue;
        }
        match fs::remove_dir_all(&path) {
            Ok(()) => removed += 1,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => deferred.push(failure(error)),
        }
    }
    if let Some(first) = deferred.first() {
        return Err(ImportError::new(ErrorCode::SnapshotUnavailable, format!(
            "preview cleanup deferred for {} preview(s); removed {removed} verified disposable preview(s): {}",
            deferred.len(), first.message)));
    }
    Ok(removed)
}
