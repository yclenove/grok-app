use super::errors::{ErrorCode, ImportError};
use super::jobs::*;
use super::selection::SelectionSet;
#[cfg(test)]
use crate::session_migration::publish_timing::{span as timing_span, Stage as TimingStage};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{OnceLock, Weak};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

const VERIFIED_PUBLISHED_CACHE_LIMIT: usize = 8;
const PERSISTED_PROOF_LIMIT: usize = 64;
const PERSISTED_PROOF_MAX_BYTES: u64 = 8 * 1024 * 1024;
// v1 proved file/selection integrity but not a receipt's explicit member pin.
// Old proofs must revalidate even when their no-member receipt key is unchanged.
const PERSISTED_PROOF_VERSION: u32 = 2;
const PERSISTED_PROOF_DOMAIN: &str = "session-import-published-verification-v2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublishedFileFingerprint {
    len: u64,
    marker: [u64; 8],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublishedFingerprint {
    files: BTreeMap<String, PublishedFileFingerprint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PublishedVerificationKey {
    revision_dir: PathBuf,
    receipt_digest: String,
}

#[derive(Debug, Clone)]
struct PublishedVerificationCacheEntry {
    key: PublishedVerificationKey,
    fingerprint: PublishedFingerprint,
    selection_set: SelectionSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistedVerificationPayload {
    version: u32,
    revision_dir: String,
    receipt_digest: String,
    fingerprint: PublishedFingerprint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistedVerificationEnvelope {
    payload: PersistedVerificationPayload,
    mac: String,
}

static VERIFIED_PUBLISHED_CACHE: OnceLock<Mutex<VecDeque<PublishedVerificationCacheEntry>>> =
    OnceLock::new();
static PUBLISHED_VERIFICATION_LOCKS: OnceLock<
    Mutex<HashMap<PublishedVerificationKey, Weak<Mutex<()>>>>,
> = OnceLock::new();

#[cfg(test)]
static FULL_PUBLISHED_READ_VERIFICATIONS: AtomicUsize = AtomicUsize::new(0);

fn published_verification_error(message: impl Into<String>) -> ImportError {
    ImportError::new(ErrorCode::HistoryAccessUnavailable, message)
}

fn published_file_paths(
    dir: &Path,
    rec: &Receipt,
) -> Result<BTreeMap<String, PathBuf>, ImportError> {
    let mut paths = BTreeMap::new();
    for name in [
        "manifest.json",
        "events.jsonl",
        "assets.json",
        "selection.json",
    ] {
        let path = dir.join(name);
        crate::session_migration::catalog::ensure_inside_imports(&path)?;
        paths.insert(name.to_string(), path);
    }

    let blobs = dir.join("blobs");
    match fs::symlink_metadata(&blobs) {
        Ok(metadata) => {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(published_verification_error(
                    "published blobs path is not a regular directory",
                ));
            }
            for entry in fs::read_dir(&blobs).map_err(|error| {
                ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
            })? {
                let entry = entry.map_err(|error| {
                    ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
                })?;
                let file_type = entry.file_type().map_err(|error| {
                    ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
                })?;
                if !file_type.is_file() || file_type.is_symlink() {
                    return Err(published_verification_error(
                        "published blob is not a regular file",
                    ));
                }
                let key = format!("blobs/{}", entry.file_name().to_string_lossy());
                if paths.insert(key, entry.path()).is_some() {
                    return Err(published_verification_error(
                        "published blob names are not unique",
                    ));
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                error.to_string(),
            ));
        }
    }

    let actual = paths.keys().cloned().collect::<BTreeSet<_>>();
    let expected = rec.file_hashes.keys().cloned().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(published_verification_error(
            "published revision file set does not match its receipt",
        ));
    }
    Ok(paths)
}

#[cfg(windows)]
fn published_file_fingerprint(path: &Path) -> Result<PublishedFileFingerprint, ImportError> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        FileBasicInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_BASIC_INFO,
    };

    let metadata = fs::symlink_metadata(path)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(published_verification_error(
            "published revision file is not regular",
        ));
    }
    let file = fs::File::open(path)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    let handle = HANDLE(file.as_raw_handle());
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    let mut basic = FILE_BASIC_INFO::default();
    // SAFETY: `file` owns a live handle for both calls, and both output
    // pointers refer to correctly sized initialized structures.
    unsafe {
        GetFileInformationByHandle(handle, &mut info)
            .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
        GetFileInformationByHandleEx(
            handle,
            FileBasicInfo,
            (&mut basic as *mut FILE_BASIC_INFO).cast(),
            std::mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || basic.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
    {
        return Err(published_verification_error(
            "published revision file is a reparse point",
        ));
    }
    let len = ((info.nFileSizeHigh as u64) << 32) | info.nFileSizeLow as u64;
    Ok(PublishedFileFingerprint {
        len,
        marker: [
            info.dwVolumeSerialNumber as u64,
            ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
            basic.CreationTime as u64,
            basic.LastWriteTime as u64,
            basic.ChangeTime as u64,
            basic.FileAttributes as u64,
            info.nNumberOfLinks as u64,
            0,
        ],
    })
}

#[cfg(unix)]
fn published_file_fingerprint(path: &Path) -> Result<PublishedFileFingerprint, ImportError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let symlink_metadata = fs::symlink_metadata(path)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    if !symlink_metadata.is_file() || symlink_metadata.file_type().is_symlink() {
        return Err(published_verification_error(
            "published revision file is not regular",
        ));
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    let metadata = file
        .metadata()
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    if !metadata.is_file() {
        return Err(published_verification_error(
            "published revision file is not regular",
        ));
    }
    Ok(PublishedFileFingerprint {
        len: metadata.len(),
        marker: [
            metadata.dev(),
            metadata.ino(),
            metadata.mtime() as u64,
            metadata.mtime_nsec() as u64,
            metadata.ctime() as u64,
            metadata.ctime_nsec() as u64,
            metadata.mode() as u64,
            metadata.nlink(),
        ],
    })
}

#[cfg(not(any(windows, unix)))]
fn published_file_fingerprint(path: &Path) -> Result<PublishedFileFingerprint, ImportError> {
    fn timestamp(value: Result<std::time::SystemTime, std::io::Error>) -> u64 {
        value
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or_default()
    }

    let metadata = fs::symlink_metadata(path)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(published_verification_error(
            "published revision file is not regular",
        ));
    }
    Ok(PublishedFileFingerprint {
        len: metadata.len(),
        marker: [
            timestamp(metadata.created()),
            timestamp(metadata.modified()),
            timestamp(metadata.accessed()),
            0,
            0,
            0,
            0,
            0,
        ],
    })
}

