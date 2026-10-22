//! Proof row versions are independent of store schema1/input3/receipt2.
//! Explicit dispatch; no untagged fallback and no new wrapper on v1 bytes.
use super::{bounded, digest_shape, refusal, InputIdentity, Proof};
use crate::session_migration::{
    errors::ImportError, hash, job_input_association::AssociationInput,
};
use serde::{Deserialize, Serialize, Serializer};

#[path = "registration_association_old_owner.rs"]
pub(in crate::session_migration) mod old_owner;

#[derive(Debug, Clone)]
pub(in crate::session_migration) enum ProofRecord {
    Legacy(Box<Proof>),
    Association(Box<AssociationProof>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::session_migration) struct AssociationProof {
    version: u32,
    pub(in crate::session_migration) import_id: String,
    original_installation: String,
    original_provider: String,
    original_native: String,
    original_locator: String,
    original_normalizer_version: String,
    expected_revision: String,
    anchor_receipt: String,
    anchor_digest: String,
    receipt_cutoff: i64,
    host: String,
    native_family: String,
    current_normalizer_version: String,
    association: AssociationInput,
    input: InputIdentity,
}

fn wire_error() -> ImportError {
    ImportError::new(
        crate::session_migration::errors::ErrorCode::SchemaUnsupported,
        "registration provenance record is invalid or unsupported",
    )
    .job_local()
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

impl AssociationProof {
    /// Exact original tuple selector only; callers still authenticate the
    /// accepted proof closure before deriving a negative publication fact.
    pub(in crate::session_migration) fn original_tuple_digest(
        &self,
    ) -> Result<String, ImportError> {
        self.validate_shape()?;
        Ok(hash::options_hash(&serde_json::json!([
            self.original_installation,
            self.original_provider,
            self.original_native
        ])))
    }

    pub(super) fn validate_shape(&self) -> Result<(), ImportError> {
        self.association.confirmation.validate()?;
        let import = crate::session_migration::ids::parse_import_uuid(&self.import_id)
            .map_err(|_| refusal())?;
        let current_native = self
            .input
            .native_session_id
            .strip_prefix("cursor-identity-v1:transcript-locator:");
        if self.version != 2
            || self.import_id != format!("imp:{import}")
            || self.host != "local"
            || self.original_provider != "cursor"
            || self.input.provider != self.original_provider
            || self.original_installation != self.input.source_id
            || !canonical_uuid(&self.original_installation)
            || !canonical_uuid(&self.expected_revision)
            || !canonical_uuid(&self.anchor_receipt)
            || !canonical_uuid(&self.association.job_id)
            || !canonical_uuid(&self.association.plan_id)
            || self.association.request_id.is_empty()
            || self.association.confirmation.import_id != self.import_id
            || self.association.confirmation.expected_revision != self.expected_revision
            || !digest_shape(&self.association.plan_hash)
            || !digest_shape(&self.anchor_digest)
            || !digest_shape(&self.input.digest)
            || self.receipt_cutoff <= 0
            || self.native_family != "cursor-transcript-jsonl-v1"
            || self.original_normalizer_version != "1.0.0"
            || self.current_normalizer_version != "1.0.6"
            || !bounded(&self.original_native, 4096)
            || !bounded(&self.input.candidate_id, 4096)
            || self.original_native == self.input.native_session_id
            || !bounded(&self.original_locator, 8192)
            || !bounded(&self.input.legacy_id, 8192)
            || !self.original_locator.starts_with("cursor-transcript:")
            || !self.input.legacy_id.starts_with("cursor-transcript:")
            || !current_native.is_some_and(digest_shape)
            || !crate::session_migration::sources::locator_spellings(&self.original_locator)
                .iter()
                .any(|old| {
                    crate::session_migration::sources::locator_spellings(&self.input.legacy_id)
                        .contains(old)
                })
        {
            return Err(refusal());
        }
        Ok(())
    }

    fn digest(&self) -> Result<String, ImportError> {
        Ok(hash::options_hash(
            &serde_json::to_value(self).map_err(|_| refusal())?,
        ))
    }

    /// Protocol evidence only, until a real atomic accepted producer exists.
    #[cfg(test)]
    pub(in crate::session_migration) fn draft(
        witness: &crate::session_migration::identity_association::InspectionWitness,
        association: AssociationInput,
        input: InputIdentity,
        job: &crate::session_migration::jobs::JobRecord,
        encoded: &str,
    ) -> Result<Self, ImportError> {
        association.require_job(job)?;
        let verified =
            crate::session_migration::job_control::association_acceptance::inspect_encoded(
                &crate::session_migration::catalog::open_catalog()?,
                encoded,
                job,
            )?;
        if &association != verified.association() || &input != verified.identity() {
            return Err(refusal());
        }
        Self::authenticated(witness, &verified)
    }

    /// Production construction consumes a non-serializable verified input,
    /// never caller-provided summaries or the test-only draft constructor.
    pub(in crate::session_migration) fn authenticated(
        witness: &crate::session_migration::identity_association::InspectionWitness,
        verified: &crate::session_migration::job_control::association_acceptance::VerifiedInput,
    ) -> Result<Self, ImportError> {
        verified.require_witness(witness)?;
        if witness.anchor_row > witness.receipt_cutoff {
            return Err(refusal());
        }
        let association = verified.association().clone();
        let input = verified.identity().clone();
        let result = Self {
            version: 2,
            import_id: witness.entry.import_id.clone(),
            original_installation: witness.entry.installation_id.clone(),
            original_provider: witness.entry.provider.clone(),
            original_native: witness.entry.native_session_id.clone(),
            original_locator: witness.receipt.legacy_id.clone(),
            original_normalizer_version: witness.old_normalizer.clone(),
            expected_revision: witness.expected_revision.clone(),
            anchor_receipt: witness.receipt.receipt_id.clone(),
            anchor_digest: super::receipt_digest(&witness.receipt)?,
            receipt_cutoff: witness.receipt_cutoff,
            host: "local".into(),
            native_family: "cursor-transcript-jsonl-v1".into(),
            current_normalizer_version: crate::session_migration::types::NORMALIZER_VERSION.into(),
            association,
            input,
        };
        result.validate_shape()?;
        Ok(result)
    }

    pub(in crate::session_migration) fn verify_accepted_input(
        &self,
        conn: &rusqlite::Connection,
    ) -> Result<(), ImportError> {
        self.verify_accepted_digest(conn)?;
        let verified =
            crate::session_migration::job_control::association_acceptance::accepted_input(
                conn,
                &self.association,
                &self.input.candidate_id,
            )?;
        if verified.identity() != &self.input {
            return Err(refusal());
        }
        Ok(())
    }

    fn verify_accepted_digest(&self, conn: &rusqlite::Connection) -> Result<(), ImportError> {
        self.validate_shape()?;
        // A checksum-correct edited proof must not replace the producer's
        // original accepted facts (cutoff/anchor included). This SQL witness
        // is atomically inserted with the proof, entry, job and inputs; it is
        // neither the recovery receipt nor a reader grant on its own.
        let digest = crate::session_migration::catalog::retention_metadata_text(
            conn,
            &self.acceptance_key(),
            64,
        )?;
        if digest.as_deref() != Some(self.digest()?.as_str()) {
            return Err(refusal());
        }
        Ok(())
    }

    /// Candidate selection only: parsed proof intent is never job-read authority.
    pub(in crate::session_migration) fn selects_job(&self, job_id: &str) -> bool {
        self.association.job_id == job_id
    }

    pub(in crate::session_migration) fn job_members<'snapshot>(
        &self,
        conn: &'snapshot rusqlite::Connection,
    ) -> Result<
        crate::session_migration::job_control::association_members::AcceptedMembers<'snapshot>,
        ImportError,
    > {
        self.verify_accepted_digest(conn)?;
        let job = self
            .association
            .stored_job(conn, &self.association.job_id)?;
        crate::session_migration::job_control::association_members::verify_job(
            conn,
            &job,
            &self.input.candidate_id,
        )
    }

    /// One complete member scan per entry, even with several accepted proofs.
    pub(in crate::session_migration) fn verify_job_member(
        &self,
        conn: &rusqlite::Connection,
        members: &crate::session_migration::job_control::association_members::AcceptedMembers<'_>,
    ) -> Result<(), ImportError> {
        self.verify_accepted_digest(conn)?;
        members.require(conn, &self.association, &self.input)
    }

    fn acceptance_key(&self) -> String {
        format!(
            "registration_association_accepted:{}:{}",
            self.association.job_id,
            hash::sha256_bytes(self.input.candidate_id.as_bytes())
        )
    }

    /// Retention must authenticate the exact row it is classifying, not another
    /// valid input which happens to name the same imported entry.
    pub(in crate::session_migration) fn verify_retained_input(
        &self,
        conn: &rusqlite::Connection,
        association: &AssociationInput,
        input: &InputIdentity,
    ) -> Result<(), ImportError> {
        self.verify_accepted_input(conn)?;
        if &self.association != association || &self.input != input {
            return Err(refusal());
        }
        Ok(())
    }

    pub(in crate::session_migration) fn record_acceptance(
        &self,
        conn: &rusqlite::Connection,
    ) -> Result<(), ImportError> {
        if conn.is_autocommit() {
            return Err(refusal());
        }
        self.validate_shape()?;
        conn.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2)",
            [self.acceptance_key(), self.digest()?],
        )
        .map_err(crate::session_migration::errors::sqlite_storage_error)?;
        old_owner::record(conn, self)
    }

    pub(super) fn authorize(
        &self,
        conn: &rusqlite::Connection,
        entry: &crate::session_migration::catalog::CatalogEntry,
        receipt: &crate::session_migration::jobs::Receipt,
    ) -> Result<(), ImportError> {
        use crate::session_migration::registration_provenance::receipt_store;
        self.verify_accepted_input(conn)?;
        if receipt.import_id.as_deref() != Some(entry.import_id.as_str())
            || entry.import_id != self.import_id
            || entry.installation_id != self.input.source_id
            || entry.provider != self.input.provider
            || entry.native_session_id != self.input.native_session_id
            || receipt.provider != self.original_provider
        {
            return Err(refusal());
        }
        self.require_anchor(conn)?;
        let (row, actual) = receipt_store::point(conn, &receipt.receipt_id)?;
        if actual.phase != "committed"
            || super::receipt_digest(&actual)? != super::receipt_digest(receipt)?
        {
            return Err(refusal());
        }
        if row <= self.receipt_cutoff {
            if receipt.installation_id != self.original_installation
                || receipt.native_session_id != self.original_native
                || !crate::session_migration::sources::locator_spellings(&receipt.legacy_id)
                    .contains(&self.original_locator)
            {
                return Err(refusal());
            }
        } else if receipt.installation_id != self.input.source_id
            || receipt.native_session_id != self.input.native_session_id
        {
            return Err(refusal());
        }
        Ok(())
    }

    fn require_anchor(&self, conn: &rusqlite::Connection) -> Result<(), ImportError> {
        use crate::session_migration::{catalog, registration_provenance::receipt_store};
        let (anchor_row, anchor) = receipt_store::point(conn, &self.anchor_receipt)?;
        let (_, cutoff) = receipt_store::read_row(conn, self.receipt_cutoff)?;
        let revision = catalog::get_revision(conn, &self.import_id, &self.expected_revision)?
            .ok_or_else(refusal)?;
        if anchor_row > self.receipt_cutoff
            || anchor.phase != "committed"
            || anchor.import_id.as_deref() != Some(self.import_id.as_str())
            || anchor.revision_id.as_deref() != Some(self.expected_revision.as_str())
            || anchor.installation_id != self.original_installation
            || anchor.provider != self.original_provider
            || anchor.native_session_id != self.original_native
            || anchor.legacy_id != self.original_locator
            || super::receipt_digest(&anchor)? != self.anchor_digest
            || revision.source_snapshot_id != anchor.snapshot_hash
            || cutoff.import_id.as_deref() != Some(self.import_id.as_str())
            || cutoff.installation_id != self.original_installation
            || cutoff.provider != self.original_provider
            || cutoff.native_session_id != self.original_native
        {
            return Err(refusal());
        }
        Ok(())
    }

    /// Only accepted publication may use this target. Intent, a parsed proof,
    /// or a receipt alone cannot select or allocate a replacement entry.
    pub(in crate::session_migration) fn publication_target(
        &self,
        conn: &rusqlite::Connection,
        receipt: Option<&crate::session_migration::jobs::Receipt>,
    ) -> Result<crate::session_migration::catalog::CatalogEntry, ImportError> {
        use crate::session_migration::{catalog, registration_provenance::receipt_store};
        if conn.is_autocommit() {
            let tx = conn
                .unchecked_transaction()
                .map_err(super::sqlite_storage_error)?;
            return self.publication_target(&tx, receipt);
        }
        self.verify_accepted_input(conn)?;
        self.require_anchor(conn)?;
        super::registered(conn, &self.input)?;
        let entry = catalog::get_entry(conn, &self.import_id)?.ok_or_else(refusal)?;
        if entry.hidden
            || entry.installation_id != self.input.source_id
            || entry.provider != self.input.provider
            || entry.native_session_id != self.input.native_session_id
            || catalog::is_tombstoned(
                conn,
                &self.original_installation,
                &self.original_provider,
                &self.original_native,
            )?
            || catalog::is_tombstoned(
                conn,
                &self.input.source_id,
                &self.input.provider,
                &self.input.native_session_id,
            )?
        {
            return Err(refusal());
        }
        if let Some(receipt) = receipt {
            if receipt.import_id.as_deref() != Some(self.import_id.as_str())
                || receipt.previous_revision.as_deref() != Some(self.expected_revision.as_str())
                || receipt.created_entry
                || receipt.job_id != self.association.job_id
                || receipt.candidate_id != self.input.candidate_id
                || receipt.installation_id != self.input.source_id
                || receipt.provider != self.input.provider
                || receipt.native_session_id != self.input.native_session_id
            {
                return Err(refusal());
            }
            let (_, actual) = receipt_store::point(conn, &receipt.receipt_id)?;
            if super::receipt_digest(&actual)? != super::receipt_digest(receipt)? {
                return Err(refusal());
            }
            if receipt.phase == "committed" {
                self.authorize(conn, &entry, receipt)?;
                return Ok(entry);
            }
        }
        // Completed recovery can keep a legitimate later display revision;
        // pending publication must still CAS the originally confirmed revision.
        if entry.current_revision.as_deref() != Some(self.expected_revision.as_str()) {
            return Err(crate::session_migration::errors::ImportError::new(
                crate::session_migration::errors::ErrorCode::MappingConflict,
                "association target has a later or missing revision",
            ));
        }
        Ok(entry)
    }
}

