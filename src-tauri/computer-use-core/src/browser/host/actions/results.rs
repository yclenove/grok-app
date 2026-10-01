use super::*;
use crate::browser::extension_completion::CompletionRetirement;

fn pending_index(state: &Inner, dispatch: &ActionDispatch) -> Result<usize, String> {
    if dispatch.proof.binding != dispatch.request.binding() || !live(state, &dispatch.request) {
        return Err("action binding unavailable".into());
    }
    state
        .extension_actions
        .iter()
        .position(|pending| {
            pending.delivered
                && pending.request == dispatch.request
                && pending.cancellation.check().is_ok()
        })
        .ok_or_else(|| "action request unavailable".into())
}

impl ExistingTabHost {
    pub fn claim_existing_action(
        &self,
        origin: &str,
        token: &str,
        dispatch: &ActionDispatch,
    ) -> Result<(), String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        state
            .pairing
            .authenticate_connection(origin, token, &dispatch.request.connection)?;
        let index = pending_index(&state, dispatch)?;
        state.extension_completions.claim_bound(&dispatch.proof)?;
        state.extension_actions[index].claimed = true;
        Ok(())
    }

    pub fn complete_existing_action(
        &self,
        origin: &str,
        token: &str,
        result: ActionResult,
    ) -> Result<(), String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        state
            .pairing
            .authenticate_connection(origin, token, &result.request.connection)?;
        if !result.outcome.valid() {
            return Err("invalid action outcome".into());
        }
        let dispatch = ActionDispatch {
            request: result.request,
            proof: result.proof,
        };
        let index = pending_index(&state, &dispatch)?;
        let retirement = state.extension_completions.retirement(&dispatch.proof)?;
        if matches!(
            result.outcome.status,
            ActionStatus::Applied | ActionStatus::Verified
        ) && (!state.extension_actions[index].claimed
            || retirement == CompletionRetirement::Pending)
        {
            return Err("action completion unconfirmed".into());
        }
        // Negative business results never release a claimed physical receipt.
        let pending = state
            .extension_actions
            .remove(index)
            .ok_or("action request unavailable")?;
        pending
            .sender
            .try_send(Ok(result.outcome))
            .map_err(|_| "action receiver unavailable".into())
    }
}