fn published_fingerprint(dir: &Path, rec: &Receipt) -> Result<PublishedFingerprint, ImportError> {
    let mut files = BTreeMap::new();
    for (name, path) in published_file_paths(dir, rec)? {
        crate::session_migration::catalog::ensure_inside_imports(&path)?;
        files.insert(name, published_file_fingerprint(&path)?);
    }
    Ok(PublishedFingerprint { files })
}

fn published_verification_key(
    dir: &Path,
    rec: &Receipt,
) -> Result<PublishedVerificationKey, ImportError> {
    crate::session_migration::catalog::ensure_inside_imports(dir)?;
    let revision_dir = dir
        .canonicalize()
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    let mut receipt = serde_json::json!({
        "candidateId": rec.candidate_id,
        "createdEntry": rec.created_entry,
        "fileHashes": rec.file_hashes,
        "importId": rec.import_id,
        "installationId": rec.installation_id,
        "jobId": rec.job_id,
        "legacyId": rec.legacy_id,
        "nativeSessionId": rec.native_session_id,
        "optionsHash": rec.options_hash,
        "previousRevision": rec.previous_revision,
        "provider": rec.provider,
        "revisionId": rec.revision_id,
        "rootPath": rec.root_path,
        "selectionHash": rec.selection_hash,
        "selectionSet": rec.selection_set,
        "snapshotHash": rec.snapshot_hash,
        "sourceId": rec.source_id,
        "version": rec.version,
    });
    // Preserve the canonical legacy no-member key, not the weaker v1 proof.
    if let Some(pin) = &rec.source_member {
        receipt["sourceMember"] = serde_json::to_value(pin)
            .map_err(|error| published_verification_error(error.to_string()))?;
    }
    let receipt = crate::session_migration::hash::canonical_json(&receipt);
    Ok(PublishedVerificationKey {
        revision_dir,
        receipt_digest: crate::session_migration::hash::sha256_bytes(receipt.as_bytes()),
    })
}

