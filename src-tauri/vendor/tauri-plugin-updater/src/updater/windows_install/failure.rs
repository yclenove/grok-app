//! A signed native failure callback explains an exact installer's nonzero exit.
//! It is deliberately NOT a completion/rollback capability and cannot retire,
//! resume, replace, or clear the original durable candidate.
use super::super::windows_journal::{self as journal, Record};
use super::*;
use std::os::windows::ffi::OsStringExt;

pub(super) const PROTOCOL: &str = "grok-nsis-install-failed-v1";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Failed,
    Cancelled,
}

impl Outcome {
    pub fn phase(&self) -> &'static str {
        match self {
            Self::Failed => "installer_failed",
            Self::Cancelled => "installer_cancelled",
        }
    }
}

pub(super) fn receipt_path(root: &Path, record: &Record) -> Result<PathBuf> {
    Ok(completion::receipt_path(root, record)?.with_extension("nsis-failed"))
}

pub(super) fn ensure_absent(
    root: &Path,
    record: &Record,
    contract: &completion::Contract,
) -> Result<()> {
    if contract.failure_protocol.is_none() {
        return Ok(());
    }
    match std::fs::symlink_metadata(receipt_path(root, record)?) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(journal::pending(error)),
        Ok(_) => Err(journal::pending(
            "Failure callback exists; successful completion is not proven",
        )),
    }
}

pub(super) fn bind_plan(
    plan: launch_plan::LaunchPlan,
    root: &Path,
    record: &Record,
    contract: &completion::Contract,
) -> Result<launch_plan::LaunchPlan> {
    if contract.failure_protocol.is_none() {
        return Ok(plan);
    }
    ensure_absent(root, record, contract)?;
    plan.with_failure_receipt(&receipt_path(root, record)?)
        .map_err(journal::pending)
}

pub(super) fn observe(update: &Update, record: &Record, root: &Path) -> Result<Option<Outcome>> {
    let Some(inventory) = &record.inventory else {
        return Ok(None);
    };
    let Some(contract) = inventory.completion(&update.config.pubkey, record)? else {
        return Ok(None);
    };
    if contract.failure_protocol.is_none() {
        return Ok(None);
    }
    contract.validate(record.kind)?;
    let executable = PathBuf::from(OsString::from_wide(&record.binding.executable));
    if record.binding.app_name != update.app_name
        || record.target != update.target
        || executable != std::env::current_exe().map_err(journal::pending)?
    {
        return Err(journal::pending(
            "Failure callback belongs to another App/target",
        ));
    }
    let _directories = journal::pin_existing_directories(root).map_err(journal::pending)?;
    let mut receipt = match journal::protected_read(&receipt_path(root, record)?) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(journal::pending(error)),
    };
    match std::fs::symlink_metadata(completion::receipt_path(root, record)?) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(journal::pending(error)),
        Ok(_) => {
            return Err(journal::pending(
                "Conflicting successful and failed installer callbacks",
            ))
        }
    }
    if receipt.metadata().map_err(journal::pending)?.len() > 32 * 1024 {
        return Err(journal::pending("Oversized failure callback"));
    }
    let mut bytes = Vec::new();
    receipt.read_to_end(&mut bytes).map_err(journal::pending)?;
    let outcome = completion::verify_callback(
        &bytes,
        record,
        &contract,
        PROTOCOL,
        &["failed", "cancelled"],
        false,
    )?;
    Ok(Some(if outcome == "failed" {
        Outcome::Failed
    } else {
        Outcome::Cancelled
    }))
}
