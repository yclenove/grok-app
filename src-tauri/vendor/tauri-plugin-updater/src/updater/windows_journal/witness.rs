//! Append-only, user-DPAPI-protected observations from a separate process. The
//! helper never takes the App's journal owner lock and cannot retire a candidate.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::updater) struct Ticket {
    pub id: String,
    pub helper: ProcessIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: u32,
    candidate: String,
    ticket: Ticket,
    process: ProcessIdentity,
    exit: Option<ProcessExit>,
}

pub(in crate::updater) struct Store {
    pub(super) root: PathBuf,
    _directories: Vec<File>,
}

impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        Ok(Self {
            root: root.to_owned(),
            _directories: pin_directories(root).map_err(pending)?,
        })
    }

    pub fn executable(&self, candidate: &str, ticket: &str) -> Result<PathBuf> {
        validate_id(candidate)?;
        validate_id(ticket)?;
        Ok(self.root.join(format!("{candidate}.{ticket}.witness.exe")))
    }

    #[cfg(test)]
    pub fn copy_executable(&self, record: &Record, ticket: &str) -> Result<(PathBuf, File)> {
        self.copy_image(record, self.executable(&record.candidate_id, ticket)?)
    }

    pub(super) fn copy_image(&self, record: &Record, target: PathBuf) -> Result<(PathBuf, File)> {
        let current = std::env::current_exe().map_err(pending)?;
        if current.as_os_str().encode_wide().collect::<Vec<_>>() != record.binding.executable {
            return Err(pending("Witness source is not the bound App executable"));
        }
        let mut source = protected_read(&current).map_err(pending)?;
        let mut bytes = Vec::new();
        source.read_to_end(&mut bytes).map_err(pending)?;
        if digest(&bytes) != record.binding.executable_digest {
            return Err(pending("Witness source bytes differ from the bound App"));
        }
        write_new(&target, &bytes).map_err(pending)?;
        let mut lease = protected_read(&target).map_err(pending)?;
        let mut readback = Vec::new();
        lease.read_to_end(&mut readback).map_err(pending)?;
        if readback != bytes {
            return Err(pending("Witness executable copy changed"));
        }
        Ok((target, lease))
    }

    /// Read-only bootstrap, intentionally without the exclusive App owner lock.
    pub fn record(&self) -> Result<Record> {
        let encrypted = read_snapshot(&self.root.join("active.dpapi"))?;
        let plain = protect(&encrypted, &self.entropy(None), false).map_err(pending)?;
        let envelope: Envelope = serde_json::from_slice(&plain).map_err(pending)?;
        if envelope.schema != 1 {
            return Err(pending("Unsupported witness journal schema"));
        }
        let record = envelope
            .record
            .ok_or_else(|| pending("Witness has no candidate"))?;
        validate(&record)?;
        Ok(record)
    }

    fn entropy(&self, record: Option<&Record>) -> Vec<u8> {
        let mut bytes: Vec<u8> = self
            .root
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect();
        if let Some(record) = record {
            bytes.extend_from_slice(b"\0grok-update-process-witness-v1\0");
            bytes.extend_from_slice(record.candidate_id.as_bytes());
            if let Some(ticket) = &record.witness {
                bytes.extend_from_slice(ticket.id.as_bytes());
            }
        }
        bytes
    }

    fn receipt(&self, record: &Record, exit: Option<ProcessExit>) -> Result<Receipt> {
        validate(record)?;
        let process = record
            .process
            .clone()
            .ok_or_else(|| pending("Witness has no accepted process"))?;
        if exit
            .as_ref()
            .is_some_and(|x| x.identity != process || x.exited == 0 || x.exited < process.created)
        {
            return Err(pending("Witness exit identity/time mismatch"));
        }
        Ok(Receipt {
            schema: 1,
            candidate: record.candidate_id.clone(),
            ticket: record
                .witness
                .clone()
                .ok_or_else(|| pending("Witness ticket missing"))?,
            process,
            exit,
        })
    }

    fn path(&self, record: &Record, exited: bool) -> Result<PathBuf> {
        let receipt = self.receipt(record, None)?;
        Ok(self.root.join(format!(
            "{}.{}.{}.dpapi",
            receipt.candidate,
            receipt.ticket.id,
            if exited { "exit" } else { "ready" }
        )))
    }

    pub fn publish(&self, record: &Record, exit: Option<ProcessExit>) -> Result<()> {
        let receipt = self.receipt(record, exit)?;
        let target = self.path(record, receipt.exit.is_some())?;
        if let Some(previous) = self.read(record, receipt.exit.is_some())? {
            return if previous == receipt {
                Ok(())
            } else {
                Err(pending("Conflicting witness receipt"))
            };
        }
        let plain = serde_json::to_vec(&receipt).map_err(pending)?;
        let encrypted = protect(&plain, &self.entropy(Some(record)), true).map_err(pending)?;
        let temp = self
            .root
            .join(format!("{}.witness-writing", uuid::Uuid::new_v4()));
        write_new(&temp, &encrypted).map_err(pending)?;
        // No replace: published evidence is immutable, including across helpers.
        if unsafe {
            MoveFileExW(
                wide(&temp).as_ptr(),
                wide(&target).as_ptr(),
                MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(pending(std::io::Error::last_os_error()));
        }
        if self.read(record, receipt.exit.is_some())? != Some(receipt) {
            return Err(pending("Independent witness publication readback failed"));
        }
        Ok(())
    }

    fn read(&self, record: &Record, exited: bool) -> Result<Option<Receipt>> {
        let path = self.path(record, exited)?;
        if !path.try_exists().map_err(pending)? {
            return Ok(None);
        }
        let bytes = read_limited(&path, MAX_RECORD)?;
        let plain = protect(&bytes, &self.entropy(Some(record)), false).map_err(pending)?;
        let receipt: Receipt = serde_json::from_slice(&plain).map_err(pending)?;
        if receipt != self.receipt(record, receipt.exit.clone())?
            || receipt.exit.is_some() != exited
        {
            return Err(pending("Independent witness receipt binding mismatch"));
        }
        Ok(Some(receipt))
    }

    #[cfg(test)]
    pub fn ready(&self, record: &Record) -> Result<bool> {
        Ok(self.read(record, false)?.is_some())
    }
    pub fn exit(&self, record: &Record) -> Result<Option<ProcessExit>> {
        Ok(self.read(record, true)?.and_then(|x| x.exit))
    }
}

impl Journal {
    pub fn witness_store(&self) -> Result<Store> {
        Store::open(&self.root)
    }

    #[cfg(test)]
    pub fn authorize_witness(&self, id: &str, ticket: Ticket) -> Result<Record> {
        let mut slot = self.lock()?;
        let original = slot
            .as_ref()
            .filter(|r| {
                r.candidate_id == id
                    && r.phase == Phase::Accepted
                    && r.witness.is_none()
                    && r.dispatcher.is_none()
            })
            .ok_or_else(|| pending("Independent witness cannot replace an existing owner"))?;
        let mut record = original.clone();
        record.witness = Some(ticket);
        validate(&record)?;
        self.write(&Some(record.clone()))?;
        *slot = Some(record.clone());
        Ok(record)
    }
}