fn persisted_proof_dir() -> PathBuf {
    crate::session_migration::catalog::imports_dir().join(".verification-cache")
}

fn persisted_proof_path(key: &PublishedVerificationKey) -> PathBuf {
    let identity = crate::session_migration::hash::canonical_json(&serde_json::json!({
        "receiptDigest": key.receipt_digest,
        "revisionDir": key.revision_dir.to_string_lossy(),
    }));
    persisted_proof_dir().join(format!(
        "{}.json",
        crate::session_migration::hash::sha256_bytes(identity.as_bytes())
    ))
}

fn proof_payload_bytes(payload: &PersistedVerificationPayload) -> Result<Vec<u8>, ImportError> {
    let value = serde_json::to_value(payload)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    Ok(crate::session_migration::hash::canonical_json(&value).into_bytes())
}

fn expected_proof_payload(
    key: &PublishedVerificationKey,
    fingerprint: &PublishedFingerprint,
) -> PersistedVerificationPayload {
    PersistedVerificationPayload {
        version: PERSISTED_PROOF_VERSION,
        revision_dir: key.revision_dir.to_string_lossy().into_owned(),
        receipt_digest: key.receipt_digest.clone(),
        fingerprint: fingerprint.clone(),
    }
}

fn persisted_proof_matches(
    key: &PublishedVerificationKey,
    fingerprint: &PublishedFingerprint,
) -> Result<bool, ImportError> {
    let path = persisted_proof_path(key);
    crate::session_migration::catalog::ensure_inside_imports(&path)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                error.to_string(),
            ));
        }
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > PERSISTED_PROOF_MAX_BYTES
    {
        return Ok(false);
    }
    let raw = fs::read(&path)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    let envelope: PersistedVerificationEnvelope = match serde_json::from_slice(&raw) {
        Ok(envelope) => envelope,
        Err(_) => return Ok(false),
    };
    let expected = expected_proof_payload(key, fingerprint);
    if envelope.payload != expected {
        return Ok(false);
    }
    let mac = match hex::decode(&envelope.mac) {
        Ok(mac) if mac.len() == 32 => mac,
        _ => return Ok(false),
    };
    let payload = proof_payload_bytes(&envelope.payload)?;
    super::cursor::verify_installation_payload(PERSISTED_PROOF_DOMAIN, &payload, &mac)
}

fn ensure_persisted_proof_dir() -> Result<PathBuf, ImportError> {
    let dir = persisted_proof_dir();
    crate::session_migration::catalog::ensure_inside_imports(&dir)?;
    fs::create_dir_all(&dir)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    let metadata = fs::symlink_metadata(&dir)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(published_verification_error(
            "published verification cache is not a regular directory",
        ));
    }
    Ok(dir)
}

fn prune_persisted_proofs(dir: &Path) {
    #[cfg(test)]
    let _timing = timing_span(TimingStage::ProofPrune);
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut proofs = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).ok()?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || path.extension().and_then(|value| value.to_str()) != Some("json")
            {
                return None;
            }
            Some((metadata.modified().ok(), path))
        })
        .collect::<Vec<_>>();
    proofs.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    for (_, path) in proofs.into_iter().skip(PERSISTED_PROOF_LIMIT) {
        let _ = fs::remove_file(path);
    }
}

fn persist_verification_proof(
    key: &PublishedVerificationKey,
    fingerprint: &PublishedFingerprint,
) -> Result<(), ImportError> {
    let dir = ensure_persisted_proof_dir()?;
    let path = persisted_proof_path(key);
    crate::session_migration::catalog::ensure_inside_imports(&path)?;
    let payload = expected_proof_payload(key, fingerprint);
    let payload_bytes = proof_payload_bytes(&payload)?;
    let envelope = PersistedVerificationEnvelope {
        mac: hex::encode(super::cursor::sign_installation_payload(
            PERSISTED_PROOF_DOMAIN,
            &payload_bytes,
        )?),
        payload,
    };
    let bytes = serde_json::to_vec(&envelope)
        .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
    if bytes.len() as u64 > PERSISTED_PROOF_MAX_BYTES {
        return Err(published_verification_error(
            "published verification proof exceeds its byte limit",
        ));
    }
    let tmp = dir.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let write = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&tmp)
            .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
        use std::io::Write;
        file.write_all(&bytes)
            .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
        {
            #[cfg(test)]
            let _timing = timing_span(TimingStage::ProofSync);
            file.sync_all().map_err(|error| {
                ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
            })?;
        }
        drop(file);
        if path.exists() {
            fs::remove_file(&path).map_err(|error| {
                ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
            })?;
        }
        fs::rename(&tmp, &path)
            .map_err(|error| ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string()))?;
        Ok(())
    })();
    if write.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    write?;
    prune_persisted_proofs(&dir);
    Ok(())
}

