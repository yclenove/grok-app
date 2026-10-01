//! All checks and transitions use the same lock as grants, pairing and tab reuse.
use std::collections::HashSet;

use super::{existing::expire_pairing, ExistingTabHost, Inner};
use crate::browser::extension_completion::{
    CompletionBinding, CompletionProof, CompletionRetirement, CompletionStatus,
    ACTION_COMPLETION_PROTOCOL,
};
use crate::execution::ActionCancellation;
use crate::pairing::PairingConnection;

pub(super) fn live(state: &Inner, binding: &CompletionBinding) -> bool {
    let Some(tab) = state.tabs.get(&binding.tab_id) else {
        return false;
    };
    let Some(offer) = state.shared.get(&binding.tab_id) else {
        return false;
    };
    state.pairing.live_connection().as_ref() == Some(&binding.connection)
        && !state.stopped_runs.contains(&binding.run_id)
        && tab.info.user_owned
        && tab.info.borrowed
        && !tab.info.closed
        && tab.disconnect.is_none()
        && tab.info.session == binding.session
        && tab.info.run_id == binding.run_id
        && tab.info.generation == binding.grant_generation
        && tab.info.document_generation == binding.document_generation
        && tab.info.connection_generation == binding.connection.generation
        && offer.connection_generation == binding.connection.generation
        && offer.document_generation == binding.document_generation
        && tab.pairing_token.as_deref() == state.pairing.session_key()
        && tab.current_observation.as_ref().is_some_and(|observation| {
            observation.snapshot_id == binding.snapshot_id
                && observation.page_id == binding.document_id
                && observation.page_generation == binding.document_generation
        })
}

pub(super) fn sweep(state: &mut Inner) {
    super::actions::sweep(state);
    let revoked: HashSet<_> = state
        .extension_completions
        .bindings()
        .filter(|binding| !live(state, binding))
        .map(|binding| binding.request_id.clone())
        .collect();
    state.extension_completions.sweep(&revoked);
}

fn receipt_origin(state: &Inner, origin: &str) -> Result<(), String> {
    if origin != format!("chrome-extension://{}", state.installed_extension_id) {
        return Err("completion origin unavailable".into());
    }
    Ok(())
}

impl ExistingTabHost {
    /// Internal preparation only, not an extension/model authorization API. No action is executed here.
    pub fn offer_existing_completion(
        &self,
        session: &str,
        run: &str,
        tab: &str,
        snapshot: &str,
        cancellation: ActionCancellation,
    ) -> Result<CompletionProof, String> {
        cancellation.check()?;
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        if super::transport::request_busy(&state, tab) {
            return Err("extension request busy".into());
        }
        let record = state.tabs.get(tab).ok_or("existing tab unavailable")?;
        let observed = record
            .current_observation
            .as_ref()
            .ok_or("observation unavailable")?;
        let binding = CompletionBinding {
            protocol: ACTION_COMPLETION_PROTOCOL,
            request_id: uuid::Uuid::new_v4().to_string(),
            connection: state
                .pairing
                .live_connection()
                .ok_or("extension unavailable")?,
            session: session.into(),
            run_id: run.into(),
            tab_id: tab.into(),
            document_id: observed.page_id.clone(),
            document_generation: record.info.document_generation,
            grant_generation: record.info.generation,
            snapshot_id: snapshot.into(),
        };
        if !live(&state, &binding) {
            return Err("existing completion grant unavailable".into());
        }
        let browser_session_id = state.browser_session_id.clone();
        state
            .extension_completions
            .offer_with_browser(binding, cancellation, browser_session_id)
    }

    pub fn browser_restart(
        &self,
        origin: &str,
        token: &str,
        connection: &PairingConnection,
        current_id: &str,
        previous_id: Option<&str>,
        cleanups: &[crate::browser::extension_completion::BrowserExitCleanup],
    ) -> Result<Vec<(String, &'static str)>, String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        state
            .pairing
            .authenticate_connection(origin, token, connection)?;
        let uuid = |value: &str| uuid::Uuid::parse_str(value).is_ok() && value.len() == 36;
        if !uuid(current_id)
            || previous_id.is_some_and(|previous| !uuid(previous) || previous == current_id)
        {
            return Err("invalid browser session identity".into());
        }
        state.browser_session_id = Some(current_id.into());
        Ok(previous_id.map_or_else(Vec::new, |previous| {
            state
                .extension_completions
                .converge_browser_exit(previous, cleanups)
        }))
    }

    pub fn claim_existing_completion(
        &self,
        origin: &str,
        token: &str,
        proof: &CompletionProof,
    ) -> Result<(), String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        state
            .pairing
            .authenticate_connection(origin, token, &proof.binding.connection)?;
        if !live(&state, &proof.binding) {
            return Err("existing completion grant unavailable".into());
        }
        state.extension_completions.claim(proof)
    }

    /// Deliberately accepts no pairing Bearer. The proof can only inspect/retire its own occupancy.
    pub fn existing_completion_status(
        &self,
        origin: &str,
        proof: &CompletionProof,
    ) -> Result<CompletionStatus, String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        receipt_origin(&state, origin)?;
        state.extension_completions.status(proof)
    }

    pub fn settle_existing_completion(
        &self,
        origin: &str,
        proof: &CompletionProof,
    ) -> Result<(), String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        receipt_origin(&state, origin)?;
        state.extension_completions.settle(proof)
    }

    /// Read-only cleanup state. It cannot claim or settle a pending action.
    pub fn existing_completion_retirement(
        &self,
        origin: &str,
        proof: &CompletionProof,
    ) -> Result<CompletionRetirement, String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        receipt_origin(&state, origin)?;
        state.extension_completions.retirement(proof)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn expire_completion_tombstones_for_test(&self) {
        self.inner
            .lock()
            .extension_completions
            .expire_tombstones_for_test();
    }
}

#[cfg(test)]
mod tests;
