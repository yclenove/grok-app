//! Operation-local loaded crypto only; owners are reselected in the real snapshot.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use super::{missing, JobCrypto};
use crate::session_migration::errors::{sqlite_storage_error, ImportError};

pub(in crate::session_migration) struct JobCryptos {
    jobs: BTreeMap<String, Result<JobCrypto, ImportError>>,
}

impl JobCryptos {
    pub(in crate::session_migration) fn prepare(
        conn: &Connection,
        candidates: &[String],
        extra_jobs: &[String],
    ) -> Result<Self, ImportError> {
        if !conn.is_autocommit() {
            return Err(missing());
        }
        let selected = serde_json::to_string(candidates).map_err(|_| missing())?;
        let mut statement = conn
            .prepare(
                "SELECT DISTINCT job_id FROM job_items
            WHERE candidate_id IN (SELECT value FROM json_each(?1))",
            )
            .map_err(sqlite_storage_error)?;
        let rows = statement
            .query_map([selected], |row| row.get::<_, String>(0))
            .map_err(sqlite_storage_error)?;
        let mut ids = BTreeSet::new();
        for row in rows {
            match row {
                Ok(id) => {
                    ids.insert(id);
                }
                // Do not turn a malformed owner ID into a global failure for
                // independent previews. Its point owner query still refuses
                // the bad type in the real snapshot; no skipped ID is granted.
                Err(
                    rusqlite::Error::InvalidColumnType(..)
                    | rusqlite::Error::FromSqlConversionFailure(..),
                ) => {}
                Err(error) => return Err(sqlite_storage_error(error)),
            }
        }
        ids.extend(extra_jobs.iter().cloned());
        let jobs = ids
            .into_iter()
            .map(|id| {
                let crypto = JobCrypto::prepare(conn, &id);
                (id, crypto)
            })
            .collect();
        // A bad owner stays a per-owner error, not permission to skip it and
        // not an unrelated healthy preview's global failure.
        Ok(Self { jobs })
    }

    pub(in crate::session_migration) fn get(&self, id: &str) -> Result<&JobCrypto, ImportError> {
        self.jobs
            .get(id)
            .ok_or_else(missing)?
            .as_ref()
            .map_err(Clone::clone)
    }
}

#[cfg(test)]
#[path = "publication_crypto_set_tests.rs"]
mod tests;
