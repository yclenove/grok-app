//! Explicit Host discovery freezes native identities before the user selects.
use super::existing_share_preparation::ExistingSharePreparationGuard;
use super::*;
use crate::adapter::ScopeCandidatePage;
use crate::execution::ActionCancellation;
use crate::task_consent::{
    HostRequestKey, NativeScopeProof, ScopeMember, ScopeSelectionPermit, MAX_SCOPE_MEMBERS,
};

pub const MAX_SCOPE_CANDIDATES: usize = 128;
const DISCOVERY_BUDGET: Duration = Duration::from_secs(5);
const MAX_METADATA_CHARS: usize = 512;

#[cfg(any(test, feature = "test-support"))]
mod discovery_probe;

/// Host-only preparation readback, never an execution grant or model DTO.
pub struct HostExistingSharePreparationView {
    pub challenge: Option<crate::pairing::PairingChallenge>,
    pub pairing_confirmed: bool,
    pub available_tabs: usize,
    pub incompatible_tabs: usize,
    pub borrowed_tabs: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostScopeCandidateView {
    /// Opaque picker-local reference. It cannot construct a native identity.
    pub candidate_id: String,
    pub title: String,
    pub app_name: String,
    pub kind: String,
    pub surface: String,
}

struct Candidate {
    view: HostScopeCandidateView,
    member: ScopeMember,
}

pub struct HostScopePickerCall {
    broker: Arc<ComputerUseBroker>,
    permit: ScopeSelectionPermit,
    key: HostRequestKey,
    cancellation: ActionCancellation,
    completed: bool,
    share_preparation: Option<ExistingSharePreparationGuard>,
}

#[derive(Clone)]
pub struct HostScopePickerCancel {
    scope: HostScopeSelectionCancel,
    cancellation: ActionCancellation,
}

impl HostScopePickerCancel {
    pub fn fence(&self) {
        self.cancellation.cancel();
        self.scope.fence();
    }
}

/// This receipt stays in the Host. Only redacted views go to the WebView.
pub struct HostScopePicker {
    broker: Arc<ComputerUseBroker>,
    permit: ScopeSelectionPermit,
    key: HostRequestKey,
    cancellation: ActionCancellation,
    candidates: Vec<Candidate>,
    truncated: bool,
    share_preparation: Option<ExistingSharePreparationGuard>,
    transferred: bool,
}

impl ComputerUseBroker {
    /// User picker gesture only. Reserve authority before starting any native
    /// helper, discovery, worker, or target metadata read.
    pub fn begin_task_scope_picker(
        self: &Arc<Self>,
        token: &str,
        key: &HostRequestKey,
        surface: SurfaceKind,
    ) -> Result<HostScopePickerCall, BrokerError> {
        let _change = self.changes.notice();
        if !matches!(
            surface,
            SurfaceKind::Desktop | SurfaceKind::ExistingTab | SurfaceKind::WebView
        ) {
            return Err(BrokerError::TargetUnauthorized);
        }
        let ctx = self.live_context(token)?;
        let (permit, share_preparation) = {
            let mut inner = self.inner.lock();
            inner.consent.require_host_request(&ctx, key)?;
            let epoch = inner.feature_epoch;
            let permit = inner.consent.begin_scope_selection(
                &ctx,
                &key.request_id,
                key.revision,
                surface,
                epoch,
            )?;
            let share = if surface == SurfaceKind::ExistingTab {
                match self.begin_existing_share_preparation(&mut inner, &permit, key) {
                    Ok(guard) => Some(guard),
                    Err(error) => {
                        inner.consent.fence_scope_selection(&permit);
                        return Err(error);
                    }
                }
            } else {
                None
            };
            (permit, share)
        };
        Ok(HostScopePickerCall {
            broker: self.clone(),
            permit,
            key: key.clone(),
            cancellation: ActionCancellation::default(),
            completed: false,
            share_preparation,
        })
    }
}

fn current(
    broker: &ComputerUseBroker,
    permit: &ScopeSelectionPermit,
    cancel: &ActionCancellation,
) -> bool {
    if cancel.check().is_err() {
        return false;
    }
    let inner = broker.inner.lock();
    inner
        .consent
        .recheck_scope_selection(permit, inner.feature_epoch)
        .is_ok()
}

fn cancel_handle(
    broker: &Arc<ComputerUseBroker>,
    permit: &ScopeSelectionPermit,
    cancellation: &ActionCancellation,
) -> HostScopePickerCancel {
    HostScopePickerCancel {
        scope: HostScopeSelectionCancel::new(broker.clone(), permit.clone()),
        cancellation: cancellation.clone(),
    }
}

impl HostScopePickerCall {
    pub fn surface(&self) -> SurfaceKind {
        self.permit.surface
    }

