//! One publication attempt of a job item for the worker. A failed cancellation
//! read is that error, never a user cancel, so the caller can retry SourceBusy.
use rusqlite::Connection;

use super::durable_reader;
use crate::session_migration::errors::ImportError;
use crate::session_migration::jobs::{self, JobItem};
use crate::session_migration::publication_crypto::JobCrypto;

pub(super) fn attempt(
    conn: &Connection,
    id: &str,
    index: usize,
    candidate: &str,
    previous: &JobItem,
    crypto: &JobCrypto,
) -> Result<jobs::ItemProgress, ImportError> {
    let cancel_fault: std::cell::RefCell<Option<ImportError>> = std::cell::RefCell::new(None);
    let result = {
        let cancelled = || {
            #[cfg(test)]
            let _span =
                crate::session_migration::codec_profile::Span::start("worker-cancel-check", 0);
            #[cfg(test)]
            if let Some(error) = super::take_cancel_check_fault() {
                *cancel_fault.borrow_mut() = Some(error);
                return true;
            }
            match crate::session_migration::job_progress::cancelling(conn, id) {
                Ok(cancelling) => {
                    cancel_fault.borrow_mut().take();
                    cancelling
                }
                Err(error) => {
                    *cancel_fault.borrow_mut() = Some(error);
                    true
                }
            }
        };
        durable_reader::load(conn, id, candidate, crypto).and_then(|input| {
            match durable_reader::recover_progress(conn, id, index, previous, &input, crypto)? {
                Some(committed) => Ok(committed),
                None => input.into_item().and_then(|input| {
                    jobs::publish_for_worker(conn, id, index, &input, &cancelled, crypto)
                }),
            }
        })
    };
    match (result, cancel_fault.into_inner()) {
        // Publish stopped because the check failed. That Ok(cancelled) is not
        // a user cancel, so it is not recorded.
        (Ok(_), Some(error)) => Err(error),
        (result, _) => result,
    }
}
