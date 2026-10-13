//! Host UI metadata. These records contain no transport credential or native proof.
use serde::{Deserialize, Serialize};

use super::types::{RequestRecord, RequestState};
use super::{ConsentBinding, ConsentState, TransportContext};
use crate::error::BrokerError;

/// Correlation only. Every decision still checks the original live transport.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostRequestKey {
    pub connection_id: String,
    pub task_id: String,
    pub request_id: String,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostRequestPhase {
    Pending,
    Preparing,
    Approved,
    Denied,
    Cancelled,
    Expired,
    Retired,
    Completed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostRequestView {
    pub key: HostRequestKey,
    pub phase: HostRequestPhase,
    pub purpose: String,
    pub scope_intent: String,
    pub surfaces: Vec<String>,
    pub capabilities: Vec<String>,
    /// Monotonic Host deadline, expressed as a duration for display only.
    pub remaining_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostScopeMemberView {
    pub target_id: String,
    pub surface: String,
    /// Retained display metadata; this is neither an identity nor a live title.
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostGrantView {
    pub key: HostRequestKey,
    pub run_id: Option<String>,
    pub members: Vec<HostScopeMemberView>,
    pub capabilities: Vec<String>,
    pub suspended: bool,
    pub executable: bool,
}

pub(crate) fn connection_id(ctx: &TransportContext) -> String {
    format!(
        "{}:{}:{}",
        ctx.owner_nonce, ctx.incarnation, ctx.connection_generation
    )
}

impl ConsentState {
    /// Only bounded authority records may contribute execution rows. Retained
    /// Broker history cannot expand UI readback or adopt a legacy run.
    pub(crate) fn host_execution_key(
        &self,
        ctx: &TransportContext,
        run_id: &str,
        binding: Option<&ConsentBinding>,
    ) -> Option<HostRequestKey> {
        let (request_id, task_id, revision) = if let Some(binding) = binding {
            if binding.app_session_id != ctx.app_session_id
                || binding.owner_nonce != ctx.owner_nonce
                || binding.incarnation != ctx.incarnation
                || binding.connection_generation != ctx.connection_generation
                || ctx.agent_session_id.as_deref() != Some(&binding.agent_session_id)
                || binding.run_id != run_id
            {
                return None;
            }
            (
                &binding.request_id,
                &binding.task_id,
                binding.grant_revision,
            )
        } else {
            let prep = self.preparations.iter().find(|prep| {
                prep.run_id == run_id
                    && prep.opened_run
                    && !prep.rolled_back
                    && prep.app_session_id == ctx.app_session_id
                    && prep.owner_nonce == ctx.owner_nonce
                    && prep.incarnation == ctx.incarnation
                    && prep.connection_generation == ctx.connection_generation
                    && ctx.agent_session_id.as_deref() == Some(&prep.agent_session_id)
            })?;
            (&prep.request_id, &prep.task_id, prep.revision)
        };
        let key = HostRequestKey {
            connection_id: connection_id(ctx),
            task_id: task_id.clone(),
            request_id: request_id.clone(),
            revision,
        };
        self.require_host_request(ctx, &key).ok().map(|()| key)
    }

    pub(crate) fn host_current_task(
        &self,
        ctx: &TransportContext,
    ) -> Result<Option<String>, BrokerError> {
        self.require_model_owner(ctx)?;
        Ok(self.owners[&ctx.app_session_id].task_id.clone())
    }

    pub(crate) fn host_request_view(
        &self,
        ctx: &TransportContext,
    ) -> Result<Option<HostRequestView>, BrokerError> {
        self.require_model_owner(ctx)?;
        Ok(self.latest_request(ctx).map(|request| HostRequestView {
            key: request_key(ctx, request),
            phase: match request.state {
                RequestState::Awaiting if request.selecting => HostRequestPhase::Preparing,
                RequestState::Awaiting => HostRequestPhase::Pending,
                RequestState::Approving => HostRequestPhase::Preparing,
                RequestState::Approved => HostRequestPhase::Approved,
                RequestState::Denied => HostRequestPhase::Denied,
                RequestState::Cancelled => HostRequestPhase::Cancelled,
                RequestState::Expired => HostRequestPhase::Expired,
                RequestState::Retired => HostRequestPhase::Retired,
                RequestState::Completed => HostRequestPhase::Completed,
            },
            purpose: request.purpose.clone(),
            scope_intent: request.scope_intent.clone(),
            surfaces: request
                .surfaces
                .iter()
                .map(|surface| surface.as_wire().into())
                .collect(),
            capabilities: request
                .capabilities
                .iter()
                .map(|cap| cap.as_str().into())
                .collect(),
            remaining_ms: if request.state.is_pending() {
                request
                    .deadline
                    .saturating_sub(self.clock.now())
                    .as_millis() as u64
            } else {
                0
            },
        }))
    }

    pub(crate) fn host_grant_views(
        &self,
        ctx: &TransportContext,
        enabled: bool,
        epoch: u64,
    ) -> Vec<HostGrantView> {
        self.grants
            .iter()
            .filter(|grant| {
                grant.app_session_id == ctx.app_session_id
                    && grant.owner_nonce == ctx.owner_nonce
                    && grant.incarnation == ctx.incarnation
                    && grant.connection_generation == ctx.connection_generation
                    && ctx.agent_session_id.as_deref() == Some(&grant.agent_session_id)
            })
            .map(|grant| HostGrantView {
                key: HostRequestKey {
                    connection_id: connection_id(ctx),
                    task_id: grant.task_id.clone(),
                    request_id: grant.request_id.clone(),
                    revision: grant.revision,
                },
                run_id: grant.execution_run_id.clone(),
                members: grant
                    .members
                    .iter()
                    .map(|member| HostScopeMemberView {
                        target_id: member.target_id.clone(),
                        surface: member.surface.as_wire().into(),
                        display_name: member.display_name.clone(),
                    })
                    .collect(),
                capabilities: grant
                    .capabilities
                    .iter()
                    .map(|cap| cap.as_str().into())
                    .collect(),
                suspended: grant.suspended,
                executable: enabled && grant.feature_epoch == epoch && self.grant_executable(grant),
            })
            .collect()
    }

    pub(crate) fn require_host_request(
        &self,
        ctx: &TransportContext,
        key: &HostRequestKey,
    ) -> Result<(), BrokerError> {
        self.require_model_owner(ctx)?;
        if key.connection_id != connection_id(ctx)
            || !self.requests.iter().any(|request| {
                request_key(ctx, request) == *key
                    && request.app_session_id == ctx.app_session_id
                    && request.owner_nonce == ctx.owner_nonce
                    && request.incarnation == ctx.incarnation
                    && request.connection_generation == ctx.connection_generation
            })
        {
            return Err(BrokerError::IdentityMismatch("request"));
        }
        Ok(())
    }

    pub(crate) fn stop_host_task(
        &mut self,
        ctx: &TransportContext,
        key: &HostRequestKey,
    ) -> Result<Vec<String>, BrokerError> {
        self.require_host_request(ctx, key)?;
        if self.owners[&ctx.app_session_id]
            .task_id
            .as_ref()
            .is_some_and(|task| task != &key.task_id)
        {
            return Err(BrokerError::IdentityMismatch("task"));
        }
        // A duplicate Stop has no authority to stop a future task. New requests
        // allocate a new task id and are rejected by the check above.
        Ok(self.stop_owner(&ctx.app_session_id))
    }
}

fn request_key(ctx: &TransportContext, request: &RequestRecord) -> HostRequestKey {
    HostRequestKey {
        connection_id: connection_id(ctx),
        task_id: request.task_id.clone(),
        request_id: request.request_id.clone(),
        revision: request.revision,
    }
}
