//! Request execution requires current authority; its cleanup proof survives revoke.
use super::{existing::expire_pairing, transport, ExistingTabHost, Inner};
use crate::browser::extension_completion::{CompletionRetirement, CompletionStatus};
use crate::browser::extension_observation::ObservationProof;

fn receipt_origin(state: &Inner, origin: &str) -> Result<(), String> {
    if origin != format!("chrome-extension://{}", state.installed_extension_id) {
        return Err("observation completion origin unavailable".into());
    }
    Ok(())
}

impl ExistingTabHost {
    /// Only one delivered original request can claim this observation receipt.
    /// First observation uses the authenticated Share document, not an old snapshot.
    pub fn claim_extension_observation(
        &self,
        origin: &str,
        token: &str,
        proof: &ObservationProof,
    ) -> Result<(), String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        state
            .pairing
            .authenticate_connection(origin, token, &proof.proof.binding.connection)?;
        let request = state
            .extension_requests
            .iter()
            .find(|pending| {
                pending.request.completion == *proof && pending.delivered && !pending.claimed
            })
            .map(|pending| pending.request.clone())
            .ok_or("observation claim unavailable")?;
        if !transport::live_request(&state, &request)
            || request.completion_binding() != proof.proof.binding
        {
            return Err("observation claim unavailable".into());
        }
        state.observation_completions.claim_bound(&proof.proof)?;
        state
            .extension_requests
            .iter_mut()
            .find(|pending| pending.request == request)
            .ok_or("observation claim unavailable")?
            .claimed = true;
        Ok(())
    }

    /// No Pairing Bearer or execution-ready check. This proof cannot grant input.
    pub fn extension_observation_status(
        &self,
        origin: &str,
        proof: &ObservationProof,
    ) -> Result<CompletionStatus, String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        receipt_origin(&state, origin)?;
        state.observation_completions.status(&proof.proof)
    }

    pub fn settle_extension_observation(
        &self,
        origin: &str,
        proof: &ObservationProof,
    ) -> Result<(), String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        receipt_origin(&state, origin)?;
        state.observation_completions.settle(&proof.proof)?;
        if let Some(pending) = state
            .extension_requests
            .iter_mut()
            .find(|pending| pending.request.completion == *proof)
        {
            pending.settled = true;
            if !pending.claimed {
                pending.cancellation.cancel();
            }
        }
        transport::sweep_requests(&mut state);
        Ok(())
    }

    pub fn extension_observation_retirement(
        &self,
        origin: &str,
        proof: &ObservationProof,
    ) -> Result<CompletionRetirement, String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        receipt_origin(&state, origin)?;
        state.observation_completions.retirement(&proof.proof)
    }
}
