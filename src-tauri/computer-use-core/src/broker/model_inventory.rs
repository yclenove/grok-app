//! Inventory reads exact approved instance metadata, never ambient discovery.
use super::*;
use crate::task_consent::ExecutionPermit;
use serde_json::{json, Value};

impl ComputerUseBroker {
    /// Compatibility browser inventory still uses the exact task member set,
    /// not every tab/profile that happens to be retained by this Host run.
    pub(crate) fn task_tab_ids_for_permit(
        &self,
        permit: &ExecutionPermit,
    ) -> Result<std::collections::HashSet<String>, BrokerError> {
        let inner = self.inner.lock();
        Self::recheck_scope_call_locked(&inner, permit)?;
        Ok(inner
            .consent
            .scope_members_for_permit(permit)
            .into_iter()
            .filter(|member| {
                matches!(
                    member.surface,
                    SurfaceKind::ManagedBrowser | SurfaceKind::ExistingTab
                )
            })
            .map(|member| member.target_id)
            .collect())
    }

    pub(super) fn recheck_scope_call_locked(
        inner: &Inner,
        permit: &ExecutionPermit,
    ) -> Result<(), BrokerError> {
        inner
            .consent
            .recheck_permit(permit, inner.feature_enabled, inner.feature_epoch)?;
        let run = inner
            .runs
            .get(&permit.run_id)
            .ok_or(BrokerError::RunNotFound)?;
        if run.stop != StopState::Running {
            return Err(BrokerError::StopRequested);
        }
        if run.generation != permit.execution_generation {
            return Err(BrokerError::IdentityMismatch("execution generation"));
        }
        match &run.consent {
            ConsentGate::Bound(binding)
                if binding.owner_nonce == permit.owner_nonce
                    && binding.request_id == permit.request_id
                    && binding.task_id == permit.task_id
                    && binding.grant_revision == permit.grant_revision
                    && binding.feature_epoch == permit.feature_epoch =>
            {
                Ok(())
            }
            _ => Err(BrokerError::TargetUnauthorized),
        }
    }

    pub(super) fn recheck_scope_call(
        &self,
        permit: &ExecutionPermit,
    ) -> Result<crate::task_consent::ScopeMember, BrokerError> {
        let member = {
            let inner = self.inner.lock();
            Self::recheck_scope_call_locked(&inner, permit)?;
            inner
                .consent
                .scope_member_for_permit(permit)
                .ok_or(BrokerError::TargetUnauthorized)?
        };
        self.recheck_scope_members(std::slice::from_ref(&member))?;
        Self::recheck_scope_call_locked(&self.inner.lock(), permit)?;
        Ok(member)
    }

    pub fn approved_task_targets(
        &self,
        token: &str,
        args: &Value,
    ) -> Result<Vec<Value>, BrokerError> {
        let options = args
            .as_object()
            .ok_or_else(|| BrokerError::Schema("invalid inventory options".into()))?;
        if options
            .keys()
            .any(|key| !matches!(key.as_str(), "runId" | "run_id" | "targetId" | "target_id"))
        {
            return Err(BrokerError::Schema("invalid inventory options".into()));
        }
        let claim = crate::tools_claim::execution_claim(args)?;
        let context = self.live_context(token)?;
        let bindings = {
            let inner = self.inner.lock();
            inner
                .consent
                .readable_bindings(&context, inner.feature_enabled, inner.feature_epoch)?
        };
        let selected = bindings
            .into_iter()
            .filter(|binding| {
                claim.run_id.is_none_or(|run| binding.run_id == run)
                    && claim
                        .target_id
                        .is_none_or(|target| binding.target_id == target)
            })
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return Err(BrokerError::TargetUnauthorized);
        }
        let mut permits = Vec::with_capacity(selected.len());
        let mut rows = Vec::with_capacity(selected.len());
        for binding in selected {
            let permit = self.begin_model_call_claiming(
                token,
                "computer_list_targets",
                Some(&binding.run_id),
                Some(&binding.target_id),
            )?;
            let member = self.recheck_scope_call(&permit)?;
            let executor = self.require_surface_executor(member.surface)?;
            let target = self
                .inner
                .lock()
                .runs
                .get(&permit.run_id)
                .filter(|run| !run.switch_in_flight && !run.switch_cleanup_pending && !run.paused)
                .and_then(|run| run.target.clone());
            let active = target
                .as_ref()
                .filter(|target| target.target_id == member.target_id);
            let info = if member.native.is_some() {
                Some(self.scope_member_info(&permit.run_id, &member, || {
                    Self::recheck_scope_call_locked(&self.inner.lock(), &permit)
                })?)
            } else {
                None
            };
            // An inactive approved share is deliberately not borrowed yet.
            // Its retained native proof was checked above; runtime liveness
            // would incorrectly hide it merely because it is not active.
            if (member.surface == SurfaceKind::ExistingTab && member.native.is_some())
                || executor.adapter.target_alive(&member.target_id)
            {
                rows.push(json!({"targetId":member.target_id,
                    "title":info.as_ref().map(|row| row.title.as_str())
                        .or_else(|| active.map(|target| target.title.as_str())).unwrap_or(&member.target_id),
                    "runId":permit.run_id,"surface":member.surface.as_wire(),
                    "active":active.is_some(),"targetGeneration":active.map(|target| target.target_generation)}));
            }
            self.recheck_scope_call(&permit)?;
            permits.push(permit);
        }
        // The final fence covers earlier rows when a later callback switches,
        // stops, or replaces an owner. No partially stale response is published.
        let inner = self.inner.lock();
        for permit in permits {
            Self::recheck_scope_call_locked(&inner, &permit)?;
        }
        Ok(rows)
    }
}
