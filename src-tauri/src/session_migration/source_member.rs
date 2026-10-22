//! Versioned installation-attested provenance, not source-body equivalence.
use super::errors::{io_storage_error, ErrorCode, ImportError};
use super::scan::CandidateRef;
use super::source_locations::SourceLocation;
use super::types::Provider;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, io::Write, path::Path, sync::Arc};

const VERSION: u32 = 1;
const DOMAIN: &str = "session-import-source-member-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemberReference {
    pub candidate_id: String,
    pub source_id: String,
    pub provider: Provider,
    pub host: String,
    pub location: SourceLocation,
    pub root_path: String,
    pub legacy_id: String,
    pub locator: String,
    pub source_revision: String,
    pub native_session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceMemberPin {
    pub version: u32,
    pub scan_id: String,
    pub epoch: u64,
    pub group_id: String,
    pub proof_version: u32,
    pub proof: String,
    pub reference: MemberReference,
    pub seal: String,
}

/// Durable records carry only identity metadata, never a live source handle.
/// Installation ownership is deliberately separate from the source identity.
pub(crate) struct RecordIdentity<'a> {
    pub candidate_id: &'a str,
    pub source_id: &'a str,
    pub provider: &'a str,
    pub root_path: &'a str,
    pub legacy_id: &'a str,
    pub native_session_id: &'a str,
}

pub(crate) fn validate_record(
    pin: Option<&SourceMemberPin>,
    identity: RecordIdentity<'_>,
) -> Result<(), ImportError> {
    let Some(pin) = pin else { return Ok(()) };
    pin.verify()?;
    let r = &pin.reference;
    if r.candidate_id != identity.candidate_id
        || r.source_id != identity.source_id
        || r.provider.as_str() != identity.provider
        || r.root_path != identity.root_path
        || r.legacy_id != identity.legacy_id
        || r.native_session_id != identity.native_session_id
    {
        return Err(stale("durable record differs from its fixed source member"));
    }
    Ok(())
}

fn stale(message: &str) -> ImportError {
    ImportError::new(ErrorCode::PlanStale, message)
}

impl MemberReference {
    fn capture(reference: &CandidateRef) -> Result<Self, ImportError> {
        let location = reference
            .location
            .clone()
            .ok_or_else(|| stale("source member location missing"))?;
        Ok(Self {
            candidate_id: reference.candidate.candidate_id.clone(),
            source_id: reference.source_id.clone(),
            provider: reference.candidate.provider,
            host: location.host.clone(),
            location,
            root_path: reference.root_path.clone(),
            legacy_id: reference.legacy_id.clone(),
            locator: reference.candidate.locator.clone(),
            source_revision: reference.candidate.source_revision.clone(),
            native_session_id: reference.candidate.native_session_id.clone(),
        })
    }
}

impl SourceMemberPin {
    fn payload(&self) -> Vec<u8> {
        super::hash::canonical_json(&json!({
            "version":self.version, "scanId":self.scan_id, "epoch":self.epoch,
            "groupId":self.group_id, "proofVersion":self.proof_version,
            "proof":self.proof, "reference":self.reference
        }))
        .into_bytes()
    }

