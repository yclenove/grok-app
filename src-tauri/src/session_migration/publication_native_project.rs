//! Opaque verified allocation for the native store, not a catalog writer grant.
//! The store seals provenance only when it atomically adds a new project row.
use super::*;
use crate::store::Project;

const DOMAIN: &str = "session-import-native-project-creation-v1";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Origin {
    version: u32,
    binding_digest: String,
    allocation_digest: String,
    project_id: String,
    path_digest: String,
    snapshot_digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OriginEnvelope {
    body: Origin,
    mac: String,
}

/// Contains loaded crypto and exact signed allocation context only. It cannot
/// authorize catalog changes or substitute for acceptance/final-result checks.
pub(crate) struct ProjectCreation<'crypto> {
    auth: &'crypto InstallationAuthenticator,
    binding_digest: String,
    allocation_digest: String,
    project_id: String,
    path_digest: String,
}

impl VerifiedRecord {
    pub(crate) fn project_creation<'crypto>(
        &self,
        auth: &'crypto InstallationAuthenticator,
    ) -> Result<ProjectCreation<'crypto>, ImportError> {
        self.body.validate()?;
        verify(
            auth,
            self.encoded.as_bytes(),
            &self.body.binding,
            StageKind::Allocated,
        )?;
        // Only the new protocol allocates native producer identities. Ordinary
        // historical records cannot be upgraded into project ownership.
        crate::session_migration::publication_protocol::PublicationProtocol::SignedOutcomeV1
            .require_job_id(&self.body.binding.job_id)?;
        let Stage::Allocated(record::Allocation {
            project:
                record::ProjectAllocation::Create {
                    project_id,
                    path_digest,
                    ..
                },
            ..
        }) = &self.body.stage
        else {
            return Err(invalid());
        };
        Ok(ProjectCreation {
            auth,
            binding_digest: sha256_bytes(canonical(&self.body.binding)?.as_bytes()),
            allocation_digest: self.digest.clone(),
            project_id: project_id.clone(),
            path_digest: path_digest.clone(),
        })
    }
}

pub(super) fn snapshot_digest(project: &Project) -> Result<String, ImportError> {
    let mut value = serde_json::to_value(project).map_err(|_| invalid())?;
    // These two fields are derived by normal open/health checks. All user
    // settings and the exact saved identity/path stay part of creation origin.
    let object = value.as_object_mut().ok_or_else(invalid)?;
    object.remove("lastOpenedAt");
    object.remove("pathOk");
    Ok(sha256_bytes(
        crate::session_migration::hash::canonical_json(&value).as_bytes(),
    ))
}

impl ProjectCreation<'_> {
    pub(crate) fn project_id(&self) -> &str {
        &self.project_id
    }

    pub(crate) fn require_path(&self, path: &str) -> Result<(), ImportError> {
        if sha256_bytes(path.as_bytes()) != self.path_digest {
            return Err(invalid());
        }
        Ok(())
    }

    /// Requires the native snapshot's private, checked new-row capability.
    pub(crate) fn seal_new(
        &self,
        new_project: crate::store::NewImportProject<'_>,
    ) -> Result<String, ImportError> {
        let project = new_project.project();
        if project.id != self.project_id {
            return Err(invalid());
        }
        self.require_path(&project.path)?;
        let body = Origin {
            version: 1,
            binding_digest: self.binding_digest.clone(),
            allocation_digest: self.allocation_digest.clone(),
            project_id: self.project_id.clone(),
            path_digest: self.path_digest.clone(),
            snapshot_digest: snapshot_digest(project)?,
        };
        let encoded = canonical(&OriginEnvelope {
            mac: hex::encode(self.auth.sign(DOMAIN, canonical(&body)?.as_bytes())),
            body,
        })?;
        if encoded.len() > RECORD_BYTES {
            return Err(invalid());
        }
        Ok(encoded)
    }

    /// A valid other producer is a match, never a recovered create. Corrupt or
    /// future origin is an error; neither case allows overwriting that row.
    pub(crate) fn owns(&self, encoded: &str, project: &Project) -> Result<bool, ImportError> {
        if encoded.len() > RECORD_BYTES || encoded.as_bytes().starts_with(&[0xef, 0xbb, 0xbf]) {
            return Err(invalid());
        }
        let envelope: OriginEnvelope = serde_json::from_str(encoded).map_err(|_| invalid())?;
        let body = &envelope.body;
        if body.version != 1 {
            return Err(ImportError::new(
                if body.version > 1 {
                    ErrorCode::ImportSchemaTooNew
                } else {
                    ErrorCode::SchemaUnsupported
                },
                "unsupported native project creation schema",
            ));
        }
        for digest in [
            &body.binding_digest,
            &body.allocation_digest,
            &body.path_digest,
            &body.snapshot_digest,
            &envelope.mac,
        ] {
            record::digest(digest)?;
        }
        let mac = hex::decode(&envelope.mac).map_err(|_| invalid())?;
        if !self.auth.verify(DOMAIN, canonical(body)?.as_bytes(), &mac)
            || body.project_id != project.id
            || body.path_digest != sha256_bytes(project.path.as_bytes())
        {
            return Err(invalid());
        }
        Ok(body.project_id == self.project_id
            && body.binding_digest == self.binding_digest
            && body.allocation_digest == self.allocation_digest
            && body.path_digest == self.path_digest
            && body.snapshot_digest == snapshot_digest(project)?)
    }
}
