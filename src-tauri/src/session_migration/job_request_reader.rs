//! Pre-snapshot crypto selection is not authority over a request's current owner.
use rusqlite::{Connection, OptionalExtension};

use super::{get_job_with_crypto, JobRecord};
use crate::session_migration::{
    errors::{sqlite_storage_error, ErrorCode, ImportError},
    publication_crypto::JobCrypto,
};

fn changed() -> ImportError {
    ImportError::new(
        ErrorCode::PlanStale,
        "request owner changed before its snapshot",
    )
    .job_local()
}

fn owner(conn: &Connection, request: &str) -> Result<Option<String>, ImportError> {
    conn.query_row(
        "SELECT job_id FROM jobs WHERE request_id=?1",
        [request],
        |row| row.get(0),
    )
    .optional()
    .map_err(sqlite_storage_error)
}

pub(in crate::session_migration) struct PreparedRequest {
    request: String,
    owner: Option<(String, JobCrypto)>,
}

impl PreparedRequest {
    pub(in crate::session_migration) fn prepare(
        conn: &Connection,
        request: &str,
    ) -> Result<Self, ImportError> {
        if !conn.is_autocommit() {
            return Err(ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "request crypto must be prepared before its snapshot",
            )
            .job_local());
        }
        Self::capture(conn, request, JobCrypto::prepare)
    }

    // Existing transaction callers retain legacy behavior, never a hidden key loader.
    pub(super) fn legacy_snapshot(conn: &Connection, request: &str) -> Result<Self, ImportError> {
        Self::capture(conn, request, JobCrypto::legacy_snapshot)
    }

    fn capture(
        conn: &Connection,
        request: &str,
        prepare: impl FnOnce(&Connection, &str) -> Result<JobCrypto, ImportError>,
    ) -> Result<Self, ImportError> {
        let selected = owner(conn, request)?
            .map(|id| {
                let crypto = prepare(conn, &id)?;
                Ok::<_, ImportError>((id, crypto))
            })
            .transpose()?;
        Ok(Self {
            request: request.into(),
            owner: selected,
        })
    }

    /// SQL-only selection within the caller's snapshot. No body fetch or grant.
    pub(in crate::session_migration) fn selected<'a>(
        &'a self,
        conn: &Connection,
        request: &str,
    ) -> Result<Option<(&'a str, &'a JobCrypto)>, ImportError> {
        if conn.is_autocommit() || request != self.request {
            return Err(changed());
        }
        let current = owner(conn, request)?;
        if current.as_deref() != self.owner.as_ref().map(|(id, _)| id.as_str()) {
            return Err(changed());
        }
        Ok(self
            .owner
            .as_ref()
            .map(|(id, crypto)| (id.as_str(), crypto)))
    }

    pub(in crate::session_migration) fn read(
        &self,
        conn: &Connection,
        request: &str,
    ) -> Result<Option<JobRecord>, ImportError> {
        if conn.is_autocommit() {
            let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
            return self.read(&tx, request);
        }
        let Some((id, crypto)) = self.selected(conn, request)? else {
            return Ok(None);
        };
        let job = get_job_with_crypto(conn, id, crypto)?.ok_or_else(changed)?;
        if job.job_id != id || job.request_id != request {
            return Err(changed());
        }
        Ok(Some(job))
    }
}

#[cfg(test)]
#[path = "job_request_reader_tests.rs"]
mod tests;
