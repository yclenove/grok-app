//! Exact, journaled seed publication. Caller holds the parent mutation lock.
//! A prefix or an exited PID is never ownership evidence for recovery/deletion.

use super::{seed_tree_digest, sha256_hex, PrepareError};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_JOURNAL_BYTES: u64 = 4096;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Publication {
    schema: u32,
    seed_name: String,
    staging_name: String,
    transaction: String,
    prepared_digest: String,
    previous_digest: Option<String>,
    staging_marker_digest: Option<String>,
}

fn io(error: std::io::Error, phase: &str) -> PrepareError {
    PrepareError::new("io", format!("{phase}: {error}"))
}

fn conflict(message: &str) -> PrepareError {
    PrepareError::new("publication_recovery_required", message)
}

fn parent(seed: &Path) -> Result<&Path, PrepareError> {
    seed.parent()
        .ok_or_else(|| conflict("seed parent is missing"))
}

fn file_name(path: &Path) -> Result<String, PrepareError> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| conflict("seed name is invalid"))?;
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':']) {
        return Err(conflict("seed name is not a single path component"));
    }
    Ok(name.into())
}

fn uuid_name(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id))
}

fn linked(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        } // REPARSE_POINT.
    }
    meta.file_type().is_symlink()
}

fn directory_exists(path: &Path) -> Result<bool, PrepareError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !linked(&meta) => Ok(true),
        Ok(_) => Err(conflict("publication path is not an owned plain directory")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io(error, "inspect publication directory")),
    }
}

pub(super) fn journal_path(seed: &Path) -> Result<PathBuf, PrepareError> {
    let name = file_name(seed)?;
    #[cfg(windows)]
    let name = name.to_lowercase();
    Ok(parent(seed)?.join(format!(
        ".seed-publication-{}.json",
        sha256_hex(name.as_bytes())
    )))
}

impl Publication {
    pub(super) fn new(staging: &Path, seed: &Path) -> Result<Self, PrepareError> {
        let seed_parent =
            fs::canonicalize(parent(seed)?).map_err(|e| io(e, "resolve seed parent"))?;
        let staging_parent =
            fs::canonicalize(parent(staging)?).map_err(|e| io(e, "resolve staging parent"))?;
        let staging_name = file_name(staging)?;
        if seed_parent != staging_parent
            || !uuid_name(&staging_name, ".seed-staging-")
            || !directory_exists(staging)?
        {
            return Err(conflict(
                "staging must be an exact UUID directory beside its seed",
            ));
        }
        let record = Self {
            schema: 1,
            seed_name: file_name(seed)?,
            staging_name,
            transaction: uuid::Uuid::new_v4().to_string(),
            prepared_digest: seed_tree_digest(staging)?,
            previous_digest: if directory_exists(seed)? {
                Some(seed_tree_digest(seed)?)
            } else {
                None
            },
            staging_marker_digest: staging_marker_bytes(staging)?.map(|bytes| sha256_hex(&bytes)),
        };
        record.validate(seed)?;
        Ok(record)
    }

    fn validate(&self, seed: &Path) -> Result<(), PrepareError> {
        let digest = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        };
        if self.schema != 1
            || self.seed_name != file_name(seed)?
            || !uuid_name(&self.staging_name, ".seed-staging-")
            || !uuid_name(
                &format!(".seed-backup-{}", self.transaction),
                ".seed-backup-",
            )
            || !digest(&self.prepared_digest)
            || self.previous_digest.as_ref().is_some_and(|d| !digest(d))
            || self
                .staging_marker_digest
                .as_ref()
                .is_some_and(|d| !digest(d))
            || self.staging_name == self.seed_name
            || format!(".seed-backup-{}", self.transaction) == self.seed_name
        {
            return Err(conflict(
                "publication journal does not match this seed transaction",
            ));
        }
        Ok(())
    }

    pub(super) fn backup(&self, seed: &Path) -> Result<PathBuf, PrepareError> {
        Ok(parent(seed)?.join(format!(".seed-backup-{}", self.transaction)))
    }

    fn staging(&self, seed: &Path) -> Result<PathBuf, PrepareError> {
        Ok(parent(seed)?.join(&self.staging_name))
    }
}

pub(super) fn write_journal(seed: &Path, record: &Publication) -> Result<(), PrepareError> {
    record.validate(seed)?;
    let path = journal_path(seed)?;
    let bytes =
        serde_json::to_vec(record).map_err(|_| conflict("cannot encode publication journal"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| {
            io(
                e,
                "create publication journal without replacing an existing record",
            )
        })?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        // Nothing has been renamed. Retain a failed/partial journal for diagnosis,
        // rather than deleting a record whose durable state was not established.
        return Err(io(error, "persist publication journal"));
    }
    Ok(())
}

fn read_journal(seed: &Path) -> Result<Option<Publication>, PrepareError> {
    let path = journal_path(seed)?;
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io(error, "inspect publication journal")),
    };
    if !meta.is_file() || linked(&meta) || meta.len() > MAX_JOURNAL_BYTES {
        return Err(conflict(
            "publication journal is linked, oversized, or not a file",
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| io(e, "open publication journal"))?
        .take(MAX_JOURNAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io(e, "read publication journal"))?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(conflict("publication journal is oversized"));
    }
    let record: Publication =
        serde_json::from_slice(&bytes).map_err(|_| conflict("publication journal is invalid"))?;
    record.validate(seed)?;
    Ok(Some(record))
}

fn remove_journal(seed: &Path) -> Result<(), PrepareError> {
    fs::remove_file(journal_path(seed)?).map_err(|e| io(e, "remove completed publication journal"))
}

