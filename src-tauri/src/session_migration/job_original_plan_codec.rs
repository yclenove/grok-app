//! Memoize a PURE canonical/shape result for exact original ordinary bytes.
//! No SQL facts, context, member, fence or writer authority are cached here.
use std::cell::RefCell;
use std::collections::VecDeque;

#[cfg(test)]
use super::Record;
use super::{engine, hash, refusal, ImportError};
use crate::session_migration::publication_protocol::PublicationProtocol;

const MAX_ENTRIES: usize = 16;

struct Key {
    plan_id: String,
    plan_hash: String,
    json_digest: String,
    protocol: PublicationProtocol,
    has_associations: bool,
}

thread_local! {
    static CACHE: RefCell<VecDeque<Key>> = const { RefCell::new(VecDeque::new()) };
}

#[cfg(test)]
pub(super) fn require_ordinary(scope: &Record, original: &str) -> Result<(), ImportError> {
    let (_, has_associations) = plan_identity(
        &scope.plan_id,
        &scope.plan_hash,
        &scope.plan_json_digest,
        original,
    )?;
    if has_associations {
        return Err(refusal());
    }
    Ok(())
}

pub(super) fn plan_identity(
    plan_id: &str,
    plan_hash: &str,
    json_digest: &str,
    original: &str,
) -> Result<(PublicationProtocol, bool), ImportError> {
    // The caller must still read the CURRENT snapshot fact before calling us.
    // Never trust a caller's precomputed raw digest as a cache key.
    #[cfg(test)]
    let bytes_hash = crate::session_migration::codec_profile::Span::start(
        "original-plan-bytes-hash",
        original.len(),
    );
    let digest = hash::sha256_bytes(original.as_bytes());
    #[cfg(test)]
    drop(bytes_hash);
    if digest != json_digest {
        return Err(refusal());
    }
    let hit = CACHE.with(|cache| {
        cache
            .borrow()
            .iter()
            .find(|key| {
                key.plan_id == plan_id && key.plan_hash == plan_hash && key.json_digest == digest
            })
            .map(|key| (key.protocol, key.has_associations))
    });
    if let Some(parsed) = hit {
        #[cfg(test)]
        let _hit = crate::session_migration::codec_profile::Span::start("original-codec-hit", 0);
        return Ok(parsed);
    }
    #[cfg(test)]
    let _miss = crate::session_migration::codec_profile::Span::start("original-codec-miss", 0);
    // This codec performs no SQLite/filesystem I/O or fresh-acceptance TTL gate.
    // Its output depends only on these exact bytes and expected plan identity.
    let plan = engine::job_entry_plan_json(plan_id, plan_hash, original)
        .map_err(ImportError::job_local)?;
    let parsed = (
        plan.publication_protocol,
        !plan.identity_associations.is_empty(),
    );
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() == MAX_ENTRIES {
            cache.pop_front();
        }
        cache.push_back(Key {
            plan_id: plan_id.into(),
            plan_hash: plan_hash.into(),
            json_digest: digest,
            protocol: parsed.0,
            has_associations: parsed.1,
        });
    });
    // This is only an immutable codec fact. The caller still validates current
    // job metadata, proofs/input/member bindings in its own SQL snapshot and
    // creates a NEW connection/job/total_changes-bound writer witness there.
    Ok(parsed)
}

#[cfg(test)]
#[path = "job_original_plan_codec_tests.rs"]
mod tests;
