//! Remote Host traits with bounded RPC waits and fail-closed disconnection.
use super::{data, PhysicalActivity, StopMirror};
use crate::Host;
use async_trait::async_trait;
use gcu_host_api::*;
use gcu_host_protocol::{
    messages as w,
    server::{Peer, RPC_TIMEOUT},
    Error,
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
/// Host adapter scoped to an App session; wire identity never comes from a tool.
pub struct RemoteHost {
    pub(super) peer: Peer,
    session_id: String,
    policy: Arc<Mutex<(u64, PolicyConfig)>>,
    physical: Arc<dyn PhysicalActivity>,
    stop: StopMirror,
}
impl RemoteHost {
    /// Load initial policy before opening any session. Five-second RPC deadline;
    /// failure refuses startup. Physical input is injected by the engine driver.
    pub async fn new(
        peer: Peer,
        stop: StopMirror,
        physical: Arc<dyn PhysicalActivity>,
    ) -> Result<Arc<Self>, Error> {
        let policy: w::Policy = peer
            .call("host.policy.load", &w::Empty {}, RPC_TIMEOUT)
            .await?;
        gcu_protocol::builtin_browser::with_shared(|book| book.note_policy(policy.version));
        crate::browser_tool::install_remote(peer.clone());
        Ok(Arc::new(Self {
            peer,
            session_id: String::new(),
            policy: Arc::new(Mutex::new((policy.version, data::policy(policy)))),
            physical,
            stop,
        }))
    }
    /// Scope traits to the App's real session identity. Memory only, no wait.
    pub fn for_session(&self, id: &str) -> Arc<Self> {
        Arc::new(Self {
            peer: self.peer.clone(),
            session_id: id.into(),
            policy: self.policy.clone(),
            physical: self.physical.clone(),
            stop: self.stop.clone(),
        })
    }
    /// Assemble the same core Host used by route A, with App as sole authority.
    pub fn host(self: &Arc<Self>) -> Host {
        Host {
            approvals: self.clone(),
            policy: self.clone(),
            events: self.clone(),
            activity: self.clone(),
        }
    }
    /// Shared Stop mirror for the independent authenticated control task.
    pub fn stop(&self) -> StopMirror {
        self.stop.clone()
    }
    /// Refresh strictly after a new Host revision; never accept rollback or stale
    /// loads. Five-second deadline. Caller pauses affected work until completion.
    pub async fn policy_changed(&self, version: u64) -> Result<(), Error> {
        if version <= self.policy.lock().unwrap_or_else(|e| e.into_inner()).0 {
            return Err(Error::Invalid);
        }
        // Revoke cached persistent/foreground allowances before the reload wait;
        // a failed reload leaves a conservative policy instead of stale authority.
        {
            let mut current = self.policy.lock().unwrap_or_else(|e| e.into_inner());
            current.1.always.clear();
            current.1.allow_foreground = ForegroundPolicy::Never;
            current.1.sensitive_action_confirmation = ConfirmationLevel::Strict;
        }
        let policy: w::Policy = self
            .peer
            .call("host.policy.load", &w::Empty {}, RPC_TIMEOUT)
            .await?;
        let mut current = self.policy.lock().unwrap_or_else(|e| e.into_inner());
        if policy.version < version || policy.version <= current.0 {
            return Err(Error::Invalid);
        }
        gcu_protocol::builtin_browser::with_shared(|book| book.note_policy(policy.version));
        *current = (policy.version, data::policy(policy));
        Ok(())
    }
    /// Mark activity-guard degradation using existing DoctorReport fields. No IO;
    /// foreground never/ask policy and Stop remain enforced.
    pub fn doctor(
        &self,
        mut report: gcu_core::driver::DoctorReport,
    ) -> gcu_core::driver::DoctorReport {
        if self.physical.ms_since_physical_input().is_none() {
            report.capabilities.user_activity_guard = false;
            report
                .issues
                .push("user_activity_guard: unavailable".into());
        }
        report
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(self.load().timeout_secs.min(120))
    }
}
#[async_trait]
impl ApprovalProvider for RemoteHost {
    async fn request_app(
        &self,
        _: &SessionCtx,
        app: &AppInfo,
        reason: Option<&ModelReason>,
        risk: Risk,
        cancel: &Cancel,
    ) -> ApprovalDecision {
        if cancel.is_cancelled() || !self.peer.connected() {
            return ApprovalDecision::Deny;
        }
        let params = w::RequestApp {
            session_id: self.session_id.clone(),
            app: data::app(app),
            reason: reason.map(|r| r.as_str().into()),
            risk: if risk == Risk::High {
                w::Risk::High
            } else {
                w::Risk::Low
            },
        };
        let result: Result<w::AppDecision, Error> = tokio::select! {biased;_=cancel.cancelled()=>return ApprovalDecision::Deny,result=self.peer.call("host.approval.requestApp",&params,self.timeout())=>result};
        if cancel.is_cancelled() || !self.peer.connected() {
            return ApprovalDecision::Deny;
        }
        match result {
            Ok(r) => match r.decision {
                w::Decision::Turn => ApprovalDecision::Turn,
                w::Decision::Session => ApprovalDecision::Session,
                w::Decision::Always => ApprovalDecision::Always,
                w::Decision::Deny => ApprovalDecision::Deny,
            },
            Err(Error::Timeout | Error::Remote(-32001)) => ApprovalDecision::Timeout,
            Err(_) => ApprovalDecision::Deny,
        }
    }
    async fn confirm_action(
        &self,
        _: &SessionCtx,
        app: &AppInfo,
        summary: &ActionSummary,
        crop: Option<&ImageRef>,
        cancel: &Cancel,
    ) -> bool {
        if cancel.is_cancelled() {
            return false;
        }
        let params = w::ConfirmAction {
            session_id: self.session_id.clone(),
            app: data::app(app),
            summary: w::ActionSummary {
                text: summary.text.clone(),
                snapshot_id: summary.snapshot_id.clone(),
                action_digest: summary.action_digest,
            },
            crop: crop.map(data::image),
        };
        let result: Result<w::Confirmed, Error> = tokio::select! {biased;_=cancel.cancelled()=>return false,result=self.peer.call("host.approval.confirmAction",&params,self.timeout())=>result};
        !cancel.is_cancelled() && self.peer.connected() && result.is_ok_and(|r| r.confirmed)
    }
    async fn handoff(&self, _: &SessionCtx, app: &AppInfo, reason: HandoffReason, cancel: &Cancel) {
        let reason = match reason {
            HandoffReason::Login => "login".into(),
            HandoffReason::Captcha => "captcha".into(),
            HandoffReason::SystemPrompt => "system_prompt".into(),
            HandoffReason::Other(s) => s,
        };
        let params = w::Handoff {
            session_id: self.session_id.clone(),
            app: data::app(app),
            reason,
        };
        let _: Result<w::Empty, Error> = tokio::select! {biased;_=cancel.cancelled()=>return,result=self.peer.call("host.approval.handoff",&params,Duration::from_secs(120))=>result};
    }
}
impl PolicyStore for RemoteHost {
    fn load(&self) -> PolicyConfig {
        self.policy
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .clone()
    }
    fn always_allowed(&self, id: &AppIdentity) -> bool {
        self.peer.connected() && self.load().always.iter().any(|g| g.grant_eq(id))
    }
    // engine.policy.changed awaits policy_changed before replying; this synchronous
    // API must never block a runtime waiting for another RPC.
    fn refresh(&self) {}
}
impl UserActivity for RemoteHost {
    fn ms_since_physical_input(&self) -> Option<u64> {
        self.physical.ms_since_physical_input()
    }
    fn stop_requested(&self) -> bool {
        self.latched_stop()
    }
    fn latched_stop(&self) -> bool {
        if !self.peer.connected() {
            self.stop.latch(None);
        }
        self.stop.stopped(&self.session_id)
    }
    fn clear_stop_by_user(&self) {
        if self.peer.connected() {
            self.stop.clear(Some(&self.session_id));
        }
    }
}
impl EventSink for RemoteHost {
    fn emit(&self, _: &SessionCtx, event: CuEvent) {
        let _ = self.peer.notify(
            "host.event.emit",
            &w::Event {
                session_id: self.session_id.clone(),
                tool: event.tool,
                target: event.target,
                outcome: event.outcome,
                duration_ms: event.duration_ms,
                thumbnail: event.thumbnail.as_ref().map(data::image),
            },
        );
    }
}
impl OverlayProvider for RemoteHost {
    fn show_target(&self, _: &SessionCtx, window: &WindowInfo, at: ScreenPoint, kind: CursorKind) {
        let kind = match kind {
            CursorKind::Pointer => w::CursorKind::Pointer,
            CursorKind::Caret => w::CursorKind::Caret,
            CursorKind::Drag => w::CursorKind::Drag,
            CursorKind::Scroll => w::CursorKind::Scroll,
        };
        let _ = self.peer.notify(
            "host.overlay.show",
            &w::OverlayShow {
                session_id: self.session_id.clone(),
                window_id: window.id.clone(),
                title: window.title.clone(),
                frame: window.frame,
                scale: window.scale,
                x: at.x,
                y: at.y,
                kind,
            },
        );
    }
    fn status(&self, _: &SessionCtx, text: &StatusText) {
        let _ = self.peer.notify(
            "host.overlay.status",
            &w::OverlayStatus {
                session_id: self.session_id.clone(),
                message: text.message.clone(),
                foreground: text.foreground,
            },
        );
    }
    fn hide(&self, _: &SessionCtx) {
        let _ = self.peer.notify(
            "host.overlay.hide",
            &w::SessionId {
                session_id: self.session_id.clone(),
            },
        );
    }
    fn exclude_from_capture_supported(&self) -> bool {
        false
    }
}
#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
