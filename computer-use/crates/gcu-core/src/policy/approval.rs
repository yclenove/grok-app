//! Turn, session, and always grants, plus approval binding checks.
//!
//! The engine stores turn and session grants only. [`gcu_host_api::ApprovalDecision::Always`]
//! is not stored: the Host writes it into [`gcu_host_api::PolicyConfig::always`].
//! Always-allow follows architecture §7.2 through [`AppIdentity::grant_eq`]. A
//! path, content hash, cdhash, certificate, publisher, team, product, or build
//! change needs a new grant. A missing required fact does not match. Linux also
//! requires the device id and inode carried on the grant proof.
//!
//! Turn and session grants also require the same process id and structural identity.
//! Each use re-checks that pair. `always_allow_enabled == false` ignores stored
//! always grants.
//!
//! [`revalidate`] is what the router calls before delivery. It compares structural
//! identity before process id. A change yields [`Invalidated`] and the action is
//! not delivered.
//!
//! Approval waits are biased: session cancellation, then the timeout, then the
//! provider. Timeout and session close cancel the per-request token so Host UI is
//! dismissed. A cancelled or closed wait returns deny and records nothing. A turn
//! grant is stored only when that turn is still open.

use gcu_host_api::{
    ActionSummary, AppIdentity, AppInfo, ApprovalDecision, ApprovalProvider, Cancel, ImageRef,
    ModelReason, Risk, SessionCtx,
};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Which grant currently covers an app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantScope {
    /// Dropped when the turn ends.
    Turn,
    /// Dropped when the session closes.
    Session,
    /// Host-stored persistent grant. The engine does not write it.
    Always,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HeldGrant {
    identity: AppIdentity,
    pid: Option<u32>,
}

/// Turn and session grants for one session. Always grants stay on [`gcu_host_api::PolicyConfig`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GrantTable {
    turn: Vec<HeldGrant>,
    session: Vec<HeldGrant>,
}

impl GrantTable {
    /// Forget turn grants. Does not touch session grants or the stop latch.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: `&mut self`.
    pub fn turn_ended(&mut self) {
        self.turn.clear();
    }

    /// Forget every engine-owned grant.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: `&mut self`.
    pub fn session_closed(&mut self) {
        self.turn.clear();
        self.session.clear();
    }

    /// Record a turn grant when the caller has already checked the turn generation.
    ///
    /// Duplicate structural identities are stored once. Always, deny, and timeout
    /// are not recorded here.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: `&mut self`.
    pub fn record_turn(&mut self, app: &AppInfo) {
        remember(&mut self.turn, app);
    }

    /// Record a session grant.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: `&mut self`.
    pub fn record_session(&mut self, app: &AppInfo) {
        remember(&mut self.session, app);
    }

    /// The narrowest grant that covers `app`, if any.
    ///
    /// Turn and session need the same pid and structural [`Eq`] identity. Always
    /// uses [`always_identity_ok`] and ignores pid. When `always_allow_enabled`
    /// is false, stored always grants are ignored.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: shared
    /// references only.
    pub fn covers(&self, config: &gcu_host_api::PolicyConfig, app: &AppInfo) -> Option<GrantScope> {
        if self.turn.iter().any(|grant| instance_ok(grant, app)) {
            return Some(GrantScope::Turn);
        }
        if self.session.iter().any(|grant| instance_ok(grant, app)) {
            return Some(GrantScope::Session);
        }
        if config.always_allow_enabled
            && config
                .always
                .iter()
                .any(|granted| always_identity_ok(granted, &app.identity))
        {
            return Some(GrantScope::Always);
        }
        None
    }
}

fn remember(grants: &mut Vec<HeldGrant>, app: &AppInfo) {
    if grants.iter().any(|grant| instance_ok(grant, app)) {
        return;
    }
    grants.push(HeldGrant {
        identity: app.identity.clone(),
        pid: app.pid,
    });
}

fn instance_ok(grant: &HeldGrant, app: &AppInfo) -> bool {
    grant.pid == app.pid && grant.identity == app.identity
}

