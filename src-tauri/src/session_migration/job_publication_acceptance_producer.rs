//! Only the real registered scope capture can prepare an acceptance producer.
//! Loading the opaque crypto object happens here, before the acceptance writer.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use super::*;
use crate::session_migration::job_control::{
    association_acceptance::Identity, association_context, DurableInput,
};
use crate::session_migration::{
    engine,
    jobs::{JobRecord, JobState, PreparedRevision},
    publication_protocol::PublicationProtocol,
};

pub(in crate::session_migration::job_control) struct PreparedAcceptance {
    auth: InstallationAuthenticator,
    header: SealedHeader,
    members: Vec<(String, SealedMember)>,
    allocations: allocation::PreparedAllocations,
    expires: chrono::DateTime<chrono::FixedOffset>,
}

impl PreparedAcceptance {
    pub(in crate::session_migration::job_control) fn capture(
        conn: &rusqlite::Connection,
        job: &JobRecord,
        plan_json: &str,
        scope_digest: &str,
        prepared: &[(DurableInput, Arc<PreparedRevision>)],
        identities: &[Identity],
    ) -> Result<Self, ImportError> {
        let plan = engine::job_entry_plan_json(&job.plan_id, &job.plan_hash, plan_json)?;
        if plan.publication_protocol != PublicationProtocol::SignedOutcomeV1
            || job.state != JobState::Queued
            || job.state_version != 1
        {
            return Err(invalid());
        }
        plan.publication_protocol.require_job_id(&job.job_id)?;
        record::digest(scope_digest)?;
        #[derive(Deserialize)]
        struct Lifetime {
            expiry: String,
        }
        let lifetime: Lifetime = serde_json::from_str(plan_json).map_err(|_| invalid())?;
        let expires =
            chrono::DateTime::parse_from_rfc3339(&lifetime.expiry).map_err(|_| invalid())?;
        let mut covered = BTreeSet::new();
        let mut inputs = BTreeMap::new();
        let mut candidates = BTreeMap::new();
        for (input, _) in prepared {
            input.validate_shape()?;
            let selected = input.selection_set.single_entry()?;
            let expected = plan
                .selection_set
                .entries
                .iter()
                .find(|entry| entry.preview_id == selected.preview_id)
                .ok_or_else(invalid)?;
            let confirmation = plan
                .identity_associations
                .iter()
                .find(|confirmation| confirmation.preview_id == selected.preview_id);
            let matches = match (&input.identity_association, confirmation) {
                (Some(association), Some(confirmation)) => {
                    association.require_job(job)?;
                    association.confirmation == *confirmation && selected == expected
                }
                (None, None) => {
                    input.selection_set
                        == crate::session_migration::selection::SelectionSet::single(
                            expected.clone(),
                        )
                        .canonicalized()
                }
                _ => false,
            };
            if !matches
                || !covered.insert(selected.preview_id.as_str())
                || input.candidate_id.is_empty()
            {
                return Err(invalid());
            }
            plan.snapshots
                .get(&selected.preview_id)
                .ok_or_else(invalid)?
                .require_metadata(&input.preview_digest, input.source_member.as_ref())?;
            association_context::validate_mapping(input, &plan)?;
            let candidate_digest = hash::sha256_bytes(input.candidate_id.as_bytes());
            if inputs
                .insert(candidate_digest.clone(), input.input_identity()?.digest)
                .is_some()
                || candidates
                    .insert(candidate_digest, input.candidate_id.clone())
                    .is_some()
            {
                return Err(invalid());
            }
        }
        if covered.len() != plan.preview_ids.len()
            || plan
                .preview_ids
                .iter()
                .any(|preview| !covered.contains(preview.as_str()))
        {
            return Err(invalid());
        }
        let allocations = allocation::PreparedAllocations::capture(conn, prepared, identities)?;
        let commitments = inputs
            .iter()
            .map(|(digest, input)| {
                let candidate = candidates.get(digest).ok_or_else(invalid)?;
                Ok((
                    digest.clone(),
                    record::member_commitment(input, allocations.identity(candidate)?),
                ))
            })
            .collect::<Result<BTreeMap<_, _>, ImportError>>()?;
        let body = Header {
            version: VERSION,
            outcome_version: crate::session_migration::publication_outcome::VERSION,
            job_id: job.job_id.clone(),
            context_digest: record::context_digest(job),
            plan_id: job.plan_id.clone(),
            plan_hash: job.plan_hash.clone(),
            plan_digest: hash::sha256_bytes(plan_json.as_bytes()),
            scope_digest: scope_digest.into(),
            member_count: u32::try_from(inputs.len()).map_err(|_| invalid())?,
            members_digest: record::members_digest(&commitments),
        };
        body.validate()?;
        // All original shape/plan/input checks precede the only key loader.
        let auth = InstallationAuthenticator::load()?;
        let encoded = seal(&auth, HEADER_DOMAIN, &body)?;
        let header = SealedHeader {
            body,
            digest: hash::sha256_bytes(encoded.as_bytes()),
            encoded,
        };
        let mut members = Vec::with_capacity(inputs.len());
        for (candidate_digest, input_digest) in inputs {
            let candidate = candidates.remove(&candidate_digest).ok_or_else(invalid)?;
            let body = Member {
                version: VERSION,
                job_id: job.job_id.clone(),
                header_digest: header.digest.clone(),
                candidate_digest: candidate_digest.clone(),
                input_digest,
                allocation_digest: allocations.identity(&candidate)?.into(),
            };
            body.validate()?;
            let encoded = seal(&auth, MEMBER_DOMAIN, &body)?;
            members.push((candidate, SealedMember { body, encoded }));
        }
        Ok(Self {
            auth,
            header,
            members,
            allocations,
            expires,
        })
    }

