//! Authenticated confirmed-plan acceptance; ordinary enqueue refuses associations.
//! The witness, durable bytes and plan are fixed before any writer reservation.
use super::{failure, DurableInput};
use crate::session_migration::{
    errors::{ErrorCode, ImportError},
    hash,
    identity_association::{self, InspectionWitness},
    job_input_association::AssociationInput,
    jobs::{CommitItem, JobRecord, PreparedRevision},
    registration_provenance::{self, AssociationProof, InputIdentity, Proof},
};
use rusqlite::Connection;
use std::sync::Arc;

#[cfg(test)]
thread_local! { static AFTER_PREFLIGHT: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = std::cell::RefCell::new(None); }

#[cfg(test)]
pub(in crate::session_migration) fn test_after_preflight(hook: impl FnOnce() + 'static) {
    AFTER_PREFLIGHT.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

#[derive(Clone, Copy)]
pub(super) enum Lane {
    #[cfg(test)]
    Ordinary,
    Registered,
    ConfirmedAssociations,
}

impl Lane {
    pub(super) fn require_item(self, item: &CommitItem) -> Result<(), ImportError> {
        if !matches!(self, Self::ConfirmedAssociations) {
            crate::session_migration::job_input_association::refuse_publication(
                item.identity_association.as_ref(),
            )?;
        }
        Ok(())
    }
}

pub(super) enum Identity {
    Legacy(InputIdentity, Option<Box<Proof>>),
    Association(
        Box<InspectionWitness>,
        Box<AssociationProof>,
        Box<VerifiedInput>,
    ),
}

impl Identity {
    pub(super) fn requires_publication_lock(&self) -> bool {
        !matches!(self, Self::Legacy(_, None))
    }

    pub(super) fn accept(&self, conn: &Connection) -> Result<(), ImportError> {
        match self {
            Self::Legacy(input, proof) => {
                registration_provenance::accept(conn, input, proof.as_deref())
            }
            Self::Association(witness, proof, input) => {
                input.compare_plan(conn)?;
                witness.compare_catalog(conn)?;
                crate::session_migration::registration_provenance_store::insert_record(
                    conn,
                    &registration_provenance::ProofRecord::Association(proof.clone()),
                )?;
                proof.record_acceptance(conn)?;
                let count = conn.execute("UPDATE entries SET native_session_id=?1 WHERE import_id=?2
                    AND installation_id=?3 AND provider=?4 AND native_session_id=?5 AND current_revision=?6 AND hidden=0",
                    rusqlite::params![input.identity.native_session_id, witness.entry.import_id,
                        witness.entry.installation_id, witness.entry.provider, witness.entry.native_session_id,
                        witness.expected_revision]).map_err(crate::session_migration::errors::sqlite_storage_error)?;
                if count != 1 {
                    return Err(stale("association target changed during acceptance"));
                }
                Ok(())
            }
        }
    }
}

fn stale(message: &str) -> ImportError {
    ImportError::new(ErrorCode::PlanStale, message)
}

/// Not serializable or caller-constructible. Always derived from canonical
/// DurableInput bytes plus the complete validated persistent plan context.
pub(in crate::session_migration) struct VerifiedInput {
    association: AssociationInput,
    identity: InputIdentity,
    validated: crate::session_migration::selection::PreparedSelectionValidation,
    preview: crate::session_migration::selection::PreviewSelection,
    hashes: std::collections::BTreeMap<String, String>,
    plan_json: String,
    expiry: chrono::DateTime<chrono::FixedOffset>,
}

impl VerifiedInput {
    pub(in crate::session_migration) fn association(&self) -> &AssociationInput {
        &self.association
    }
    pub(in crate::session_migration) fn identity(&self) -> &InputIdentity {
        &self.identity
    }

    pub(in crate::session_migration) fn require_witness(
        &self,
        witness: &InspectionWitness,
    ) -> Result<(), ImportError> {
        let review = witness.review()?;
        let confirmation = &self.association.confirmation;
        if confirmation.version != review.version
            || confirmation.import_id != review.import_id
            || confirmation.expected_revision != review.expected_revision
            || confirmation.preview_id != review.preview_id
            || confirmation.review_digest != review.review_digest
            || self.preview != witness.preview
            || self.validated != witness.validated
            || self.hashes != witness.prepared_hashes
        {
            return Err(stale(
                "association input differs from its authenticated inspection witness",
            ));
        }
        Ok(())
    }

    fn compare_plan(&self, conn: &Connection) -> Result<(), ImportError> {
        if self.expiry.with_timezone(&chrono::Utc) <= chrono::Utc::now() {
            return Err(stale("association plan expired before acceptance"));
        }
        if bounded_plan(conn, &self.association)? != self.plan_json {
            return Err(stale(
                "association plan changed after detached authentication",
            ));
        }
        Ok(())
    }

    fn capture(
        conn: &Connection,
        input: &DurableInput,
        job: &JobRecord,
        encoded: &str,
    ) -> Result<Self, ImportError> {
        if conn.is_autocommit() {
            let tx = conn.unchecked_transaction().map_err(failure)?;
            return Self::capture(&tx, input, job, encoded);
        }
        let association = input
            .identity_association
            .clone()
            .ok_or_else(|| stale("explicit association input is required"))?;
        let plan_json = bounded_plan(conn, &association)?;
        let plan: serde_json::Value = serde_json::from_str(&plan_json).map_err(failure)?;
        let expiry = plan
            .get("expiry")
            .and_then(serde_json::Value::as_str)
            .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
            .ok_or_else(|| stale("association plan expiry is invalid"))?;
        input.validate_shape()?;
        input.validate_context(conn, job)?;
        let canonical = serde_json::to_string(input).map_err(failure)?;
        if canonical != encoded || input.explicit_reimport {
            return Err(stale(
                "association requires canonical input and a confirmed non-reimport plan",
            ));
        }
        Ok(Self {
            association,
            identity: input.input_identity()?,
            validated: crate::session_migration::selection::PreparedSelectionValidation {
                source_member: input.source_member.clone(),
                cwd: input.adapter.graph.cwd.clone(),
                snapshot_digest: input.preview_digest.clone(),
                candidate_id: input.candidate_id.clone(),
                source_id: input.source_id.clone(),
                root_path: input.root_path.clone(),
                legacy_id: input.legacy_id.clone(),
                provider: input.provider.clone(),
                native_session_id: input.adapter.graph.native_session_id.clone(),
            },
            preview: input.selection_set.single_entry()?.clone(),
            hashes: input.preview_file_hashes.clone(),
            plan_json,
            expiry,
        })
    }
}

fn bounded_plan(conn: &Connection, association: &AssociationInput) -> Result<String, ImportError> {
    use std::io::Read;
    let (row, kinds): (i64, [String; 4]) = conn
        .query_row(
            "SELECT rowid,typeof(json),typeof(plan_hash),typeof(created_at),typeof(expires_at) FROM plans WHERE plan_id=?1",
            [&association.plan_id],
            |row| Ok((row.get(0)?, [row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?])),
        )
        .map_err(failure)?;
    if kinds != ["text", "text", "text", "text"] {
        return Err(stale("association plan column types are invalid"));
    }
    let mut blob = conn
        .blob_open(rusqlite::DatabaseName::Main, "plans", "json", row, true)
        .map_err(failure)?;
    if blob.len() > super::input_wire::ASSOCIATION_BYTES {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "association plan exceeds record budget",
        ));
    }
    let mut json = String::with_capacity(blob.len());
    blob.read_to_string(&mut json).map_err(failure)?;
    let hash = conn
        .blob_open(
            rusqlite::DatabaseName::Main,
            "plans",
            "plan_hash",
            row,
            true,
        )
        .map_err(failure)?;
    if hash.len() != 64 {
        return Err(stale("association plan hash length is invalid"));
    }
    let mut stored = String::with_capacity(64);
    {
        let mut hash = hash;
        hash.read_to_string(&mut stored).map_err(failure)?;
    }
    if stored != association.plan_hash {
        return Err(stale("association plan hash changed"));
    }
    let value: serde_json::Value = serde_json::from_str(&json).map_err(failure)?;
    for (column, field) in [("created_at", "created"), ("expires_at", "expiry")] {
        let mut blob = conn
            .blob_open(rusqlite::DatabaseName::Main, "plans", column, row, true)
            .map_err(failure)?;
        if blob.len() > 1024 {
            return Err(stale("association plan lifetime exceeds record bounds"));
        }
        let mut text = String::with_capacity(blob.len());
        blob.read_to_string(&mut text).map_err(failure)?;
        if value.get(field).and_then(serde_json::Value::as_str) != Some(text.as_str()) {
            return Err(stale("association plan lifetime columns disagree"));
        }
    }
    Ok(json)
}