/// Whether `current` still satisfies a Host always-grant of `granted`.
///
/// This is [`AppIdentity::grant_eq`]. Path, hash, cdhash, certificate, publisher,
/// team, product, and build are not widened. A missing required fact does not match.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn always_identity_ok(granted: &AppIdentity, current: &AppIdentity) -> bool {
    granted.grant_eq(current)
}

/// Why a previously approved action must not be delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalidated {
    /// Structural application identity changed.
    AppIdentity,
    /// The process id changed.
    ProcessInstance,
    /// The window id changed.
    Window,
    /// Action parameters or the action digest changed.
    ActionParams,
    /// The snapshot id changed.
    Snapshot,
    /// The policy version changed.
    PolicyVersion,
}

/// Facts that bind an approval to one delivery attempt.
///
/// `parameters` is opaque caller text. Debug output redacts it.
#[derive(Clone, PartialEq, Eq)]
pub struct ApprovalBinding {
    /// Structural app identity captured when the user was asked.
    pub app: AppIdentity,
    /// Process id captured with the request.
    pub pid: Option<u32>,
    /// Window the action will be delivered to.
    pub window_id: String,
    /// Canonical action parameters. The router owns the canonical form.
    pub parameters: String,
    /// Digest of those parameters. This crate does not hash them.
    pub action_digest: [u8; 32],
    /// Snapshot the action was planned against.
    pub snapshot_id: String,
    /// Policy generation the decision used.
    pub policy_version: u64,
}

impl fmt::Debug for ApprovalBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApprovalBinding")
            .field("app", &self.app)
            .field("pid", &self.pid)
            .field("window_id", &self.window_id)
            .field("parameters", &"<redacted>")
            .field("action_digest", &self.action_digest)
            .field("snapshot_id", &self.snapshot_id)
            .field("policy_version", &self.policy_version)
            .finish()
    }
}

/// Compare the approval the user saw with the action about to be delivered.
///
/// Identity is checked before process id. The first difference wins.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
pub fn revalidate(binding: &ApprovalBinding, current: &ApprovalBinding) -> Result<(), Invalidated> {
    if binding.app != current.app {
        return Err(Invalidated::AppIdentity);
    }
    if binding.pid != current.pid {
        return Err(Invalidated::ProcessInstance);
    }
    if binding.window_id != current.window_id {
        return Err(Invalidated::Window);
    }
    if binding.parameters != current.parameters || binding.action_digest != current.action_digest {
        return Err(Invalidated::ActionParams);
    }
    if binding.snapshot_id != current.snapshot_id {
        return Err(Invalidated::Snapshot);
    }
    if binding.policy_version != current.policy_version {
        return Err(Invalidated::PolicyVersion);
    }
    Ok(())
}

/// Inputs for one app-authorization wait.
pub struct AppGrantWait<'a> {
    /// Host UI.
    pub provider: &'a dyn ApprovalProvider,
    /// Session shown on the card.
    pub session: &'a SessionCtx,
    /// App being authorized.
    pub app: &'a AppInfo,
    /// Model-written reason, when the model supplied one.
    pub reason: Option<&'a ModelReason>,
    /// Coarse risk for the card.
    pub risk: Risk,
    /// Engine-side wait. Zero completes as timeout without polling the provider
    /// when cancellation is not already set, because the sleep is polled first.
    pub timeout: Duration,
    /// Session-close token. Already-cancelled waits return deny immediately.
    pub session_cancel: &'a Cancel,
    /// Turn generation captured when the wait started.
    pub turn_generation: u64,
    /// Live turn generation. A turn grant is dropped when this differs.
    pub generation: &'a AtomicU64,
    /// Set when the session has closed.
    pub closed: &'a AtomicBool,
    /// Set while a turn is open. Turn grants require it.
    pub turn_open: &'a AtomicBool,
    /// Where a turn or session grant is stored. Always is not stored.
    pub grants: &'a Mutex<GrantTable>,
}

