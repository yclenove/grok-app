//! Authorization and input policy for one action.
//!
//! The router (P0-08-2) calls [`PolicyEngine::evaluate`] and then, when the
//! decision asks for it, [`crate::session::Session::request_app`] or
//! [`crate::session::Session::request_confirm`]. It evaluates again after a
//! grant. Before delivery it calls [`revalidate`] with a fresh observation.
//! [`Invalidated`] means the action is stale and must be reviewed again.
//! [`RouteLimit::BackgroundOnly`] means the driver must not activate the
//! foreground. Stop is not queued behind this function: the session reads the
//! latch on its own channel and again here.
//!
//! Evaluation order, first match wins:
//! 1. Latched stop, including reads.
//! 2. Closed session.
//! 3. `list_apps` / `list_windows` allow observation and skip the denylist.
//! 4. Denylist and unknown execution hosts. Security prompts hand off. Other
//!    denylist hits refuse, even when an always grant exists. Appended ids
//!    cannot remove built-ins.
//! 5. Chord blacklist, unparseable keys, address-bar input, dangerous schemes,
//!    and metadata hosts. These win over surface classification.
//! 6. Execution surface. Remote title/URL hits are observation only. Known
//!    terminal, REPL, debugger, and task targets refuse every mutating path.
//!    An unclassifiable mutating target hands off.
//! 7. App authorization, except discovery. A missing grant is
//!    [`PolicyDecision::NeedApproval`], which wins over confirmation.
//! 8. Non-mutating reads allow observation. They are not paused by user
//!    activity and do not hit foreground policy.
//! 9. User activity. A foreground intent is refused while the last physical
//!    input is newer than `user_idle_ms`. `None` does not block. Equality at
//!    the idle threshold does not block. Background intents are not paused.
//! 10. Sensitive-action confirmation. Web areas are strict unless confirmation
//!     is Off. Off disables every sensitive confirm, including web areas.
//!     Sensitive confirmation wins over a foreground ask.
//! 11. Foreground policy `never | ask | always`.
//!
//! [`PolicyEngine::evaluate`] does not block, time out, or cancel. It reads
//! the stop latch and the idle clock once. Approval waits live on the session.

mod approval;
mod guard;
mod identity;
mod risk;
mod surface;
mod target;
mod terms;
mod text;

pub use approval::{
    always_identity_ok, revalidate, wait_for_app, wait_for_confirm, AppGrantWait, ApprovalBinding,
    ConfirmWait, GrantScope, GrantTable, Invalidated,
};
pub use identity::{
    classify, DenyCategory, DenyKind, DenyTable, IdentityEvidence, IdentityVerdict, PolicyDataError,
};

use crate::tree::{ProcessedNode, ProcessedTree};
use gcu_host_api::{
    ActionSummary, AppInfo, HandoffReason, ModelReason, PolicyConfig, Risk, UserActivity,
};
use gcu_protocol::ErrorCode;
use std::fmt;

/// Tool the policy decision applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// Launch an authorized app, with foreground and sensitive-action policy.
    LaunchApp,
    /// List running apps. No app authorization.
    ListApps,
    /// List windows. No app authorization.
    ListWindows,
    /// Read app state. Requires authorization.
    GetAppState,
    /// Read a window tree. Requires authorization.
    Observe,
    /// Primary click or press.
    Click,
    /// Secondary click.
    Secondary,
    /// Menu invocation.
    Menu,
    /// Set a control value.
    SetValue,
    /// Type text.
    TypeText,
    /// Press a chord.
    PressKey,
    /// Scroll.
    Scroll,
    /// Drag.
    Drag,
    /// Batch of actions.
    Batch,
    /// Script.
    Script,
}

impl Tool {
    /// `list_apps` and `list_windows`.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
    #[must_use]
    pub fn is_discovery(self) -> bool {
        matches!(self, Self::ListApps | Self::ListWindows)
    }

    /// Reads and discovery. These do not change the target.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
    #[must_use]
    pub fn is_observation(self) -> bool {
        matches!(
            self,
            Self::ListApps | Self::ListWindows | Self::GetAppState | Self::Observe
        )
    }

    /// Anything that can change the target or inject input.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
    #[must_use]
    pub fn is_mutating(self) -> bool {
        !self.is_observation()
    }

    /// Whether an app grant is required. Discovery is the exception.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
    #[must_use]
    pub fn needs_auth(self) -> bool {
        !self.is_discovery()
    }
}

