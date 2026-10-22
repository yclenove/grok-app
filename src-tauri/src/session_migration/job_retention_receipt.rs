//! Recovery intent only, outside immutable previews and published archives.
//! The acceptance owner serializes these files; no file is an accepted grant.
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::super::super::{catalog, errors::ImportError, hash, jobs};
use super::{catalog_file, deferred};

// Only identity digests and preview UUIDs live here, never input bodies. This
// limit applies to this new recovery format, not ordinary v2 stored inputs.
const RECEIPT_BYTES: u64 = 16 * 1024 * 1024;

#[cfg(test)]
thread_local! {
    static IO_FAULT: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(super) fn fail_once(point: &'static str) {
    IO_FAULT.with(|fault| fault.set(Some(point)));
}

#[cfg(test)]
fn maybe_fail(point: &str) -> Result<(), ImportError> {
    IO_FAULT.with(|fault| {
        if fault.get() == Some(point) {
            fault.set(None);
            Err(deferred(format!("synthetic recovery {point} failure")))
        } else {
            Ok(())
        }
    })
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Entry {
    pub preview_id: String,
    pub candidate_digest: String,
    pub input_digest: String,
    pub lease_digest: String,
    pub added_retention: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Receipt {
    pub version: u32,
    pub epoch_id: String,
    pub catalog_epoch: String,
    pub catalog_file: catalog_file::Identity,
    pub job_id: String,
    pub context_digest: String,
    pub entries: Vec<Entry>,
}

pub(super) fn directory() -> PathBuf {
    catalog::imports_dir().join("preview-retention")
}

pub(super) fn checked_path(path: &std::path::Path) -> Result<(), ImportError> {
    catalog::ensure_inside_imports(path)?;
    let root = catalog::imports_dir();
    let relative = path.strip_prefix(&root).map_err(deferred)?;
    let mut current = root;
    for component in std::iter::once(None).chain(relative.components().map(Some)) {
        if let Some(component) = component {
            current.push(component);
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                #[cfg(windows)]
                let reparse = {
                    use std::os::windows::fs::MetadataExt;
                    metadata.file_attributes() & 0x400 != 0
                };
                #[cfg(not(windows))]
                let reparse = metadata.file_type().is_symlink();
                if reparse || metadata.file_type().is_symlink() {
                    return Err(deferred("recovery metadata crosses a reparse boundary"));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(deferred(error)),
        }
    }
    Ok(())
}

pub(super) fn preview_path(entry: &Entry) -> PathBuf {
    catalog::staging_dir()
        .join("previews")
        .join(&entry.preview_id)
}

pub(super) fn context_digest(job: &jobs::JobRecord) -> String {
    hash::options_hash(&serde_json::json!([
        job.job_id,
        job.request_id,
        job.plan_id,
        job.plan_hash,
        job.created_at
    ]))
}

pub(super) fn lease_digest(lease: &jobs::PreviewLease) -> Result<String, ImportError> {
    let mut immutable = lease.clone();
    immutable.expires_unix = 0;
    immutable.prepare_batch_id = None;
    immutable.retained_by_job = false;
    serde_json::to_vec(&immutable)
        .map(|bytes| hash::sha256_bytes(&bytes))
        .map_err(deferred)
}

fn canonical_uuid(text: &str) -> bool {
    uuid::Uuid::parse_str(text).is_ok_and(|id| id.to_string() == text)
}

fn digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Receipt {
    fn validate(&self) -> Result<(), ImportError> {
        let mut previews = std::collections::BTreeSet::new();
        let mut candidates = std::collections::BTreeSet::new();
        if self.version != 3
            || !canonical_uuid(&self.epoch_id)
            || !canonical_uuid(&self.catalog_epoch)
            || !self.catalog_file.supported()
            || self.job_id.is_empty()
            || !digest(&self.context_digest)
            || self.entries.iter().any(|entry| {
                !canonical_uuid(&entry.preview_id)
                    || !digest(&entry.candidate_digest)
                    || !digest(&entry.input_digest)
                    || !digest(&entry.lease_digest)
                    || !previews.insert(&entry.preview_id)
                    || !candidates.insert(&entry.candidate_digest)
            })
        {
            return Err(deferred("invalid or unsupported recovery receipt"));
        }
        Ok(())
    }

    fn path(&self) -> PathBuf {
        directory().join(format!("{}.json", self.epoch_id))
    }

    fn acceptance_key(&self) -> String {
        format!("preview_retention_accepted:{}", self.epoch_id)
    }

    fn acceptance_digest(&self) -> Result<String, ImportError> {
        serde_json::to_vec(self)
            .map(|bytes| hash::sha256_bytes(&bytes))
            .map_err(deferred)
    }

    /// Recovery-only witness is committed atomically with the existing job and
    /// all input rows. It cannot grant a reader or replace membership checks.
    pub(super) fn record_acceptance(&self, conn: &Connection) -> Result<(), ImportError> {
        if conn.is_autocommit() {
            return Err(deferred("acceptance witness requires the job transaction"));
        }
        if catalog::retention_catalog_epoch(conn)?.as_deref() != Some(&self.catalog_epoch) {
            return Err(deferred("acceptance catalog differs from recovery epoch"));
        }
        conn.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2)",
            [self.acceptance_key(), self.acceptance_digest()?],
        )
        .map_err(deferred)?;
        Ok(())
    }

    pub(super) fn accepted_witness(&self, conn: &Connection) -> Result<bool, ImportError> {
        if catalog::retention_catalog_epoch(conn)?.as_deref() != Some(&self.catalog_epoch) {
            return Err(deferred(
                "recovery belongs to a missing or foreign catalog epoch",
            ));
        }
        let value =
            catalog::retention_metadata_text(conn, &self.acceptance_key(), 64).map_err(deferred)?;
        match value {
            None => Ok(false),
            Some(value) if value == self.acceptance_digest()? => Ok(true),
            Some(_) => Err(deferred("accepted recovery witness disagrees")),
        }
    }

    pub(super) fn persist(&self) -> Result<(), ImportError> {
        self.validate()?;
        checked_path(&directory())?;
        fs::create_dir_all(directory()).map_err(deferred)?;
        let path = self.path();
        checked_path(&path)?;
        if path.try_exists().map_err(deferred)? {
            return Err(deferred("recovery epoch already exists"));
        }
        let bytes = serde_json::to_vec(self).map_err(deferred)?;
        if bytes.len() as u64 > RECEIPT_BYTES {
            return Err(deferred("recovery receipt exceeds its format budget"));
        }
        let temporary = directory().join(format!("{}.pending", self.epoch_id));
        checked_path(&temporary)?;
        // A failed or interrupted write stays explicit. Do not erase partial
        // evidence or reinterpret it as a missing recovery owner.
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(deferred)?;
        #[cfg(test)]
        maybe_fail("write")?;
        file.write_all(&bytes).map_err(deferred)?;
        #[cfg(test)]
        maybe_fail("sync")?;
        file.sync_all().map_err(deferred)?;
        drop(file);
        #[cfg(test)]
        maybe_fail("rename")?;
        fs::rename(&temporary, &path).map_err(deferred)?;
        sync_directory()?;
        Ok(())
    }

    pub(super) fn remove(&self) -> Result<(), ImportError> {
        let path = self.path();
        checked_path(&path)?;
        #[cfg(test)]
        maybe_fail("remove")?;
        fs::remove_file(path).map_err(deferred)?;
        sync_directory()
    }
}

fn sync_directory() -> Result<(), ImportError> {
    #[cfg(unix)]
    {
        std::fs::File::open(directory())
            .and_then(|file| file.sync_all())
            .map_err(deferred)?;
    }
    // On Windows file.sync_all precedes the same-volume rename. Directory
    // handles cannot be flushed with std::fs::File; never claim power-loss proof.
    Ok(())
}

pub(super) fn pending() -> bool {
    if checked_path(&directory()).is_err() {
        return true;
    }
    match fs::read_dir(directory()) {
        Ok(mut entries) => entries.next().is_some(),
        Err(error) => error.kind() != std::io::ErrorKind::NotFound,
    }
}

pub(super) fn load_all() -> Result<Vec<Receipt>, ImportError> {
    checked_path(&directory())?;
    let entries = match fs::read_dir(directory()) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(deferred(error)),
    };
    let mut out = Vec::new();
    let mut previews = std::collections::BTreeSet::new();
    for entry in entries {
        let path = entry.map_err(deferred)?.path();
        checked_path(&path)?;
        let metadata = fs::symlink_metadata(&path).map_err(deferred)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > RECEIPT_BYTES
        {
            return Err(deferred("nonregular or oversized recovery metadata"));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .map_err(deferred)?
            .take(RECEIPT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(deferred)?;
        if bytes.len() as u64 > RECEIPT_BYTES {
            return Err(deferred("oversized recovery metadata"));
        }
        let receipt: Receipt = serde_json::from_slice(&bytes).map_err(deferred)?;
        receipt.validate()?;
        if path != receipt.path()
            || receipt
                .entries
                .iter()
                .any(|entry| !previews.insert(entry.preview_id.clone()))
        {
            return Err(deferred("partial, foreign or overlapping recovery epoch"));
        }
        out.push(receipt);
    }
    Ok(out)
}
