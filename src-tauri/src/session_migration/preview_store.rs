//! Disk lookup and durable prepare retention for preview snapshots.

use std::path::{Path, PathBuf};

use super::errors::{ErrorCode, ImportError};

pub(crate) fn batch_retains(path: &Path, preview_id: &str) -> Result<bool, ImportError> {
    super::jobs::read_preview_lease(path)?
        .filter(|lease| lease.preview_id == preview_id)
        .and_then(|lease| lease.prepare_batch_id)
        .map_or(Ok(false), |batch_id| {
            super::prepare::preview_is_retained_checked(&batch_id, preview_id)
        })
}

pub(crate) fn disk_snapshot_path(preview_id: &str) -> Result<PathBuf, ImportError> {
    let path = super::jobs::find_preview_dir(preview_id)?
        .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "preview expired"))?;
    let (lease, _) = super::jobs::verify_disk_preview_integrity(&path, preview_id)?;
    if lease.expires_unix > chrono::Utc::now().timestamp() {
        return Ok(path);
    }
    let retained = match lease.prepare_batch_id.as_deref() {
        Some(batch_id) => super::prepare::preview_is_retained_checked(batch_id, preview_id)?,
        None => false,
    };
    if !retained {
        return Err(ImportError::new(ErrorCode::PlanStale, "preview expired"));
    }
    Ok(path)
}
