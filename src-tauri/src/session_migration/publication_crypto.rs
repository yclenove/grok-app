//! Operation-local loaded crypto, never a cached SQL authorization decision.
use rusqlite::Connection;

use super::{
    cursor::InstallationAuthenticator,
    errors::{sqlite_storage_error, ErrorCode, ImportError},
    publication_protocol::PublicationProtocol,
};

#[path = "publication_crypto_set.rs"]
mod set;
pub(super) use set::JobCryptos;

// No Clone/Debug/Serialize. Presence and protocol are rechecked by each reader
// or writer; a context prepared for an absent job grants nothing if it appears.
pub(super) struct JobCrypto {
    job_id: String,
    auth: Option<InstallationAuthenticator>,
}

fn missing() -> ImportError {
    ImportError::new(
        ErrorCode::HistoryAccessUnavailable,
        "publication crypto must be prepared before opening the transaction",
    )
    .job_local()
}

pub(super) fn present(conn: &Connection, job_id: &str) -> Result<bool, ImportError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM jobs WHERE job_id=?1)",
        [job_id],
        |row| row.get(0),
    )
    .map_err(sqlite_storage_error)
}

impl JobCrypto {
    /// Only operation entry points call this, before their read/write snapshot.
    /// Missing jobs preserve not-found semantics; legacy does no key-file I/O.
    pub(super) fn prepare(conn: &Connection, job_id: &str) -> Result<Self, ImportError> {
        if !conn.is_autocommit() {
            return Err(missing());
        }
        let mut prepared = Self {
            job_id: job_id.into(),
            auth: None,
        };
        if !present(conn, job_id)? {
            return Ok(prepared);
        }
        let protocol = PublicationProtocol::for_job_id(job_id)?;
        if protocol == PublicationProtocol::SignedOutcomeV1 {
            prepared.auth = Some(InstallationAuthenticator::load()?);
        }
        Ok(prepared)
    }

    /// Compatibility callers already inside a snapshot may use legacy only.
    /// Never hide a loader behind get_job, a key cache, or a nested transaction.
    pub(super) fn legacy_snapshot(conn: &Connection, job_id: &str) -> Result<Self, ImportError> {
        if present(conn, job_id)? {
            let protocol = PublicationProtocol::for_job_id(job_id)?;
            if protocol != PublicationProtocol::Legacy {
                return Err(missing());
            }
        }
        Ok(Self {
            job_id: job_id.into(),
            auth: None,
        })
    }

    pub(super) fn require(
        &self,
        job_id: &str,
        protocol: PublicationProtocol,
    ) -> Result<Option<&InstallationAuthenticator>, ImportError> {
        if self.job_id != job_id {
            return Err(missing());
        }
        match protocol {
            PublicationProtocol::Legacy => Ok(None),
            PublicationProtocol::SignedOutcomeV1 => {
                self.auth.as_ref().map(Some).ok_or_else(missing)
            }
        }
    }
}

#[cfg(test)]
#[path = "publication_crypto_tests.rs"]
mod tests;
