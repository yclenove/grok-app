//! Original App binding and native documents remain pinned during publication.
//! This path does not query Tauri, page metadata, DOM/AX or the script runtime.

use super::super::adapter::NativeScopeIdentity;
use super::super::BrokerError;
use super::*;

impl BoundTab {
    pub(super) fn scope_identity(&self) -> NativeScopeIdentity {
        NativeScopeIdentity::WebViewPage {
            target_id: self.target_id(),
            native_tab_id: self.native_tab_id.clone(),
            document_generation: self.generation,
            session_id: self.session.clone(),
            run_id: self.run_id.clone(),
        }
    }
}

impl WebViewAdapter {
    pub(super) fn bound_scope_identity(
        &self,
        target_id: &str,
    ) -> Result<NativeScopeIdentity, String> {
        let bound = self.bound_target(target_id)?;
        let identity = bound.scope_identity();
        if !self.still_bound(&bound) {
            return Err("WebView scope binding retired".into());
        }
        Ok(identity)
    }

    pub(super) fn bound_scope_target_info(
        &self,
        run: &str,
        target_id: &str,
    ) -> Result<TargetInfo, String> {
        let bound = self.bound_target(target_id)?;
        if bound.run_id != run {
            return Err("WebView run identity changed".into());
        }
        let title = bound.view.title()?;
        let title = if title.trim().is_empty() {
            bound.view.url()?
        } else {
            title
        };
        if !self.still_bound(&bound) {
            return Err("WebView scope binding retired".into());
        }
        Ok(target_info(
            &bound,
            title.chars().take(selection::MAX_METADATA_CHARS).collect(),
        ))
    }

    pub(super) fn with_bound_authority(
        &self,
        identities: &[NativeScopeIdentity],
        commit: &mut dyn FnMut() -> Result<(), BrokerError>,
    ) -> Result<(), BrokerError> {
        if identities.is_empty()
            || identities.len() > grok_computer_use_core::task_consent::MAX_SCOPE_MEMBERS
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        let current = self.bound.lock();
        let mut selected = Vec::with_capacity(identities.len());
        for identity in identities {
            let NativeScopeIdentity::WebViewPage { target_id, .. } = identity else {
                return Err(BrokerError::TargetUnauthorized);
            };
            let bound = current.get(target_id).ok_or(BrokerError::DeadTarget)?;
            if bound.scope_identity() != *identity || bound.binding_cancellation.check().is_err() {
                return Err(BrokerError::DeadTarget);
            }
            // Binding admission is held, so no new native script can start
            // between this occupancy check and final publication.
            if !self.scripts.native_is_idle(&bound.native_key()) {
                return Err(BrokerError::Adapter(
                    "WebView native execution is still pending".into(),
                ));
            }
            // A repeated native object would acquire the same lock recursively.
            if selected
                .iter()
                .any(|old: &&Arc<BoundTab>| old.native_tab_id == bound.native_tab_id)
            {
                return Err(BrokerError::TargetUnauthorized);
            }
            selected.push(bound);
        }
        selected.sort_by(|a, b| a.native_tab_id.cmp(&b.native_tab_id));
        let result = with_documents(&selected, commit);
        drop(current);
        result
    }
}

fn with_documents(
    selected: &[&Arc<BoundTab>],
    commit: &mut dyn FnMut() -> Result<(), BrokerError>,
) -> Result<(), BrokerError> {
    match selected.split_first() {
        Some((bound, rest)) => bound
            .view
            .with_document_authority(bound.generation, &mut || with_documents(rest, commit)),
        None => commit(),
    }
}

#[cfg(test)]
mod tests;