    pub(in crate::session_migration::job_control) fn record(
        &self,
        tx: &rusqlite::Transaction<'_>,
        job: &JobRecord,
    ) -> Result<(), ImportError> {
        if record::context_digest(job) != self.header.body.context_digest
            || self.expires.with_timezone(&chrono::Utc) <= crate::session_migration::clock::now()
        {
            return Err(invalid());
        }
        // Validate exact current SQL facts before issuing any acceptance rows.
        self.require_initial_job(tx)?;
        for (candidate, member) in &self.members {
            if journal::input_digest(tx, &job.job_id, candidate)? != member.body.input_digest {
                return Err(invalid());
            }
        }
        self.allocations.require_current(tx)?;
        // Existing acceptance means allocation was already born atomically.
        // Never repair a missing row or mint another set on a producer retry.
        if journal::header_present(tx, &job.job_id)? {
            let accepted = journal::load(tx, &self.auth, &job.job_id)?;
            for (candidate, _) in &self.members {
                accepted.allocated(tx, candidate)?;
            }
        }
        journal::insert_header(tx, &self.auth, &self.header)?;
        let verified = verify_header(&self.auth, self.header.encoded.as_bytes(), &job.job_id)?;
        for (_, member) in &self.members {
            journal::insert_member(tx, &self.auth, &verified, member)?;
        }
        // Re-read the persisted header/full metadata membership under this SAME
        // transaction; triggers or a conflicting retry must roll back acceptance.
        let accepted = journal::load(tx, &self.auth, &job.job_id)?;
        let bindings = self
            .members
            .iter()
            .map(|(candidate, _)| Ok((candidate.clone(), accepted.outcome_binding(tx, candidate)?)))
            .collect::<Result<Vec<_>, ImportError>>()?;
        self.allocations.record(tx, &self.auth, bindings)?;
        // Allocation inserts may run triggers too. Refresh the transaction
        // witness and recheck every original input, allocation and SQL target.
        let accepted = journal::load(tx, &self.auth, &job.job_id)?;
        for (candidate, _) in &self.members {
            accepted.allocated(tx, candidate)?;
        }
        self.allocations.require_current(tx)?;
        self.require_initial_job(tx)?;
        Ok(())
    }

    fn require_initial_job(&self, tx: &rusqlite::Transaction<'_>) -> Result<(), ImportError> {
        // Fresh issuance still compares the actual draft fixed by preflight.
        // Later readers rely on the atomically retained original snapshot.
        let (stored, original) =
            crate::session_migration::catalog::load_plan_json(tx, &self.header.body.plan_id)?;
        if stored != self.header.body.plan_hash
            || hash::sha256_bytes(original.as_bytes()) != self.header.body.plan_digest
        {
            return Err(invalid());
        }
        let job = journal::require_header_facts(tx, &self.header.body)?;
        if job.state != JobState::Queued || job.state_version != 1 {
            return Err(invalid());
        }
        Ok(())
    }
}
