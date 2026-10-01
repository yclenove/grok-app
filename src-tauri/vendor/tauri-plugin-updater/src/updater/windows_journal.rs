//! Windows update write-ahead journal. A missing/corrupt existing journal is
//! never an empty transaction. No journal path or executable comes from IPC.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use windows_sys::Win32::Storage::FileSystem::*;

const MAX_RECORD: u64 = 1024 * 1024;
const STORE_READY: &[u8] = b"grok-windows-update-journal-v1\n";

pub(super) mod dispatch;
mod publication;
pub(super) mod witness;

pub(super) fn pending(message: impl ToString) -> Error {
    Error::WindowsInstallPending {
        phase: "journal",
        message: message.to_string(),
    }
}

fn publication_error(stage: &'static str, error: impl std::fmt::Display) -> Error {
    pending(format!("journal publish/{stage}: {error}"))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    pub executable: Vec<u16>,
    pub executable_digest: [u8; 32],
    pub app_name: String,
    pub key_digest: [u8; 32],
    pub install_mode: String,
    pub installer_args: Vec<Vec<u16>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Phase {
    Prepared,
    LaunchIntent,
    Accepted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Kind {
    Nsis,
    Msi,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProcessIdentity {
    pub pid: u32,
    pub created: u64,
}

/// An exact OS process exit is evidence, not an installation/rollback receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProcessExit {
    pub identity: ProcessIdentity,
    pub exited: u64,
    pub code: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub candidate_id: String,
    pub source_version: String,
    pub version: String,
    pub target: String,
    pub signature: String,
    pub payload_digest: [u8; 32],
    pub installer_digest: [u8; 32],
    pub kind: Kind,
    pub archive_entry: Option<String>,
    pub binding: Binding,
    pub current_args: Vec<Vec<u16>>,
    pub phase: Phase,
    pub process: Option<ProcessIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_exit: Option<ProcessExit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness: Option<witness::Ticket>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatcher: Option<witness::Ticket>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inventory: Option<super::windows_install::inventory::SignedInventory>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema: u32,
    record: Option<Record>,
}

pub(super) struct Journal {
    root: PathBuf,
    // Order matters: release files before the pinned directory chain.
    _owner: File,
    _directories: Vec<File>,
    state: Mutex<Option<Record>>,
    // An uncertain write cannot be repaired by rereading our old in-memory state.
    fault: Mutex<Option<String>>,
}

impl Journal {
    pub fn open(root: &Path) -> Result<Self> {
        let directories = pin_directories(root).map_err(pending)?;
        let owner = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(root.join("owner.lock"))
            .map_err(pending)?;
        regular_file(&owner).map_err(pending)?;
        let journal = Self {
            root: root.to_path_buf(),
            _owner: owner,
            _directories: directories,
            state: Mutex::new(None),
            fault: Mutex::new(None),
        };
        if !root.join("active.dpapi").try_exists().map_err(pending)? {
            // An interrupted directory/lock creation can be retried. A store
            // that has ever returned successfully has a permanent sentinel;
            // absence of its active record must never become a new candidate.
            let entries = std::fs::read_dir(root).map_err(pending)?;
            for entry in entries {
                if entry.map_err(pending)?.file_name() != "owner.lock" {
                    return Err(pending(
                        "Existing update store has no authoritative active record",
                    ));
                }
            }
            journal.write(&None)?;
        }
        let encrypted = read_limited(&journal.root.join("active.dpapi"), MAX_RECORD)?;
        let plain = protect(&encrypted, &journal.entropy(), false).map_err(pending)?;
        let envelope: Envelope = serde_json::from_slice(&plain).map_err(pending)?;
        if envelope.schema != 1 {
            return Err(pending("Unsupported Windows update journal schema"));
        }
        if let Some(record) = &envelope.record {
            validate(record)?;
        }
        let ready = root.join("store.ready");
        if !ready.try_exists().map_err(pending)? {
            write_new(&ready, STORE_READY).map_err(pending)?;
        }
        if read_limited(&ready, 128)? != STORE_READY {
            return Err(pending("Update store initialization identity has changed"));
        }
        *journal.lock()? = envelope.record;
        Ok(journal)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Option<Record>>> {
        if let Some(message) = self.fault.lock().map_err(pending)?.as_ref() {
            return Err(pending(message));
        }
        self.state.lock().map_err(pending)
    }

    fn entropy(&self) -> Vec<u8> {
        self.root
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    pub fn snapshot(&self) -> Result<Option<Record>> {
        Ok(self.lock()?.clone())
    }

    pub(in crate::updater) fn completion_directory(&self) -> &Path {
        &self.root
    }

    pub(in crate::updater) fn completed_document(&self, id: &str) -> Result<Option<Vec<u8>>> {
        validate_id(id)?;
        let slot = self.lock()?;
        // An archive alone is not a terminal outcome while active still owns it.
        if slot
            .as_ref()
            .is_some_and(|record| record.candidate_id == id)
        {
            return Ok(None);
        }
        let path = self.root.join(format!("{id}.completed.dpapi"));
        let file = match protected_read(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(pending(error)),
        };
        let mut entropy = self.entropy();
        entropy.extend_from_slice(b"\0nsis-completed-v1");
        let bytes = protect(&read_bounded(file, MAX_RECORD)?, &entropy, false).map_err(pending)?;
        Ok(Some(bytes))
    }

    pub(in crate::updater) fn retire_completed(
        &self,
        proof: &super::windows_install::completion::VerifiedCompletion,
    ) -> Result<()> {
        let mut slot = self.lock()?;
        let original = slot
            .as_ref()
            .filter(|r| *r == proof.original())
            .ok_or_else(|| pending("Completion no longer owns the original journal candidate"))?;
        let terminal = self
            .root
            .join(format!("{}.completed.dpapi", original.candidate_id));
        let plain = serde_json::to_vec(proof).map_err(pending)?;
        let mut entropy = self.entropy();
        entropy.extend_from_slice(b"\0nsis-completed-v1");
        if terminal.try_exists().map_err(pending)? {
            let existing =
                protect(&read_limited(&terminal, MAX_RECORD)?, &entropy, false).map_err(pending)?;
            if existing != plain {
                return Err(pending("Conflicting completed installer receipt"));
            }
        } else {
            let encrypted = protect(&plain, &entropy, true).map_err(pending)?;
            if encrypted.len() as u64 > MAX_RECORD {
                return Err(pending("Completion record exceeds journal limit"));
            }
            write_new(&terminal, &encrypted).map_err(pending)?;
        }
        // Preserve the authenticated terminal receipt before clearing active.
        // A crash between these writes leaves the original candidate blocked;
        // re-verification can idempotently finish, never infer success by time.
        let mut terminal_lease = protected_read(&terminal).map_err(pending)?;
        if terminal_lease.metadata().map_err(pending)?.len() > MAX_RECORD {
            return Err(pending("Completion record exceeds journal limit"));
        }
        let mut readback = Vec::new();
        terminal_lease.read_to_end(&mut readback).map_err(pending)?;
        let readback = protect(&readback, &entropy, false).map_err(pending)?;
        if readback != plain {
            return Err(pending("Completion receipt readback mismatch"));
        }
        self.write(&None)?;
        *slot = None;
        // Keep the archive immutable until the durable active-slot clear lands.
        drop(terminal_lease);
        Ok(())
    }

    pub fn path(&self, id: &str, kind: Option<Kind>) -> Result<PathBuf> {
        validate_id(id)?;
        let suffix = match kind {
            None => "payload",
            Some(Kind::Nsis) => "exe",
            Some(Kind::Msi) => "msi",
        };
        Ok(self.root.join(format!("{id}.{suffix}")))
    }

    pub fn prepare(&self, record: Record, payload: &[u8], installer: &[u8]) -> Result<File> {
        validate(&record)?;
        let mut slot = self.lock()?;
        if slot.is_some() || record.phase != Phase::Prepared || record.process.is_some() {
            return Err(pending("Original journal candidate cannot be replaced"));
        }
        if digest(payload) != record.payload_digest || digest(installer) != record.installer_digest
        {
            return Err(pending(
                "Prepared journal bytes do not match the verified candidate",
            ));
        }
        let payload_path = self.path(&record.candidate_id, None)?;
        let installer_path = self.path(&record.candidate_id, Some(record.kind))?;
        write_new(&payload_path, payload).map_err(pending)?;
        write_new(&installer_path, installer).map_err(pending)?;
        let mut payload_lease = protected_read(&payload_path).map_err(pending)?;
        let mut lease = protected_read(&installer_path).map_err(pending)?;
        let mut actual_payload = Vec::new();
        let mut actual_installer = Vec::new();
        payload_lease
            .read_to_end(&mut actual_payload)
            .map_err(pending)?;
        lease.read_to_end(&mut actual_installer).map_err(pending)?;
        lease.seek(SeekFrom::Start(0)).map_err(pending)?;
        if actual_payload != payload || actual_installer != installer {
            return Err(pending("Durable candidate changed before publication"));
        }
        self.write(&Some(record.clone()))?;
        *slot = Some(record);
        Ok(lease)
    }

    /// Compare-and-set, serialized across all threads and all App processes.
    #[cfg(test)]
    pub fn transition(
        &self,
        id: &str,
        expected: Phase,
        next: Phase,
        process: Option<ProcessIdentity>,
    ) -> Result<()> {
        let mut slot = self.lock()?;
        let original = slot
            .as_ref()
            .filter(|r| r.candidate_id == id && r.phase == expected && r.dispatcher.is_none())
            .ok_or_else(|| pending("Original update phase cannot authorize this transition"))?;
        if !matches!(
            (expected, next),
            (Phase::Prepared, Phase::LaunchIntent)
                | (Phase::LaunchIntent, Phase::Prepared)
                | (Phase::LaunchIntent, Phase::Accepted)
        ) {
            return Err(pending("Invalid update journal transition"));
        }
        let mut record = original.clone();
        record.phase = next;
        record.process = process;
        validate(&record)?;
        self.write(&Some(record.clone()))?;
        *slot = Some(record);
        Ok(())
    }

    /// Persist only a matched, signaled OS handle's exit. Never change the launch
    /// phase or grant a retry: exit 0 can still mean a delegated installer failed.
    pub fn record_process_exit(&self, id: &str, exit: &ProcessExit) -> Result<()> {
        let mut slot = self.lock()?;
        let original = slot
            .as_ref()
            .filter(|r| {
                r.candidate_id == id
                    && r.phase == Phase::Accepted
                    && r.process.as_ref() == Some(&exit.identity)
            })
            .ok_or_else(|| pending("Process exit does not belong to the accepted installer"))?;
        if let Some(previous) = &original.process_exit {
            return if previous == exit {
                Ok(())
            } else {
                Err(pending("Conflicting installer process exit evidence"))
            };
        }
        let mut record = original.clone();
        record.process_exit = Some(exit.clone());
        validate(&record)?;
        self.write(&Some(record.clone()))?;
        *slot = Some(record);
        Ok(())
    }

    fn write(&self, record: &Option<Record>) -> Result<()> {
        let result = (|| {
            let plain = serde_json::to_vec(&Envelope {
                schema: 1,
                record: record.clone(),
            })
            .map_err(|e| publication_error("serialize", e))?;
            let encrypted = protect(&plain, &self.entropy(), true)
                .map_err(|e| publication_error("protect", e))?;
            if encrypted.len() as u64 > MAX_RECORD {
                return Err(pending("Windows update journal exceeds its size limit"));
            }
            let temp = self.root.join(format!("{}.writing", uuid::Uuid::new_v4()));
            write_new(&temp, &encrypted).map_err(|e| publication_error("create-and-flush", e))?;
            let active = self.root.join("active.dpapi");
            let from = wide(&temp);
            let to = wide(&active);
            let exists = active
                .try_exists()
                .map_err(|e| publication_error("inspect-target", e))?;
            // A native POSIX-style rename keeps existing snapshot handles valid
            // without MoveFileEx's open-target refusal or ReplaceFile's path gap.
            // Never retry/fallback after an uncertain publication.
            if exists {
                publication::replace(&temp, &active)
                    .map_err(|e| publication_error("replace", e))?;
            } else {
                // First initialization only; do not overwrite an unexpected file.
                if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
                    return Err(publication_error(
                        "replace",
                        std::io::Error::last_os_error(),
                    ));
                }
            }
            if read_limited(&active, MAX_RECORD).map_err(|e| publication_error("readback", e))?
                != encrypted
            {
                return Err(pending("Published update journal could not be verified"));
            }
            Ok(())
        })();
        if let Err(error) = &result {
            *self.fault.lock().map_err(pending)? = Some(error.to_string());
        }
        result
    }
}

pub(super) fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn validate_id(id: &str) -> Result<()> {
    let uuid = uuid::Uuid::parse_str(id).map_err(pending)?;
    if uuid.get_version_num() != 4 || uuid.hyphenated().to_string() != id {
        return Err(pending("Noncanonical update candidate identity"));
    }
    Ok(())
}

pub(super) fn validate(record: &Record) -> Result<()> {
    validate_id(&record.candidate_id)?;
    if let Some(inventory) = &record.inventory {
        inventory.validate_size()?;
    }
    if let Some(ticket) = &record.dispatcher {
        validate_id(&ticket.id)?;
        if record.phase == Phase::Prepared
            || record.witness.is_some()
            || ticket.helper.pid == 0
            || ticket.helper.created == 0
        {
            return Err(pending("Invalid independent installer dispatcher binding"));
        }
    }
    if let Some(ticket) = &record.witness {
        validate_id(&ticket.id)?;
        if record.phase != Phase::Accepted
            || record.process.is_none()
            || ticket.helper.pid == 0
            || ticket.helper.created == 0
        {
            return Err(pending("Invalid independent process witness binding"));
        }
    }
    semver::Version::parse(&record.source_version).map_err(pending)?;
    semver::Version::parse(&record.version).map_err(pending)?;
    if record.signature.is_empty()
        || record.target.is_empty()
        || record.binding.executable.is_empty()
        || (record.phase != Phase::Accepted && record.process.is_some())
        || record
            .process
            .as_ref()
            .is_some_and(|p| p.pid == 0 || p.created == 0)
        || record.process_exit.as_ref().is_some_and(|exit| {
            record.phase != Phase::Accepted
                || record.process.as_ref() != Some(&exit.identity)
                || exit.exited < exit.identity.created
                || exit.exited == 0
        })
    {
        return Err(pending("Invalid update recovery record"));
    }
    Ok(())
}

pub(super) fn protected_read(path: &Path) -> std::io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    regular_file(&file)?;
    Ok(file)
}

fn regular_file(file: &File) -> std::io::Result<()> {
    ordinary_file(file, false)
}

fn ordinary_file(file: &File, allow_retired_snapshot: bool) -> std::io::Result<()> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    if info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0
        || (info.nNumberOfLinks != 1 && !(allow_retired_snapshot && info.nNumberOfLinks == 0))
    {
        return Err(std::io::Error::other(
            "Update files must be ordinary single-link files",
        ));
    }
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_WRITE_THROUGH)
        .open(path)?;
    regular_file(&file)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = protected_read(path).map_err(pending)?;
    read_bounded(file, limit)
}

