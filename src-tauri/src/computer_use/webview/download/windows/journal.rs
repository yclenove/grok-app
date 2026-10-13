//! Bounded write-ahead ownership for one exact private native download buffer.
//! Records contain kernel/file identities, never URLs, credentials or payloads.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::File;
use std::io::Write;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

mod buffer;
mod owner;
mod retirement;
mod safety;

pub(super) use buffer::PrivateBuffer;
pub(super) use owner::Owners;

const MAX_RECORD: u64 = 16 * 1024;
const MAX_SLOTS: usize = 64;
const DIRECTORY: &str = "native-download-buffers";
const MARKER: &str = "owner.json";
static STORE_ADMISSION: Mutex<()> = Mutex::new(());

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: u32,
    id: String,
    root: safety::Identity,
    app_root: safety::Identity,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema: u32,
    transaction: String,
    store: Header,
    record: safety::Identity,
    temp_parent: Vec<u16>,
    temp_parent_identity: safety::Identity,
    buffer: safety::Identity,
    app: owner::Identity,
    browser: owner::Identity,
    native_instance: u64,
    expectation: u64,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Record {
    binding: Binding,
    marker: safety::Identity,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Receipt {
    record: Record,
    identity: Option<safety::Identity>,
    // Captured after original callbacks/caller retire, before any deletion.
    // A crash retry must not delete a later file at the same download pathname.
    data: Option<safety::Identity>,
}

struct Store {
    path: PathBuf,
    header: Header,
    owner: File,
    pins: Vec<File>,
    _lock: File,
    _admission: parking_lot::MutexGuard<'static, ()>,
}

impl Store {
    fn open(app_root: &Path, create: bool) -> Result<Self, String> {
        let admission = STORE_ADMISSION.lock();
        let mut pins = safety::parents(app_root, false)?;
        let app_identity =
            safety::identity(pins.last().ok_or("native journal App root missing")?, true)?;
        let parent = app_root.join("computer-use");
        pins.extend(safety::parents(&parent, create)?);
        let path = parent.join(DIRECTORY);
        let fresh = if create {
            match std::fs::create_dir(&path) {
                Ok(()) => true,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
                Err(error) => return Err(safety::error(error)),
            }
        } else {
            false
        };
        pins.extend(safety::parents(&path, false)?);
        let root = safety::identity(
            pins.last().ok_or("native journal store root missing")?,
            true,
        )?;
        let lock = safety::lock(&path.join("store.lock"), fresh)?;
        if fresh {
            let header = Header {
                schema: 1,
                id: uuid::Uuid::new_v4().simple().to_string(),
                root: root.clone(),
                app_root: app_identity.clone(),
            };
            drop(safety::write_new(&path.join(MARKER), &json(&header)?)?);
        }
        let owner = safety::file(&path.join(MARKER), false)?;
        let bytes = safety::read(&owner, MAX_RECORD)?;
        let header: Header = serde_json::from_slice(&bytes).map_err(safety::error)?;
        if header.schema != 1
            || !valid_id(&header.id)
            || header.root != root
            || header.app_root != app_identity
            || json(&header)? != bytes
        {
            return Err("native download journal store identity changed".into());
        }
        let store = Self {
            path,
            header,
            owner,
            pins,
            _lock: lock,
            _admission: admission,
        };
        store.slots()?;
        Ok(store)
    }

    fn slots(&self) -> Result<BTreeSet<String>, String> {
        let mut slots = BTreeSet::new();
        for entry in std::fs::read_dir(&self.path).map_err(safety::error)? {
            let entry = entry.map_err(safety::error)?;
            let name = entry.file_name();
            if name == MARKER || name == "store.lock" {
                continue;
            }
            let name = name
                .to_str()
                .ok_or("native download journal filename invalid")?;
            let id = name
                .strip_suffix(".retired.json")
                .or_else(|| name.strip_suffix(".json"))
                .ok_or("native download journal unknown file; retained")?;
            if !valid_id(id) {
                return Err("native download journal filename invalid; retained".into());
            }
            slots.insert(id.to_string());
            if slots.len() > MAX_SLOTS {
                return Err("native download journal capacity exceeded; retained".into());
            }
        }
        Ok(slots)
    }
}

fn valid_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && uuid::Uuid::parse_str(value).is_ok()
}

fn json(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(value).map_err(safety::error)?;
    if bytes.len() as u64 > MAX_RECORD {
        return Err("native download journal record too large".into());
    }
    Ok(bytes)
}

