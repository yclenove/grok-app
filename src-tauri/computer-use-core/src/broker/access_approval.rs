//! A Host reservation separates persistence, execution preparation, and commit.
//! It cannot be serialized into a model tool call and does not admit execution.
use super::*;
use crate::task_consent::{AbortEffect, BearerClass, CommitOutcome, CommitReady, PrepareTicket};
use crate::task_consent::{ManagedResourceReady, ManagedResourceTicket, WebViewSelectionTicket};

pub(super) enum ApprovalTicket {
    Bound(PrepareTicket),
    Managed(ManagedResourceTicket),
    WebViewSelection(WebViewSelectionTicket),
}

impl ApprovalTicket {
    fn id(&self) -> u64 {
        match self {
            Self::Bound(ticket) => ticket.id,
            Self::Managed(ticket) => ticket.id,
            Self::WebViewSelection(ticket) => ticket.id,
        }
    }
    fn surface(&self) -> SurfaceKind {
        match self {
            Self::Bound(ticket) => ticket.surface,
            Self::Managed(_) => SurfaceKind::ManagedBrowser,
            Self::WebViewSelection(_) => SurfaceKind::WebView,
        }
    }
    fn bound(&self) -> Result<&PrepareTicket, BrokerError> {
        match self {
            Self::Bound(ticket) => Ok(ticket),
            Self::Managed(_) | Self::WebViewSelection(_) => Err(BrokerError::TargetUnauthorized),
        }
    }
}

pub enum AccessApprovalReady {
    Already(CommitOutcome),
    Prepare(AccessApproval),
}

pub struct AccessApproval {
    broker: Arc<ComputerUseBroker>,
    pub(super) ticket: ApprovalTicket,
    initially_enabled: bool,
    published: bool,
    execution_prepared: Option<AuthorizedTarget>,
    publication_prepared: bool,
    completed: bool,
    webview_bindings: Vec<String>,
}

/// Cancellation fences the original reservation without discarding ownership
/// of a native preparation that may still return a late run.
pub struct AccessApprovalCancel {
    broker: Arc<ComputerUseBroker>,
    id: u64,
}

impl ComputerUseBroker {
    /// Host UI entry for one isolated browser; its full request address is not
    /// a model-provided profile, run, or target.
    pub fn reserve_host_managed_access_approval(
        self: &Arc<Self>,
        token: &str,
        key: &crate::task_consent::HostRequestKey,
    ) -> Result<AccessApprovalReady, BrokerError> {
        self.reserve_managed_approval(token, &key.request_id, key.revision, Some(key))
    }

    /// Explicit Host choice of one isolated task browser and its initial tab.
    /// This is not a model tool and accepts no caller-provided profile/run id.
    pub fn reserve_managed_access_approval(
        self: &Arc<Self>,
        token: &str,
        request_id: &str,
        revision: u64,
    ) -> Result<AccessApprovalReady, BrokerError> {
        self.reserve_managed_approval(token, request_id, revision, None)
    }

    fn reserve_managed_approval(
        self: &Arc<Self>,
        token: &str,
        request_id: &str,
        revision: u64,
        host_key: Option<&crate::task_consent::HostRequestKey>,
    ) -> Result<AccessApprovalReady, BrokerError> {
        let _change = self.changes.notice();
        let mut inner = self.inner.lock();
        let ctx = match inner.consent.classify(token) {
            BearerClass::Live(ctx) => ctx,
            _ => return Err(BrokerError::IdentityMismatch("transport")),
        };
        if let Some(key) = host_key {
            // Verify all four address fields under the same authority lock as
            // resource reservation. A separate readback check would race task
            // or transport replacement and could consume the wrong decision.
            inner.consent.require_host_request(&ctx, key)?;
        }
        let enabled = inner.feature_enabled;
        let epoch = inner.feature_epoch;
        match inner
            .consent
            .start_managed_resources(&ctx, request_id, revision, enabled, epoch)?
        {
            ManagedResourceReady::Already(outcome) => Ok(AccessApprovalReady::Already(outcome)),
            ManagedResourceReady::Prepare(ticket) => Ok(AccessApprovalReady::Prepare(
                AccessApproval::new(self.clone(), ApprovalTicket::Managed(ticket), enabled),
            )),
        }
    }