// A helper polls atomic snapshots without denying the owner's replacement.
// Deny WRITE sharing; DELETE permits rename, not in-place mutation. Pin and
// validate the opened file, which is either the complete old or new snapshot.
fn read_snapshot(path: &Path) -> Result<Vec<u8>> {
    read_bounded(snapshot_file(path)?, MAX_RECORD)
}

fn snapshot_file(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(pending)?;
    // Atomic POSIX replacement can unlink this exact already-open old snapshot
    // before handle validation. Zero links is valid ONLY for a read-only helper
    // snapshot; hardlinks, reparse points and directories remain forbidden.
    // DPAPI + schema + candidate validation still apply before granting anything.
    ordinary_file(&file, true).map_err(pending)?;
    Ok(file)
}

fn read_bounded(file: File, limit: u64) -> Result<Vec<u8>> {
    if file.metadata().map_err(pending)?.len() > limit {
        return Err(pending("Oversized update journal"));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(pending)?;
    if bytes.len() as u64 > limit {
        return Err(pending("Oversized update journal"));
    }
    Ok(bytes)
}

// Pin each existing/created ancestor before using a descendant, rejecting
// junctions/symlinks and denying deletion/rename for the entire owner lifetime.
fn pin_directories(root: &Path) -> std::io::Result<Vec<File>> {
    pin_directory_chain(root, true)
}

pub(super) fn pin_existing_directories(root: &Path) -> std::io::Result<Vec<File>> {
    pin_directory_chain(root, false)
}

fn pin_directory_chain(root: &Path, create: bool) -> std::io::Result<Vec<File>> {
    if !root.is_absolute() {
        return Err(std::io::Error::other("Update root must be absolute"));
    }
    let mut path = PathBuf::new();
    let mut leases = Vec::new();
    for component in root.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                path.push(component);
                continue;
            }
            Component::Normal(_) => path.push(component),
            _ => return Err(std::io::Error::other("Unsafe update root component")),
        }
        if create {
            match std::fs::create_dir(&path) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(e),
            };
        }
        let file = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)?;
        let attributes = file.metadata()?.file_attributes();
        if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || attributes & FILE_ATTRIBUTE_DIRECTORY == 0
        {
            return Err(std::io::Error::other(
                "Update directory is not a plain directory",
            ));
        }
        leases.push(file);
    }
    Ok(leases)
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