/// What the caller wants to do. The router fills this from the tool call.
///
/// `text` and `parameters` are redacted in [`Debug`] output. Coordinates are
/// `f64`, so this type is [`PartialEq`] and not [`Eq`].
#[derive(Clone, PartialEq)]
pub struct ActionIntent {
    /// Tool being considered.
    pub tool: Tool,
    /// Snapshot element index, when the caller named one.
    pub element_index: Option<u32>,
    /// Window-relative physical point, when the caller named one.
    pub at: Option<(f64, f64)>,
    /// Chord text, when the action presses a key.
    pub key: Option<String>,
    /// Text to type or submit. Required for the scheme guard to see it.
    pub text: Option<String>,
    /// Whether delivery would use the foreground path.
    pub foreground: bool,
    /// Model-written reason for an approval card. Not an OS fact.
    pub reason: Option<ModelReason>,
    /// Snapshot the action was planned against.
    pub snapshot_id: String,
    /// Caller-supplied digest of the canonical parameters.
    pub action_digest: [u8; 32],
    /// Opaque canonical parameters. Copied into [`ApprovalBinding`].
    pub parameters: String,
}

impl ActionIntent {
    /// Empty intent for `tool`. Fields the caller cares about are filled after.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
    #[must_use]
    pub fn new(tool: Tool) -> Self {
        Self {
            tool,
            element_index: None,
            at: None,
            key: None,
            text: None,
            foreground: false,
            reason: None,
            snapshot_id: String::new(),
            action_digest: [0; 32],
            parameters: String::new(),
        }
    }
}

impl fmt::Debug for ActionIntent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ActionIntent")
            .field("tool", &self.tool)
            .field("element_index", &self.element_index)
            .field("at", &self.at)
            .field("key", &self.key)
            .field("text", &self.text.as_ref().map(|_| "<redacted>"))
            .field("foreground", &self.foreground)
            .field("reason", &self.reason)
            .field("snapshot_id", &self.snapshot_id)
            .field("action_digest", &self.action_digest)
            .field("parameters", &"<redacted>")
            .finish()
    }
}

/// Window facts policy can see without owning the driver window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyWindow {
    /// Engine window id.
    pub id: String,
    /// OS window title.
    pub title: String,
    /// Page or document URL, when the driver has one.
    pub url: Option<String>,
}

/// Everything [`PolicyEngine::evaluate`] may consult.
///
/// The stop latch is read from `activity` during evaluation, not from a cached
/// flag. `grants` is a snapshot from [`crate::session::Session::grants`].
pub struct PolicyCtx<'a> {
    /// Session id. Not parsed.
    pub session_id: &'a str,
    /// OS-verified app. The driver supplies the canonical image path.
    pub app: &'a AppInfo,
    /// Target window, when the action has one.
    pub window: Option<&'a PolicyWindow>,
    /// Processed tree, when the action was planned against one.
    pub tree: Option<&'a ProcessedTree>,
    /// Live idle clock and stop latch.
    pub activity: &'a dyn UserActivity,
    /// Host policy snapshot.
    pub config: &'a PolicyConfig,
    /// Engine turn and session grants. Always grants are on `config`.
    pub grants: &'a GrantTable,
    /// Host policy generation. Copied into the approval binding.
    pub policy_version: u64,
    /// The session has closed. Closed sessions refuse.
    pub closed: bool,
}

/// How far a driver may go when the decision is allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteLimit {
    /// Foreground activation and input are allowed.
    ForegroundAllowed,
    /// Do not activate the foreground.
    BackgroundOnly,
    /// Read only. Do not inject input.
    ObserveOnly,
}

/// What the user is being asked to authorize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    /// App the card names.
    pub app: AppInfo,
    /// Coarse risk.
    pub risk: Risk,
    /// Model-provided reason, when present.
    pub reason: Option<ModelReason>,
    /// Binding the router revalidates before delivery.
    pub binding: ApprovalBinding,
}

/// Why a second confirmation is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    /// Sensitive action, or strict confirmation of any mutation.
    Sensitive,
    /// Foreground policy is `ask` and no sensitive confirm applies.
    Foreground,
}

/// Sensitive-action or foreground confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmRequest {
    /// Which card to show. Sensitive wins over foreground when both apply.
    pub kind: ConfirmKind,
    /// Engine-written summary. It does not include typed text.
    pub summary: ActionSummary,
    /// Coarse risk.
    pub risk: Risk,
    /// Binding the router revalidates before delivery.
    pub binding: ApprovalBinding,
}

