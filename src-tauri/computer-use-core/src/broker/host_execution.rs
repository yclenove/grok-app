//! Exact Host UI controls. Correlation is checked at actual run admission.
use super::*;
use crate::task_consent::{HostRequestKey, TransportContext};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostExecutionAddress {
    pub key: HostRequestKey,
    pub run_id: String,
    /// Canonical decimal u64; renderer numbers cannot preserve this identity.
    pub generation: String,
}

pub(super) fn require_execution(
    inner: &Inner,
    ctx: &TransportContext,
    address: &HostExecutionAddress,
) -> Result<(), BrokerError> {
    require_execution_identity(inner, ctx, address)?;
    super::admit_consent(
        &inner.consent,
        inner.feature_enabled,
        inner.feature_epoch,
        &inner.runs[&address.run_id],
    )
}

fn require_execution_identity<'a>(
    inner: &'a Inner,
    ctx: &TransportContext,
    address: &HostExecutionAddress,
) -> Result<&'a crate::task_consent::ConsentBinding, BrokerError> {
    inner.consent.require_host_request(ctx, &address.key)?;
    if inner.consent.host_current_task(ctx)?.as_deref() != Some(address.key.task_id.as_str()) {
        return Err(BrokerError::IdentityMismatch("task"));
    }
    let run = inner
        .runs
        .get(&address.run_id)
        .ok_or(BrokerError::RunNotFound)?;
    let ConsentGate::Bound(binding) = &run.consent else {
        return Err(BrokerError::TargetUnauthorized);
    };
    if run.app_session_id != ctx.app_session_id
        || address.generation != run.generation.to_string()
        || inner
            .consent
            .host_execution_key(ctx, &address.run_id, Some(binding))
            .as_ref()
            != Some(&address.key)
    {
        return Err(BrokerError::IdentityMismatch("execution"));
    }
    Ok(binding)
}

impl ComputerUseBroker {
    /// Publish only a native failure from this exact admitted generation.
    /// Metadata reads never query liveness or interpret error/trace strings.
    pub(super) fn record_closed_target(&self, run_id: &str, generation: u64, target_id: &str) {
        let _change = self.changes.notice();
        let mut inner = self.inner.lock();
        let Some(run) = inner.runs.get_mut(run_id) else {
            return;
        };
        if run.generation != generation || run.stop != StopState::Running {
            return;
        }
        let Some(target) = run.target.as_mut() else {
            return;
        };
        if target.target_id == target_id {
            target.closed = true;
            run.last_image_ok = false;
        }
    }

    pub fn pause_host_execution(
        &self,
        token: &str,
        address: &HostExecutionAddress,
    ) -> Result<(), BrokerError> {
        let ctx = self.live_context(token)?;
        // The identity check and pause fence share the very same Broker lock.
        self.pause_due_to_checked(&address.run_id, "user", |inner| {
            let binding = require_execution_identity(inner, &ctx, address)?;
            let run = &inner.runs[&address.run_id];
            if run.target.as_ref().is_some_and(|target| {
                target.surface == SurfaceKind::ExistingTab && target.target_id == binding.target_id
            }) {
                // Pause fences authority; it is not permission to execute.
                // An interrupted switch must not prevent the original Host
                // from pausing while its physical cleanup is still pending.
                return inner.consent.recheck_pause_binding(
                    binding,
                    inner.feature_enabled,
                    inner.feature_epoch,
                );
            }
            require_execution(inner, &ctx, address)
        })
    }

    pub fn preview_host_execution(
        &self,
        token: &str,
        address: &HostExecutionAddress,
    ) -> Result<Observation, BrokerError> {
        let ctx = self.live_context(token)?;
        self.capture(
            &address.run_id,
            false,
            true,
            None,
            None,
            Some((&ctx, address)),
        )
    }
}
