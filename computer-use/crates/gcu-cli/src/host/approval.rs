//! Trusted approval decisions. The `gcu` binary does not read `GCU_APPROVAL_TEST_AUTOCLICK`.
use super::{dialog, page, policy_store::HostPolicy};
use crate::{exit::Failure, private};
use async_trait::async_trait;
use gcu_host_api::{
    ActionSummary, AppInfo, ApprovalDecision, ApprovalProvider, Cancel, HandoffReason, ImageRef,
    ModelReason, PolicyStore, Risk, SessionCtx,
};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Inputs for one approval wait. Tests pass `autoclick`; production leaves it empty.
pub struct HookRequest<'a> {
    /// Grant file. `None` never persists.
    pub file: Option<&'a Path>,
    /// OS-verified app. Name and icon are display-only.
    pub app: &'a AppInfo,
    /// Model-provided reason. Shown as plain text with a model-provided label.
    pub reason: Option<&'a str>,
    /// Engine risk.
    pub risk: Risk,
    /// How long to wait for a person or a `timeout` hook.
    pub timeout: Duration,
    /// When false, Always is denied and nothing is written.
    pub always_enabled: bool,
    /// Cancel wins over a late choice.
    pub cancel: &'a Cancel,
    /// `turn`, `session`, `always`, `deny`, `timeout`, `cancel`, `confirm-allow`, or `confirm-deny`.
    pub autoclick: Option<&'a str>,
    /// `GCU_APPROVAL` value. Honored only in eval debug builds.
    pub mode: Option<&'a str>,
    /// When false, no dialog is spawned.
    pub interactive: bool,
    /// Engine action summary for confirmation.
    pub summary: &'a str,
    /// Whether a redacted crop is attached. Bytes stay in memory.
    pub crop_attached: bool,
}

/// Read the test hook. The production provider never calls this.
pub fn autoclick_from_env() -> Option<String> {
    std::env::var("GCU_APPROVAL_TEST_AUTOCLICK")
        .ok()
        .filter(|value| !value.is_empty())
}

/// Resolve an app decision. Always is written only after this future is still the winner.
pub async fn decide_with_autoclick(req: &HookRequest<'_>) -> ApprovalDecision {
    if req.cancel.is_cancelled() {
        return ApprovalDecision::Deny;
    }
    let decision = tokio::select! {
        biased;
        _ = req.cancel.cancelled() => ApprovalDecision::Deny,
        _ = tokio::time::sleep(req.timeout) => ApprovalDecision::Timeout,
        decision = choose_app(req) => decision,
    };
    if decision != ApprovalDecision::Always || req.cancel.is_cancelled() || !req.always_enabled {
        return if decision == ApprovalDecision::Always {
            ApprovalDecision::Deny
        } else {
            decision
        };
    }
    persist_always(req).await
}

/// Resolve a sensitive confirmation. Deny, timeout, and cancel are false.
pub async fn confirm_for_test(req: &HookRequest<'_>) -> bool {
    if req.cancel.is_cancelled() {
        return false;
    }
    let answer = async {
        if let Some(policy) = eval_policy(req.mode) {
            // The Host API shares this callback between sensitive and foreground
            // confirmations. Neither is implied by an app grant. Until trusted,
            // action-bound manifest approvals exist, both default to deny; never
            // infer the confirmation kind or permission from summary text.
            return policy.sensitive && policy.foreground;
        }
        if let Some(hook) = req.autoclick {
            return match hook {
                "confirm-allow" | "turn" | "session" | "always" => true,
                "timeout" | "cancel" => std::future::pending().await,
                _ => false,
            };
        }
        if !req.interactive {
            return false;
        }
        dialog::ask(
            &page::confirm_prompt(
                req.app,
                req.summary,
                req.crop_attached,
                seconds(req.timeout),
            ),
            req.cancel,
        )
        .await
            == dialog::Choice::Allow
    };
    tokio::select! {
        biased;
        _ = req.cancel.cancelled() => false,
        _ = tokio::time::sleep(req.timeout) => false,
        answer = answer => answer,
    }
}

/// Tell the user to take the window. A non-interactive caller returns immediately.
pub async fn handoff_for_test(
    app: &AppInfo,
    reason: &HandoffReason,
    cancel: &Cancel,
    interactive: bool,
    timeout: Duration,
) {
    if cancel.is_cancelled() || !interactive {
        tracing::info!("handoff requested");
        return;
    }
    let prompt = page::handoff_prompt(app, reason, seconds(timeout));
    tokio::select! {
        biased;
        _ = cancel.cancelled() => {}
        _ = tokio::time::sleep(timeout) => {}
        _ = dialog::ask(&prompt, cancel) => {}
    }
}

pub(crate) struct Approvals {
    mode: Option<String>,
    policy: Arc<HostPolicy>,
}

impl Approvals {
    pub(crate) fn new(mode: Option<String>, policy: Arc<HostPolicy>) -> Self {
        Self { mode, policy }
    }
}

