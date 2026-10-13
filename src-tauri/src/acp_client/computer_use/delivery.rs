//! Bounded Host delivery state. A claim is retained until real stdin admission.
use super::*;
use grok_computer_use_core::task_consent::{AuthorityClaim, NoAuthorityNotice};
use std::collections::VecDeque;

const MAX_NOTICES: usize = 8;
const MAX_RECEIPTS: usize = 32;

#[cfg(test)]
#[path = "delivery_tests.rs"]
mod tests;

#[derive(Default)]
pub(super) struct DeliveryLedger {
    pub(super) last_error: Option<&'static str>,
    retry_after: Option<Instant>,
    generation: u64,
    chat_stopped: bool,
    notices: VecDeque<NoAuthorityNotice>,
    admitted: VecDeque<String>,
}

#[derive(Clone)]
enum Decision {
    Continue(AuthorityClaim),
    NoAuthority(NoAuthorityNotice),
}

#[derive(Clone)]
pub(crate) struct DecisionDelivery {
    transport: Arc<RequestTransport>,
    decision: Decision,
    generation: u64,
}

impl DecisionDelivery {
    pub(crate) fn was_admitted(&self) -> bool {
        self.transport
            .deliveries
            .lock()
            .admitted
            .iter()
            .any(|id| id == self.dispatch_id())
    }
    pub(crate) fn app_session(&self) -> &str {
        match &self.decision {
            Decision::Continue(claim) => &claim.app_session_id,
            Decision::NoAuthority(notice) => &notice.app_session_id,
        }
    }

    pub(crate) fn agent_session(&self) -> &str {
        match &self.decision {
            Decision::Continue(claim) => &claim.agent_session_id,
            Decision::NoAuthority(notice) => &notice.agent_session_id,
        }
    }

    fn dispatch_id(&self) -> &str {
        match &self.decision {
            Decision::Continue(claim) => &claim.dispatch_id,
            Decision::NoAuthority(notice) => &notice.dispatch_id,
        }
    }

    fn validate(&self) -> Result<(), String> {
        match &self.decision {
            Decision::Continue(claim) => self.transport.broker.validate_continuation(claim),
            Decision::NoAuthority(notice) => self.transport.broker.validate_no_authority(notice),
        }
        .map_err(|_| "Computer Use decision retired".into())
    }

    fn message(&self) -> String {
        let payload = match &self.decision {
            Decision::Continue(claim) => {
                json!({"decision":"committed","requestId":claim.request_id,"taskId":claim.task_id,"grantRevision":claim.grant_revision,"dispatchId":claim.dispatch_id})
            }
            Decision::NoAuthority(notice) => {
                json!({"decision":notice.reason,"authority":"none","requestId":notice.request_id,"taskId":notice.task_id,"revision":notice.revision,"dispatchId":notice.dispatch_id})
            }
        };
        format!("[Host Computer Use decision]\n{payload}\nContinue the existing task using this decision. A committed decision requires a fresh observation before actions. A denied, cancelled or expired decision grants no computer access; explain it briefly or use an available alternative. Do not repeat the user's task, poll for approval or request the same denied scope again.")
    }
}

pub(crate) enum DeliveryAttempt {
    NotStarted,
    Completed,
    /// Admission happened. Any failure afterwards is conservatively unknown;
    /// never issue the same prompt again to find out whether it took effect.
    Unknown(AgentError),
}

/// Separate from the UI FSM: keep a lease until the real prompt future ends.
pub(crate) struct PromptCallGuard<'a> {
    client: &'a AcpClient,
    agent: String,
    id: String,
}

impl<'a> PromptCallGuard<'a> {
    pub(crate) fn begin(
        client: &'a AcpClient,
        agent: &str,
        turn: Option<&str>,
    ) -> Result<Self, AgentError> {
        let mut calls = client.prompt_calls.lock();
        if calls.contains_key(agent) || calls.len() >= MAX_CONNECTIONS {
            return Err(AgentError::new(
                AgentErrorCode::ConnectFailed,
                "prompt RPC still running",
            ));
        }
        let id = turn.unwrap_or_default().to_string();
        calls.insert(agent.into(), id.clone());
        Ok(Self {
            client,
            agent: agent.into(),
            id,
        })
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }
}

pub(crate) struct PromptWaitGuard<'a> {
    pub(crate) client: &'a AcpClient,
    pub(crate) id: u64,
}

impl Drop for PromptWaitGuard<'_> {
    fn drop(&mut self) {
        self.client.pending.lock().remove(&self.id);
    }
}

impl Drop for PromptCallGuard<'_> {
    fn drop(&mut self) {
        let mut calls = self.client.prompt_calls.lock();
        if calls.get(&self.agent) == Some(&self.id) {
            calls.remove(&self.agent);
        }
    }
}

impl AcpClient {
    pub(crate) fn prompt_rpc_in_flight(&self, agent: &str) -> bool {
        self.prompt_calls.lock().contains_key(agent)
    }

