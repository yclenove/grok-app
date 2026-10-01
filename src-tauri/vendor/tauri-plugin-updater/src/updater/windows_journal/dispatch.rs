//! One-shot, exact-process delegation. Receipt files only add facts; neither
//! a helper exit nor a timeout grants a new launch or certifies installation.
use super::{
    witness::{Store, Ticket},
    *,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::updater) enum Event {
    Ready,
    Accepted { process: Option<ProcessIdentity> },
    Rejected { message: String },
    Exited(ProcessExit),
}

impl Event {
    fn suffix(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Exited(_) => "exit",
            _ => "outcome",
        }
    }
    fn validate(&self) -> Result<()> {
        let invalid = match self {
            Self::Accepted { process: Some(p) } => p.pid == 0 || p.created == 0,
            Self::Exited(e) => {
                e.identity.pid == 0
                    || e.identity.created == 0
                    || e.exited < e.identity.created
                    || e.exited == 0
            }
            Self::Rejected { message } => message.is_empty() || message.len() > 8192,
            _ => false,
        };
        if invalid {
            Err(pending("Malformed dispatch receipt"))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: u32,
    candidate: String,
    fingerprint: [u8; 32],
    ticket: Ticket,
    event: Event,
}

pub(in crate::updater) fn fingerprint(record: &Record) -> Result<[u8; 32]> {
    validate(record)?;
    let mut immutable = record.clone();
    immutable.phase = Phase::Prepared;
    immutable.process = None;
    immutable.process_exit = None;
    immutable.witness = None;
    immutable.dispatcher = None;
    Ok(digest(&serde_json::to_vec(&immutable).map_err(pending)?))
}

impl Store {
    pub fn artifact_path(&self, id: &str, kind: Option<Kind>) -> Result<PathBuf> {
        validate_id(id)?;
        let suffix = match kind {
            None => "payload",
            Some(Kind::Nsis) => "exe",
            Some(Kind::Msi) => "msi",
        };
        Ok(self.root.join(format!("{id}.{suffix}")))
    }
    pub fn dispatcher_executable(&self, candidate: &str, ticket: &str) -> Result<PathBuf> {
        validate_id(candidate)?;
        validate_id(ticket)?;
        Ok(self
            .root
            .join(format!("{candidate}.{ticket}.dispatcher.exe")))
    }
    pub fn copy_dispatcher(&self, record: &Record, ticket: &str) -> Result<(PathBuf, File)> {
        self.copy_image(
            record,
            self.dispatcher_executable(&record.candidate_id, ticket)?,
        )
    }
    fn dispatch_path(&self, record: &Record, ticket: &Ticket, suffix: &str) -> Result<PathBuf> {
        validate(record)?;
        validate_id(&ticket.id)?;
        if ticket.helper.pid == 0
            || ticket.helper.created == 0
            || !matches!(suffix, "ready" | "outcome" | "exit")
        {
            return Err(pending("Invalid dispatch receipt identity"));
        }
        Ok(self.root.join(format!(
            "{}.{}.dispatch-{suffix}.dpapi",
            record.candidate_id, ticket.id
        )))
    }
    fn dispatch_entropy(&self, record: &Record, ticket: &Ticket) -> Vec<u8> {
        let mut bytes: Vec<_> = self
            .root
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect();
        bytes.extend_from_slice(b"\0grok-update-dispatch-v1\0");
        bytes.extend_from_slice(record.candidate_id.as_bytes());
        bytes.extend_from_slice(ticket.id.as_bytes());
        bytes
    }
    pub fn dispatch_event(
        &self,
        record: &Record,
        ticket: &Ticket,
        suffix: &str,
    ) -> Result<Option<Event>> {
        let path = self.dispatch_path(record, ticket, suffix)?;
        if !path.try_exists().map_err(pending)? {
            return Ok(None);
        }
        let encrypted = read_limited(&path, MAX_RECORD)?;
        let plain =
            protect(&encrypted, &self.dispatch_entropy(record, ticket), false).map_err(pending)?;
        let receipt: Receipt = serde_json::from_slice(&plain).map_err(pending)?;
        receipt.event.validate()?;
        if receipt.schema != 1
            || receipt.candidate != record.candidate_id
            || receipt.fingerprint != fingerprint(record)?
            || &receipt.ticket != ticket
            || receipt.event.suffix() != suffix
        {
            return Err(pending("Dispatch receipt binding mismatch"));
        }
        Ok(Some(receipt.event))
    }
    pub fn publish_dispatch(&self, record: &Record, ticket: &Ticket, event: Event) -> Result<()> {
        event.validate()?;
        let suffix = event.suffix();
        let target = self.dispatch_path(record, ticket, suffix)?;
        if let Some(previous) = self.dispatch_event(record, ticket, suffix)? {
            return if previous == event {
                Ok(())
            } else {
                Err(pending("Conflicting dispatch receipt"))
            };
        }
        let receipt = Receipt {
            schema: 1,
            candidate: record.candidate_id.clone(),
            fingerprint: fingerprint(record)?,
            ticket: ticket.clone(),
            event: event.clone(),
        };
        let encrypted = protect(
            &serde_json::to_vec(&receipt).map_err(pending)?,
            &self.dispatch_entropy(record, ticket),
            true,
        )
        .map_err(pending)?;
        if encrypted.len() as u64 > MAX_RECORD {
            return Err(pending("Oversized dispatch receipt"));
        }
        let temp = self
            .root
            .join(format!("{}.dispatch-writing", uuid::Uuid::new_v4()));
        write_new(&temp, &encrypted).map_err(pending)?;
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
        if self.dispatch_event(record, ticket, suffix)? != Some(event) {
            return Err(pending("Dispatch publication readback failed"));
        }
        Ok(())
    }
}

impl Journal {
    pub fn delegate_dispatch(&self, id: &str, ticket: Ticket) -> Result<Record> {
        let mut slot = self.lock()?;
        let original = slot
            .as_ref()
            .filter(|r| {
                r.candidate_id == id && r.phase == Phase::Prepared && r.dispatcher.is_none()
            })
            .ok_or_else(|| pending("Original candidate is not delegable"))?;
        if self
            .witness_store()?
            .dispatch_event(original, &ticket, "ready")?
            != Some(Event::Ready)
        {
            return Err(pending(
                "Dispatcher preflight has not acknowledged this exact candidate",
            ));
        }
        let store = self.witness_store()?;
        if store
            .dispatch_event(original, &ticket, "outcome")?
            .is_some()
            || store.dispatch_event(original, &ticket, "exit")?.is_some()
        {
            return Err(pending(
                "A dispatcher with an outcome can never be delegated again",
            ));
        }
        let mut record = original.clone();
        record.phase = Phase::LaunchIntent;
        record.dispatcher = Some(ticket);
        validate(&record)?;
        self.write(&Some(record.clone()))?;
        *slot = Some(record.clone());
        Ok(record)
    }

    /// Import immutable exact-worker facts under the exclusive App journal lock.
    /// Only an explicit pre-accept OS refusal can restore the original candidate.
    pub fn reconcile_dispatch(&self) -> Result<Option<Record>> {
        let mut slot = self.lock()?;
        let Some(original) = slot.as_ref() else {
            return Ok(None);
        };
        let Some(ticket) = original.dispatcher.as_ref() else {
            return Ok(slot.clone());
        };
        let store = self.witness_store()?;
        let outcome = store.dispatch_event(original, ticket, "outcome")?;
        let exit = match store.dispatch_event(original, ticket, "exit")? {
            Some(Event::Exited(exit)) => Some(exit),
            None => None,
            _ => return Err(pending("Invalid dispatch exit event")),
        };
        let mut record = original.clone();
        match outcome {
            Some(Event::Rejected { .. }) => {
                if original.phase != Phase::LaunchIntent || exit.is_some() {
                    return Err(pending(
                        "Dispatch refusal contradicts acceptance/exit evidence",
                    ));
                }
                record.phase = Phase::Prepared;
                record.dispatcher = None;
            }
            Some(Event::Accepted { process }) => {
                if original.phase == Phase::Accepted && original.process != process {
                    return Err(pending("Conflicting dispatcher process identity"));
                }
                if exit
                    .as_ref()
                    .is_some_and(|e| process.as_ref() != Some(&e.identity))
                {
                    return Err(pending("Dispatch exit does not match acceptance"));
                }
                record.phase = Phase::Accepted;
                record.process = process;
            }
            None => {
                // Even if acceptance publication failed, a matched signaled
                // original handle proves acceptance, never installation success.
                if let Some(exit) = &exit {
                    if original.phase == Phase::Accepted
                        && original.process.as_ref() != Some(&exit.identity)
                    {
                        return Err(pending("Dispatch exit contradicts recorded process"));
                    }
                    record.phase = Phase::Accepted;
                    record.process = Some(exit.identity.clone());
                }
            }
            _ => return Err(pending("Invalid dispatch outcome event")),
        }
        if let Some(exit) = exit {
            if original
                .process_exit
                .as_ref()
                .is_some_and(|old| old != &exit)
            {
                return Err(pending("Conflicting dispatcher exit evidence"));
            }
            record.process_exit = Some(exit);
        }
        if record != *original {
            validate(&record)?;
            self.write(&Some(record.clone()))?;
            *slot = Some(record);
        }
        Ok(slot.clone())
    }
}