    pub fn existing_share_preparation(
        &self,
    ) -> Result<HostExistingSharePreparationView, BrokerError> {
        self.share_preparation
            .as_ref()
            .ok_or(BrokerError::TargetUnauthorized)?
            .view()
    }

    /// Host-only code display, never returned by HTTP or a model tool.
    pub fn pairing_challenge(
        &self,
    ) -> Result<Option<crate::pairing::PairingChallenge>, BrokerError> {
        self.share_preparation
            .as_ref()
            .ok_or(BrokerError::TargetUnauthorized)?
            .challenge()
    }

    /// Explicit confirmation on this original picker, not an execution approval.
    pub fn confirm_existing_pairing(&self, nonce: &str) -> Result<(), BrokerError> {
        self.share_preparation
            .as_ref()
            .ok_or(BrokerError::TargetUnauthorized)?
            .confirm(nonce)
    }

    pub fn cancel_handle(&self) -> HostScopePickerCancel {
        cancel_handle(&self.broker, &self.permit, &self.cancellation)
    }

    pub fn is_current(&self) -> bool {
        current(&self.broker, &self.permit, &self.cancellation)
    }

    pub fn discover(mut self) -> Result<HostScopePicker, BrokerError> {
        let started = Instant::now();
        let broker = self.broker.clone();
        let permit = self.permit.clone();
        let cancellation = self.cancellation.clone();
        let is_current: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
            current(&broker, &permit, &cancellation) && started.elapsed() < DISCOVERY_BUDGET
        });
        if !is_current() {
            return Err(BrokerError::StopRequested);
        }
        let executor = self.broker.require_surface_executor(self.permit.surface)?;
        let ScopeCandidatePage {
            candidates: native,
            truncated,
        } = executor
            .adapter
            .scope_candidates_for_task(MAX_SCOPE_CANDIDATES, is_current.clone())
            .map_err(BrokerError::Adapter)?;
        #[cfg(any(test, feature = "test-support"))]
        self.broker.run_scope_discovery_hook(native.len());
        if !is_current() {
            return Err(BrokerError::StopRequested);
        }
        if native.len() > MAX_SCOPE_CANDIDATES {
            return Err(BrokerError::Adapter(
                "scope picker producer exceeded its bound".into(),
            ));
        }
        let mut candidates = Vec::with_capacity(native.len());
        for candidate in native {
            if !is_current() {
                return Err(BrokerError::StopRequested);
            }
            let info = candidate.info;
            if info.target_id.len() > 512
                || SurfaceRouter::classify_target_id(&info.target_id) != self.permit.surface
                || candidate.identity.target_id().as_deref() != Some(&info.target_id)
                || (self.permit.surface == SurfaceKind::Desktop
                    && crate::adapter::skip_desktop_target(&info.title, &info.app_name))
                || candidates
                    .iter()
                    .any(|row: &Candidate| row.member.target_id == info.target_id)
            {
                return Err(BrokerError::TargetUnauthorized);
            }
            // Recheck the retained producer identity, never capture a replacement
            // witness or pick a target by its current title/focus.
            let member = ScopeMember {
                target_id: info.target_id,
                surface: self.permit.surface,
                display_name: (!info.title.trim().is_empty())
                    .then(|| bounded_metadata(&info.title)),
                native: Some(NativeScopeProof {
                    executor_generation: executor.generation,
                    identity: candidate.identity,
                }),
            };
            self.broker
                .recheck_scope_members(std::slice::from_ref(&member))?;
            if !is_current() {
                return Err(BrokerError::StopRequested);
            }
            candidates.push(Candidate {
                view: HostScopeCandidateView {
                    candidate_id: Uuid::new_v4().simple().to_string(),
                    title: bounded_metadata(&info.title),
                    app_name: bounded_metadata(&info.app_name),
                    kind: bounded_metadata(&info.kind),
                    surface: self.permit.surface.as_wire().into(),
                },
                member,
            });
        }
        if !is_current() {
            return Err(BrokerError::StopRequested);
        }
        self.completed = true;
        Ok(HostScopePicker {
            broker: self.broker.clone(),
            permit: self.permit.clone(),
            key: self.key.clone(),
            cancellation: self.cancellation.clone(),
            candidates,
            truncated,
            share_preparation: self.share_preparation.take(),
            transferred: false,
        })
    }
}

