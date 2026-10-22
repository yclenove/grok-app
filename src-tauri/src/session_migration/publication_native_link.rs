//! Actual native dispositions, never receipt.created_project or caller booleans.
use super::{record::ProjectResult, *};
use crate::{
    session_migration::jobs::Receipt,
    store::{self, import_publication_projects as native},
};

pub(super) fn planned(allocation: &Allocation, receipt: &mut Receipt) {
    receipt.created_project = false;
    let (id, decision) = match &allocation.project {
        ProjectAllocation::Create { project_id, .. } => (Some(project_id.clone()), "create"),
        ProjectAllocation::Match { project_id, .. } => (Some(project_id.clone()), "match"),
        ProjectAllocation::Unassigned {} => (None, "unassigned"),
    };
    receipt.mapping_project_id = id;
    receipt.project_decision = Some(decision.into());
    receipt.project_name_at_link = None;
    receipt.project_path_at_link = None;
    receipt.project_pinned_at_link = None;
    receipt.project_trusted_at_link = None;
}

pub(super) fn require_planned(
    allocation: &Allocation,
    receipt: &Receipt,
) -> Result<(), ImportError> {
    let mut expected = receipt.clone();
    planned(allocation, &mut expected);
    if super::receipt::digest(&expected)? != super::receipt::digest(receipt)? {
        return Err(invalid());
    }
    Ok(())
}

fn project(
    receipt: &mut Receipt,
    project: &store::Project,
    created: bool,
) -> Result<ProjectResult, ImportError> {
    receipt.mapping_project_id = Some(project.id.clone());
    receipt.created_project = created;
    receipt.project_decision = Some(if created { "create" } else { "match" }.into());
    receipt.project_name_at_link = Some(project.name.clone());
    receipt.project_path_at_link = Some(project.path.clone());
    receipt.project_pinned_at_link = Some(project.pinned);
    receipt.project_trusted_at_link = Some(project.trusted);
    let path_digest = sha256_bytes(project.path.as_bytes());
    let snapshot_digest = native_project::snapshot_digest(project)?;
    Ok(if created {
        ProjectResult::Created {
            project_id: project.id.clone(),
            path_digest,
            snapshot_digest,
        }
    } else {
        ProjectResult::Matched {
            project_id: project.id.clone(),
            path_digest,
            snapshot_digest,
        }
    })
}

fn unassigned(receipt: &mut Receipt, reported: bool) -> ProjectResult {
    receipt.mapping_project_id = None;
    receipt.created_project = false;
    receipt.project_decision = Some(
        if reported {
            "unassigned-reported"
        } else {
            "unassigned"
        }
        .into(),
    );
    receipt.project_name_at_link = None;
    receipt.project_path_at_link = None;
    receipt.project_pinned_at_link = None;
    receipt.project_trusted_at_link = None;
    if reported {
        ProjectResult::UnassignedReported {}
    } else {
        ProjectResult::Unassigned {}
    }
}

pub(super) fn apply(
    allocated: &VerifiedRecord,
    auth: &InstallationAuthenticator,
    path: &str,
    receipt: &mut Receipt,
) -> Result<ProjectResult, ImportError> {
    let allocation = allocated.allocation()?;
    require_planned(allocation, receipt)?;
    let result = match &allocation.project {
        ProjectAllocation::Unassigned {} => unassigned(receipt, false),
        ProjectAllocation::Match {
            project_id,
            path_digest,
            allow_unassigned,
        } => {
            let selected = store::load_projects_snapshot()
                .map_err(|message| ImportError::new(ErrorCode::SnapshotUnavailable, message))?
                .into_iter()
                .find(|project| &project.id == project_id);
            match selected {
                Some(selected) if sha256_bytes(selected.path.as_bytes()) == *path_digest => {
                    project(receipt, &selected, false)?
                }
                _ if *allow_unassigned => unassigned(receipt, true),
                _ => {
                    return Err(ImportError::new(
                        ErrorCode::MappingConflict,
                        "accepted project match changed or disappeared",
                    ))
                }
            }
        }
        ProjectAllocation::Create {
            allow_unassigned,
            path_digest: target_digest,
            ..
        } => match native::ensure(&allocated.project_creation(auth)?, path) {
            Ok(ensured) => {
                let result = project(
                    receipt,
                    &ensured.project,
                    ensured.disposition != native::Disposition::Matched,
                )?;
                match result {
                    ProjectResult::Matched {
                        project_id,
                        path_digest,
                        snapshot_digest,
                    } if &path_digest != target_digest => ProjectResult::MatchedAlias {
                        project_id,
                        path_digest,
                        snapshot_digest,
                        target_path_digest: target_digest.clone(),
                    },
                    result => result,
                }
            }
            Err(native::EnsureError::Target(_)) if *allow_unassigned => unassigned(receipt, true),
            Err(native::EnsureError::Target(message)) => {
                return Err(ImportError::new(ErrorCode::MappingConflict, message))
            }
            Err(native::EnsureError::Provenance(error)) => return Err(error),
            Err(native::EnsureError::Store(message)) => {
                return Err(ImportError::new(ErrorCode::SnapshotUnavailable, message))
            }
        },
    };
    require_result(allocation, receipt, &result)?;
    Ok(result)
}

/// A signed result cannot disagree with the complete receipt it binds. This
/// checks saved fields only; it never probes today's mutable native store.
pub(super) fn require_result(
    allocation: &Allocation,
    receipt: &Receipt,
    result: &ProjectResult,
) -> Result<(), ImportError> {
    allocation.project.permits(result)?;
    match result {
        ProjectResult::Unassigned {} | ProjectResult::UnassignedReported {} => {
            let mut expected = receipt.clone();
            unassigned(
                &mut expected,
                matches!(result, ProjectResult::UnassignedReported {}),
            );
            if super::receipt::digest(&expected)? != super::receipt::digest(receipt)? {
                return Err(invalid());
            }
        }
        ProjectResult::Created {
            project_id,
            path_digest,
            ..
        }
        | ProjectResult::Matched {
            project_id,
            path_digest,
            ..
        }
        | ProjectResult::MatchedAlias {
            project_id,
            path_digest,
            ..
        } => {
            let created = matches!(result, ProjectResult::Created { .. });
            if receipt.mapping_project_id.as_deref() != Some(project_id.as_str())
                || receipt.created_project != created
                || receipt.project_decision.as_deref()
                    != Some(if created { "create" } else { "match" })
                || !receipt
                    .project_path_at_link
                    .as_ref()
                    .is_some_and(|path| sha256_bytes(path.as_bytes()) == *path_digest)
                || receipt.project_name_at_link.is_none()
                || receipt.project_pinned_at_link.is_none()
                || receipt.project_trusted_at_link.is_none()
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}
