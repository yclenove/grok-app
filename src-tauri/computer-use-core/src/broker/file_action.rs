//! Retained file-field identity and observation retirement shared by file actions.
use super::*;

pub(super) fn require_file_ref(
    run: &RunState,
    target_id: &str,
    target_generation: u64,
    snapshot_id: &str,
    element_ref: &str,
) -> Result<(), BrokerError> {
    if run.last_unknown {
        return Err(BrokerError::Schema(
            "unknown result must observe before the next action".into(),
        ));
    }
    let target = run.target.as_ref().ok_or(BrokerError::TargetUnauthorized)?;
    if target.target_id != target_id || target.target_generation != target_generation {
        return Err(BrokerError::IdentityMismatch("targetGeneration"));
    }
    if run.snapshot_id.as_deref() != Some(snapshot_id) {
        return Err(BrokerError::IdentityMismatch("snapshotId"));
    }
    if !run.element_refs.contains(element_ref) {
        return Err(BrokerError::IdentityMismatch("elementRef"));
    }
    Ok(())
}

pub(super) fn invalidate_observation(run: &mut RunState) {
    run.snapshot_id = None;
    run.geometry_revision = 0;
    run.geometry_epoch = 0;
    run.last_image_ok = false;
    run.image_size = (0, 0);
    run.element_refs.clear();
    run.element_boxes.clear();
    run.upload_refs.clear();
}
