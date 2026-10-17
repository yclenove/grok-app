//! App-card projection and the persisted Always identity.

use crate::auth::ProcessInstance;
use crate::messages::{Identity, Policy, RequestApp};

use super::super::{sanitize_plain, timeout_secs, CardView, ImageBinding};

/// Card the App renders. Name and reason are plain text; the image is the binding.
pub(super) fn card_view(
    request_id: u64,
    request: &RequestApp,
    image: &ImageBinding,
    engine: &ProcessInstance,
    policy: &Policy,
    opened_ms: u64,
) -> CardView {
    CardView {
        request_id,
        session_id: request.session_id.clone(),
        engine_pid: engine.pid,
        engine_started: engine.started,
        policy_version: policy.version,
        image: image.clone(),
        name: sanitize_plain(&request.app.name),
        reason: request
            .reason
            .as_deref()
            .map(sanitize_plain)
            .unwrap_or_default(),
        risk: request.risk,
        timeout_secs: timeout_secs(policy.timeout_secs),
        opened_ms,
        has_icon: request.app.icon.is_some(),
        always_enabled: policy.always_allow_enabled,
    }
}

/// Remember one OS image. A second Always for the same image does not duplicate it.
pub(super) fn push_identity(policy: &mut Policy, image: &ImageBinding) {
    let identity = image.to_identity();
    if policy
        .always
        .iter()
        .any(|saved| same_identity(saved, &identity))
    {
        return;
    }
    policy.always.push(identity);
}

fn same_identity(saved: &Identity, identity: &Identity) -> bool {
    saved.platform == identity.platform
        && saved.key == identity.key
        && saved.team_id == identity.team_id
        && saved.publisher == identity.publisher
        && saved.sha256 == identity.sha256
        && saved.signature == identity.signature
        && saved.image_path == identity.image_path
}