impl Drop for HostScopePickerCall {
    fn drop(&mut self) {
        if !self.completed {
            self.cancel_handle().fence();
        }
    }
}

impl HostScopePicker {
    /// A metadata-only empty ExistingTab result may be retried under the SAME
    /// permit and preparation. Never refresh a nonempty receipt: its selected
    /// opaque IDs must continue to refer to the original native instances.
    pub fn into_empty_retry(mut self) -> Result<HostScopePickerCall, BrokerError> {
        if self.permit.surface != SurfaceKind::ExistingTab || !self.candidates.is_empty() {
            return Err(BrokerError::TargetUnauthorized);
        }
        if !self.is_current() {
            return Err(BrokerError::StopRequested);
        }
        self.existing_share_preparation()?;
        let call = HostScopePickerCall {
            broker: self.broker.clone(),
            permit: self.permit.clone(),
            key: self.key.clone(),
            cancellation: self.cancellation.clone(),
            completed: false,
            share_preparation: self.share_preparation.take(),
        };
        self.transferred = true;
        Ok(call)
    }

    pub fn existing_share_preparation(
        &self,
    ) -> Result<HostExistingSharePreparationView, BrokerError> {
        self.share_preparation
            .as_ref()
            .ok_or(BrokerError::TargetUnauthorized)?
            .view()
    }

    pub fn pairing_challenge(
        &self,
    ) -> Result<Option<crate::pairing::PairingChallenge>, BrokerError> {
        self.share_preparation
            .as_ref()
            .ok_or(BrokerError::TargetUnauthorized)?
            .challenge()
    }

    pub fn confirm_existing_pairing(&self, nonce: &str) -> Result<(), BrokerError> {
        self.share_preparation
            .as_ref()
            .ok_or(BrokerError::TargetUnauthorized)?
            .confirm(nonce)
    }

    pub fn key(&self) -> &HostRequestKey {
        &self.key
    }
    pub fn surface(&self) -> SurfaceKind {
        self.permit.surface
    }
    pub fn is_current(&self) -> bool {
        current(&self.broker, &self.permit, &self.cancellation)
    }
    pub fn cancel_handle(&self) -> HostScopePickerCancel {
        cancel_handle(&self.broker, &self.permit, &self.cancellation)
    }
    pub fn views(&self) -> Result<Vec<HostScopeCandidateView>, BrokerError> {
        if !self.is_current() {
            return Err(BrokerError::StopRequested);
        }
        Ok(self
            .candidates
            .iter()
            .map(|candidate| candidate.view.clone())
            .collect())
    }
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// User choices refer to this receipt's original members. The existing
    /// reservation rechecks every selected native instance outside its lock.
    pub fn select(&self, candidate_ids: &[String]) -> Result<HostScopeSelection, BrokerError> {
        if !self.is_current() {
            return Err(BrokerError::StopRequested);
        }
        let members = self.selected_members(candidate_ids)?;
        Ok(HostScopeSelection::from_picker(
            &self.broker,
            self.permit.clone(),
            members,
        ))
    }

    pub fn committed_outcome(
        &self,
        token: &str,
        candidate_ids: &[String],
    ) -> Result<Option<crate::task_consent::CommitOutcome>, BrokerError> {
        if self.broker.live_context(token)? != self.permit.context {
            return Err(BrokerError::IdentityMismatch("picker owner"));
        }
        let members = self.selected_members(candidate_ids)?;
        let targets = members
            .iter()
            .map(|member| (member.surface, member.target_id.as_str()))
            .collect::<Vec<_>>();
        self.broker.committed_scope_selection(
            token,
            &self.key.request_id,
            self.key.revision,
            &targets,
        )
    }

    fn selected_members(&self, candidate_ids: &[String]) -> Result<Vec<ScopeMember>, BrokerError> {
        if !(1..=MAX_SCOPE_MEMBERS).contains(&candidate_ids.len())
            || candidate_ids
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != candidate_ids.len()
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        candidate_ids
            .iter()
            .map(|id| {
                self.candidates
                    .iter()
                    .find(|candidate| candidate.view.candidate_id == *id)
                    .map(|candidate| candidate.member.clone())
                    .ok_or(BrokerError::TargetUnauthorized)
            })
            .collect()
    }
}

impl Drop for HostScopePicker {
    fn drop(&mut self) {
        if !self.transferred {
            self.cancel_handle().fence();
        }
    }
}

fn bounded_metadata(value: &str) -> String {
    value.chars().take(MAX_METADATA_CHARS).collect()
}
