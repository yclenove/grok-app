//! A successful exit or a matching file tree alone is not completion. Only a
//! publisher-declared NSIS callback, bound to this nonce and exact OS process,
//! may combine with locked installed files to retire the original transaction.
use super::super::windows_journal::{self as journal, Kind, Record};
use super::*;
use serde::{Deserialize, Serialize};
use std::os::windows::ffi::OsStringExt;

pub(super) const PROTOCOL: &str = "grok-nsis-install-complete-v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Contract {
    pub protocol: String,
    pub bundle_id: String,
    pub install_scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_protocol: Option<String>,
}

impl Contract {
    pub fn validate(&self, kind: Kind) -> Result<()> {
        if kind != Kind::Nsis
            || self.protocol != PROTOCOL
            || self.install_scope != "currentUser"
            || self
                .failure_protocol
                .as_deref()
                .is_some_and(|p| p != super::failure::PROTOCOL)
            || self.bundle_id.is_empty()
            || self.bundle_id.len() > 200
            || !self.bundle_id.as_bytes()[0].is_ascii_alphanumeric()
            || !self
                .bundle_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(journal::pending(
                "Unsupported signed installer completion contract",
            ));
        }
        Ok(())
    }
}

pub(super) fn receipt_path(root: &Path, record: &Record) -> Result<PathBuf> {
    let id = uuid::Uuid::parse_str(&record.candidate_id).map_err(journal::pending)?;
    if id.get_version_num() != 4 || id.hyphenated().to_string() != record.candidate_id {
        return Err(journal::pending("Invalid completion nonce"));
    }
    Ok(root.join(format!("{}.nsis-complete", record.candidate_id)))
}

pub(super) fn bind_plan(
    plan: launch_plan::LaunchPlan,
    root: &Path,
    record: &Record,
    key: &str,
) -> Result<launch_plan::LaunchPlan> {
    let Some(signed) = &record.inventory else {
        return Ok(plan);
    };
    let Some(contract) = signed.completion(key, record)? else {
        return Ok(plan);
    };
    let receipt = receipt_path(root, record)?;
    match std::fs::symlink_metadata(&receipt) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(journal::pending(error)),
        Ok(_) => {
            return Err(journal::pending(
                "Completion receipt path already exists before dispatch",
            ))
        }
    }
    let plan = plan
        .with_completion_receipt(&record.candidate_id, &receipt)
        .map_err(journal::pending)?;
    super::failure::bind_plan(plan, root, record, &contract)
}

pub(super) fn verify_receipt(bytes: &[u8], record: &Record, contract: &Contract) -> Result<()> {
    verify_callback(bytes, record, contract, PROTOCOL, &["complete"], true).map(|_| ())
}

pub(super) fn verify_callback(
    bytes: &[u8],
    record: &Record,
    contract: &Contract,
    protocol: &str,
    outcomes: &[&str],
    success: bool,
) -> Result<String> {
    if bytes.len() > 32 * 1024
        || bytes.len() < 2
        || bytes.len() % 2 != 0
        || bytes[..2] != [0xff, 0xfe]
    {
        return Err(journal::pending(
            "Invalid completion receipt encoding/length",
        ));
    }
    let units: Vec<_> = bytes[2..]
        .chunks_exact(2)
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
        .collect();
    let text = String::from_utf16(&units).map_err(journal::pending)?;
    let body = text
        .strip_suffix("\r\n")
        .ok_or_else(|| journal::pending("Incomplete completion receipt"))?;
    let fields: Vec<_> = body.split("\r\n").collect();
    let original = PathBuf::from(OsString::from_wide(&record.binding.executable));
    let exit = record
        .process_exit
        .as_ref()
        .ok_or_else(|| journal::pending("No exact installer exit"))?;
    if fields.len() != 10
        || fields
            .iter()
            .any(|s| s.is_empty() || s.chars().any(|c| c < ' '))
        || record.phase != Phase::Accepted
        || record.process.as_ref() != Some(&exit.identity)
        || (exit.code == 0) != success
        || exit.exited <= exit.identity.created
        || fields[0] != protocol
        || fields[1] != record.candidate_id
        || fields[2] != record.version
        || fields[3] != record.binding.app_name
        || Some(OsStr::new(fields[4])) != original.file_name()
        || fields[5] != contract.bundle_id
        || Some(Path::new(fields[6])) != original.parent()
        || fields[7] != exit.identity.pid.to_string()
        || fields[8] != exit.identity.created.to_string()
        || !outcomes.contains(&fields[9])
    {
        return Err(journal::pending(
            "NSIS callback does not match the accepted candidate/process/layout",
        ));
    }
    Ok(fields[9].to_owned())
}