    /// A successful explicit enable supersedes preparation, without retiring
    /// already committed task grants or reusing their feature epoch.
    pub fn supersede_access_approvals_for_explicit_enable(&self) {
        let _change = self.changes.notice();
        self.inner
            .lock()
            .consent
            .supersede_preparations_for_explicit_enable();
    }

    /// Only the Host's retained transport may reserve an approval. The MCP
    /// request handler never calls this entry point.
    pub fn reserve_access_approval(
        self: &Arc<Self>,
        token: &str,
        request_id: &str,
        revision: u64,
    ) -> Result<AccessApprovalReady, BrokerError> {
        let _change = self.changes.notice();
        let mut inner = self.inner.lock();
        let ctx = match inner.consent.classify(token) {
            BearerClass::Live(ctx) => ctx,
            _ => return Err(BrokerError::IdentityMismatch("transport")),
        };
        let enabled = inner.feature_enabled;
        let epoch = inner.feature_epoch;
        let ready = inner
            .consent
            .start_preparation(&ctx, request_id, revision, enabled, epoch)?;
        Ok(self.wrap_access_approval(ready, enabled))
    }

    pub(super) fn wrap_access_approval(
        self: &Arc<Self>,
        ready: CommitReady,
        enabled: bool,
    ) -> AccessApprovalReady {
        match ready {
            CommitReady::Already(outcome) => AccessApprovalReady::Already(outcome),
            CommitReady::Prepare(ticket) => AccessApprovalReady::Prepare(AccessApproval::new(
                self.clone(),
                ApprovalTicket::Bound(ticket),
                enabled,
            )),
            CommitReady::WebViewSelection(ticket) => {
                AccessApprovalReady::Prepare(AccessApproval::new(
                    self.clone(),
                    ApprovalTicket::WebViewSelection(ticket),
                    enabled,
                ))
            }
        }
    }
}

impl AccessApproval {
    fn new(
        broker: Arc<ComputerUseBroker>,
        ticket: ApprovalTicket,
        initially_enabled: bool,
    ) -> Self {
        Self {
            broker,
            ticket,
            initially_enabled,
            published: false,
            execution_prepared: None,
            publication_prepared: false,
            completed: false,
            webview_bindings: Vec::new(),
        }
    }
    /// Frozen by the Host reservation, never inferred from model target text.
    pub fn surface(&self) -> SurfaceKind {
        self.ticket.surface()
    }

    pub fn cancel_handle(&self) -> AccessApprovalCancel {
        AccessApprovalCancel {
            broker: Arc::clone(&self.broker),
            id: self.ticket.id(),
        }
    }

    pub fn initially_enabled(&self) -> bool {
        self.initially_enabled
    }

    pub fn is_current(&self) -> bool {
        !self.completed && self.broker.preparation_current(self.ticket.id())
    }

    /// Call only after the preference write succeeds under the Host feature
    /// transition lock. No grant or continuation is issued here.
    pub fn publish_feature(&mut self) -> Result<(), BrokerError> {
        self.broker.publish_preparation(self.ticket.id())?;
        self.published = true;
        Ok(())
    }

    /// May block in an adapter. Stop and feature-off remain free to fence the
    /// reservation; a late result is never enough to establish a grant.
    pub fn prepare_execution(&mut self) -> Result<(), BrokerError> {
        self.publication_prepared = false;
        if !self.published || !self.is_current() {
            return Err(BrokerError::StopRequested);
        }
        if let ApprovalTicket::Managed(ticket) = &self.ticket {
            let bound = self.broker.prepare_managed_task_scope(ticket)?;
            self.ticket = ApprovalTicket::Bound(bound);
        }
        if let ApprovalTicket::WebViewSelection(ticket) = &self.ticket {
            let bound = self.broker.prepare_webview_task_scope(ticket)?;
            self.webview_bindings = bound
                .members
                .iter()
                .map(|member| member.target_id.clone())
                .collect();
            self.ticket = ApprovalTicket::Bound(bound);
        }
        let ticket = self.ticket.bound()?;
        let prepared = self
            .broker
            .bind_granted_target(ticket, self.broker.run_exists(&ticket.run_id))?;
        if !self.is_current() {
            return Err(BrokerError::StopRequested);
        }
        self.execution_prepared = Some(prepared);
        Ok(())
    }