    pub(crate) fn computer_use_decision(
        &self,
        session: &str,
        agent: &str,
    ) -> Option<DecisionDelivery> {
        let slots = self.computer_use_connections.lock();
        let transport = slots.get(session)?.transport.as_ref()?.clone();
        if !self.is_alive() || !transport.bound_to(agent) {
            return None;
        }
        let BearerClass::Live(context) = transport.broker.classify_bearer(&transport.token) else {
            return None;
        };
        let mut ledger = transport.deliveries.lock();
        if ledger.chat_stopped
            || ledger
                .retry_after
                .is_some_and(|retry| Instant::now() < retry)
        {
            return None;
        }
        // Drain only when the previous batch is empty. Core is capped at eight
        // notices per owner; busy chats retain this exact original batch.
        if ledger.notices.is_empty() {
            let notices = transport
                .broker
                .take_no_authority(session, context.owner_nonce);
            for notice in notices {
                if ledger.notices.len() >= MAX_NOTICES {
                    break;
                }
                if transport.broker.validate_no_authority(&notice).is_ok()
                    && !ledger.admitted.contains(&notice.dispatch_id)
                {
                    ledger.notices.push_back(notice);
                }
            }
        }
        ledger
            .notices
            .retain(|notice| transport.broker.validate_no_authority(notice).is_ok());
        let decision = if let Some(claim) = transport.broker.claim_continuation(session) {
            if transport.broker.validate_continuation(&claim).is_ok() {
                Some(Decision::Continue(claim))
            } else {
                None
            }
        } else {
            None
        }
        .or_else(|| ledger.notices.front().cloned().map(Decision::NoAuthority))?;
        let generation = ledger.generation;
        drop(ledger);
        Some(DecisionDelivery {
            transport,
            decision,
            generation,
        })
    }

    pub(crate) fn computer_use_chat_stopped(&self, session: &str) {
        let slots = self.computer_use_connections.lock();
        if let Some(transport) = slots.get(session).and_then(|slot| slot.transport.as_ref()) {
            let mut ledger = transport.deliveries.lock();
            ledger.generation = ledger
                .generation
                .checked_add(1)
                .expect("delivery generation exhausted");
            ledger.chat_stopped = true;
            ledger.notices.clear();
            transport.broker.fence_task_consent(session);
        }
    }

    pub(crate) fn computer_use_chat_started(&self, session: &str) {
        let slots = self.computer_use_connections.lock();
        if let Some(transport) = slots.get(session).and_then(|slot| slot.transport.as_ref()) {
            let mut ledger = transport.deliveries.lock();
            if ledger.chat_stopped {
                // Explicit user send permits future decisions, but cannot revive
                // pre-Stop grants or deliver the old stopped notice as a prompt.
                if let BearerClass::Live(context) =
                    transport.broker.classify_bearer(&transport.token)
                {
                    transport
                        .broker
                        .take_no_authority(session, context.owner_nonce);
                }
                ledger.chat_stopped = false;
            }
        }
    }

    pub(crate) async fn prompt_computer_use(
        &self,
        delivery: &DecisionDelivery,
        turn: &str,
        turn_is_current: &(dyn Fn() -> Result<(), String> + Sync),
    ) -> DeliveryAttempt {
        let message = delivery.message();
        // Persist only the fingerprint, off the runtime thread, before stdin
        // admission. A failed store write admits no continuation and remains
        // a redacted, retryable diagnostic instead of importing a fake bubble.
        if let Some(path) = delivery.transport.history_path.clone() {
            let agent = delivery.agent_session().to_string();
            let body = message.clone();
            let stored = tauri::async_runtime::spawn_blocking(move || {
                crate::computer_use::host_history::record(&path, &agent, &body)
            })
            .await;
            let mut ledger = delivery.transport.deliveries.lock();
            if !matches!(stored, Ok(Ok(()))) {
                ledger.last_error =
                    Some("Computer Use Host decision history unavailable; delivery pending");
                ledger.retry_after = Some(Instant::now() + Duration::from_secs(5));
                return DeliveryAttempt::NotStarted;
            }
            ledger.last_error = None;
            ledger.retry_after = None;
        }
        let admitted = AtomicBool::new(false);
        let admission = || {
            turn_is_current()?;
            let slots = self.computer_use_connections.lock();
            let current = slots
                .get(delivery.app_session())
                .and_then(|slot| slot.transport.as_ref());
            if !self.is_alive()
                || !current.is_some_and(|transport| Arc::ptr_eq(transport, &delivery.transport))
                || !delivery.transport.bound_to(delivery.agent_session())
            {
                return Err("Computer Use connection retired".into());
            }
            let mut ledger = delivery.transport.deliveries.lock();
            if ledger.chat_stopped
                || ledger.generation != delivery.generation
                || ledger
                    .admitted
                    .iter()
                    .any(|id| id == delivery.dispatch_id())
            {
                return Err("Computer Use delivery retired".into());
            }
            delivery.validate()?;
            match &delivery.decision {
                Decision::Continue(claim) => delivery
                    .transport
                    .broker
                    .consume_continuation(claim)
                    .map_err(|_| "Computer Use decision retired".to_string())?,
                Decision::NoAuthority(notice) => {
                    if !ledger
                        .notices
                        .iter()
                        .any(|row| row.dispatch_id == notice.dispatch_id)
                    {
                        return Err("Computer Use notice retired".into());
                    }
                    ledger
                        .notices
                        .retain(|row| row.dispatch_id != notice.dispatch_id);
                }
            }
            if ledger.admitted.len() == MAX_RECEIPTS {
                ledger.admitted.pop_front();
            }
            ledger.admitted.push_back(delivery.dispatch_id().into());
            admitted.store(true, Ordering::SeqCst);
            Ok(())
        };
        match self
            .prompt_for_admitting(
                delivery.agent_session(),
                &message,
                Some(&admission),
                Some(turn),
            )
            .await
        {
            Ok(()) => DeliveryAttempt::Completed,
            Err(error) if admitted.load(Ordering::SeqCst) => DeliveryAttempt::Unknown(error),
            Err(_) => DeliveryAttempt::NotStarted,
        }
    }
}
