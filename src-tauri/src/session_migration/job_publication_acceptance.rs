//! Signed original job/input acceptance, separate from legacy scope/receipt.
//! Only the genuine registered producer issues seals; readers never repair them.
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::session_migration::{
    cursor::InstallationAuthenticator,
    errors::{ErrorCode, ImportError},
    hash,
};

const VERSION: u32 = 1;
const RECORD_BYTES: usize = 8192;
const HEADER_DOMAIN: &str = "session-import-publication-acceptance-header-v1";
const MEMBER_DOMAIN: &str = "session-import-publication-acceptance-member-v1";

#[path = "job_publication_allocation.rs"]
mod allocation;
#[path = "job_publication_acceptance_codec.rs"]
mod codec;
#[path = "job_publication_acceptance_journal.rs"]
pub(in crate::session_migration) mod journal;
#[path = "job_publication_acceptance_producer.rs"]
mod producer;
#[path = "job_publication_acceptance_record.rs"]
mod record;
pub(super) use producer::PreparedAcceptance;
use record::{Header, Member};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    body: T,
    mac: String,
}

// Detached crypto records, not cached SQL permissions. No Debug/Clone/Serialize.
struct SealedHeader {
    body: Header,
    encoded: String,
    digest: String,
}
struct SealedMember {
    body: Member,
    encoded: String,
}
struct VerifiedHeader {
    body: Header,
    digest: String,
}
// Immutable crypto facts only. Sharing one never retains current SQL facts or
// the connection/transaction/total_changes-bound AcceptedJob writer witness.
type VerifiedMember = Arc<MemberFact>;
struct MemberFact {
    body: Member,
    digest: String,
    // Derived only from authenticated exact member bytes, never SQL state.
    commitment: String,
}

fn invalid() -> ImportError {
    ImportError::new(
        ErrorCode::HistoryAccessUnavailable,
        "signed publication acceptance is missing or inconsistent",
    )
    .job_local()
}

fn canonical<T: Serialize>(value: &T) -> Result<String, ImportError> {
    Ok(hash::canonical_json(
        &serde_json::to_value(value).map_err(|_| invalid())?,
    ))
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ImportError> {
    require_record_bytes(bytes)?;
    serde_json::from_slice(bytes).map_err(|_| invalid())
}

fn require_record_bytes(bytes: &[u8]) -> Result<(), ImportError> {
    if bytes.len() > RECORD_BYTES || bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(invalid());
    }
    Ok(())
}

fn seal<T: Serialize>(
    auth: &InstallationAuthenticator,
    domain: &str,
    body: T,
) -> Result<String, ImportError> {
    let payload = canonical(&body)?;
    let envelope = Envelope {
        body,
        mac: hex::encode(auth.sign(domain, payload.as_bytes())),
    };
    let encoded = canonical(&envelope)?;
    if encoded.len() > RECORD_BYTES {
        return Err(invalid());
    }
    Ok(encoded)
}

fn verify_mac<T: Serialize>(
    auth: &InstallationAuthenticator,
    domain: &str,
    envelope: &Envelope<T>,
) -> Result<String, ImportError> {
    #[cfg(test)]
    let _span = crate::session_migration::codec_profile::Span::start("acceptance-envelope-mac", 0);
    record::digest(&envelope.mac)?;
    let mac = hex::decode(&envelope.mac).map_err(|_| invalid())?;
    if !auth.verify(domain, canonical(&envelope.body)?.as_bytes(), &mac) {
        return Err(invalid());
    }
    Ok(hash::sha256_bytes(canonical(envelope)?.as_bytes()))
}

fn verify_header(
    auth: &InstallationAuthenticator,
    bytes: &[u8],
    job_id: &str,
) -> Result<VerifiedHeader, ImportError> {
    let envelope: Envelope<Header> = decode(bytes)?;
    envelope.body.validate()?;
    if envelope.body.job_id != job_id {
        return Err(invalid());
    }
    let digest = verify_mac(auth, HEADER_DOMAIN, &envelope)?;
    Ok(VerifiedHeader {
        body: envelope.body,
        digest,
    })
}

fn verify_member(
    auth: &InstallationAuthenticator,
    bytes: &[u8],
    header: &VerifiedHeader,
    candidate_digest: &str,
) -> Result<VerifiedMember, ImportError> {
    #[cfg(test)]
    let _span = crate::session_migration::codec_profile::Span::start(
        "acceptance-member-verify",
        bytes.len(),
    );
    // Re-read current SQL bytes in journal first. Memoize ONLY this exact
    // typed decode/MAC result under the preloaded key, never membership/input
    // existence, body hashes, header facts, allocation or a writer permission.
    require_record_bytes(bytes)?;
    let key = codec::Key::new(auth, bytes);
    let member = if let Some(member) = codec::get(&key, bytes) {
        #[cfg(test)]
        let _hit =
            crate::session_migration::codec_profile::Span::start("acceptance-member-cache-hit", 0);
        member
    } else {
        #[cfg(test)]
        let _miss =
            crate::session_migration::codec_profile::Span::start("acceptance-member-cache-miss", 0);
        let envelope: Envelope<Member> = decode(bytes)?;
        envelope.body.validate()?;
        let digest = verify_mac(auth, MEMBER_DOMAIN, &envelope)?;
        let member = Arc::new(MemberFact {
            commitment: record::member_commitment(
                &envelope.body.input_digest,
                &envelope.body.allocation_digest,
            ),
            body: envelope.body,
            digest,
        });
        codec::insert(key, bytes, member.clone());
        member
    };
    // Expected context is fresh on every call, including a pure codec hit.
    if member.body.job_id != header.body.job_id
        || member.body.header_digest != header.digest
        || member.body.candidate_digest != candidate_digest
    {
        return Err(invalid());
    }
    Ok(member)
}

#[cfg(test)]
#[path = "job_publication_acceptance_tests.rs"]
mod tests;