fn buffer_path(binding: &Binding) -> Result<PathBuf, String> {
    if binding.schema != 1
        || !valid_id(&binding.transaction)
        || binding.native_instance == 0
        || binding.expectation == 0
        || !binding.app.valid()
        || !binding.browser.valid()
        || binding.temp_parent.len() > 32767
        || binding.temp_parent.contains(&0)
    {
        return Err("native download journal record invalid".into());
    }
    let parent = PathBuf::from(std::ffi::OsString::from_wide(&binding.temp_parent));
    safety::path(&parent)?;
    Ok(parent.join(format!("grok-cu-download-{}", binding.transaction)))
}

pub(super) struct Lease {
    app_root: PathBuf,
    buffer_path: PathBuf,
    record_path: PathBuf,
    receipt_path: PathBuf,
    record: Record,
    state: Mutex<State>,
}

struct State {
    buffer: Option<File>,
    marker: Option<File>,
    record: Option<File>,
    retired: Option<File>,
    receipt: Option<Receipt>,
    data: Option<File>,
    prepared: bool,
    _owner: File,
    _pins: Vec<File>,
    complete: bool,
}

impl Lease {
    pub(super) fn publish(
        owners: &Owners,
        instance: u64,
        expectation: u64,
        mut buffer: PrivateBuffer,
    ) -> Result<Self, String> {
        if !owners.live() || instance == 0 || expectation == 0 {
            return Err("native download original owners unavailable before dispatch".into());
        }
        let store = Store::open(&owners.app_root, true)?;
        if store.slots()?.len() >= MAX_SLOTS {
            return Err(
                "native download journal capacity reached; explicit maintenance required".into(),
            );
        }
        let record_path = store.path.join(format!("{}.json", buffer.transaction));
        let receipt_path = store
            .path
            .join(format!("{}.retired.json", buffer.transaction));
        // Reserve a journal entry before creating the ownership marker. An
        // interrupted publication remains discoverable and fails closed.
        let mut record_pin = safety::write_new(&record_path, &[])?;
        buffer.retain = true;
        let parent_path = buffer
            .directory
            .parent()
            .ok_or("native buffer parent missing")?;
        let parent = buffer
            .pins
            .last()
            .ok_or("native buffer parent pin missing")?;
        let binding = Binding {
            schema: 1,
            transaction: buffer.transaction.clone(),
            store: store.header.clone(),
            record: safety::identity(&record_pin, false)?,
            temp_parent: parent_path.as_os_str().encode_wide().collect(),
            temp_parent_identity: safety::identity(parent, true)?,
            buffer: safety::identity(
                buffer
                    .directory_pin
                    .as_ref()
                    .ok_or("native buffer pin missing")?,
                true,
            )?,
            app: owners.app.identity.clone(),
            browser: owners.browser.identity.clone(),
            native_instance: instance,
            expectation,
        };
        if buffer_path(&binding)? != buffer.directory {
            return Err("native buffer transaction path changed".into());
        }
        let marker = safety::write_new(&buffer.directory.join(MARKER), &json(&binding)?)?;
        let record = Record {
            binding,
            marker: safety::identity(&marker, false)?,
        };
        let bytes = json(&record)?;
        record_pin.write_all(&bytes).map_err(safety::error)?;
        record_pin.sync_all().map_err(safety::error)?;
        if safety::read(&record_pin, MAX_RECORD)? != bytes || !owners.live() {
            return Err("native download original owner or record changed before dispatch".into());
        }
        let mut pins = std::mem::take(&mut buffer.pins);
        pins.extend(store.pins);
        Ok(Self {
            app_root: owners.app_root.clone(),
            buffer_path: buffer.directory.clone(),
            record_path,
            receipt_path,
            record,
            state: Mutex::new(State {
                buffer: buffer.directory_pin.take(),
                marker: Some(marker),
                record: Some(record_pin),
                retired: None,
                receipt: None,
                data: None,
                prepared: false,
                _owner: store.owner,
                _pins: pins,
                complete: false,
            }),
        })
    }

    /// Invoked only after the original caller/callback joint retirement.
    /// Drop alone leaves the durable record for explicit Host maintenance.
    pub(super) fn cleanup(&self) -> Result<(), String> {
        retirement::cleanup(self)
    }
}

pub(crate) fn maintain(app_root: &Path) -> Result<(), String> {
    let path = app_root.join("computer-use").join(DIRECTORY);
    if !path.try_exists().map_err(safety::error)? {
        return Ok(());
    }
    retirement::maintain(app_root)
}

#[cfg(test)]
mod tests;