pub(in crate::session_migration) fn inspect_encoded(
    conn: &Connection,
    encoded: &str,
    job: &JobRecord,
) -> Result<VerifiedInput, ImportError> {
    if encoded.len() > super::input_wire::ASSOCIATION_BYTES {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "association input exceeds record budget",
        ));
    }
    let input = DurableInput::parse(encoded)?;
    VerifiedInput::capture(conn, &input, job, encoded)
}

/// Exact accepted metadata point read. No source/preview reopening, TTL check,
/// source discovery, worker publication or authority inferred from a receipt.
pub(in crate::session_migration) fn accepted_input(
    conn: &Connection,
    association: &AssociationInput,
    candidate: &str,
) -> Result<VerifiedInput, ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(failure)?;
        return accepted_input(&tx, association, candidate);
    }
    let job = association.stored_job(conn, &association.job_id)?;
    let encoded = super::input_reader::load_registration(conn, &association.job_id, candidate)?;
    let input = inspect_encoded(conn, &encoded, &job)?;
    super::association_members::verify(conn, &job, candidate)?;
    if &input.association != association
        || input.identity.candidate_id != candidate
        || input.identity.digest != hash::sha256_bytes(encoded.as_bytes())
    {
        return Err(stale(
            "accepted association job/input/confirmation disagree",
        ));
    }
    Ok(input)
}