fn remove_owned_directory(path: &Path) -> Result<(), PrepareError> {
    if directory_exists(path)? {
        fs::remove_dir_all(path).map_err(|e| io(e, "remove exact journal-owned directory"))?;
    }
    Ok(())
}

fn staging_marker_bytes(staging: &Path) -> Result<Option<Vec<u8>>, PrepareError> {
    let path = crate::runtime_mutation::staging_owner_path(staging);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io(error, "inspect staging ownership marker")),
    };
    if !meta.is_file() || linked(&meta) || meta.len() > MAX_JOURNAL_BYTES {
        return Err(conflict(
            "staging ownership marker is not a bounded plain file",
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| io(e, "open staging ownership marker"))?
        .take(MAX_JOURNAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io(e, "read staging ownership marker"))?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(conflict("staging ownership marker is oversized"));
    }
    Ok(Some(bytes))
}

fn remove_owned_staging(staging: &Path, record: &Publication) -> Result<(), PrepareError> {
    let remove_marker = if let Some(expected) = &record.staging_marker_digest {
        match staging_marker_bytes(staging)? {
            Some(bytes) if sha256_hex(&bytes) == *expected => true,
            Some(_) => {
                return Err(conflict(
                    "staging ownership marker changed; preserving journal",
                ))
            }
            None => false, // A prior exact cleanup may already have removed it.
        }
    } else {
        false
    }; // A marker not captured in this journal is not ours to delete.
    remove_owned_directory(staging)?;
    if remove_marker {
        fs::remove_file(crate::runtime_mutation::staging_owner_path(staging))
            .map_err(|e| io(e, "remove exact staging ownership marker"))?;
    }
    Ok(())
}

fn rollback_error(
    seed: &Path,
    original: PrepareError,
    restore: Result<(), PrepareError>,
) -> PrepareError {
    if let Err(restore) = restore {
        return PrepareError::new(
            "publication_rollback_failed",
            format!(
                "{}; restoring prior seed also failed ({}): {}; journal retained",
                original.message, restore.code, restore.message
            ),
        );
    }
    // Only forget the intent after a proven rollback/no-previous-seed result.
    if let Err(cleanup) = remove_journal(seed) {
        return PrepareError::new(
            "publication_cleanup_pending",
            format!(
                "{}; rollback completed but {}",
                original.message, cleanup.message
            ),
        );
    }
    original
}

pub(super) fn publish_seed(staging: &Path, seed: &Path) -> Result<(), PrepareError> {
    publish_with_rename(staging, seed, &mut |from, to| fs::rename(from, to))
}

pub(super) fn publish_with_rename(
    staging: &Path,
    seed: &Path,
    rename: &mut impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), PrepareError> {
    let record = Publication::new(staging, seed)?;
    let backup = record.backup(seed)?;
    if directory_exists(&backup)? {
        return Err(conflict("new publication backup path already exists"));
    }
    write_journal(seed, &record)?;
    if record.previous_digest.is_some() {
        if let Err(error) = rename(seed, &backup) {
            return Err(rollback_error(
                seed,
                io(error, "move existing seed to owned backup"),
                Ok(()),
            ));
        }
    }
    if let Err(error) = rename(staging, seed) {
        let restore = if record.previous_digest.is_some() {
            match directory_exists(seed) {
                Ok(false) => {
                    rename(&backup, seed).map_err(|e| io(e, "restore exact previous seed"))
                }
                Ok(true) => Err(conflict("rollback destination is unexpectedly occupied")),
                Err(error) => Err(error),
            }
        } else {
            Ok(())
        };
        return Err(rollback_error(
            seed,
            io(error, "move prepared staging to seed"),
            restore,
        ));
    }
    // Verify the published bytes before discarding the sole old copy. Failures
    // retain both the backup and journal and are NOT reported as a clean success.
    recover_seed(seed)
}

pub(super) fn recover_seed(seed: &Path) -> Result<(), PrepareError> {
    let present = directory_exists(seed)?;
    let Some(record) = read_journal(seed)? else {
        if !present && parent(seed)?.exists() {
            for entry in fs::read_dir(parent(seed)?)
                .map_err(|e| io(e, "inspect legacy publication state"))?
            {
                let entry = entry.map_err(|e| io(e, "inspect legacy publication entry"))?;
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".seed-backup-")
                {
                    return Err(conflict(
                        "unjournaled backup exists; do not guess its destination or delete it",
                    ));
                }
            }
        }
        return Ok(()); // Prefix directories alone never authorize deletion.
    };
    let backup = record.backup(seed)?;
    let staging = record.staging(seed)?;
    let has_backup = directory_exists(&backup)?;
    let has_staging = directory_exists(&staging)?;
    if present {
        let digest = seed_tree_digest(seed)?;
        if !has_staging && digest == record.prepared_digest {
            if has_backup && record.previous_digest.is_none() {
                return Err(conflict("unexpected backup for initial publication"));
            }
            remove_owned_directory(&backup)?;
            remove_owned_staging(&staging, &record)?;
            return remove_journal(seed);
        }
        if !has_backup && record.previous_digest.as_ref() == Some(&digest) {
            remove_owned_staging(&staging, &record)?;
            return remove_journal(seed);
        }
        return Err(conflict(
            "current seed conflicts with journal; preserving all copies",
        ));
    }
    match (&record.previous_digest, has_backup) {
        (Some(expected), true) => {
            if seed_tree_digest(&backup)? != *expected {
                return Err(conflict("prior backup digest differs; refusing recovery"));
            }
            fs::rename(&backup, seed).map_err(|e| io(e, "recover exact prior seed"))?;
        }
        (None, false) => {}
        _ => {
            return Err(conflict(
                "prior seed backup is missing or unexpected; preserving journal",
            ))
        }
    }
    remove_owned_staging(&staging, &record)?;
    remove_journal(seed)
}
