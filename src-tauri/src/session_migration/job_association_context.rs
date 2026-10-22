//! Shared retained-plan intent checks; these are never an acceptance grant.
use super::DurableInput;
use crate::session_migration::{
    engine::PlanState,
    errors::{ErrorCode, ImportError},
    jobs::JobRecord,
    mapping_scope,
};

pub(super) fn validate(
    input: &DurableInput,
    job: &JobRecord,
    plan: &PlanState,
) -> Result<(), ImportError> {
    let Some(association) = &input.identity_association else {
        return Ok(());
    };
    association.require_job(job)?;
    association.require_plan(plan, &input.selection_set)?;
    let snapshot = plan
        .snapshots
        .get(&association.confirmation.preview_id)
        .ok_or_else(|| {
            ImportError::new(ErrorCode::PlanStale, "association plan snapshot is missing")
        })?;
    snapshot.require_metadata(&input.preview_digest, input.source_member.as_ref())?;
    validate_mapping(input, plan)
}

pub(super) fn validate_mapping(input: &DurableInput, plan: &PlanState) -> Result<(), ImportError> {
    let subject = mapping_scope::MappingSubject::new(
        input.source_id.clone(),
        input.provider.clone(),
        input.adapter.graph.cwd.clone(),
    );
    let row = mapping_scope::resolve(&plan.mappings, &subject)?;
    if input.options != mapping_scope::options(row)
        || input.mapping_project_id != row.and_then(|m| m.target_project_id.clone())
        || input.create_project != row.is_some_and(|m| m.create_project)
        || input.import_unassigned_anyway != row.is_some_and(|m| m.import_unassigned_anyway)
        || input.explicit_target != row.and_then(|m| m.explicit_target.clone())
    {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "association input mapping differs from its plan",
        ));
    }
    Ok(())
}
