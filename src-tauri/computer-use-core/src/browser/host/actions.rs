//! Admission, negotiation and delivery share the tab authority lock.
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{completion, existing::expire_pairing, ExistingTabHost, Inner};
use crate::browser::extension_action::*;
use crate::browser::extension_completion::ACTION_COMPLETION_PROTOCOL;
use crate::error::BrokerError;
use crate::execution::ActionCancellation;
use crate::pairing::PairingConnection;

const ACTION_TTL: Duration = Duration::from_secs(10);
type ActionReceiver = Receiver<Result<ActionOutcome, BrokerError>>;

pub(super) struct PendingAction {
    pub(super) request: ExtensionActionRequest,
    deadline: Instant,
    cancellation: ActionCancellation,
    delivered: bool,
    claimed: bool,
    sender: SyncSender<Result<ActionOutcome, BrokerError>>,
}

pub(super) fn additional_occupancy(state: &Inner) -> usize {
    // A settled action still waiting to submit its business result owns its slot.
    state
        .extension_actions
        .iter()
        .filter(|pending| {
            !state
                .extension_completions
                .bindings()
                .any(|binding| binding.request_id == pending.request.request_id)
        })
        .count()
}

pub(super) fn has_tab(state: &Inner, tab: &str) -> bool {
    state.extension_completions.has_tab(tab)
        || state
            .extension_actions
            .iter()
            .any(|pending| pending.request.tab_id == tab)
}

fn live(state: &Inner, request: &ExtensionActionRequest) -> bool {
    completion::live(state, &request.binding())
        && state
            .action_negotiation
            .as_ref()
            .is_some_and(|negotiation| {
                negotiation.connection == request.connection
                    && negotiation.actions.contains(&request.command.name())
            })
        && state
            .tabs
            .get(&request.tab_id)
            .and_then(|tab| tab.current_observation.as_ref())
            .is_some_and(|observation| observation.refs.contains(request.command.element_ref()))
}

pub(super) fn sweep(state: &mut Inner) {
    let now = Instant::now();
    let stale: Vec<_> = state
        .extension_actions
        .iter()
        .filter(|pending| {
            now >= pending.deadline
                || pending.cancellation.check().is_err()
                || !live(state, &pending.request)
        })
        .map(|pending| pending.request.request_id.clone())
        .collect();
    state.extension_actions.retain(|pending| {
        if !stale.contains(&pending.request.request_id) {
            return true;
        }
        pending.cancellation.cancel();
        let error = if now >= pending.deadline {
            BrokerError::Timeout
        } else {
            BrokerError::TargetUnauthorized
        };
        let _ = pending.sender.try_send(Err(error));
        false
    });
}

impl ExistingTabHost {
    pub fn negotiate_existing_actions(
        &self,
        origin: &str,
        token: &str,
        mut negotiation: ActionNegotiation,
    ) -> Result<(), String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        state
            .pairing
            .authenticate_connection(origin, token, &negotiation.connection)?;
        if !negotiation.valid() {
            return Err("unsupported action protocol".into());
        }
        negotiation.actions.sort();
        if state
            .action_negotiation
            .as_ref()
            .is_some_and(|old| old != &negotiation)
        {
            return Err("action negotiation already fixed".into());
        }
        state.action_negotiation = Some(negotiation);
        Ok(())
    }

    fn enqueue_existing_action(
        &self,
        input: ExistingActionInput<'_>,
    ) -> Result<(ExtensionActionRequest, ActionReceiver), BrokerError> {
        input.cancellation.check().map_err(BrokerError::Adapter)?;
        if !input.command.valid() {
            return Err(BrokerError::Schema("invalid extension action".into()));
        }
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        let connection = state
            .pairing
            .live_connection()
            .ok_or(BrokerError::TargetUnauthorized)?;
        let record = state
            .tabs
            .get(input.tab)
            .ok_or(BrokerError::TargetUnauthorized)?;
        let observed = record
            .current_observation
            .as_ref()
            .ok_or(BrokerError::TargetUnauthorized)?;
        // Chrome tab/document IDs, never arbitrary target strings or frame selectors.
        if input.tab.parse::<u64>().ok().is_none_or(|id| {
            id > crate::protocol::JS_MAX_SAFE_INTEGER || id.to_string() != input.tab
        }) || observed.page_id.len() != 32
            || !observed.page_id.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        let sequence = state
            .extension_sequence
            .checked_add(1)
            .filter(|seq| *seq <= crate::protocol::JS_MAX_SAFE_INTEGER)
            .ok_or_else(|| BrokerError::Schema("extension sequence exhausted".into()))?;
        let request = ExtensionActionRequest {
            protocol: ACTION_COMPLETION_PROTOCOL,
            request_id: uuid::Uuid::new_v4().to_string(),
            sequence,
            deadline_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
                + ACTION_TTL.as_millis() as u64,
            connection,
            session: input.session.into(),
            run_id: input.run.into(),
            tab_id: input.tab.into(),
            document_id: observed.page_id.clone(),
            document_generation: input.document_generation,
            grant_generation: input.grant_generation,
            command: input.command,
        };
        if !live(&state, &request) {
            return Err(BrokerError::TargetUnauthorized);
        }
        if super::transport::request_busy(&state, input.tab) {
            return Err(BrokerError::Schema("extension request busy".into()));
        }
        let browser_session_id = state.browser_session_id.clone();
        state
            .extension_completions
            .reserve_with_browser(
                request.binding(),
                input.cancellation.clone(),
                true,
                browser_session_id,
            )
            .map_err(BrokerError::Adapter)?;
        state.extension_sequence = sequence;
        let (sender, receiver) = mpsc::sync_channel(1);
        state.extension_actions.push_back(PendingAction {
            request: request.clone(),
            deadline: Instant::now() + ACTION_TTL,
            cancellation: input.cancellation,
            delivered: false,
            claimed: false,
            sender,
        });
        Ok((request, receiver))
    }

    /// Called by a Host-authorized adapter/fixture only. Never exposed as HTTP admission.
    pub fn execute_existing_action(
        &self,
        input: ExistingActionInput<'_>,
    ) -> Result<ActionOutcome, BrokerError> {
        let cancellation = input.cancellation.clone();
        let (request, receiver) = self.enqueue_existing_action(input)?;
        loop {
            match receiver.recv_timeout(Duration::from_millis(25)) {
                Ok(result) => {
                    let state = self.inner.lock();
                    return if cancellation.check().is_ok() && live(&state, &request) {
                        result
                    } else {
                        Err(BrokerError::TargetUnauthorized)
                    };
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(BrokerError::TargetUnauthorized)
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    expire_pairing(&mut self.inner.lock());
                }
            }
        }
    }

    pub fn poll_existing_action(
        &self,
        origin: &str,
        token: &str,
        connection: &PairingConnection,
    ) -> Result<Option<ActionDispatch>, String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        state
            .pairing
            .authenticate_connection(origin, token, connection)?;
        if state
            .action_negotiation
            .as_ref()
            .is_none_or(|n| &n.connection != connection)
        {
            return Err("action protocol not negotiated".into());
        }
        if state
            .extension_actions
            .iter()
            .any(|pending| pending.delivered)
        {
            return Ok(None);
        }
        let Some(request) = state
            .extension_actions
            .front()
            .map(|pending| pending.request.clone())
        else {
            return Ok(None);
        };
        let proof = state.extension_completions.deliver(&request.request_id)?;
        state
            .extension_actions
            .front_mut()
            .ok_or("action unavailable")?
            .delivered = true;
        Ok(Some(ActionDispatch { request, proof }))
    }
}

mod results;
#[cfg(test)]
mod tests;