pub(super) fn preflight(
    conn: &Connection,
    job: &JobRecord,
    prepared: &[(DurableInput, Arc<PreparedRevision>)],
    lane: Lane,
) -> Result<Vec<Identity>, ImportError> {
    #[cfg(test)]
    if matches!(lane, Lane::Ordinary) {
        return Ok(vec![]);
    }
    let mut identities = Vec::new();
    let mut targets = std::collections::HashSet::new();
    for (input, _) in prepared {
        if let Some(association) = &input.identity_association {
            if !matches!(lane, Lane::ConfirmedAssociations) {
                return Err(stale("wrong association acceptance lane"));
            }
            if !targets.insert(association.confirmation.import_id.clone()) {
                return Err(stale("association targets cannot repeat in one acceptance"));
            }
            let witness =
                identity_association::inspect_witness(&identity_association::AssociationRequest {
                    version: association.confirmation.version,
                    import_id: association.confirmation.import_id.clone(),
                    expected_revision: association.confirmation.expected_revision.clone(),
                    preview: input.selection_set.single_entry()?.clone(),
                })?;
            let encoded = serde_json::to_string(input).map_err(failure)?;
            let verified = VerifiedInput::capture(conn, input, job, &encoded)?;
            let proof = AssociationProof::authenticated(&witness, &verified)?;
            identities.push(Identity::Association(
                Box::new(witness),
                Box::new(proof),
                Box::new(verified),
            ));
        } else {
            let identity = input.registration_identity()?;
            let proof = registration_provenance::preflight(conn, identity.clone(), job)?;
            identities.push(Identity::Legacy(identity, proof.map(Box::new)));
        }
    }
    if matches!(lane, Lane::ConfirmedAssociations) && targets.is_empty() {
        return Err(stale(
            "confirmed association lane requires a genuine association",
        ));
    }
    if let Some(Identity::Association(_, _, first)) = identities
        .iter()
        .find(|identity| matches!(identity, Identity::Association(..)))
    {
        let plan = crate::session_migration::engine::association_input_plan(
            conn,
            &job.plan_id,
            &job.plan_hash,
        )?;
        let previews: std::collections::HashSet<_> = prepared
            .iter()
            .map(|(input, _)| {
                input
                    .selection_set
                    .single_entry()
                    .map(|entry| entry.preview_id.as_str())
            })
            .collect::<Result<_, _>>()?;
        if previews.len() != prepared.len()
            || previews.len() != plan.preview_ids.len()
            || plan
                .preview_ids
                .iter()
                .any(|preview| !previews.contains(preview.as_str()))
        {
            return Err(stale(
                "association acceptance must cover the entire confirmed plan exactly once",
            ));
        }
        for (input, _) in prepared {
            let preview = input.selection_set.single_entry()?;
            let expected = plan
                .selection_set
                .entries
                .iter()
                .find(|entry| entry.preview_id == preview.preview_id)
                .ok_or_else(|| stale("acceptance input is missing from its plan"))?;
            let matches = if input.identity_association.is_some() {
                preview == expected
            } else {
                input.selection_set
                    == crate::session_migration::selection::SelectionSet::single(expected.clone())
                        .canonicalized()
            };
            if !matches {
                return Err(stale(
                    "acceptance selection differs from the confirmed plan",
                ));
            }
            plan.snapshots
                .get(&preview.preview_id)
                .ok_or_else(|| stale("acceptance plan snapshot is missing"))?
                .require_metadata(&input.preview_digest, input.source_member.as_ref())?;
        }
        // The writer later compares these exact plan bytes, without reopening
        // the preview or re-running source-member authentication.
        if first.expiry.with_timezone(&chrono::Utc) <= chrono::Utc::now() {
            return Err(stale(
                "association plan expired before acceptance preflight",
            ));
        }
    }
    #[cfg(test)]
    if let Some(hook) = AFTER_PREFLIGHT.with(|slot| slot.borrow_mut().take()) {
        hook();
    }
    Ok(identities)
}