// User-scoped DPAPI authenticates metadata and protects original restart args.
// Never use LOCAL_MACHINE or prompt for credentials. This is not a defense
// against arbitrary code already running as the same Windows user/admin.
fn protect(bytes: &[u8], entropy: &[u8], encrypt: bool) -> std::io::Result<Vec<u8>> {
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::*};
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len())
            .map_err(|_| std::io::Error::other("DPAPI input too large"))?,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_ptr() as *mut u8,
    };
    let mut output: CRYPT_INTEGER_BLOB = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> Record {
        Record {
            candidate_id: uuid::Uuid::new_v4().to_string(),
            source_version: "1.0.0".into(),
            version: "1.0.1".into(),
            target: "windows-x86_64".into(),
            signature: "test-only-journal-not-an-installer".into(),
            payload_digest: digest(b"signed package"),
            installer_digest: digest(b"inert test payload"),
            kind: Kind::Nsis,
            archive_entry: None,
            binding: Binding {
                executable: vec![120],
                executable_digest: [1; 32],
                app_name: "test".into(),
                key_digest: [2; 32],
                install_mode: "passive".into(),
                installer_args: vec![],
            },
            current_args: vec![vec![65]],
            phase: Phase::Prepared,
            process: None,
            process_exit: None,
            witness: None,
            dispatcher: None,
            inventory: None,
        }
    }

    #[test]
    fn matched_exit_is_append_only_and_does_not_authorize_replay() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("exit-evidence");
        let journal = Journal::open(&path).unwrap();
        let value = record();
        // Older schema-1 records omit this optional evidence field entirely.
        let legacy = serde_json::to_value(&value).unwrap();
        assert!(legacy.get("process_exit").is_none());
        assert_eq!(serde_json::from_value::<Record>(legacy).unwrap(), value);
        let lease = journal
            .prepare(value.clone(), b"signed package", b"inert test payload")
            .unwrap();
        let exit = ProcessExit {
            identity: ProcessIdentity {
                pid: 42,
                created: 100,
            },
            exited: 101,
            code: 259,
        };
        assert!(journal
            .record_process_exit(&value.candidate_id, &exit)
            .is_err());
        journal
            .transition(
                &value.candidate_id,
                Phase::Prepared,
                Phase::LaunchIntent,
                None,
            )
            .unwrap();
        assert!(journal
            .record_process_exit(&value.candidate_id, &exit)
            .is_err());
        journal
            .transition(
                &value.candidate_id,
                Phase::LaunchIntent,
                Phase::Accepted,
                Some(exit.identity.clone()),
            )
            .unwrap();
        for invalid in [
            ProcessExit {
                identity: ProcessIdentity {
                    pid: 42,
                    created: 99,
                },
                ..exit.clone()
            },
            ProcessExit {
                exited: 99,
                ..exit.clone()
            },
        ] {
            assert!(journal
                .record_process_exit(&value.candidate_id, &invalid)
                .is_err());
        }
        assert!(journal
            .record_process_exit(&uuid::Uuid::new_v4().to_string(), &exit)
            .is_err());
        journal
            .record_process_exit(&value.candidate_id, &exit)
            .unwrap();
        let encrypted = std::fs::read(path.join("active.dpapi")).unwrap();
        journal
            .record_process_exit(&value.candidate_id, &exit)
            .unwrap();
        assert_eq!(
            std::fs::read(path.join("active.dpapi")).unwrap(),
            encrypted,
            "identical polling must not publish again"
        );
        assert!(journal
            .record_process_exit(
                &value.candidate_id,
                &ProcessExit {
                    code: 0,
                    ..exit.clone()
                }
            )
            .is_err());
        assert!(journal
            .transition(&value.candidate_id, Phase::Accepted, Phase::Prepared, None)
            .is_err());
        drop(lease);
        drop(journal);
        let restored = Journal::open(&path).unwrap().snapshot().unwrap().unwrap();
        assert_eq!(restored.phase, Phase::Accepted);
        assert_eq!(restored.process_exit, Some(exit));
    }

    #[test]
    fn restart_retains_identity_bytes_phase_and_exclusive_owner() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery");
        let journal = Journal::open(&path).unwrap();
        assert!(journal.snapshot().unwrap().is_none());
        assert!(Journal::open(&path).is_err());
        let record = record();
        let lease = journal
            .prepare(record.clone(), b"signed package", b"inert test payload")
            .unwrap();
        assert!(std::fs::remove_dir_all(&path).is_err());
        assert!(std::fs::write(
            journal
                .path(&record.candidate_id, Some(record.kind))
                .unwrap(),
            b"replacement"
        )
        .is_err());
        assert!(journal
            .transition("stale", Phase::Prepared, Phase::LaunchIntent, None)
            .is_err());
        journal
            .transition(
                &record.candidate_id,
                Phase::Prepared,
                Phase::LaunchIntent,
                None,
            )
            .unwrap();
        drop(lease);
        drop(journal);
        let recovered = Journal::open(&path).unwrap();
        let pending = recovered.snapshot().unwrap().unwrap();
        assert_eq!(pending.candidate_id, record.candidate_id);
        assert_eq!(pending.phase, Phase::LaunchIntent);
        assert!(recovered
            .prepare(record, b"signed package", b"inert test payload")
            .is_err());
    }

    #[test]
    fn interrupted_empty_store_initialization_recovers_but_missing_history_does_not() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("owner.lock"), b"").unwrap();
        drop(Journal::open(&path).unwrap());
        assert_eq!(
            std::fs::read(path.join("store.ready")).unwrap(),
            STORE_READY
        );
        std::fs::remove_file(path.join("active.dpapi")).unwrap();
        assert!(Journal::open(&path).is_err());
    }

    #[test]
    fn directory_junction_is_rejected_without_touching_its_target() {
        use std::os::windows::process::CommandExt;
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        let junction = root.path().join("junction");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("sentinel"), b"do not touch").unwrap();
        let command = format!(
            "mklink /J \"{}\" \"{}\"",
            junction.display(),
            target.display()
        );
        let cmd =
            PathBuf::from(crate::updater::windows_install::windows_system_directory().unwrap())
                .join("cmd.exe");
        let result = std::process::Command::new(cmd)
            .args(["/D", "/C"])
            .raw_arg(&command)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let refused = Journal::open(&junction).is_err();
        std::fs::remove_dir(&junction).unwrap();
        assert!(refused);
        assert_eq!(
            std::fs::read(target.join("sentinel")).unwrap(),
            b"do not touch"
        );
        assert_eq!(std::fs::read_dir(target).unwrap().count(), 1);
    }

    #[test]
    fn missing_corrupt_truncated_and_wrong_scope_journals_are_not_empty() {
        let root = tempfile::tempdir().unwrap();
        for mode in ["missing", "corrupt", "truncated", "foreign"] {
            let path = root.path().join(mode);
            drop(Journal::open(&path).unwrap());
            let active = path.join("active.dpapi");
            match mode {
                "missing" => std::fs::remove_file(active).unwrap(),
                "corrupt" => {
                    let mut bytes = std::fs::read(&active).unwrap();
                    // Corrupt authenticated data, not DPAPI's provider header
                    // (Windows may ignore unused header bytes).
                    let last = bytes.len() - 1;
                    bytes[last] ^= 1;
                    std::fs::write(active, bytes).unwrap();
                }
                "truncated" => std::fs::write(active, b"broken").unwrap(),
                "foreign" => {
                    let other = root.path().join("other");
                    drop(Journal::open(&other).unwrap());
                    std::fs::copy(other.join("active.dpapi"), active).unwrap();
                }
                _ => unreachable!(),
            }
            assert!(Journal::open(&path).is_err(), "{mode}");
        }
    }

    #[test]
    fn publication_failure_stays_blocked_and_preserves_old_record() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery");
        let journal = Journal::open(&path).unwrap();
        let record = record();
        let lease = journal
            .prepare(record.clone(), b"signed package", b"inert test payload")
            .unwrap();
        let publication_fence = protected_read(&path.join("active.dpapi")).unwrap();
        let error = journal
            .transition(
                &record.candidate_id,
                Phase::Prepared,
                Phase::LaunchIntent,
                None,
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("journal publish/replace:"),
            "{error}"
        );
        assert!(error.to_string().contains("os error"), "{error}");
        assert!(journal.snapshot().is_err());
        drop(publication_fence);
        drop(lease);
        drop(journal);
        // Failure occurred before OS dispatch; the prior prepared state is safe.
        let recovered = Journal::open(&path).unwrap();
        assert_eq!(
            recovered.snapshot().unwrap().unwrap().phase,
            Phase::Prepared
        );
    }

    #[test]
    fn held_helper_snapshot_keeps_old_record_while_new_grant_is_published() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("journal 空格 $ 😀");
        let journal = Journal::open(&path).unwrap();
        let original = record();
        let _lease = journal
            .prepare(original.clone(), b"signed package", b"inert test payload")
            .unwrap();
        let old = snapshot_file(&path.join("active.dpapi")).unwrap();
        assert!(OpenOptions::new()
            .write(true)
            .open(path.join("active.dpapi"))
            .is_err());
        journal
            .transition(
                &original.candidate_id,
                Phase::Prepared,
                Phase::LaunchIntent,
                None,
            )
            .unwrap();
        // The old handle has zero links after atomic replacement, but only a
        // helper's immutable snapshot reader may accept this state.
        assert!(regular_file(&old).is_err());
        ordinary_file(&old, true).unwrap();
        let plain = protect(
            &read_bounded(old, MAX_RECORD).unwrap(),
            &journal.entropy(),
            false,
        )
        .unwrap();
        let old: Envelope = serde_json::from_slice(&plain).unwrap();
        assert_eq!(old.record, Some(original.clone()));
        let fresh = journal.witness_store().unwrap().record().unwrap();
        assert_eq!(fresh.candidate_id, original.candidate_id);
        assert_eq!(fresh.phase, Phase::LaunchIntent);
        assert_eq!(journal.snapshot().unwrap(), Some(fresh));
        assert!(journal
            .prepare(original, b"signed package", b"inert test payload")
            .is_err());
    }

    #[test]
    fn readonly_target_is_not_overridden_or_retried_by_publication() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("readonly-target");
        let journal = Journal::open(&path).unwrap();
        let original = record();
        let _lease = journal
            .prepare(original.clone(), b"signed package", b"inert test payload")
            .unwrap();
        let active = path.join("active.dpapi");
        let bytes = std::fs::read(&active).unwrap();
        let before = unsafe { GetFileAttributesW(wide(&active).as_ptr()) };
        assert_ne!(before, INVALID_FILE_ATTRIBUTES);
        assert_ne!(
            unsafe { SetFileAttributesW(wide(&active).as_ptr(), before | FILE_ATTRIBUTE_READONLY) },
            0
        );
        let outcome = journal.transition(
            &original.candidate_id,
            Phase::Prepared,
            Phase::LaunchIntent,
            None,
        );
        let unchanged = std::fs::read(&active).unwrap() == bytes;
        // Restore only this fixture-owned attribute before making assertions.
        assert_ne!(
            unsafe { SetFileAttributesW(wide(&active).as_ptr(), before) },
            0
        );
        let error = outcome.unwrap_err();
        assert!(
            error.to_string().contains("journal publish/replace:"),
            "{error}"
        );
        assert!(unchanged);
        assert!(journal.snapshot().is_err());
        assert!(journal
            .transition(
                &original.candidate_id,
                Phase::Prepared,
                Phase::LaunchIntent,
                None
            )
            .is_err());
        assert_eq!(
            std::fs::read_dir(&path)
                .unwrap()
                .filter(|entry| entry
                    .as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "writing"))
                .count(),
            1
        );
    }

    #[test]
    fn concurrent_helper_snapshots_do_not_interrupt_atomic_publication() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Barrier,
        };
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("concurrent-snapshots");
        let journal = Journal::open(&path).unwrap();
        let original = record();
        let _lease = journal
            .prepare(original.clone(), b"signed package", b"inert test payload")
            .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let barrier = Arc::new(Barrier::new(5));
        let readers: Vec<_> = (0..4)
            .map(|_| {
                let path = path.clone();
                let stop = stop.clone();
                let barrier = barrier.clone();
                let original = original.clone();
                std::thread::spawn(move || -> std::result::Result<usize, String> {
                    barrier.wait();
                    let store = witness::Store::open(&path).map_err(|e| e.to_string())?;
                    let mut reads = 0;
                    loop {
                        let snapshot = store.record().map_err(|e| e.to_string())?;
                        if snapshot != original {
                            return Err(
                                "Concurrent helper received different/incomplete snapshot".into()
                            );
                        }
                        reads += 1;
                        if stop.load(Ordering::SeqCst) {
                            return Ok(reads);
                        }
                    }
                })
            })
            .collect();
        barrier.wait();
        let mut failure = None;
        for iteration in 0..128 {
            // Same authoritative record, freshly encrypted on each publication.
            // No process or installer is involved in this owned-store fixture.
            if let Err(error) = journal.write(&Some(original.clone())) {
                failure = Some((iteration, error));
                break;
            }
        }
        stop.store(true, Ordering::SeqCst);
        let outcomes: Vec<_> = readers.into_iter().map(|reader| reader.join()).collect();
        assert!(
            failure.is_none(),
            "publication failure: {failure:?}; readers: {outcomes:?}"
        );
        for outcome in outcomes {
            assert!(outcome.unwrap().unwrap() > 0);
        }
        assert_eq!(journal.snapshot().unwrap(), Some(original));
    }

    #[test]
    fn noncanonical_identity_hardlinks_and_directory_substitution_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery");
        let journal = Journal::open(&path).unwrap();
        for id in [
            "../victim",
            "x:y",
            "",
            "00000000-0000-0000-0000-000000000000",
        ] {
            assert!(journal.path(id, Some(Kind::Nsis)).is_err());
        }
        assert!(std::fs::rename(&path, root.path().join("replacement")).is_err());
        let file = path.join("hardlinked");
        std::fs::write(&file, b"owned").unwrap();
        std::fs::hard_link(&file, path.join("alias")).unwrap();
        assert!(protected_read(&file).is_err());
        assert!(snapshot_file(&file).is_err());
        assert!(snapshot_file(&path).is_err());
    }

    #[test]
    fn malformed_authenticated_record_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery");
        let journal = Journal::open(&path).unwrap();
        let mut value = record();
        value.process = Some(ProcessIdentity { pid: 4, created: 5 });
        assert!(journal
            .prepare(value, b"signed package", b"inert test payload")
            .is_err());
        let value = Envelope {
            schema: 999,
            record: None,
        };
        let encrypted = protect(
            &serde_json::to_vec(&value).unwrap(),
            &journal.entropy(),
            true,
        )
        .unwrap();
        drop(journal);
        std::fs::write(path.join("active.dpapi"), encrypted).unwrap();
        assert!(Journal::open(&path).is_err());
    }
}
