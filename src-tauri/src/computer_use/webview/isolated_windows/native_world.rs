//! Exact native-world ownership and cached isolated-document identity.
//! No COM objects cross into the failure-only exit witness worker.

use super::{process, ScriptOperation, MAX_WORLD_ATTEMPTS};
use parking_lot::Mutex;
use std::sync::{mpsc::Sender, Arc};

#[derive(Default)]
pub(crate) struct NativeWorld(Mutex<WorldState>);

#[derive(Default)]
struct WorldState {
    generation: u64,
    attempts: usize,
    context: Option<String>,
    retired: bool,
    closed: bool,
    process_observed: bool,
    pending: Option<Pending>,
}

struct Pending {
    ticket: uuid::Uuid,
    generation: u64,
    operation: ScriptOperation,
    reply: Sender<Result<String, String>>,
    witness: Option<Arc<dyn process::PhysicalExit>>,
    exit_watch_started: bool,
}

impl std::fmt::Debug for NativeWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.0.lock();
        f.debug_struct("NativeWorld")
            .field("generation", &state.generation)
            .field("retired", &state.retired)
            .field("closed", &state.closed)
            .field("pending", &state.pending.is_some())
            .finish_non_exhaustive()
    }
}

impl NativeWorld {
    pub(super) fn reserve(&self, generation: u64) -> Result<Option<String>, String> {
        let mut state = self.0.lock();
        if state.closed {
            return Err("WebView native view closed".into());
        }
        if state.generation > generation || (state.generation == generation && state.retired) {
            return Err("WebView isolated document retired".into());
        }
        if state.generation != generation {
            state.generation = generation;
            state.attempts = 0;
            state.context = None;
            state.retired = false;
        }
        if state.context.is_some() {
            return Ok(state.context.clone());
        }
        if state.attempts >= MAX_WORLD_ATTEMPTS {
            return Err("WebView isolated context setup exhausted; navigate to retry".into());
        }
        state.attempts += 1;
        Ok(None)
    }

    pub(super) fn publish(&self, generation: u64, context: String) -> bool {
        let mut state = self.0.lock();
        if state.generation != generation || state.retired || state.closed {
            return false;
        }
        state.context = Some(context);
        true
    }

    pub(super) fn admit(
        &self,
        ticket: uuid::Uuid,
        generation: u64,
        operation: ScriptOperation,
        reply: Sender<Result<String, String>>,
    ) -> Result<(), String> {
        let mut state = self.0.lock();
        if state.closed {
            return Err("WebView native view closed".into());
        }
        if state.pending.is_some() {
            return Err("WebView native execution is still pending".into());
        }
        state.pending = Some(Pending {
            ticket,
            generation,
            operation,
            reply,
            witness: None,
            exit_watch_started: false,
        });
        Ok(())
    }

    pub(super) fn completed(&self, ticket: uuid::Uuid) {
        let mut state = self.0.lock();
        if state
            .pending
            .as_ref()
            .is_some_and(|pending| pending.ticket == ticket)
        {
            state.pending.take();
        }
    }

    pub(super) fn attach(
        &self,
        ticket: uuid::Uuid,
        witness: Arc<dyn process::PhysicalExit>,
    ) -> bool {
        let mut state = self.0.lock();
        if state.retired || state.closed {
            return false;
        }
        let Some(pending) = state
            .pending
            .as_mut()
            .filter(|pending| pending.ticket == ticket)
        else {
            return false;
        };
        pending.witness = Some(witness);
        true
    }

    pub(super) fn process_failed(&self, failure: process::Failure) -> Option<uuid::Uuid> {
        let (pending, watch) = {
            let mut state = self.0.lock();
            if let Some(pending) = &state.pending {
                state.generation = state.generation.max(pending.generation);
            }
            state.retired = true;
            state.context = None;
            // A late event from an old renderer must not settle a replacement.
            // No witness means this job is still in no-script setup. Otherwise
            // require the held handle of THIS evaluation's renderer to signal.
            let safe = state.pending.as_ref().is_some_and(|pending| {
                pending
                    .witness
                    .as_ref()
                    .is_none_or(|witness| witness.exited())
            });
            if safe {
                (state.pending.take(), None)
            } else {
                let watch = state.pending.as_mut().and_then(|pending| {
                    if pending.exit_watch_started {
                        return None;
                    }
                    pending.exit_watch_started = true;
                    Some(pending.ticket)
                });
                (None, watch)
            }
        };
        if let Some(pending) = pending {
            finish_exited(pending, failure);
        }
        watch
    }

    /// Seal this native instance before its controller is closed. A normal
    /// Close can discard every event handler; the retained handle must outlive
    /// that native callback surface. Close itself is not renderer-exit proof.
    pub(super) fn close(&self) -> Option<uuid::Uuid> {
        self.0.lock().closed = true;
        self.process_failed(process::Failure::NativeViewClosed)
    }

    /// True means the exact ticket still needs physical exit evidence. Neither
    /// a live handle nor a wait failure can be expired into false idle.
    pub(super) fn poll_exit(&self, ticket: uuid::Uuid, failure: process::Failure) -> bool {
        let pending = {
            let mut state = self.0.lock();
            let Some(pending) = state
                .pending
                .as_ref()
                .filter(|pending| pending.ticket == ticket)
            else {
                return false;
            };
            if pending
                .witness
                .as_ref()
                .is_some_and(|witness| !witness.exited())
            {
                return true;
            }
            state.pending.take()
        };
        if let Some(pending) = pending {
            finish_exited(pending, failure);
        }
        false
    }

    pub(super) fn exit_watch_failed(&self, ticket: uuid::Uuid) {
        let mut state = self.0.lock();
        if let Some(pending) = state
            .pending
            .as_mut()
            .filter(|pending| pending.ticket == ticket)
        {
            pending.exit_watch_started = false;
        }
    }
}

fn finish_exited(pending: Pending, failure: process::Failure) {
    pending.operation.finished();
    let error = match failure {
        process::Failure::MainRendererExited => "WebView renderer exited; outcome unknown",
        process::Failure::BrowserExited => "WebView browser exited; outcome unknown",
        process::Failure::NativeViewClosed => "WebView closed; outcome unknown",
    };
    let _ = pending.reply.send(Err(error.into()));
}

impl NativeWorld {
    pub(super) fn process_observed(&self) -> bool {
        self.0.lock().process_observed
    }
    pub(super) fn mark_process_observed(&self) {
        self.0.lock().process_observed = true;
    }
    #[cfg(feature = "computer-use-probe")]
    pub(super) fn probe_witness(
        &self,
        ticket: uuid::Uuid,
    ) -> Option<Arc<dyn process::PhysicalExit>> {
        self.0
            .lock()
            .pending
            .as_ref()
            .filter(|pending| pending.ticket == ticket)
            .and_then(|pending| pending.witness.clone())
    }
    #[cfg(feature = "computer-use-probe")]
    pub(super) fn diagnostic(&self) -> String {
        let state = self.0.lock();
        match &state.pending {
            Some(pending) => format!(
                "pending=true watcher={} {}",
                pending.exit_watch_started,
                pending
                    .witness
                    .as_ref()
                    .map_or_else(|| "witness=none".into(), |witness| witness.diagnostic())
            ),
            None => "pending=false".into(),
        }
    }
    #[cfg(test)]
    pub(super) fn pending_ticket(&self) -> Option<uuid::Uuid> {
        self.0.lock().pending.as_ref().map(|pending| pending.ticket)
    }
}
