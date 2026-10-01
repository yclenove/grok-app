//! Publisher-authenticated installed-file expectations. A matching inventory is
//! evidence only: it neither certifies installer completion nor retires a nonce.
use super::super::windows_journal::{self as journal, Kind, Record};
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::AsRawHandle;

const MAX_DOCUMENT: usize = 256 * 1024;
const MAX_FILES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::updater) struct SignedInventory {
    // Sign the exact UTF-8 bytes, not a reserialized JSON value.
    pub document: String,
    pub signature: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inventory {
    schema: u32,
    domain: String,
    app_name: String,
    version: String,
    target: String,
    arch: String,
    kind: Kind,
    payload_sha256: String,
    executable: String,
    files: Vec<Entry>,
    #[serde(default)]
    completion: Option<super::completion::Contract>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Evidence {
    pub document_sha256: [u8; 32],
    pub files: usize,
    pub bytes: u64,
}

fn invalid(message: impl ToString) -> Error {
    Error::WindowsInstallPreflight(format!("Install inventory: {}", message.to_string()))
}

fn hex(digest: &[u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn relative_path(value: &str) -> Result<()> {
    if value.is_empty() || value.encode_utf16().count() > 1024 {
        return Err(invalid("empty or oversized relative path"));
    }
    for part in value.split('/') {
        let stem = part.split('.').next().unwrap_or_default().to_uppercase();
        let device = ["CON", "PRN", "AUX", "NUL", "CLOCK$", "CONIN$", "CONOUT$"]
            .contains(&stem.as_str())
            || ["COM", "LPT"].iter().any(|prefix| {
                stem.strip_prefix(prefix).is_some_and(|n| {
                    matches!(
                        n,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                })
            });
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || part.encode_utf16().count() > 255
            || device
            || part.chars().any(|c| c < ' ' || "\\:*?\"<>|~".contains(c))
        {
            return Err(invalid("unsafe or aliased Windows relative path"));
        }
    }
    Ok(())
}

impl SignedInventory {
    pub fn validate_size(&self) -> Result<()> {
        if self.document.is_empty()
            || self.document.len() > MAX_DOCUMENT
            || self.signature.is_empty()
            || self.signature.len() > 8192
        {
            return Err(invalid("missing or oversized signed envelope"));
        }
        Ok(())
    }

    fn authenticate(&self, key: &str, record: &Record) -> Result<Inventory> {
        self.validate_size()?;
        if key.is_empty() || journal::digest(key.as_bytes()) != record.binding.key_digest {
            return Err(invalid(
                "compiled publisher key does not match original candidate",
            ));
        }
        verify_signature(self.document.as_bytes(), &self.signature, key).map_err(invalid)?;
        let raw: serde_json::Value = serde_json::from_str(&self.document).map_err(invalid)?;
        if raw
            .get("completion")
            .and_then(|c| c.get("failure_protocol"))
            .is_some_and(|p| !p.is_string())
        {
            return Err(invalid(
                "explicit failure protocol must be a supported string",
            ));
        }
        if raw.get("completion").is_some_and(|v| !v.is_object()) {
            return Err(invalid(
                "explicit completion contract cannot be null or non-object",
            ));
        }
        let value: Inventory = serde_json::from_str(&self.document).map_err(invalid)?;
        if let Some(contract) = &value.completion {
            contract.validate(record.kind)?;
        }
        if value.schema != 1
            || value.domain != "grok-windows-install-inventory-v1"
            || value.app_name != record.binding.app_name
            || value.version != record.version
            || value.target != record.target
            || value.arch != std::env::consts::ARCH
            || value.kind != record.kind
            || value.payload_sha256 != hex(&record.payload_digest)
            || value.files.is_empty()
            || value.files.len() > MAX_FILES
        {
            return Err(invalid(
                "signed release does not bind the exact candidate/App/target",
            ));
        }
        relative_path(&value.executable)?;
        let executable = PathBuf::from(OsString::from_wide(&record.binding.executable));
        if value.executable.contains('/')
            || executable.file_name().and_then(|s| s.to_str()) != Some(&value.executable)
        {
            return Err(invalid(
                "executable is not the original installed App filename",
            ));
        }
        let mut names = BTreeSet::new();
        let mut total = 0u64;
        for entry in &value.files {
            relative_path(&entry.path)?;
            if !valid_hash(&entry.sha256)
                || entry.size > 16 * 1024 * 1024 * 1024
                || !names.insert(entry.path.to_uppercase())
            {
                return Err(invalid("invalid digest, size or duplicate file identity"));
            }
            total = total
                .checked_add(entry.size)
                .ok_or_else(|| invalid("file size overflow"))?;
            if total > 64 * 1024 * 1024 * 1024 {
                return Err(invalid("inventory exceeds installed size limit"));
            }
        }
        for entry in &value.files {
            let parts: Vec<_> = entry.path.split('/').collect();
            for end in 1..parts.len() {
                if names.contains(&parts[..end].join("/").to_uppercase()) {
                    return Err(invalid("file is also used as an ancestor directory"));
                }
            }
        }
        if !value
            .files
            .iter()
            .any(|f| f.path == value.executable && f.size != 0)
        {
            return Err(invalid("main executable is missing or empty"));
        }
        Ok(value)
    }

    pub fn verify(&self, key: &str, record: &Record) -> Result<()> {
        self.authenticate(key, record).map(|_| ())
    }

    pub(super) fn completion(
        &self,
        key: &str,
        record: &Record,
    ) -> Result<Option<super::completion::Contract>> {
        Ok(self.authenticate(key, record)?.completion)
    }

    #[cfg(test)]
    fn inspect(&self, key: &str, record: &Record, root: &Path) -> Result<Evidence> {
        self.with_inspected(key, record, root, Ok)
    }

    fn with_inspected<T>(
        &self,
        key: &str,
        record: &Record,
        root: &Path,
        accept: impl FnOnce(Evidence) -> Result<T>,
    ) -> Result<T> {
        let value = self.authenticate(key, record)?;
        // Open and retain every directory/file before concluding. No directories
        // are created; junctions, symlinks, hardlinks and sharing races fail.
        let mut directories = journal::pin_existing_directories(root).map_err(invalid)?;
        let mut seen_directories = BTreeSet::new();
        let mut leases = Vec::with_capacity(value.files.len());
        let mut identities = BTreeSet::new();
        let mut bytes = 0;
        for entry in &value.files {
            let path = root.join(&entry.path);
            let parent = path
                .parent()
                .ok_or_else(|| invalid("file parent unavailable"))?;
            if seen_directories.insert(parent.to_path_buf()) {
                directories.extend(journal::pin_existing_directories(parent).map_err(invalid)?);
            }
            let mut file = journal::protected_read(&path).map_err(invalid)?;
            let mut info: windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION =
                unsafe { std::mem::zeroed() };
            if unsafe {
                windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle(
                    file.as_raw_handle(),
                    &mut info,
                )
            } == 0
            {
                return Err(invalid(std::io::Error::last_os_error()));
            }
            if !identities.insert((
                info.dwVolumeSerialNumber,
                info.nFileIndexHigh,
                info.nFileIndexLow,
            )) {
                return Err(invalid("multiple inventory paths resolve to one file"));
            }
            if file.metadata().map_err(invalid)?.len() != entry.size {
                return Err(invalid("installed file length mismatch"));
            }
            let mut hash = Sha256::new();
            let count = std::io::copy(&mut file, &mut hash).map_err(invalid)?;
            if count != entry.size || hex(&hash.finalize().into()) != entry.sha256 {
                return Err(invalid("installed file contents mismatch"));
            }
            bytes += count;
            leases.push(file);
        }
        let evidence = Evidence {
            document_sha256: journal::digest(self.document.as_bytes()),
            files: leases.len(),
            bytes,
        };
        // Keep the whole tree pinned through the caller's terminal publication.
        // Existing observation callers still receive point-in-time facts only.
        let result = accept(evidence);
        drop(leases);
        drop(directories);
        result
    }
}

pub(super) fn from_update(update: &Update, record: &Record) -> Result<Option<SignedInventory>> {
    let root = &update.raw_json;
    let value = if let Some(platforms) = root.get("platforms") {
        if root.get("windows_install_inventory").is_some() {
            return Err(invalid("ambiguous root and per-platform inventory"));
        }
        let platforms = platforms
            .as_object()
            .ok_or_else(|| invalid("invalid platform metadata"))?;
        let mut selected = platforms.values().filter(|p| {
            p.get("url")
                .and_then(|x| x.as_str())
                .and_then(|s| Url::parse(s).ok())
                .as_ref()
                == Some(&update.download_url)
                && p.get("signature").and_then(|x| x.as_str()) == Some(&update.signature)
        });
        let first = selected
            .next()
            .ok_or_else(|| invalid("checked candidate is absent from platform metadata"))?;
        let expected = first.get("windows_install_inventory");
        if selected.any(|p| p.get("windows_install_inventory") != expected) {
            return Err(invalid(
                "conflicting inventory for duplicate candidate aliases",
            ));
        }
        expected
    } else {
        root.get("windows_install_inventory")
    };
    let Some(value) = value else {
        return Ok(None);
    };
    let signed: SignedInventory = serde_json::from_value(value.clone()).map_err(invalid)?;
    signed.verify(&update.config.pubkey, record)?;
    Ok(Some(signed))
}

pub(super) fn observe_installed(update: &Update, record: &Record) -> Result<Option<Evidence>> {
    with_observed_installed(update, record, Ok)
}

pub(super) fn with_observed_installed<T>(
    update: &Update,
    record: &Record,
    accept: impl FnOnce(Evidence) -> Result<T>,
) -> Result<Option<T>> {
    let Some(signed) = &record.inventory else {
        return Ok(None);
    };
    if record.phase != Phase::Accepted
        || record.process_exit.is_none()
        || update.current_version != record.version
        || update.app_name != record.binding.app_name
        || update.target != record.target
    {
        return Err(invalid(
            "not running the candidate version after accepted process exit",
        ));
    }
    let current = std::env::current_exe().map_err(invalid)?;
    let original = PathBuf::from(OsString::from_wide(&record.binding.executable));
    if current != original {
        return Err(invalid(
            "running App is not at the original installation path",
        ));
    }
    signed
        .with_inspected(
            &update.config.pubkey,
            record,
            current
                .parent()
                .ok_or_else(|| invalid("installed App parent missing"))?,
            accept,
        )
        .map(Some)
}

#[cfg(test)]
mod tests;
