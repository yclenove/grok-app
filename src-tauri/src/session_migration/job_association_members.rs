//! Complete retained-plan membership, separate from mutable header projection.
use super::{failure, input_reader, DurableInput};
use crate::session_migration::{errors::ImportError, jobs::JobRecord};
use rusqlite::Connection;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
thread_local! {
    static COMPLETE_MEMBER_SCANS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(in crate::session_migration) fn take_complete_member_scans() -> u32 {
    COMPLETE_MEMBER_SCANS.with(|count| count.replace(0))
}

/// Same-snapshot canonical members, not serializable or caller constructible.
/// Proofs must still authenticate their accepted digest and exact member below.
pub(in crate::session_migration) struct AcceptedMembers<'snapshot> {
    connection: &'snapshot Connection,
    inputs: BTreeMap<
        String,
        (
            crate::session_migration::job_input_association::AssociationInput,
            crate::session_migration::registration_provenance::InputIdentity,
        ),
    >,
}

impl AcceptedMembers<'_> {
    /// Undo selects association targets from accepted input membership, never
    /// from the mutable result IDs. Ordinary members keep their existing codec.
    pub(in crate::session_migration) fn undo_receipt(
        &self,
        conn: &Connection,
        job_id: &str,
        item: &crate::session_migration::jobs::JobItem,
        crypto: &crate::session_migration::publication_crypto::JobCrypto,
    ) -> Result<crate::session_migration::jobs::entry::UndoReceipt, ImportError> {
        use crate::session_migration::{
            catalog, errors::ErrorCode, jobs, jobs::entry::UndoReceipt,
        };
        if conn.is_autocommit() || !std::ptr::eq(self.connection, conn) {
            return Err(failure(
                "association Undo requires its authenticated snapshot",
            ));
        }
        let Some((association, identity)) = self.inputs.get(&item.candidate_id) else {
            return Ok(UndoReceipt::Ordinary);
        };
        self.require(conn, association, identity)?;
        let input = super::durable_reader::load(conn, job_id, &item.candidate_id, crypto)?;
        let accepted =
            super::association_publication::AcceptedPublication::for_input(conn, job_id, &input)?
                .ok_or_else(|| failure("accepted Undo member has no association input"))?;
        let receipt = super::association_publication::latest_receipt(
            conn,
            job_id,
            &item.candidate_id,
            Some(&accepted),
        )?;
        // Preserve the existing explicit missing-receipt conflict. No result
        // IDs are allowed to authorize a write when this point is absent.
        let Some(receipt) = receipt else {
            return Ok(UndoReceipt::Captured(None));
        };
        jobs::validate_receipt_shape(&receipt)?;
        let refusal = || {
            ImportError::new(
                ErrorCode::HistoryAccessUnavailable,
                "association Undo result does not match its committed owner",
            )
        };
        if receipt.phase != "committed"
            || item.import_id != receipt.import_id
            || item.revision_id != receipt.revision_id
            || item.mapping_project_id != receipt.mapping_project_id
            // A mutable result cannot invent a project-creation permission
            // absent from this member's authenticated retained mapping.
            || (receipt.created_project && !input.create_project)
        {
            return Err(refusal());
        }
        accepted.target(conn, Some(&receipt))?;
        input.validate_committed_receipt(job_id, &receipt)?;
        let import = receipt.import_id.as_deref().ok_or_else(refusal)?;
        let revision = receipt.revision_id.as_deref().ok_or_else(refusal)?;
        let published = catalog::get_revision(conn, import, revision)?.ok_or_else(refusal)?;
        if published.source_snapshot_id != receipt.snapshot_hash {
            return Err(refusal());
        }
        // Undo changes the display pointer, not archive bytes. Keep long file
        // hashing out of this writer; published readers enforce archive hashes.
        Ok(UndoReceipt::Captured(Some(Box::new(receipt))))
    }

    pub(in crate::session_migration) fn require(
        &self,
        conn: &Connection,
        association: &crate::session_migration::job_input_association::AssociationInput,
        input: &crate::session_migration::registration_provenance::InputIdentity,
    ) -> Result<(), ImportError> {
        if conn.is_autocommit()
            || !std::ptr::eq(self.connection, conn)
            || !self
                .inputs
                .get(&input.candidate_id)
                .is_some_and(|(a, i)| a == association && i == input)
        {
            return Err(failure(
                "accepted association proof differs from its canonical member",
            ));
        }
        Ok(())
    }

    pub(in crate::session_migration) fn count(&self) -> usize {
        self.inputs.len()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Member {
    candidate_id: String,
    phase: String,
    #[serde(default)]
    title: Option<String>,
    import_id: Option<String>,
    revision_id: Option<String>,
    outcome: Option<String>,
    error: Option<String>,
    #[serde(default)]
    mapping_project_id: Option<String>,
}

pub(super) fn verify(
    conn: &Connection,
    job: &JobRecord,
    candidate: &str,
) -> Result<(), ImportError> {
    verify_job(conn, job, candidate).map(|_| ())
}

pub(in crate::session_migration) fn verify_job<'snapshot>(
    conn: &'snapshot Connection,
    job: &JobRecord,
    candidate: &str,
) -> Result<AcceptedMembers<'snapshot>, ImportError> {
    #[cfg(test)]
    COMPLETE_MEMBER_SCANS.with(|count| count.set(count.get() + 1));
    if conn.is_autocommit() {
        return Err(failure(
            "accepted association membership requires one snapshot",
        ));
    }
    // The caller checked this header and all display bytes against the original
    // 16 MiB record cap in this snapshot, including split display rows.
    let members: Vec<Member> =
        crate::session_migration::jobs::item_store::page(conn, &job.job_id, 0, usize::MAX)
            .map_err(failure)?;
    let mut expected = BTreeMap::new();
    for member in members {
        if member.candidate_id.is_empty()
            || member.candidate_id.len() > 16384
            || !matches!(
                member.phase.as_str(),
                "queued"
                    | "running"
                    | "recovery_needed"
                    | "failed"
                    | "cancelled"
                    | "completed"
                    | "committed"
                    | "undone"
                    | "skipped"
                    | "conflict"
                    | "partial"
                    | "tombstoned"
            )
            || expected.insert(member.candidate_id, member.phase).is_some()
        {
            return Err(failure("accepted association job membership is invalid"));
        }
        // These are mutable result/display fields, not association authority.
        let _ = (
            member.title,
            member.import_id,
            member.revision_id,
            member.outcome,
            member.error,
            member.mapping_project_id,
        );
    }
    if expected.len() != job.item_count as usize || !expected.contains_key(candidate) {
        return Err(failure(
            "accepted association job does not contain its input exactly once",
        ));
    }
    let count: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM job_items WHERE job_id=?1",
            [&job.job_id],
            |row| row.get(0),
        )
        .map_err(failure)?;
    if count != expected.len() as u64 {
        return Err(failure(
            "accepted association durable input set is incomplete",
        ));
    }
    // Matching mutable JSON/SQL projections do not prove membership. Bind the
    // entire set to the authenticated retained plan in this same read snapshot.
    // Accepted history must not reopen disposed previews or reapply plan TTL.
    let plan = crate::session_migration::engine::association_input_plan(
        conn,
        &job.plan_id,
        &job.plan_hash,
    )?;
    if expected.len() != plan.preview_ids.len() {
        return Err(failure(
            "accepted association member set differs from the confirmed plan",
        ));
    }
    let selections: BTreeMap<_, _> = plan
        .selection_set
        .entries
        .iter()
        .map(|entry| (entry.preview_id.as_str(), entry))
        .collect();
    let confirmations: BTreeMap<_, _> = plan
        .identity_associations
        .iter()
        .map(|confirmation| (confirmation.preview_id.as_str(), confirmation))
        .collect();
    let mut covered = BTreeSet::new();
    let mut accepted = AcceptedMembers {
        connection: conn,
        inputs: BTreeMap::new(),
    };
    for (candidate, expected_phase) in expected {
        let (row, kinds): (i64, [String; 3]) = conn.query_row(
            "SELECT rowid,typeof(candidate_id),typeof(phase),typeof(result_json) FROM job_items WHERE job_id=?1 AND candidate_id=?2",
            rusqlite::params![job.job_id, candidate], |row| Ok((row.get(0)?, [row.get(1)?, row.get(2)?, row.get(3)?]))).map_err(failure)?;
        if kinds != ["text", "text", "text"] {
            return Err(failure("accepted association member SQL types are invalid"));
        }
        let mut phase = conn
            .blob_open(
                rusqlite::DatabaseName::Main,
                "job_items",
                "phase",
                row,
                true,
            )
            .map_err(failure)?;
        if phase.len() > 64 {
            return Err(failure(
                "accepted association member phase exceeds record bounds",
            ));
        }
        let mut actual = String::with_capacity(phase.len());
        std::io::Read::read_to_string(&mut phase, &mut actual).map_err(failure)?;
        if actual != expected_phase {
            return Err(failure("accepted association job/item phases disagree"));
        }
        drop(phase);
        // Use the existing version-dispatch codec: association v3 is bounded,
        // ordinary v2 keeps its established capacity even in a mixed plan.
        let encoded = input_reader::load(conn, &job.job_id, &candidate)?;
        let input = DurableInput::parse(&encoded)?;
        let selection = input.selection_set.single_entry()?;
        let planned = selections
            .get(selection.preview_id.as_str())
            .copied()
            .ok_or_else(|| failure("accepted input preview is outside the confirmed plan"))?;
        let confirmed = confirmations.get(selection.preview_id.as_str()).copied();
        let selection_matches = match (&input.identity_association, confirmed) {
            (Some(association), Some(confirmation)) => {
                association.require_job(job)?;
                association.validate(&input.selection_set)?;
                &association.confirmation == confirmation && selection == planned
            }
            (None, None) => {
                input.selection_set
                    == crate::session_migration::selection::SelectionSet::single(planned.clone())
                        .canonicalized()
            }
            _ => false,
        };
        if input.candidate_id != candidate
            || !selection_matches
            || !covered.insert(selection.preview_id.clone())
        {
            return Err(failure(
                "accepted input selection or identity differs from confirmed plan membership",
            ));
        }
        plan.snapshots
            .get(&selection.preview_id)
            .ok_or_else(|| failure("accepted member plan snapshot is missing"))?
            .require_metadata(&input.preview_digest, input.source_member.as_ref())?;
        if let Some(association) = &input.identity_association {
            // Exact plan confirmation/selection and snapshot were compared
            // above via indexed maps. Do not scan the whole plan per member.
            super::association_context::validate_mapping(&input, &plan)?;
            if input.explicit_reimport || serde_json::to_string(&input).map_err(failure)? != encoded
            {
                return Err(failure("accepted association member is not canonical"));
            }
            accepted
                .inputs
                .insert(candidate, (association.clone(), input.input_identity()?));
        }
    }
    if covered.len() != plan.preview_ids.len()
        || plan
            .preview_ids
            .iter()
            .any(|preview| !covered.contains(preview))
    {
        return Err(failure(
            "accepted inputs do not cover the entire confirmed plan",
        ));
    }
    Ok(accepted)
}
