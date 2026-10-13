//! Host-only selected-instance receipts. No receipt is accepted from tool JSON.
use super::*;
use crate::task_consent::{NativeScopeProof, ScopeMember, ScopeSelectionPermit, MAX_SCOPE_MEMBERS};
use std::sync::Weak;

/// Private provenance binds this decision to the original Broker, owner,
/// request, picker generation, feature epoch and retained executor identity.
/// It cannot be serialized, cloned, or constructed from a target string.
pub struct HostScopeSelection {
    broker: Weak<ComputerUseBroker>,
    permit: ScopeSelectionPermit,
    members: Vec<ScopeMember>,
}

/// Synchronous authority reservation precedes any native query or worker. The
/// original command retains its cancel handle while the owned worker captures.
pub struct HostScopeSelectionCall {
    broker: Arc<ComputerUseBroker>,
    permit: ScopeSelectionPermit,
    targets: Vec<(SurfaceKind, String)>,
    completed: bool,
}

#[derive(Clone)]
pub struct HostScopeSelectionCancel {
    broker: Arc<ComputerUseBroker>,
    permit: ScopeSelectionPermit,
}

impl HostScopeSelectionCancel {
    pub(super) fn new(broker: Arc<ComputerUseBroker>, permit: ScopeSelectionPermit) -> Self {
        Self { broker, permit }
    }

    pub fn fence(&self) {
        let _change = self.broker.changes.notice();
        let runs = {
            let mut inner = self.broker.inner.lock();
            let runs = inner.consent.fence_scope_selection(&self.permit);
            self.broker.sweep_existing_share_preparation(&mut inner);
            runs
        };
        self.broker.handoff_fences(&runs);
    }
}

impl HostScopeSelectionCall {
    pub fn cancel_handle(&self) -> HostScopeSelectionCancel {
        HostScopeSelectionCancel {
            broker: self.broker.clone(),
            permit: self.permit.clone(),
        }
    }

    pub fn is_current(&self) -> bool {
        let inner = self.broker.inner.lock();
        inner
            .consent
            .recheck_scope_selection(&self.permit, inner.feature_epoch)
            .is_ok()
    }

    /// Exact identity queries only; no discovery, content read or execution.
    pub fn capture(mut self) -> Result<HostScopeSelection, BrokerError> {
        let mut members = Vec::with_capacity(self.targets.len());
        for (surface, target_id) in &self.targets {
            if !self.is_current() {
                return Err(BrokerError::StopRequested);
            }
            let executor = self.broker.require_surface_executor(*surface)?;
            let identity = executor
                .adapter
                .scope_identity(target_id)
                .map_err(BrokerError::Adapter)?;
            if identity.target_id().as_deref() != Some(target_id) {
                return Err(BrokerError::TargetUnauthorized);
            }
            if !self.is_current() {
                return Err(BrokerError::StopRequested);
            }
            members.push(ScopeMember {
                target_id: target_id.clone(),
                surface: *surface,
                display_name: None,
                native: Some(NativeScopeProof {
                    executor_generation: executor.generation,
                    identity,
                }),
            });
        }
        self.completed = true;
        Ok(HostScopeSelection {
            broker: Arc::downgrade(&self.broker),
            permit: self.permit.clone(),
            members,
        })
    }
}

impl Drop for HostScopeSelectionCall {
    fn drop(&mut self) {
        if !self.completed {
            self.cancel_handle().fence();
        }
    }
}

impl HostScopeSelection {
    pub(super) fn from_picker(
        broker: &Arc<ComputerUseBroker>,
        permit: ScopeSelectionPermit,
        members: Vec<ScopeMember>,
    ) -> Self {
        Self {
            broker: Arc::downgrade(broker),
            permit,
            members,
        }
    }

    pub fn target_id(&self) -> &str {
        &self.members[0].target_id
    }
    pub fn surface(&self) -> SurfaceKind {
        self.permit.surface
    }

    pub fn members(&self) -> &[ScopeMember] {
        &self.members
    }
}

impl ComputerUseBroker {
    /// Invoke only for a Host user selection. Captures exact instance metadata
    /// without opening a run, publishing feature state, or reading target content.
    /// Preparing the selected runtime is the Host's explicit picker transaction.
    pub fn capture_task_scope_selection(
        self: &Arc<Self>,
        token: &str,
        request_id: &str,
        revision: u64,
        surface: SurfaceKind,
        target_id: &str,
    ) -> Result<HostScopeSelection, BrokerError> {
        self.capture_task_scope_group(token, request_id, revision, &[(surface, target_id)])
    }

