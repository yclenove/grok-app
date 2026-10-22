//! Acceptance-time allocation data, not a publication/Undo authorization grant.
//! Only the producer writes it; workers must load the required original record.
use rusqlite::Transaction;

use super::*;

pub(in crate::session_migration) struct PreparedAllocation {
    allocation: Allocation,
    identity: String,
}

impl PreparedAllocation {
    pub(in crate::session_migration) fn new(allocation: Allocation) -> Result<Self, ImportError> {
        let identity = sha256_bytes(canonical(&allocation)?.as_bytes());
        Ok(Self {
            allocation,
            identity,
        })
    }

    pub(in crate::session_migration) fn identity(&self) -> &str {
        &self.identity
    }

    pub(in crate::session_migration) fn record(
        &self,
        tx: &Transaction<'_>,
        auth: &InstallationAuthenticator,
        binding: Binding,
    ) -> Result<(), ImportError> {
        let sealed = seal(auth, binding, Stage::Allocated(self.allocation.clone()))?;
        journal::insert(tx, auth, &sealed)
    }
}

impl VerifiedRecord {
    pub(in crate::session_migration) fn allocation(&self) -> Result<&Allocation, ImportError> {
        match &self.body.stage {
            Stage::Allocated(allocation) => Ok(allocation),
            _ => Err(invalid()),
        }
    }

    pub(in crate::session_migration) fn load_allocation(
        tx: &Transaction<'_>,
        auth: &InstallationAuthenticator,
        binding: &Binding,
        identity: &str,
    ) -> Result<Self, ImportError> {
        let record = journal::load_required(tx, auth, binding, StageKind::Allocated)?;
        record.require_allocation_identity(identity)?;
        Ok(record)
    }

    pub(in crate::session_migration) fn require_allocation_identity(
        &self,
        identity: &str,
    ) -> Result<(), ImportError> {
        let allocation = self.allocation()?;
        if sha256_bytes(canonical(allocation)?.as_bytes()) != identity {
            return Err(invalid());
        }
        Ok(())
    }
}