/// Decision for one intent. The router does not deliver on the non-allow arms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// The action may proceed, within `route_limit`.
    Allow {
        /// Driver ceiling.
        route_limit: RouteLimit,
    },
    /// Ask for an app grant, then evaluate again.
    NeedApproval(ApprovalRequest),
    /// Ask for a confirmation, then evaluate again.
    NeedConfirm(ConfirmRequest),
    /// Refuse with a stable error code and a reason that does not echo input.
    Refuse(ErrorCode, String),
    /// The user has to take the window. The engine does not deliver the action.
    Handoff(HandoffReason),
}

/// Compiled policy tables.
#[derive(Clone, Copy)]
pub struct PolicyEngine {
    deny: &'static DenyTable,
    terms: &'static terms::TermIndex,
    surfaces: &'static surface::SurfaceList,
}

impl PolicyEngine {
    /// Load the embedded denylist, sensitive terms, and execution-surface keywords.
    ///
    /// Blocking: none after the first call. The first call parses memory.
    /// Timeout: none. Cancellation: none. Thread safety: the engine is `Copy`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            deny: DenyTable::builtin(),
            terms: terms::TermIndex::builtin(),
            surfaces: surface::SurfaceList::builtin(),
        }
    }

    /// Decide `intent` from `ctx`.
    ///
    /// See the module docs for the order. This call does not show UI, write a
    /// grant, or clear the stop latch.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: shared
    /// references only. The stop latch is read live from `ctx.activity`.
    pub fn evaluate(&self, ctx: &PolicyCtx<'_>, intent: &ActionIntent) -> PolicyDecision {
        if ctx.activity.latched_stop() || ctx.activity.stop_requested() {
            return refuse(ErrorCode::StoppedByUser, "stopped until the user resumes");
        }
        if ctx.closed {
            return refuse(ErrorCode::ApprovalDenied, "the session is closed");
        }
        if intent.tool.is_discovery() {
            return allow(RouteLimit::ObserveOnly);
        }
        if let Some(decision) = self.identity_gate(ctx) {
            return decision;
        }
        let resolved = ctx.tree.and_then(|tree| target::resolve(tree, intent));
        if let Some(decision) = input_gate(intent, ctx.tree, resolved.as_ref().map(|hit| hit.node))
        {
            return decision;
        }
        if let Some(decision) = self.surface_gate(ctx, intent) {
            return decision;
        }
        let assessment = risk::assess(
            self.terms,
            ctx.app,
            ctx.window.and_then(|window| window.url.as_deref()),
            ctx.tree,
            intent,
        );
        let risk_level = if assessment.high {
            Risk::High
        } else {
            Risk::Low
        };
        let binding = binding_of(ctx, intent);
        if intent.tool.needs_auth() && ctx.grants.covers(ctx.config, ctx.app).is_none() {
            return PolicyDecision::NeedApproval(ApprovalRequest {
                app: ctx.app.clone(),
                risk: risk_level,
                reason: intent.reason.clone(),
                binding,
            });
        }
        if !intent.tool.is_mutating() {
            return allow(RouteLimit::ObserveOnly);
        }
        if let Some(decision) = user_active(ctx, intent) {
            return decision;
        }
        let node = resolved.as_ref().map(|hit| hit.node);
        if risk::needs_confirm(ctx.config.sensitive_action_confirmation, assessment, true) {
            return PolicyDecision::NeedConfirm(ConfirmRequest {
                kind: ConfirmKind::Sensitive,
                summary: summary_of(intent, node),
                risk: risk_level,
                binding,
            });
        }
        foreground_or_allow(ctx, intent, node, &binding)
    }

    fn identity_gate(&self, ctx: &PolicyCtx<'_>) -> Option<PolicyDecision> {
        let verdict = classify(
            self.deny,
            &IdentityEvidence {
                identity: &ctx.app.identity,
                display_name: &ctx.app.name,
            },
            &ctx.config.extra_denied_apps,
        );
        match verdict {
            IdentityVerdict::Denied {
                kind: DenyKind::Builtin(DenyCategory::SecurityPrompt),
                ..
            } => Some(PolicyDecision::Handoff(HandoffReason::SystemPrompt)),
            IdentityVerdict::Denied { .. } => Some(refuse(
                ErrorCode::AppDenied,
                "the application is on the denylist",
            )),
            IdentityVerdict::UnknownExecutionHost => Some(PolicyDecision::Handoff(
                HandoffReason::Other("unrecognized execution host".into()),
            )),
            IdentityVerdict::Clear => None,
        }
    }

    fn surface_gate(&self, ctx: &PolicyCtx<'_>, intent: &ActionIntent) -> Option<PolicyDecision> {
        match surface::gate(self.surfaces, ctx.window, ctx.tree, intent) {
            surface::SurfaceGate::Remote if intent.tool.is_mutating() => {
                Some(PolicyDecision::Handoff(HandoffReason::Other(
                    "remote execution surface; observation only".into(),
                )))
            }
            surface::SurfaceGate::Execution => Some(refuse(
                ErrorCode::ElementNotActionable,
                "execution surfaces reject input",
            )),
            surface::SurfaceGate::Uncertain => Some(PolicyDecision::Handoff(HandoffReason::Other(
                "could not classify the execution surface".into(),
            ))),
            surface::SurfaceGate::Remote | surface::SurfaceGate::Clear => None,
        }
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn input_gate(
    intent: &ActionIntent,
    tree: Option<&ProcessedTree>,
    target: Option<&ProcessedNode>,
) -> Option<PolicyDecision> {
    let block = guard::blocks_input(intent, tree, target)?;
    Some(match block {
        guard::InputBlock::InvalidKey => {
            refuse(ErrorCode::InvalidKey, "the chord cannot be parsed")
        }
        guard::InputBlock::BlacklistedChord => {
            refuse(ErrorCode::KeyBlocked, "the chord is blocked")
        }
        guard::InputBlock::DangerousInput => refuse(ErrorCode::KeyBlocked, "the input is blocked"),
        guard::InputBlock::AddressBar => {
            refuse(ErrorCode::KeyBlocked, "address bar input is blocked")
        }
    })
}

fn user_active(ctx: &PolicyCtx<'_>, intent: &ActionIntent) -> Option<PolicyDecision> {
    if !intent.foreground {
        return None;
    }
    let paused = ctx
        .activity
        .ms_since_physical_input()
        .is_some_and(|elapsed| elapsed < ctx.config.user_idle_ms);
    paused.then(|| {
        refuse(
            ErrorCode::UserActive,
            "foreground input is paused while the user is active; wait until the user is idle and retry",
        )
    })
}

fn foreground_or_allow(
    ctx: &PolicyCtx<'_>,
    intent: &ActionIntent,
    node: Option<&ProcessedNode>,
    binding: &ApprovalBinding,
) -> PolicyDecision {
    if !intent.foreground {
        return allow(RouteLimit::BackgroundOnly);
    }
    match ctx.config.allow_foreground {
        gcu_host_api::ForegroundPolicy::Never => refuse(
            ErrorCode::CapabilityUnavailable,
            "foreground input is disabled",
        ),
        gcu_host_api::ForegroundPolicy::Ask => PolicyDecision::NeedConfirm(ConfirmRequest {
            kind: ConfirmKind::Foreground,
            summary: summary_of(intent, node),
            risk: Risk::Low,
            binding: binding.clone(),
        }),
        gcu_host_api::ForegroundPolicy::Always => allow(RouteLimit::ForegroundAllowed),
    }
}

fn binding_of(ctx: &PolicyCtx<'_>, intent: &ActionIntent) -> ApprovalBinding {
    ApprovalBinding {
        app: ctx.app.identity.clone(),
        pid: ctx.app.pid,
        window_id: ctx
            .window
            .map(|window| window.id.clone())
            .unwrap_or_default(),
        parameters: intent.parameters.clone(),
        action_digest: intent.action_digest,
        snapshot_id: intent.snapshot_id.clone(),
        policy_version: ctx.policy_version,
    }
}

fn summary_of(intent: &ActionIntent, node: Option<&ProcessedNode>) -> ActionSummary {
    let label = node
        .and_then(|node| node.node.name.as_deref())
        .filter(|name| !name.is_empty())
        .unwrap_or("element");
    let verb = match intent.tool {
        Tool::LaunchApp => "launch app",
        Tool::Click => "click",
        Tool::Secondary => "secondary click",
        Tool::Menu => "open menu",
        Tool::SetValue => "set value of",
        Tool::TypeText => "type text into",
        Tool::PressKey => "press key on",
        Tool::Scroll => "scroll",
        Tool::Drag => "drag",
        Tool::Batch => "run batch on",
        Tool::Script => "run script on",
        Tool::ListApps | Tool::ListWindows | Tool::GetAppState | Tool::Observe => "observe",
    };
    ActionSummary {
        text: format!("{verb} \"{}\"", truncate(label)),
        snapshot_id: intent.snapshot_id.clone(),
        action_digest: intent.action_digest,
    }
}

fn truncate(label: &str) -> String {
    let mut out = String::new();
    for (index, ch) in label.chars().enumerate() {
        if index == 80 {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn allow(route_limit: RouteLimit) -> PolicyDecision {
    PolicyDecision::Allow { route_limit }
}

fn refuse(code: ErrorCode, reason: &str) -> PolicyDecision {
    PolicyDecision::Refuse(code, reason.to_string())
}