    /// A single picker decision freezes the entire group. Refreshing any
    /// selection retires the previous group, including its inactive members.
    pub fn capture_task_scope_group(
        self: &Arc<Self>,
        token: &str,
        request_id: &str,
        revision: u64,
        targets: &[(SurfaceKind, &str)],
    ) -> Result<HostScopeSelection, BrokerError> {
        self.begin_task_scope_group(token, request_id, revision, targets)?
            .capture()
    }

    pub fn begin_task_scope_group(
        self: &Arc<Self>,
        token: &str,
        request_id: &str,
        revision: u64,
        targets: &[(SurfaceKind, &str)],
    ) -> Result<HostScopeSelectionCall, BrokerError> {
        let _change = self.changes.notice();
        if !(1..=MAX_SCOPE_MEMBERS).contains(&targets.len())
            || targets
                .iter()
                .any(|(surface, target)| SurfaceRouter::classify_target_id(target) != *surface)
            || targets
                .iter()
                .map(|(_, target)| *target)
                .collect::<std::collections::HashSet<_>>()
                .len()
                != targets.len()
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        let surface = targets[0].0;
        let ctx = self.live_context(token)?;
        let permit = {
            let mut inner = self.inner.lock();
            let epoch = inner.feature_epoch;
            for &(surface, _) in targets {
                inner
                    .consent
                    .check_selection_surface(&ctx, request_id, revision, surface)?;
            }
            inner
                .consent
                .begin_scope_selection(&ctx, request_id, revision, surface, epoch)?
        };
        Ok(HostScopeSelectionCall {
            broker: self.clone(),
            permit,
            targets: targets
                .iter()
                .map(|(surface, target)| (*surface, (*target).into()))
                .collect(),
            completed: false,
        })
    }

    /// Idempotent Host retry must repeat the same member set, never silently
    /// approve a changed selection. No adapter or native work happens here.
    pub fn committed_scope_selection(
        &self,
        token: &str,
        request_id: &str,
        revision: u64,
        targets: &[(SurfaceKind, &str)],
    ) -> Result<Option<crate::task_consent::CommitOutcome>, BrokerError> {
        let ctx = self.live_context(token)?;
        let inner = self.inner.lock();
        inner
            .consent
            .committed_selection_outcome(&ctx, request_id, revision, targets)
    }

    /// Selection and reservation are one authority-lock transaction. A second
    /// picker cannot retarget the selected decision in the intervening gap.
    pub fn reserve_selected_access_approval(
        self: &Arc<Self>,
        token: &str,
        selection: HostScopeSelection,
    ) -> Result<AccessApprovalReady, BrokerError> {
        let _change = self.changes.notice();
        if !selection
            .broker
            .upgrade()
            .is_some_and(|broker| Arc::ptr_eq(&broker, self))
        {
            return Err(BrokerError::IdentityMismatch("scope broker"));
        }
        let ctx = self.live_context(token)?;
        if ctx != selection.permit.context {
            return Err(BrokerError::IdentityMismatch("scope owner"));
        }
        {
            let inner = self.inner.lock();
            inner
                .consent
                .recheck_scope_selection(&selection.permit, inner.feature_epoch)?;
        }
        #[cfg(any(test, feature = "test-support"))]
        self.run_scope_reservation_hook();
        for member in &selection.members {
            {
                let inner = self.inner.lock();
                inner
                    .consent
                    .recheck_scope_selection(&selection.permit, inner.feature_epoch)?;
            }
            self.recheck_scope_members(std::slice::from_ref(member))?;
        }
        let mut inner = self.inner.lock();
        let enabled = inner.feature_enabled;
        let epoch = inner.feature_epoch;
        inner
            .consent
            .bind_host_scope_selection(&selection.permit, selection.members, epoch)?;
        let ready = inner.consent.start_preparation(
            &ctx,
            &selection.permit.request_id,
            selection.permit.revision,
            enabled,
            epoch,
        )?;
        Ok(self.wrap_access_approval(ready, enabled))
    }

