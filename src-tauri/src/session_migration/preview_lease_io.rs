//! Lease JSON transactions use their own lock, never the project store lock.
//! No callback may enter SQLite or reacquire lifecycle/lease locks.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::errors::{io_storage_error, ErrorCode, ImportError};
use super::jobs::PreviewLease;

static LEASE_WRITES: Mutex<()> = Mutex::new(());

struct LeaseLock {
    file: File,
    _process: MutexGuard<'static, ()>,
}

impl Drop for LeaseLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

fn failure(error: impl std::fmt::Display) -> ImportError {
    ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
}

fn lock() -> Result<LeaseLock, ImportError> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let process =
        super::preview_lifecycle::wait_mutex(&LEASE_WRITES, deadline, "preview lease is busy")?;
    let path = super::catalog::imports_dir().join("preview-leases.lock");
    fs::create_dir_all(super::catalog::imports_dir()).map_err(io_storage_error)?;
    super::catalog::ensure_inside_imports(&path)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(io_storage_error)?;
    loop {
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => {
                return Ok(LeaseLock {
                    file,
                    _process: process,
                })
            }
            Err(error) if super::preview_lifecycle::busy(&error) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) if super::preview_lifecycle::busy(&error) => {
                return Err(ImportError::new(
                    ErrorCode::SourceBusy,
                    "preview lease is busy",
                ));
            }
            Err(error) => return Err(io_storage_error(error)),
        }
    }
}

pub(super) fn read(dir: &Path) -> Result<Option<PreviewLease>, ImportError> {
    let _lock = lock()?;
    read_unlocked(dir)
}

fn read_unlocked(dir: &Path) -> Result<Option<PreviewLease>, ImportError> {
    let raw = match fs::read(dir.join("lease.json")) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(failure(error)),
    };
    serde_json::from_slice(&raw)
        .map(Some)
        .map_err(|error| failure(format!("invalid preview lease: {error}")))
}

pub(super) fn write(dir: &Path, lease: &PreviewLease) -> Result<(), ImportError> {
    let _operation = super::preview_lifecycle::operation()?;
    let _lock = lock()?;
    write_unlocked(dir, lease)
}

pub(super) fn update(
    dir: &Path,
    mutate: impl FnOnce(&mut PreviewLease),
) -> Result<(), ImportError> {
    update_checked(dir, |lease| {
        mutate(lease);
        Ok(())
    })
}

pub(super) fn update_checked<T>(
    dir: &Path,
    mutate: impl FnOnce(&mut PreviewLease) -> Result<T, ImportError>,
) -> Result<T, ImportError> {
    let operation = super::preview_lifecycle::operation()?;
    update_held(
        dir,
        &super::preview_lifecycle::Protection::Operation(&operation),
        mutate,
    )
}

pub(super) fn update_held<T>(
    dir: &Path,
    protection: &super::preview_lifecycle::Protection<'_>,
    mutate: impl FnOnce(&mut PreviewLease) -> Result<T, ImportError>,
) -> Result<T, ImportError> {
    protection.held();
    super::catalog::ensure_inside_imports(&dir.join("lease.json"))?;
    let _lock = lock()?;
    let mut lease = read_unlocked(dir)?
        .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "preview lease missing"))?;
    let result = mutate(&mut lease)?;
    write_unlocked(dir, &lease)?;
    Ok(result)
}

fn write_unlocked(dir: &Path, lease: &PreviewLease) -> Result<(), ImportError> {
    if !super::jobs::owned_preview_dir(dir)
        || dir.file_name().and_then(|name| name.to_str()) != Some(lease.preview_id.as_str())
    {
        return Err(failure("preview lease path does not match its identity"));
    }
    let bytes = serde_json::to_vec(lease).map_err(failure)?;
    let temporary = dir.join(format!(".lease-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(io_storage_error)?;
        #[cfg(test)]
        io_tests::maybe_fail(io_tests::Point::Write, &temporary).map_err(io_storage_error)?;
        file.write_all(&bytes).map_err(io_storage_error)?;
        #[cfg(test)]
        io_tests::maybe_fail(io_tests::Point::Sync, &temporary).map_err(io_storage_error)?;
        file.sync_all().map_err(io_storage_error)?;
        drop(file);
        #[cfg(test)]
        io_tests::maybe_fail(io_tests::Point::Rename, &temporary).map_err(io_storage_error)?;
        fs::rename(&temporary, dir.join("lease.json")).map_err(io_storage_error)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
#[path = "preview_lease_io_fault_tests.rs"]
mod io_tests;

#[cfg(test)]
pub(in crate::session_migration) use io_tests::{FaultGuard as TestIoFault, Point as TestIoPoint};