    pub(crate) fn verify(&self) -> Result<(), ImportError> {
        if self.version != VERSION || self.proof_version != super::scan_group_store::SCHEMA_VERSION
        {
            return Err(ImportError::new(
                if self.version > VERSION
                    || self.proof_version > super::scan_group_store::SCHEMA_VERSION
                {
                    ErrorCode::ImportSchemaTooNew
                } else {
                    ErrorCode::SchemaUnsupported
                },
                "unsupported fixed source member version",
            ));
        }
        let r = &self.reference;
        let location = &r.location;
        if uuid::Uuid::parse_str(&self.scan_id).is_err()
            || uuid::Uuid::parse_str(&r.source_id).is_err()
            || self.epoch != 1
            || r.provider != Provider::Cursor
            || r.host != "local"
            || !r
                .candidate_id
                .starts_with(&format!("candidate:{}:", self.scan_id))
            || location.version != super::source_locations::LOCATION_REFERENCE_VERSION
            || location.source_id != r.source_id
            || location.provider != r.provider
            || location.host != r.host
            || location.root_path != r.root_path
            || !matches!(location.kind.as_str(), "primary" | "associated")
            || !matches!(self.proof.as_str(), "typed-native" | "artifact")
            || [
                &r.candidate_id,
                &r.root_path,
                &r.legacy_id,
                &r.locator,
                &r.source_revision,
                &r.native_session_id,
            ]
            .iter()
            .any(|s| s.is_empty() || s.len() > 16384)
        {
            return Err(stale("fixed source member binding is inconsistent"));
        }
        let identity = if self.proof == "typed-native" {
            &r.native_session_id
        } else {
            &r.candidate_id
        };
        let group_hash = super::hash::options_hash(&json!([
            "scan-source-group-v1",
            r.source_id,
            r.host,
            r.provider,
            self.proof,
            identity
        ]));
        if self.group_id != format!("group:{}:{group_hash}", self.scan_id) {
            return Err(stale("fixed source member group changed"));
        }
        let seal =
            hex::decode(&self.seal).map_err(|_| stale("invalid fixed source member seal"))?;
        if seal.len() != 32
            || !super::cursor::verify_installation_payload(DOMAIN, &self.payload(), &seal)?
        {
            return Err(stale("fixed source member seal mismatch"));
        }
        Ok(())
    }
}

pub(crate) fn capture(
    scan_id: &str,
    reference: &CandidateRef,
) -> Result<Option<SourceMemberPin>, ImportError> {
    // Other providers and legacy references retain their existing contracts.
    if reference.candidate.provider != Provider::Cursor || reference.location.is_none() {
        return Ok(None);
    }
    let mut pin = SourceMemberPin {
        version: VERSION,
        scan_id: scan_id.into(),
        epoch: 1,
        group_id: super::scan_group_store::group_id(scan_id, reference),
        proof_version: super::scan_group_store::SCHEMA_VERSION,
        proof: super::scan_group_store::proof(reference).into(),
        reference: MemberReference::capture(reference)?,
        seal: String::new(),
    };
    pin.seal = hex::encode(super::cursor::sign_installation_payload(
        DOMAIN,
        &pin.payload(),
    )?);
    pin.verify()?;
    Ok(Some(pin))
}

pub(crate) fn validate_reference(reference: &CandidateRef) -> Result<(), ImportError> {
    if let Some(pin) = &reference.source_member {
        pin.verify()?;
        if pin.reference != MemberReference::capture(reference)?
            || pin.proof != super::scan_group_store::proof(reference)
        {
            return Err(stale("candidate differs from its fixed source member"));
        }
    }
    Ok(())
}

pub(crate) fn validate_scan_member(
    scan_id: &str,
    epoch: u64,
    group_id: &str,
    reference: &CandidateRef,
) -> Result<(), ImportError> {
    validate_reference(reference)?;
    if reference
        .source_member
        .as_ref()
        .is_some_and(|pin| pin.scan_id != scan_id || pin.epoch != epoch || pin.group_id != group_id)
    {
        return Err(stale(
            "fixed source member belongs to another scan or group",
        ));
    }
    Ok(())
}

/// Prepared references use attested metadata, never re-resolve an offline source.
/// Comparing Option values also rejects dropping a pin from a new input.
pub(crate) fn validate_reference_lease(
    reference: &CandidateRef,
    lease: &super::jobs::PreviewLease,
) -> Result<(), ImportError> {
    validate_reference(reference)?;
    if reference.source_member != lease.source_member
        || reference.candidate.candidate_id != lease.candidate_id
        || reference.source_id != lease.source_id
        || reference.root_path != lease.root_path
        || reference.legacy_id != lease.legacy_id
        || reference.candidate.source_revision != lease.source_revision
    {
        return Err(stale(
            "prepared reference differs from fixed preview provenance",
        ));
    }
    Ok(())
}