/// Wait for an app decision and record turn or session grants that are still valid.
///
/// Blocking: waits on the provider, `timeout`, or `session_cancel`.
/// Timeout: `timeout` returns [`ApprovalDecision::Timeout`] and cancels the
/// provider token. Cancellation: session close or an already-cancelled token
/// returns [`ApprovalDecision::Deny`] and records nothing. Thread safety: the
/// grant mutex is locked only to record.
pub async fn wait_for_app(wait: AppGrantWait<'_>) -> ApprovalDecision {
    if wait.closed.load(Ordering::Acquire) || wait.session_cancel.is_cancelled() {
        return ApprovalDecision::Deny;
    }
    let request_cancel = Cancel::new();
    let _dismiss_on_drop = DismissOnDrop(request_cancel.clone());
    let decision = tokio::select! {
        biased;
        _ = wait.session_cancel.cancelled() => {
            request_cancel.cancel();
            ApprovalDecision::Deny
        }
        _ = tokio::time::sleep(wait.timeout) => {
            request_cancel.cancel();
            ApprovalDecision::Timeout
        }
        decision = wait.provider.request_app(
            wait.session,
            wait.app,
            wait.reason,
            wait.risk,
            &request_cancel,
        ) => decision,
    };
    remember_decision(&wait, decision);
    decision
}

fn remember_decision(wait: &AppGrantWait<'_>, decision: ApprovalDecision) {
    if wait.closed.load(Ordering::Acquire) || wait.session_cancel.is_cancelled() {
        return;
    }
    let mut grants = lock(wait.grants);
    match decision {
        ApprovalDecision::Turn
            if wait.turn_open.load(Ordering::Acquire)
                && wait.generation.load(Ordering::Acquire) == wait.turn_generation =>
        {
            grants.record_turn(wait.app);
        }
        ApprovalDecision::Session => grants.record_session(wait.app),
        ApprovalDecision::Turn
        | ApprovalDecision::Always
        | ApprovalDecision::Deny
        | ApprovalDecision::Timeout => {}
    }
}

/// Inputs for one confirmation wait.
pub struct ConfirmWait<'a> {
    /// Host UI.
    pub provider: &'a dyn ApprovalProvider,
    /// Session shown on the card.
    pub session: &'a SessionCtx,
    /// App the card names.
    pub app: &'a AppInfo,
    /// Engine-written summary. It does not include typed text.
    pub summary: &'a ActionSummary,
    /// Optional crop. The router supplies this; policy does not capture it.
    pub crop: Option<&'a ImageRef>,
    /// Engine-side wait.
    pub timeout: Duration,
    /// Session-close token. Already-cancelled waits return false immediately.
    pub session_cancel: &'a Cancel,
    /// Set when the session has closed.
    pub closed: &'a AtomicBool,
}

/// Wait for a sensitive-action confirmation.
///
/// Blocking: waits on the provider, `timeout`, or `session_cancel`.
/// Timeout: returns `false` and cancels the provider token.
/// Cancellation: session close returns `false`. Thread safety: `Send` futures.
pub async fn wait_for_confirm(wait: ConfirmWait<'_>) -> bool {
    if wait.closed.load(Ordering::Acquire) || wait.session_cancel.is_cancelled() {
        return false;
    }
    let request_cancel = Cancel::new();
    let _dismiss_on_drop = DismissOnDrop(request_cancel.clone());
    let confirmed = tokio::select! {
        biased;
        _ = wait.session_cancel.cancelled() => {
            request_cancel.cancel();
            false
        }
        _ = tokio::time::sleep(wait.timeout) => {
            request_cancel.cancel();
            false
        }
        confirmed = wait.provider.confirm_action(
            wait.session,
            wait.app,
            wait.summary,
            wait.crop,
            &request_cancel,
        ) => confirmed,
    };
    if wait.closed.load(Ordering::Acquire) || wait.session_cancel.is_cancelled() {
        return false;
    }
    confirmed
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct DismissOnDrop(Cancel);
impl Drop for DismissOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