    /// May query a native adapter. Run outside every App/ACP publication lock.
    pub fn prepare_publication(&mut self) -> Result<(), BrokerError> {
        self.publication_prepared = false;
        if !self.published || !self.is_current() || self.execution_prepared.is_none() {
            return Err(BrokerError::StopRequested);
        }
        self.broker.recheck_prepared_scope(self.ticket.bound()?)?;
        if !self.is_current() {
            return Err(BrokerError::StopRequested);
        }
        self.publication_prepared = true;
        Ok(())
    }

    /// Authority only: no native query, filesystem work or reservation Drop.
    /// Borrowing keeps failure cleanup with the worker after App locks exit.
    pub fn commit_authority(&mut self) -> Result<CommitOutcome, BrokerError> {
        if self.completed || !self.publication_prepared {
            return Err(BrokerError::TargetUnauthorized);
        }
        let prepared = self
            .execution_prepared
            .as_ref()
            .ok_or(BrokerError::TargetUnauthorized)?;
        self.publication_prepared = false;
        let result = self
            .broker
            .finalize_preparation_authority(self.ticket.bound()?, prepared);
        self.completed = result.is_ok();
        result
    }

    /// App publication also proves that this opaque approval belongs to its
    /// exact retained transport, not another valid approval handed to the pin.
    pub fn commit_for_transport(&mut self, token: &str) -> Result<CommitOutcome, BrokerError> {
        let ticket = self.ticket.bound()?;
        let context = self.broker.live_context(token)?;
        let key = self
            .broker
            .inner
            .lock()
            .consent
            .host_execution_key(&context, &ticket.run_id, None)
            .ok_or(BrokerError::IdentityMismatch("approval transport"))?;
        if key.request_id != ticket.request_id
            || key.task_id != ticket.task_id
            || key.revision != ticket.revision
            || context.app_session_id != ticket.session_id
        {
            return Err(BrokerError::IdentityMismatch("approval reservation"));
        }
        self.commit_authority()
    }

    /// Convenience path for callers without an App authority pin. This includes
    /// native revalidation; App publication must use the borrowed phases above.
    pub fn commit(mut self) -> Result<CommitOutcome, BrokerError> {
        self.prepare_publication()?;
        self.commit_authority()
    }

    pub fn abort(&mut self) -> AbortEffect {
        if self.completed {
            return AbortEffect {
                feature: None,
                run_id: String::new(),
                opened_run: false,
            };
        }
        self.completed = true;
        let _change = self.broker.changes.notice();
        let effect = {
            let mut inner = self.broker.inner.lock();
            let enabled = inner.feature_enabled;
            let epoch = inner.feature_epoch;
            let effect = inner
                .consent
                .abort_preparation(self.ticket.id(), enabled, epoch);
            if let Some((enabled, epoch)) = effect.feature {
                inner.feature_enabled = enabled;
                inner.feature_epoch = epoch;
            }
            effect
        };
        if effect.opened_run {
            self.broker
                .handoff_fences(std::slice::from_ref(&effect.run_id));
        }
        if !self.webview_bindings.is_empty() {
            if let Ok(executor) = self.broker.require_surface_executor(SurfaceKind::WebView) {
                for target in self.webview_bindings.drain(..) {
                    executor
                        .adapter
                        .release_target_for_run(&effect.run_id, &target);
                }
            }
        }
        effect
    }
}

impl AccessApprovalCancel {
    pub fn fence(&self) {
        let _change = self.broker.changes.notice();
        let run = self.broker.inner.lock().consent.fence_preparation(self.id);
        if let Some(run) = run {
            self.broker.handoff_fences(std::slice::from_ref(&run));
        }
    }
}

impl Drop for AccessApproval {
    fn drop(&mut self) {
        self.abort();
    }
}