/// Only a newly built, unshared owned preview may gain provenance. Historical
/// revisions are never rewritten, and the sealed field enters the existing hash.
pub(crate) fn seal_preview(
    prepared: &mut Arc<super::jobs::PreparedRevision>,
    pin: Option<&SourceMemberPin>,
) -> Result<(), ImportError> {
    let Some(pin) = pin else {
        return Ok(());
    };
    pin.verify()?;
    let prepared = Arc::get_mut(prepared).ok_or_else(|| stale("preview is already shared"))?;
    if !super::jobs::owned_preview_dir(&prepared.path) || prepared.path.join("lease.json").exists()
    {
        return Err(stale("source member may only seal a new owned preview"));
    }
    let path = prepared.path.join("manifest.json");
    let metadata = fs::symlink_metadata(&path).map_err(io_storage_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(stale("preview manifest is not regular"));
    }
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).map_err(io_storage_error)?)
        .map_err(|_| stale("invalid preview manifest"))?;
    let object = manifest
        .as_object_mut()
        .ok_or_else(|| stale("invalid preview manifest"))?;
    if object.contains_key("sourceMember") {
        return Err(stale("preview provenance already sealed"));
    }
    object.insert(
        "sourceMember".into(),
        serde_json::to_value(pin).map_err(|_| stale("invalid source member"))?,
    );
    let bytes = serde_json::to_vec(&manifest).map_err(|_| stale("invalid preview manifest"))?;
    let temporary = prepared
        .path
        .join(format!(".source-member-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(io_storage_error)?;
        file.write_all(&bytes).map_err(io_storage_error)?;
        file.sync_all().map_err(io_storage_error)?;
        drop(file);
        fs::rename(&temporary, &path).map_err(io_storage_error)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    prepared
        .file_hashes
        .insert("manifest.json".into(), super::hash::sha256_bytes(&bytes));
    Ok(())
}

pub(crate) fn from_manifest(
    manifest: &Value,
    provider: &str,
    native: &str,
) -> Result<Option<SourceMemberPin>, ImportError> {
    let pin: Option<SourceMemberPin> = manifest
        .get("sourceMember")
        .map(|value| {
            serde_json::from_value(value.clone())
                .map_err(|_| stale("invalid source member provenance"))
        })
        .transpose()?;
    if let Some(pin) = &pin {
        pin.verify()?;
        if pin.reference.provider.as_str() != provider || pin.reference.native_session_id != native
        {
            return Err(stale("snapshot differs from its fixed source member"));
        }
    }
    Ok(pin)
}

pub(crate) fn validate_lease(
    lease: &super::jobs::PreviewLease,
    manifest_pin: Option<&SourceMemberPin>,
) -> Result<(), ImportError> {
    if lease.source_member.as_ref() != manifest_pin {
        return Err(stale("lease source member differs from snapshot"));
    }
    if let Some(pin) = manifest_pin {
        let r = &pin.reference;
        if r.candidate_id != lease.candidate_id
            || r.source_id != lease.source_id
            || r.root_path != lease.root_path
            || r.legacy_id != lease.legacy_id
            || r.source_revision != lease.source_revision
        {
            return Err(stale("lease differs from its fixed source member"));
        }
        // No canonicalize/source read: prepared snapshots remain usable offline.
        super::source_locations::validate_registered(&r.location)?;
    }
    Ok(())
}

pub(crate) fn require_preview_pin(
    path: &Path,
    expected: Option<&SourceMemberPin>,
) -> Result<(), ImportError> {
    let lease =
        super::jobs::read_preview_lease(path)?.ok_or_else(|| stale("preview lease missing"))?;
    if lease.source_member.as_ref() != expected {
        return Err(stale("resident source member changed"));
    }
    Ok(())
}
