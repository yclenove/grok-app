//! Observation identity checks shared by CLI preflight and final admission.
use super::*;
use crate::protocol::{ActionKind, ActionRequest, ActionTarget};

pub(super) fn validate_action_observation(
    run: &RunState,
    request: &ActionRequest,
) -> Result<(), BrokerError> {
    if run.last_unknown {
        return Err(BrokerError::Schema(
            "unknown result must observe before the next action".into(),
        ));
    }
    if (matches!(request.target, ActionTarget::Coord { .. }) || request.action == ActionKind::Drag)
        && !run.last_image_ok
    {
        return Err(BrokerError::Schema(
            "coordinate action requires a visual observation".into(),
        ));
    }
    let target = run.target.as_ref().ok_or(BrokerError::TargetUnauthorized)?;
    if target.target_id != request.target_id {
        return Err(BrokerError::IdentityMismatch("targetId"));
    }
    if target.target_generation != request.target_generation {
        return Err(BrokerError::IdentityMismatch("targetGeneration"));
    }
    if run.snapshot_id.as_deref() != Some(request.snapshot_id.as_str()) {
        return Err(BrokerError::IdentityMismatch("snapshotId"));
    }
    if run.geometry_revision != request.geometry_revision {
        return Err(BrokerError::IdentityMismatch("geometryRevision"));
    }
    match &request.target {
        ActionTarget::Element { element_ref } if !run.element_refs.contains(element_ref) => {
            return Err(BrokerError::IdentityMismatch("elementRef"))
        }
        ActionTarget::Coord { x, y }
            if *x < 0.0
                || *y < 0.0
                || *x >= run.image_size.0 as f64
                || *y >= run.image_size.1 as f64 =>
        {
            return Err(BrokerError::Schema(
                "coordinates outside observed image".into(),
            ))
        }
        _ => {}
    }
    if request.action == ActionKind::Drag {
        let (x, y) =
            crate::protocol::drag_destination(&request.parameters).map_err(BrokerError::Schema)?;
        if x >= run.image_size.0 as f64 || y >= run.image_size.1 as f64 {
            return Err(BrokerError::Schema(
                "drag destination outside observed image".into(),
            ));
        }
    }
    Ok(())
}
