//! Soft-respawn lifecycle. UI emission is optional; the state/process path is shared.
use super::*;
use tauri::{AppHandle, Emitter};

enum Retirement {
    NoProcess,
    Busy,
    Detached {
        acp: Arc<crate::acp_client::AcpClient>,
        process_id: String,
        was_live: bool,
    },
}

impl SessionManager {
    /// Capture focus once; subsequent map changes cannot redirect this request.
    pub async fn soft_respawn_with_reason(&self, app: &AppHandle, reason: &str) {
        let target = self.inner.lock().as_ref().map(|s| s.app_session_id.clone());
        if let Some(target) = target {
            self.soft_respawn_for_session(app, &target, reason).await;
        }
    }

    pub(super) async fn soft_respawn_for_session(
        &self,
        app: &AppHandle,
        session_id: &str,
        reason: &str,
    ) {
        self.soft_respawn_for_session_core(Some(app), session_id, reason)
            .await;
    }

    pub(crate) async fn soft_respawn_for_session_core(
        &self,
        app: Option<&AppHandle>,
        session_id: &str,
        reason: &str,
    ) {
        // Consume requests only before detaching. A later request may belong to
        // settings changed while kill yields, or to a newly connected process.
        self.pending_soft_respawn.lock().remove(session_id);
        self.retire_idle_target(app, session_id, reason).await;
    }

    /// Check busy and detach the explicit owner under the same map guard.
    /// Never nest map locks: park/demote uses a different acquisition order.
    fn detach_idle_target(&self, session_id: &str) -> Retirement {
        let _transfer = self.runtime_transfer.lock();
        {
            let mut live = self.inner.lock();
            if let Some(s) = live.as_mut().filter(|s| s.app_session_id == session_id) {
                if Self::live_session_is_busy(s) {
                    return Retirement::Busy;
                }
                let Some(acp) = s.acp.take() else {
                    return Retirement::NoProcess;
                };
                let process_id = std::mem::take(&mut s.process_id);
                s.needs_history_bootstrap = false;
                s.fsm.soft_disconnect();
                return Retirement::Detached {
                    acp,
                    process_id,
                    was_live: true,
                };
            }
        }
        {
            let mut background = self.background.lock();
            if let Some(s) = background.get(session_id) {
                if Self::live_session_is_busy(s) {
                    return Retirement::Busy;
                }
                let mut s = background
                    .remove(session_id)
                    .expect("checked under the same guard");
                return match s.acp.take() {
                    Some(acp) => Retirement::Detached {
                        acp,
                        process_id: s.process_id,
                        was_live: false,
                    },
                    None => Retirement::NoProcess,
                };
            }
        }
        let parked = self.parked.lock().remove(session_id);
        match parked {
            Some(p) => Retirement::Detached {
                acp: p.acp,
                process_id: p.process_id,
                was_live: false,
            },
            None => Retirement::NoProcess,
        }
    }

    async fn retire_idle_target(&self, app: Option<&AppHandle>, session_id: &str, reason: &str) {
        match self.detach_idle_target(session_id) {
            Retirement::Busy => {
                // A newer request queued after this one was taken wins.
                self.pending_soft_respawn
                    .lock()
                    .entry(session_id.to_owned())
                    .or_insert_with(|| reason.to_owned());
                tracing::info!(session = %session_id, reason, "soft-respawn deferred: target busy");
            }
            Retirement::NoProcess => {}
            Retirement::Detached {
                acp,
                process_id,
                was_live,
            } => {
                #[cfg(test)]
                tests::pause_after_detach(session_id);
                if self.has_other_process_tenant(&process_id, session_id) {
                    tracing::info!(session = %session_id, process = %process_id, reason, "soft-respawn detached shared ACP without killing co-tenant");
                } else {
                    Self::kill_acp_bounded(&acp).await;
                }
                if let Some(app) = app {
                    if was_live {
                        let _ = app.emit(
                            "session://agent_soft_respawn",
                            serde_json::json!({ "sessionId": session_id, "reason": reason }),
                        );
                    }
                    Self::emit_state(app, &self.snapshot());
                }
            }
        }
    }

    /// Apply a queued rebind when its session becomes idle.
    pub async fn flush_pending_soft_respawn(&self, app: &AppHandle, session_id: &str) {
        self.flush_pending_soft_respawn_core(Some(app), session_id)
            .await;
    }

    async fn flush_pending_soft_respawn_core(&self, app: Option<&AppHandle>, session_id: &str) {
        let reason = { self.pending_soft_respawn.lock().remove(session_id) };
        let Some(reason) = reason else {
            return;
        };
        #[cfg(test)]
        tests::pause_before_retirement(session_id);
        self.retire_idle_target(app, session_id, &reason).await;
    }
}

#[cfg(test)]
#[path = "retirement_tests.rs"]
mod tests;
