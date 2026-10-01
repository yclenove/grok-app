//! Requests and grants share one lock. Each item is delivered at most once.
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{existing::expire_pairing, ExistingTabHost, Inner};
use crate::adapter::CaptureOptions;
use crate::browser::extension_protocol::*;
use crate::error::BrokerError;
use crate::pairing::PairingConnection;

type ObservationReceiver = Receiver<Result<ExtensionObservation, BrokerError>>;
type EnqueuedObservation = (ExtensionRequest, ObservationReceiver);

const MAX_PENDING: usize = 8;
const REQUEST_TTL: Duration = Duration::from_secs(10);

pub(super) struct PendingExtension {
    request: ExtensionRequest,
    deadline: Instant,
    delivered: bool,
    sender: SyncSender<Result<ExtensionObservation, BrokerError>>,
    cancellation: crate::execution::ActionCancellation,
}

pub(super) fn request_busy(state: &Inner, tab: &str) -> bool {
    state.extension_requests.len()
        + state.extension_completions.len()
        + super::actions::additional_occupancy(state)
        >= MAX_PENDING
        || super::actions::has_tab(state, tab)
        || state
            .extension_requests
            .iter()
            .any(|pending| pending.request.tab_id == tab)
}

fn live_request(state: &Inner, request: &ExtensionRequest) -> bool {
    let Some(record) = state.tabs.get(&request.tab_id) else {
        return false;
    };
    let Some(offer) = state.shared.get(&request.tab_id) else {
        return false;
    };
    state.pairing.live_connection().as_ref() == Some(&request.connection)
        && record.info.user_owned
        && record.info.borrowed
        && !record.info.closed
        && record.disconnect.is_none()
        && record.info.session == request.session
        && record.info.run_id == request.run_id
        && record.info.generation == request.grant_generation
        && record.info.document_generation == request.document_generation
        && record.info.connection_generation == request.connection.generation
        && offer.document_generation == request.document_generation
        && offer.connection_generation == request.connection.generation
        && record.pairing_token.as_deref() == state.pairing.session_key()
}

pub(super) fn sweep_requests(state: &mut Inner) {
    let now = Instant::now();
    // Bounded queue: collect IDs first to avoid aliasing the registry during retain.
    let stale: Vec<_> = state
        .extension_requests
        .iter()
        .filter(|pending| {
            now >= pending.deadline
                || pending.cancellation.check().is_err()
                || !live_request(state, &pending.request)
        })
        .map(|pending| pending.request.request_id.clone())
        .collect();
    state.extension_requests.retain(|pending| {
        if stale.contains(&pending.request.request_id) {
            let error = if now >= pending.deadline {
                BrokerError::Timeout
            } else {
                BrokerError::TargetUnauthorized
            };
            let _ = pending.sender.try_send(Err(error));
            false
        } else {
            true
        }
    });
}

impl ExistingTabHost {
    #[cfg(test)]
    pub(super) fn enqueue_extension_observe(
        &self,
        session: &str,
        run: &str,
        tab: &str,
        screenshot: bool,
    ) -> Result<EnqueuedObservation, BrokerError> {
        self.enqueue_extension_capture(session, run, tab, CaptureOptions::model(screenshot))
    }

    fn enqueue_extension_capture(
        &self,
        session: &str,
        run: &str,
        tab: &str,
        options: CaptureOptions,
    ) -> Result<EnqueuedObservation, BrokerError> {
        options.cancellation.check().map_err(BrokerError::Adapter)?;
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        let connection = state
            .pairing
            .live_connection()
            .ok_or(BrokerError::TargetUnauthorized)?;
        let record = state.tabs.get(tab).ok_or(BrokerError::TargetUnauthorized)?;
        if session.is_empty()
            || session.len() > 256
            || run.is_empty()
            || run.len() > 256
            || tab.len() > 128
            || record.info.session != session
            || record.info.run_id != run
        {
            return Err(BrokerError::TargetUnauthorized);
        }
        let document_generation = record.info.document_generation;
        let grant_generation = record.info.generation;
        if request_busy(&state, tab) {
            return Err(BrokerError::Schema("extension request busy".into()));
        }
        state.extension_sequence = state
            .extension_sequence
            .checked_add(1)
            .filter(|seq| *seq <= crate::protocol::JS_MAX_SAFE_INTEGER)
            .ok_or_else(|| BrokerError::Schema("extension sequence exhausted".into()))?;
        let request = ExtensionRequest {
            protocol: EXTENSION_PROTOCOL,
            request_id: uuid::Uuid::new_v4().to_string(),
            sequence: state.extension_sequence,
            deadline_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
                + REQUEST_TTL.as_millis() as u64,
            connection,
            session: session.into(),
            run_id: run.into(),
            tab_id: tab.into(),
            document_generation,
            grant_generation,
            command: ExtensionCommand::Observe {
                snapshot_id: uuid::Uuid::new_v4().to_string(),
                screenshot: options.screenshot,
                preview: !options.for_model,
            },
        };
        if !live_request(&state, &request) {
            return Err(BrokerError::TargetUnauthorized);
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        state.extension_requests.push_back(PendingExtension {
            request: request.clone(),
            deadline: Instant::now() + REQUEST_TTL,
            delivered: false,
            sender,
            cancellation: options.cancellation,
        });
        Ok((request, receiver))
    }

    /// Called by a Host-authorized adapter. Pairing or sharing alone cannot enqueue.
    pub fn observe_existing_document(
        &self,
        session: &str,
        run: &str,
        tab: &str,
        screenshot: bool,
    ) -> Result<ExtensionObservation, BrokerError> {
        self.capture_existing_document(session, run, tab, CaptureOptions::model(screenshot))
    }

    pub fn capture_existing_document(
        &self,
        session: &str,
        run: &str,
        tab: &str,
        options: CaptureOptions,
    ) -> Result<ExtensionObservation, BrokerError> {
        let cancellation = options.cancellation.clone();
        let (request, receiver) = self.enqueue_extension_capture(session, run, tab, options)?;
        loop {
            match receiver.recv_timeout(Duration::from_millis(25)) {
                Ok(result) => {
                    let state = self.inner.lock();
                    return if cancellation.check().is_ok() && live_request(&state, &request) {
                        result
                    } else {
                        Err(BrokerError::TargetUnauthorized)
                    };
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(BrokerError::TargetUnauthorized)
                }
                Err(mpsc::RecvTimeoutError::Timeout) => sweep_requests(&mut self.inner.lock()),
            }
        }
    }

    pub fn poll_extension_request(
        &self,
        origin: &str,
        token: &str,
        connection: &PairingConnection,
    ) -> Result<Option<ExtensionRequest>, String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        state
            .pairing
            .authenticate_connection(origin, token, connection)?;
        // One in-flight browser request for this connection, even with concurrent pollers.
        if state.extension_requests.iter().any(|p| p.delivered) {
            return Ok(None);
        }
        if let Some(pending) = state.extension_requests.front_mut() {
            pending.delivered = true;
            return Ok(Some(pending.request.clone()));
        }
        Ok(None)
    }