// Construction stays in this module, after native receipt + signed live-file
// checks. The journal cannot turn a bool/IPC parameter into this capability.
#[derive(Serialize)]
pub(in crate::updater) struct VerifiedCompletion {
    schema: u32,
    protocol: &'static str,
    original: Record,
    receipt_sha256: [u8; 32],
    inventory_sha256: [u8; 32],
    files: usize,
    bytes: u64,
}
impl VerifiedCompletion {
    pub(in crate::updater) fn original(&self) -> &Record {
        &self.original
    }
}

// Read-only historical DTO, deliberately NOT the capability accepted by
// retire_completed. Deserializing an archive cannot grant journal mutation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchivedCompletion {
    schema: u32,
    protocol: String,
    original: Record,
    receipt_sha256: [u8; 32],
    inventory_sha256: [u8; 32],
    files: usize,
    bytes: u64,
}

pub(super) fn historical(
    update: &Update,
    store: &Journal,
    id: &str,
) -> Result<Option<PendingInstall>> {
    let Some(document) = store.completed_document(id)? else {
        return Ok(None);
    };
    let archive: ArchivedCompletion =
        serde_json::from_slice(&document).map_err(journal::pending)?;
    let original = &archive.original;
    journal::validate(original)?;
    let executable = PathBuf::from(OsString::from_wide(&original.binding.executable));
    if archive.schema != 1
        || archive.protocol != PROTOCOL
        || original.candidate_id != id
        || archive.files == 0
        || archive.bytes == 0
        || original.binding.app_name != update.app_name
        || original.target != update.target
        || executable != std::env::current_exe().map_err(journal::pending)?
    {
        return Err(journal::pending(
            "Completed archive does not belong to this App/candidate",
        ));
    }
    let inventory = original
        .inventory
        .as_ref()
        .ok_or_else(|| journal::pending("Completed inventory missing"))?;
    let contract = inventory
        .completion(&update.config.pubkey, original)?
        .ok_or_else(|| journal::pending("Completed contract missing"))?;
    if journal::digest(inventory.document.as_bytes()) != archive.inventory_sha256 {
        return Err(journal::pending("Completed inventory digest mismatch"));
    }
    let mut receipt =
        journal::protected_read(&receipt_path(store.completion_directory(), original)?)
            .map_err(journal::pending)?;
    if receipt.metadata().map_err(journal::pending)?.len() > 32 * 1024 {
        return Err(journal::pending("Oversized completed callback"));
    }
    let mut bytes = Vec::new();
    receipt.read_to_end(&mut bytes).map_err(journal::pending)?;
    if journal::digest(&bytes) != archive.receipt_sha256 {
        return Err(journal::pending("Completed callback digest mismatch"));
    }
    verify_receipt(&bytes, original, &contract)?;
    // Historical completion, not a claim about a later install's live tree.
    Ok(Some(PendingInstall {
        candidate_id: id.to_owned(),
        version: original.version.clone(),
        state: crate::install_recovery::RecoveryState::Completed,
        phase: "completed",
        message: None,
    }))
}

pub(super) fn reconcile<T>(
    update: &Update,
    record: &Record,
    root: &Path,
    finish: impl FnOnce(&VerifiedCompletion) -> Result<T>,
) -> Result<Option<T>> {
    let Some(signed) = &record.inventory else {
        return Ok(None);
    };
    let Some(contract) = signed.completion(&update.config.pubkey, record)? else {
        return Ok(None);
    };
    contract.validate(record.kind)?;
    super::failure::ensure_absent(root, record, &contract)?;
    let _directories = journal::pin_existing_directories(root).map_err(journal::pending)?;
    let mut receipt =
        journal::protected_read(&receipt_path(root, record)?).map_err(journal::pending)?;
    if receipt.metadata().map_err(journal::pending)?.len() > 32 * 1024 {
        return Err(journal::pending("Oversized completion receipt"));
    }
    let mut bytes = Vec::new();
    receipt.read_to_end(&mut bytes).map_err(journal::pending)?;
    verify_receipt(&bytes, record, &contract)?;
    // The receipt and all installed files remain read-leased until the durable
    // terminal record and active-slot CAS have finished, not just until hashing.
    inventory::with_observed_installed(update, record, |evidence| {
        finish(&VerifiedCompletion {
            schema: 1,
            protocol: PROTOCOL,
            original: record.clone(),
            receipt_sha256: journal::digest(&bytes),
            inventory_sha256: evidence.document_sha256,
            files: evidence.files,
            bytes: evidence.bytes,
        })
    })
}