fn insert_verified_entry(
    key: PublishedVerificationKey,
    fingerprint: PublishedFingerprint,
    selection_set: SelectionSet,
) {
    let mut cache = published_verification_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.retain(|entry| entry.key != key);
    cache.push_front(PublishedVerificationCacheEntry {
        key,
        fingerprint,
        selection_set,
    });
    while cache.len() > VERIFIED_PUBLISHED_CACHE_LIMIT {
        cache.pop_back();
    }
}

fn published_verification_cache() -> &'static Mutex<VecDeque<PublishedVerificationCacheEntry>> {
    VERIFIED_PUBLISHED_CACHE.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn published_verification_lock(key: &PublishedVerificationKey) -> std::sync::Arc<Mutex<()>> {
    let mut locks = PUBLISHED_VERIFICATION_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks.retain(|_, lock| lock.strong_count() != 0);
    if let Some(lock) = locks.get(key).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = std::sync::Arc::new(Mutex::new(()));
    locks.insert(key.clone(), std::sync::Arc::downgrade(&lock));
    lock
}

/// Verify immutable published bytes once, then retain an installation-signed
/// proof across restarts. Every read still enumerates the exact receipt file
/// set and compares strong file identity plus ctime/ChangeTime, so same-size
/// writes with a restored mtime invalidate both caches and fall back to a
/// complete SHA-256 verification.
pub(crate) fn verify_published_for_read(
    dir: &Path,
    rec: &Receipt,
) -> Result<SelectionSet, ImportError> {
    validate_receipt_shape(rec)?;
    let key = published_verification_key(dir, rec)?;
    let verification_lock = published_verification_lock(&key);
    let _verification_guard = verification_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let before = {
        #[cfg(test)]
        let _timing = timing_span(TimingStage::FingerprintBefore);
        published_fingerprint(dir, rec)?
    };
    {
        let mut cache = published_verification_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(position) = cache.iter().position(|entry| entry.key == key) {
            let entry = cache.remove(position).expect("cache position exists");
            if entry.fingerprint == before {
                let selection_set = entry.selection_set.clone();
                cache.push_front(entry);
                return Ok(selection_set);
            }
        }
    }

    let proof_matches = {
        #[cfg(test)]
        let _timing = timing_span(TimingStage::ProofLookup);
        persisted_proof_matches(&key, &before).unwrap_or(false)
    };
    if proof_matches {
        let selection_set = rec.selection_set.canonicalized();
        insert_verified_entry(key, before, selection_set.clone());
        return Ok(selection_set);
    }

    #[cfg(test)]
    FULL_PUBLISHED_READ_VERIFICATIONS.fetch_add(1, Ordering::SeqCst);
    let selection_set = {
        #[cfg(test)]
        let _timing = timing_span(TimingStage::FullVerification);
        verify_published(dir, rec)?
    };
    let after = {
        #[cfg(test)]
        let _timing = timing_span(TimingStage::FingerprintAfter);
        published_fingerprint(dir, rec)?
    };
    if before != after {
        return Err(published_verification_error(
            "published revision changed during verification",
        ));
    }
    {
        #[cfg(test)]
        let _timing = timing_span(TimingStage::ProofPersist);
        let _ = persist_verification_proof(&key, &after);
    }
    insert_verified_entry(key, after, selection_set.clone());
    Ok(selection_set)
}

#[cfg(test)]
pub fn take_full_published_read_verifications() -> usize {
    FULL_PUBLISHED_READ_VERIFICATIONS.swap(0, Ordering::SeqCst)
}

#[cfg(test)]
pub fn clear_published_read_verification_cache() {
    published_verification_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
    PUBLISHED_VERIFICATION_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

#[cfg(test)]
pub fn clear_published_read_verification_proofs() {
    let dir = persisted_proof_dir();
    if dir.starts_with(crate::session_migration::catalog::imports_dir()) {
        let _ = fs::remove_dir_all(dir);
    }
}