impl Serialize for ProofRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Legacy(proof) => proof.serialize(serializer),
            Self::Association(proof) => proof.serialize(serializer),
        }
    }
}

impl ProofRecord {
    pub(in crate::session_migration) fn validate_shape(&self) -> Result<(), ImportError> {
        match self {
            Self::Legacy(proof) => proof.validate_shape(),
            Self::Association(proof) => proof.validate_shape(),
        }
    }
    pub(in crate::session_migration) fn parse(json: &str) -> Result<Self, ImportError> {
        if json.len() > 32768 {
            return Err(wire_error());
        }
        #[derive(Deserialize)]
        struct Header {
            version: u32,
        }
        // The streaming-compatible serde header rejects duplicate version;
        // each selected record then rejects all unknown/duplicate body fields.
        let header: Header = serde_json::from_str(json).map_err(|_| wire_error())?;
        let result = match header.version {
            1 => Self::Legacy(Box::new(
                serde_json::from_str(json).map_err(|_| wire_error())?,
            )),
            2 => Self::Association(Box::new(
                serde_json::from_str(json).map_err(|_| wire_error())?,
            )),
            _ => return Err(wire_error()),
        };
        match &result {
            Self::Legacy(proof) => proof.validate_shape()?,
            Self::Association(proof) => proof.validate_shape()?,
        }
        Ok(result)
    }

    pub(in crate::session_migration) fn import_id(&self) -> &str {
        match self {
            Self::Legacy(proof) => &proof.import_id,
            Self::Association(proof) => &proof.import_id,
        }
    }

    pub(in crate::session_migration) fn selects_job(&self, job_id: &str) -> bool {
        self.job_id() == job_id
    }

    pub(in crate::session_migration) fn job_id(&self) -> &str {
        match self {
            Self::Legacy(proof) => &proof.job_id,
            Self::Association(proof) => &proof.association.job_id,
        }
    }

    pub(in crate::session_migration) fn digest(&self) -> Result<String, ImportError> {
        match self {
            Self::Legacy(proof) => proof.digest(),
            Self::Association(proof) => proof.digest(),
        }
    }
}
