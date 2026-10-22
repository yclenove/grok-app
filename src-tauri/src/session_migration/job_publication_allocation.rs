//! Private pre-writer allocation intents. No UUID is consumed by a store here.
use std::collections::BTreeMap;

use rusqlite::Connection;

use super::*;
use crate::session_migration::{
    catalog::{self, CatalogEntry},
    job_control::{association_acceptance::Identity, DurableInput},
    job_input_association::AssociationInput,
    publication_outcome::{Allocation, PreparedAllocation, ProjectAllocation},
    registration_provenance::{InputIdentity, Proof, ProofRecord},
    registration_provenance_store,
};

struct Target {
    installation: String,
    provider: String,
    native: String,
    association: Option<AssociationInput>,
    adoption: Option<Box<Proof>>,
    identity: InputIdentity,
}

impl Target {
    fn capture(input: &DurableInput, accepted: &Identity) -> Result<Self, ImportError> {
        let identity = input.input_identity()?;
        let adoption = match accepted {
            Identity::Legacy(original, proof)
                if input.identity_association.is_none() && original == &identity =>
            {
                if let Some(proof) = proof {
                    proof.require_input(&identity)?;
                }
                proof.clone()
            }
            Identity::Association(_, _, verified)
                if input.identity_association.as_ref() == Some(verified.association())
                    && verified.identity() == &identity =>
            {
                None
            }
            _ => return Err(invalid()),
        };
        Ok(Self {
            installation: input.installation_id.clone(),
            provider: input.provider.clone(),
            native: input.adapter.graph.native_session_id.clone(),
            association: input.identity_association.clone(),
            adoption,
            identity,
        })
    }

    fn selected(&self, conn: &Connection) -> Result<Option<CatalogEntry>, ImportError> {
        if let Some(proof) = &self.adoption {
            // The genuine preflight already authenticated the old archive.
            // Adoption changes its tuple, not its import/revision allocation.
            return catalog::get_entry(conn, &proof.import_id);
        }
        match &self.association {
            Some(association) => catalog::get_entry(conn, &association.confirmation.import_id),
            None => catalog::find_entry(conn, &self.installation, &self.provider, &self.native),
        }
    }

    fn current(&self, conn: &Connection) -> Result<Option<CatalogEntry>, ImportError> {
        if let Some(proof) = &self.adoption {
            return proof
                .accepted_allocation_target(conn, &self.identity)
                .map(Some);
        }
        let Some(association) = &self.association else {
            return self.selected(conn);
        };
        let Some(ProofRecord::Association(proof)) =
            registration_provenance_store::get(conn, &association.confirmation.import_id)?
        else {
            return Err(invalid());
        };
        proof.verify_retained_input(conn, association, &self.identity)?;
        proof.publication_target(conn, None).map(Some)
    }
}

struct Intent {
    target: Target,
    previous: Option<(String, Option<String>)>,
    prepared: PreparedAllocation,
}

pub(super) struct PreparedAllocations {
    intents: BTreeMap<String, Intent>,
}

fn previous(entry: &CatalogEntry) -> (String, Option<String>) {
    (entry.import_id.clone(), entry.current_revision.clone())
}

impl PreparedAllocations {
    pub(super) fn capture(
        conn: &Connection,
        inputs: &[(
            DurableInput,
            std::sync::Arc<crate::session_migration::jobs::PreparedRevision>,
        )],
        identities: &[Identity],
    ) -> Result<Self, ImportError> {
        if !conn.is_autocommit() || inputs.len() != identities.len() {
            return Err(invalid());
        }
        // Native reads belong to preflight, never the acceptance writer. Only
        // explicit matches need the current saved path; creates bind the target.
        let projects = if inputs
            .iter()
            .any(|(input, _)| !input.create_project && input.mapping_project_id.is_some())
        {
            crate::store::load_projects_snapshot()
                .map_err(|message| ImportError::new(ErrorCode::SnapshotUnavailable, message))?
        } else {
            vec![]
        };
        let mut intents = BTreeMap::new();
        for ((input, _), identity) in inputs.iter().zip(identities) {
            // A create allocates its own ID. A caller-supplied match ID cannot
            // be silently discarded or upgraded into newly created ownership.
            if input.create_project && input.mapping_project_id.is_some() {
                return Err(ImportError::new(
                    ErrorCode::MappingConflict,
                    "project creation cannot also select an existing project",
                ));
            }
            let target = Target::capture(input, identity)?;
            let existing = target.selected(conn)?;
            if (target.association.is_some() || target.adoption.is_some()) && existing.is_none() {
                return Err(invalid());
            }
            let path = input
                .explicit_target
                .as_deref()
                .filter(|path| !path.is_empty())
                .unwrap_or(&input.root_path);
            let project = if input.create_project {
                ProjectAllocation::Create {
                    project_id: uuid::Uuid::new_v4().to_string(),
                    path_digest: hash::sha256_bytes(path.as_bytes()),
                    allow_unassigned: input.import_unassigned_anyway,
                }
            } else if let Some(id) = &input.mapping_project_id {
                let project = projects.iter().find(|project| &project.id == id);
                if project.is_none() && !input.import_unassigned_anyway {
                    return Err(ImportError::new(
                        ErrorCode::MappingConflict,
                        "selected project is unavailable before acceptance",
                    ));
                }
                ProjectAllocation::Match {
                    project_id: id.clone(),
                    path_digest: hash::sha256_bytes(
                        project.map_or(path, |project| &project.path).as_bytes(),
                    ),
                    allow_unassigned: input.import_unassigned_anyway,
                }
            } else {
                ProjectAllocation::Unassigned {}
            };
            let allocation = Allocation {
                receipt_id: uuid::Uuid::new_v4().to_string(),
                import_id: existing
                    .as_ref()
                    .map(|entry| entry.import_id.clone())
                    .unwrap_or_else(crate::session_migration::ids::new_import_id),
                revision_id: uuid::Uuid::new_v4().to_string(),
                previous_revision: existing
                    .as_ref()
                    .and_then(|entry| entry.current_revision.clone()),
                created_entry: existing.is_none(),
                project,
            };
            let intent = Intent {
                target,
                previous: existing.as_ref().map(previous),
                prepared: PreparedAllocation::new(allocation)?,
            };
            if intents.insert(input.candidate_id.clone(), intent).is_some() {
                return Err(invalid());
            }
        }
        Ok(Self { intents })
    }

    pub(super) fn identity(&self, candidate: &str) -> Result<&str, ImportError> {
        Ok(self
            .intents
            .get(candidate)
            .ok_or_else(invalid)?
            .prepared
            .identity())
    }

    pub(super) fn require_current(&self, conn: &Connection) -> Result<(), ImportError> {
        if conn.is_autocommit() {
            return Err(invalid());
        }
        for intent in self.intents.values() {
            if intent.target.current(conn)?.as_ref().map(previous) != intent.previous {
                return Err(ImportError::new(
                    ErrorCode::MappingConflict,
                    "publication target changed before allocation acceptance",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn record(
        &self,
        tx: &rusqlite::Transaction<'_>,
        auth: &InstallationAuthenticator,
        bindings: Vec<(
            String,
            crate::session_migration::publication_outcome::Binding,
        )>,
    ) -> Result<(), ImportError> {
        for (candidate, binding) in bindings {
            self.intents
                .get(&candidate)
                .ok_or_else(invalid)?
                .prepared
                .record(tx, auth, binding)?;
        }
        Ok(())
    }
}