    pub fn complete_extension_request(
        &self,
        origin: &str,
        token: &str,
        result: ExtensionResult,
    ) -> Result<(), String> {
        {
            let mut state = self.inner.lock();
            expire_pairing(&mut state);
            state
                .pairing
                .authenticate_connection(origin, token, &result.request.connection)?;
            if !state
                .extension_requests
                .iter()
                .any(|p| p.request == result.request && p.delivered)
                || !live_request(&state, &result.request)
            {
                return Err("extension result does not match a live request".into());
            }
        }
        // Decode only authenticated pending data, outside the authority lock. Revocation
        // can proceed while decoding; the complete request is revalidated below.
        let ExtensionCommand::Observe {
            snapshot_id,
            screenshot,
            preview,
        } = &result.request.command;
        let observation = match result.outcome {
            ExtensionOutcome::Observation { observation } => {
                if !observation.validate(snapshot_id, *screenshot) {
                    return Err("invalid extension observation".into());
                }
                Ok(observation)
            }
            ExtensionOutcome::Rejected { .. } => Err(BrokerError::Adapter(
                "extension observation rejected".into(),
            )),
        };
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        state
            .pairing
            .authenticate_connection(origin, token, &result.request.connection)?;
        let index = state
            .extension_requests
            .iter()
            .position(|pending| pending.request == result.request && pending.delivered)
            .ok_or("extension result does not match a pending request")?;
        if !live_request(&state, &result.request) {
            return Err("extension grant changed".into());
        }
        if observation.as_ref().is_ok_and(|observation| {
            state
                .shared
                .get(&result.request.tab_id)
                .is_none_or(|offer| offer.url != observation.url)
        }) {
            return Err("invalid extension observation URL".into());
        }
        if !preview {
            if let Ok(observation) = &observation {
                let record = state
                    .tabs
                    .get_mut(&result.request.tab_id)
                    .ok_or("extension tab unavailable")?;
                record.current_observation =
                    Some(crate::browser::host_allowlist::HostObservation {
                        page_id: observation.document_id.clone(),
                        page_generation: result.request.document_generation,
                        snapshot_id: observation.snapshot_id.clone(),
                        refs: observation
                            .nodes
                            .iter()
                            .filter(|node| !node.disabled)
                            .map(|node| node.element_ref.clone())
                            .collect(),
                        has_image: observation.screenshot.is_some(),
                        image_width: observation
                            .screenshot
                            .as_ref()
                            .map_or(0, |image| image.width),
                        image_height: observation
                            .screenshot
                            .as_ref()
                            .map_or(0, |image| image.height),
                    });
            }
        }
        let pending = state
            .extension_requests
            .remove(index)
            .ok_or("extension request changed")?;
        pending
            .sender
            .try_send(observation)
            .map_err(|_| "extension receiver unavailable".into())
    }

    pub fn existing_operations_idle(&self, run: &str) -> bool {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        state.extension_completions.idle(run)
            && !state
                .extension_actions
                .iter()
                .any(|pending| pending.request.run_id == run)
            && !state
                .extension_requests
                .iter()
                .any(|pending| pending.request.run_id == run)
    }

    pub fn cancel_existing_operations(&self, run: &str) {
        let mut state = self.inner.lock();
        state.extension_completions.cancel_run(Some(run));
        for record in state
            .tabs
            .values_mut()
            .filter(|record| record.info.user_owned && record.info.run_id == run)
        {
            record.info.borrowed = false;
            record.info.generation = record.info.generation.saturating_add(1);
            record.current_observation = None;
            record.info.preview_generation = 0;
        }
        sweep_requests(&mut state);
        super::completion::sweep(&mut state);
    }
}

#[cfg(test)]
mod tests;