#[async_trait]
impl ApprovalProvider for Approvals {
    async fn request_app(
        &self,
        _: &SessionCtx,
        app: &AppInfo,
        reason: Option<&ModelReason>,
        risk: Risk,
        cancel: &Cancel,
    ) -> ApprovalDecision {
        let config = self.policy.load();
        let path = self.policy.approvals_path();
        let reason_text = reason.map(ModelReason::as_str);
        let decision = decide_with_autoclick(&HookRequest {
            file: path.as_deref(),
            app,
            reason: reason_text,
            risk,
            timeout: Duration::from_secs(config.timeout_secs.max(1)),
            always_enabled: config.always_allow_enabled,
            cancel,
            autoclick: None,
            mode: self.mode.as_deref(),
            interactive: desktop_available(),
            summary: "",
            crop_attached: false,
        })
        .await;
        if decision == ApprovalDecision::Always {
            self.policy.remember(&app.identity);
        }
        decision
    }

    async fn confirm_action(
        &self,
        _: &SessionCtx,
        app: &AppInfo,
        summary: &ActionSummary,
        crop: Option<&ImageRef>,
        cancel: &Cancel,
    ) -> bool {
        let config = self.policy.load();
        confirm_for_test(&HookRequest {
            file: None,
            app,
            reason: None,
            risk: Risk::High,
            timeout: Duration::from_secs(config.timeout_secs.max(1)),
            always_enabled: false,
            cancel,
            autoclick: None,
            mode: self.mode.as_deref(),
            interactive: desktop_available(),
            summary: summary.text.as_str(),
            crop_attached: crop.is_some(),
        })
        .await
    }

    async fn handoff(&self, _: &SessionCtx, app: &AppInfo, reason: HandoffReason, cancel: &Cancel) {
        let config = self.policy.load();
        handoff_for_test(
            app,
            &reason,
            cancel,
            desktop_available(),
            Duration::from_secs(config.timeout_secs.max(1)),
        )
        .await;
    }
}

async fn choose_app(req: &HookRequest<'_>) -> ApprovalDecision {
    if let Some(policy) = eval_policy(req.mode) {
        return policy.app;
    }
    if let Some(hook) = req.autoclick {
        return match hook {
            "turn" => ApprovalDecision::Turn,
            "session" => ApprovalDecision::Session,
            "always" => ApprovalDecision::Always,
            "deny" => ApprovalDecision::Deny,
            "timeout" | "cancel" => std::future::pending().await,
            _ => ApprovalDecision::Deny,
        };
    }
    if !req.interactive {
        tracing::info!("approval denied without an interactive desktop");
        return ApprovalDecision::Deny;
    }
    match dialog::ask(
        &page::app_prompt(
            req.app,
            req.reason,
            req.risk,
            req.always_enabled,
            seconds(req.timeout),
        ),
        req.cancel,
    )
    .await
    {
        dialog::Choice::Turn => ApprovalDecision::Turn,
        dialog::Choice::Session => ApprovalDecision::Session,
        dialog::Choice::Always => ApprovalDecision::Always,
        dialog::Choice::Timeout => ApprovalDecision::Timeout,
        dialog::Choice::Deny | dialog::Choice::Allow | dialog::Choice::Dismiss => {
            ApprovalDecision::Deny
        }
    }
}

async fn persist_always(req: &HookRequest<'_>) -> ApprovalDecision {
    let Some(path) = req.file else {
        return ApprovalDecision::Deny;
    };
    let stored = path.to_path_buf();
    let revoke_path = stored.clone();
    let identity = req.app.identity.clone();
    let key = policy_store_key(&identity);
    let wrote = private::work(move || {
        super::policy_store::grant(&stored, &identity)
            .map_err(|_| Failure::internal("Cannot store approval"))
    })
    .await
    .is_ok();
    if !wrote || req.cancel.is_cancelled() {
        if wrote {
            let _ = private::work(move || {
                super::policy_store::revoke(&revoke_path, &key)
                    .map(|_| ())
                    .map_err(|_| Failure::internal("Cannot revoke approval"))
            })
            .await;
        }
        return ApprovalDecision::Deny;
    }
    ApprovalDecision::Always
}

fn policy_store_key(identity: &gcu_host_api::AppIdentity) -> String {
    super::policy_store::storage_key(identity)
}

struct EvalPolicy {
    app: ApprovalDecision,
    sensitive: bool,
    foreground: bool,
}

fn eval_policy(mode: Option<&str>) -> Option<EvalPolicy> {
    #[cfg(all(feature = "eval-autoapprove", debug_assertions))]
    {
        let app = match mode {
            Some("auto-allow-turn") => Some(ApprovalDecision::Turn),
            Some("auto-allow-session" | "auto-allow-always") => Some(ApprovalDecision::Session),
            Some("auto-deny") => Some(ApprovalDecision::Deny),
            _ => None,
        };
        app.map(|app| EvalPolicy {
            app,
            sensitive: false,
            foreground: false,
        })
    }
    #[cfg(not(all(feature = "eval-autoapprove", debug_assertions)))]
    {
        let _ = mode;
        None
    }
}

fn desktop_available() -> bool {
    #[cfg(test)]
    {
        false
    }
    #[cfg(not(test))]
    {
        if std::env::var_os("CI").is_some()
            || std::env::var_os("GCU_APPROVAL_NONINTERACTIVE").is_some()
            || std::env::var_os("GCU_ALLOW_FAKE").is_some()
        {
            return false;
        }
        #[cfg(target_os = "linux")]
        {
            std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
        }
        #[cfg(not(target_os = "linux"))]
        {
            true
        }
    }
}

fn seconds(timeout: Duration) -> u64 {
    timeout.as_secs().max(1)
}