    pub(super) fn recheck_scope_proof(
        &self,
        target_id: &str,
        surface: SurfaceKind,
        proof: &NativeScopeProof,
    ) -> Result<(), BrokerError> {
        let executor = self.require_surface_executor(surface)?;
        if executor.generation != proof.executor_generation
            || proof.identity.target_id().as_deref() != Some(target_id)
        {
            return Err(BrokerError::IdentityMismatch("scope executor"));
        }
        if let crate::adapter::NativeScopeIdentity::WebViewSelected { identity } = &proof.identity {
            if surface != SurfaceKind::WebView {
                return Err(BrokerError::TargetUnauthorized);
            }
            return identity
                .receipt
                .require_current()
                .map_err(BrokerError::Adapter);
        }
        let current = executor
            .adapter
            .scope_identity(target_id)
            .map_err(BrokerError::Adapter)?;
        if current != proof.identity {
            return Err(BrokerError::DeadTarget);
        }
        Ok(())
    }

    pub(super) fn recheck_prepared_scope(
        &self,
        ticket: &crate::task_consent::PrepareTicket,
    ) -> Result<(), BrokerError> {
        self.recheck_scope_members(&ticket.members)
    }

    /// Retained metadata only, called with the Broker authority gate held.
    /// Executor registrations are immutable. Existing shares may change, so
    /// their Host registry lock must span validation AND the final commit.
    pub(super) fn with_prepared_scope_authority<T>(
        &self,
        ticket: &crate::task_consent::PrepareTicket,
        commit: impl FnOnce() -> Result<T, BrokerError>,
    ) -> Result<T, BrokerError> {
        self.with_scope_members_authority(
            &ticket.session_id,
            &ticket.run_id,
            &ticket.members,
            commit,
        )
    }

    pub(super) fn with_scope_members_authority<T>(
        &self,
        session_id: &str,
        run_id: &str,
        members: &[ScopeMember],
        commit: impl FnOnce() -> Result<T, BrokerError>,
    ) -> Result<T, BrokerError> {
        let mut existing = Vec::new();
        let mut webview = Vec::new();
        for member in members {
            if let Some(proof) = &member.native {
                let executor = self.require_surface_executor(member.surface)?;
                if executor.generation != proof.executor_generation
                    || proof.identity.target_id().as_deref() != Some(&member.target_id)
                {
                    return Err(BrokerError::IdentityMismatch("scope executor"));
                }
                if matches!(
                    proof.identity,
                    crate::adapter::NativeScopeIdentity::WebViewSelected { .. }
                ) {
                    return Err(BrokerError::TargetUnauthorized);
                }
                if let crate::adapter::NativeScopeIdentity::ExistingPage { identity } =
                    &proof.identity
                {
                    if member.surface != SurfaceKind::ExistingTab {
                        return Err(BrokerError::TargetUnauthorized);
                    }
                    existing.push(identity.as_ref());
                }
                if let crate::adapter::NativeScopeIdentity::WebViewPage {
                    session_id: bound_session,
                    run_id: bound_run,
                    ..
                } = &proof.identity
                {
                    if member.surface != SurfaceKind::WebView
                        || bound_session != session_id
                        || bound_run != run_id
                    {
                        return Err(BrokerError::IdentityMismatch("WebView task binding"));
                    }
                    webview.push(proof.identity.clone());
                }
            }
        }
        let commit = || {
            if existing.is_empty() {
                commit()
            } else {
                self.tabs.with_existing_scope_authority(&existing, commit)
            }
        };
        if webview.is_empty() {
            return commit();
        }
        let executor = self.require_surface_executor(SurfaceKind::WebView)?;
        let mut original = Some(commit);
        let mut outcome = None;
        // Lock order: Broker → App binding → original native documents →
        // existing shares → authority-only finalize. Never wait for UI work.
        executor.adapter.with_scope_authority(&webview, &mut || {
            let commit = original
                .take()
                .ok_or(BrokerError::IdentityMismatch("scope commit repeated"))?;
            outcome = Some(commit()?);
            Ok(())
        })?;
        outcome.ok_or(BrokerError::IdentityMismatch("scope commit missing"))
    }

    pub(super) fn recheck_scope_members(&self, members: &[ScopeMember]) -> Result<(), BrokerError> {
        for member in members {
            if let Some(proof) = &member.native {
                self.recheck_scope_proof(&member.target_id, member.surface, proof)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests_existing;
#[cfg(test)]
mod tests_webview;

#[cfg(any(test, feature = "test-support"))]
mod reservation_probe;
