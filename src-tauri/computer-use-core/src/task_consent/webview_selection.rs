//! The same Host selection becomes execution members only after its run binds.
use super::types::{schema, OpaqueIdentity, PrepPhase};
use super::{ConsentState, PrepareTicket, ScopeMember, WebViewSelectionTicket, MAX_SCOPE_MEMBERS};
use crate::adapter::{NativeScopeIdentity, SurfaceKind};
use crate::error::BrokerError;

impl ConsentState {
    pub(super) fn webview_selection_ticket(
        &self,
        id: u64,
    ) -> Result<WebViewSelectionTicket, BrokerError> {
        let prep = self
            .preparations
            .iter()
            .find(|prep| prep.id == id && prep.phase == PrepPhase::Resources)
            .filter(|prep| prep.surface == SurfaceKind::WebView && prep.managed.is_none())
            .ok_or_else(|| schema("WebView selection unavailable"))?;
        if !(1..=MAX_SCOPE_MEMBERS).contains(&prep.members.len())
            || prep.members.iter().any(|member| {
                member.surface != SurfaceKind::WebView
                    || !matches!(
                        member.native.as_ref().map(|proof| &proof.identity),
                        Some(NativeScopeIdentity::WebViewSelected { .. })
                    )
            })
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        Ok(WebViewSelectionTicket {
            id,
            session_id: prep.app_session_id.clone(),
            run_id: prep.run_id.clone(),
            request_id: prep.request_id.clone(),
            revision: prep.revision,
            task_id: prep.task_id.clone(),
            selection_revision: prep.selection_revision,
            members: prep.members.clone(),
        })
    }

    pub(crate) fn webview_selection_current(
        &self,
        ticket: &WebViewSelectionTicket,
        enabled: bool,
        epoch: u64,
    ) -> bool {
        self.preparation_current(ticket.id, enabled, epoch)
            && self.preparations.iter().any(|prep| {
                prep.id == ticket.id
                    && prep.phase == PrepPhase::Resources
                    && prep.surface == SurfaceKind::WebView
                    && prep.managed.is_none()
                    && prep.app_session_id == ticket.session_id
                    && prep.run_id == ticket.run_id
                    && prep.request_id == ticket.request_id
                    && prep.revision == ticket.revision
                    && prep.task_id == ticket.task_id
                    && prep.selection_revision == ticket.selection_revision
                    && prep.members == ticket.members
            })
    }

    pub(crate) fn bind_webview_selection_scope(
        &mut self,
        ticket: &WebViewSelectionTicket,
        members: Vec<ScopeMember>,
        enabled: bool,
        epoch: u64,
    ) -> Result<PrepareTicket, BrokerError> {
        if !self.webview_selection_current(ticket, enabled, epoch) {
            return Err(BrokerError::StopRequested);
        }
        if members.len() != ticket.members.len() {
            return Err(BrokerError::TargetUnauthorized);
        }
        for (selected, bound) in ticket.members.iter().zip(&members) {
            let original = selected
                .native
                .as_ref()
                .ok_or(BrokerError::TargetUnauthorized)?;
            let executed = bound
                .native
                .as_ref()
                .ok_or(BrokerError::TargetUnauthorized)?;
            let NativeScopeIdentity::WebViewSelected { identity } = &original.identity else {
                return Err(BrokerError::TargetUnauthorized);
            };
            if bound.surface != SurfaceKind::WebView
                || original.executor_generation != executed.executor_generation
                || executed.identity.target_id().as_deref() != Some(bound.target_id.as_str())
                || !matches!(&executed.identity, NativeScopeIdentity::WebViewPage { native_tab_id, document_generation, session_id, run_id, .. }
                    if native_tab_id == &identity.native_tab_id && document_generation == &identity.document_generation
                        && session_id == &ticket.session_id && run_id == &ticket.run_id)
            {
                return Err(BrokerError::IdentityMismatch("WebView selected binding"));
            }
        }
        let request = self
            .requests
            .iter_mut()
            .find(|request| request.request_id == ticket.request_id)
            .ok_or(BrokerError::StopRequested)?;
        request.bound = Some(
            members
                .iter()
                .map(|member| OpaqueIdentity {
                    target_id: member.target_id.clone(),
                    surface: member.surface,
                    display_name: member.display_name.clone(),
                    synthetic: false,
                    native: member.native.clone(),
                })
                .collect(),
        );
        request.selecting = false;
        let prep = self
            .preparations
            .iter_mut()
            .find(|prep| prep.id == ticket.id)
            .ok_or(BrokerError::StopRequested)?;
        prep.target_id = members[0].target_id.clone();
        prep.native = members[0].native.clone();
        prep.members = members;
        prep.phase = PrepPhase::Binding;
        self.bound_preparation_ticket(ticket.id)
    }
}
