//! Shared consent types. Pending history is not an approval.
use std::fmt;
use std::sync::{atomic::AtomicBool, Arc};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::adapter::SurfaceKind;
use crate::error::BrokerError;

pub const SYNTHETIC_DECISION_CLASS: &str = "synthetic-host-not-a-human-or-production-grant";

pub(in crate::task_consent) const REQUEST_TTL: Duration = Duration::from_secs(10 * 60);
pub(in crate::task_consent) const GRANT_IDLE_TTL: Duration = Duration::from_secs(30 * 60);
pub(in crate::task_consent) const MAX_TEXT: usize = 200;
pub(in crate::task_consent) const MAX_SURFACES: usize = 4;
pub(in crate::task_consent) const MAX_CAPS: usize = 2;
pub(in crate::task_consent) const MAX_REQUESTS_PER_OWNER: usize = 8;
pub(in crate::task_consent) const MAX_REQUESTS_GLOBAL: usize = 32;
pub(in crate::task_consent) const MAX_PREPARATIONS_PER_OWNER: usize = 4;
pub(in crate::task_consent) const MAX_PREPARATIONS_GLOBAL: usize = 32;
pub(in crate::task_consent) const MAX_DECISIONS_PER_OWNER: usize = 8;
pub(in crate::task_consent) const MAX_DECISIONS_GLOBAL: usize = 32;
pub(in crate::task_consent) const MAX_SCOPES_PER_OWNER: usize = 8;
pub(in crate::task_consent) const MAX_SCOPES_GLOBAL: usize = 16;
pub(in crate::task_consent) const MAX_RETIRED_TOKENS: usize = 64;
pub(in crate::task_consent) const MAX_OWNERS: usize = 32;
pub(in crate::task_consent) const MAX_GRANTS_GLOBAL: usize = 32;
pub(in crate::task_consent) const MAX_EXPIRED_RUNS: usize = 32;
pub const MAX_SCOPE_MEMBERS: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::task_consent) enum Capability {
    Observe,
    Input,
}

impl Capability {
    pub(in crate::task_consent) fn as_str(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Input => "input",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::task_consent) enum RequestState {
    Awaiting,
    Approving,
    Approved,
    Denied,
    Cancelled,
    Expired,
    Retired,
    Completed,
}

impl RequestState {
    pub(in crate::task_consent) fn as_str(self) -> &'static str {
        match self {
            Self::Awaiting | Self::Approving => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
            Self::Retired => "retired",
            Self::Completed => "completed",
        }
    }

    pub(in crate::task_consent) fn is_pending(self) -> bool {
        matches!(self, Self::Awaiting | Self::Approving)
    }

    pub(in crate::task_consent) fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Denied | Self::Cancelled | Self::Expired | Self::Retired | Self::Completed
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::task_consent) enum PrepPhase {
    Resources,
    Binding,
    Aborted,
}

#[derive(Clone)]
pub(in crate::task_consent) struct Clock {
    origin: Instant,
    extra: Arc<parking_lot::Mutex<Duration>>,
}

impl Clock {
    pub(in crate::task_consent) fn new() -> Self {
        Self {
            origin: Instant::now(),
            extra: Arc::new(parking_lot::Mutex::new(Duration::ZERO)),
        }
    }

    pub(in crate::task_consent) fn now(&self) -> Duration {
        self.origin.elapsed().saturating_add(*self.extra.lock())
    }

    #[cfg(test)]
    pub(in crate::task_consent) fn advance(&mut self, by: Duration) {
        let mut extra = self.extra.lock();
        *extra = extra.saturating_add(by);
    }
}

pub(in crate::task_consent) struct Owner {
    pub(in crate::task_consent) nonce: u64,
    pub(in crate::task_consent) incarnation: u64,
    pub(in crate::task_consent) connection_generation: u64,
    pub(in crate::task_consent) agent_session_id: Option<String>,
    pub(in crate::task_consent) retired: bool,
    pub(in crate::task_consent) stop_generation: u64,
    pub(in crate::task_consent) prepare_cancel: u64,
    pub(in crate::task_consent) task_id: Option<String>,
}

pub(in crate::task_consent) struct TokenBody {
    pub(in crate::task_consent) app_session_id: String,
    pub(in crate::task_consent) owner_nonce: u64,
    pub(in crate::task_consent) incarnation: u64,
    pub(in crate::task_consent) connection_generation: u64,
    pub(in crate::task_consent) agent_session_id: Option<String>,
    pub(in crate::task_consent) feature_epoch_at_issue: u64,
}

#[derive(Clone)]
pub(in crate::task_consent) struct OpaqueIdentity {
    pub(in crate::task_consent) target_id: String,
    pub(in crate::task_consent) surface: SurfaceKind,
    pub(in crate::task_consent) display_name: Option<String>,
    pub(in crate::task_consent) synthetic: bool,
    pub(in crate::task_consent) native: Option<NativeScopeProof>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct NativeScopeProof {
    pub(crate) executor_generation: u64,
    pub(crate) identity: crate::adapter::NativeScopeIdentity,
}

impl OpaqueIdentity {
    pub(in crate::task_consent) fn decision_class(&self) -> &'static str {
        self.native
            .as_ref()
            .map_or(SYNTHETIC_DECISION_CLASS, |proof| {
                proof.identity.decision_class()
            })
    }
}

#[derive(Clone)]
pub(crate) struct ScopeSelectionPermit {
    pub(crate) context: TransportContext,
    pub(crate) request_id: String,
    pub(crate) revision: u64,
    pub(crate) selection_revision: u64,
    pub(crate) feature_epoch: u64,
    pub(crate) surface: SurfaceKind,
}

pub(in crate::task_consent) struct RequestRecord {
    pub(in crate::task_consent) request_id: String,
    pub(in crate::task_consent) task_id: String,
    pub(in crate::task_consent) app_session_id: String,
    pub(in crate::task_consent) owner_nonce: u64,
    pub(in crate::task_consent) incarnation: u64,
    pub(in crate::task_consent) connection_generation: u64,
    pub(in crate::task_consent) purpose: String,
    pub(in crate::task_consent) surfaces: Vec<SurfaceKind>,
    pub(in crate::task_consent) capabilities: Vec<Capability>,
    pub(in crate::task_consent) scope_intent: String,
    pub(in crate::task_consent) canonical: String,
    pub(in crate::task_consent) intent: String,
    pub(in crate::task_consent) state: RequestState,
    pub(in crate::task_consent) revision: u64,
    pub(in crate::task_consent) selection_revision: u64,
    pub(in crate::task_consent) selecting: bool,
    pub(in crate::task_consent) deadline: Duration,
    pub(in crate::task_consent) bound: Option<Vec<OpaqueIdentity>>,
    pub(in crate::task_consent) continuation_issued: bool,
    pub(in crate::task_consent) feature_epoch: u64,
}

/// One approved target identity. A grant may carry more than one member.
/// The production resolver that chooses among surfaces is U04.
#[derive(Clone)]
pub struct ScopeMember {
    pub target_id: String,
    pub surface: SurfaceKind,
    pub(crate) native: Option<NativeScopeProof>,
    /// Bounded title retained from an explicit Host picker, never authority.
    pub(crate) display_name: Option<String>,
}

impl PartialEq for ScopeMember {
    fn eq(&self, other: &Self) -> bool {
        self.target_id == other.target_id
            && self.surface == other.surface
            && self.native == other.native
    }
}

impl Eq for ScopeMember {}

pub(in crate::task_consent) struct GrantRecord {
    pub(in crate::task_consent) task_id: String,
    pub(in crate::task_consent) request_id: String,
    pub(in crate::task_consent) app_session_id: String,
    pub(in crate::task_consent) agent_session_id: String,
    pub(in crate::task_consent) owner_nonce: u64,
    pub(in crate::task_consent) incarnation: u64,
    pub(in crate::task_consent) connection_generation: u64,
    pub(in crate::task_consent) feature_epoch: u64,
    pub(in crate::task_consent) revision: u64,
    pub(in crate::task_consent) capabilities: Vec<Capability>,
    pub(in crate::task_consent) members: Vec<ScopeMember>,
    /// The original picker decision, before any candidate → execution mapping.
    pub(in crate::task_consent) selected_targets: Vec<(SurfaceKind, String)>,
    pub(in crate::task_consent) idle_deadline: Duration,
    pub(in crate::task_consent) physical_work: Arc<AtomicBool>,
    pub(in crate::task_consent) completion: Arc<super::activity::PhysicalActivity>,
    pub(in crate::task_consent) execution_run_id: Option<String>,
    pub(in crate::task_consent) active_target_id: String,
    pub(in crate::task_consent) suspended: bool,
}

pub(in crate::task_consent) struct Preparation {
    pub(in crate::task_consent) id: u64,
    pub(in crate::task_consent) request_id: String,
    pub(in crate::task_consent) revision: u64,
    pub(in crate::task_consent) selection_revision: u64,
    pub(in crate::task_consent) app_session_id: String,
    pub(in crate::task_consent) owner_nonce: u64,
    pub(in crate::task_consent) incarnation: u64,
    pub(in crate::task_consent) connection_generation: u64,
    pub(in crate::task_consent) agent_session_id: String,
    pub(in crate::task_consent) stop_generation: u64,
    pub(in crate::task_consent) prepare_cancel: u64,
    pub(in crate::task_consent) observed_epoch: u64,
    pub(in crate::task_consent) observed_enabled: bool,
    pub(in crate::task_consent) previous_enabled: bool,
    pub(in crate::task_consent) owned_epoch: u64,
    pub(in crate::task_consent) we_enabled: bool,
    pub(in crate::task_consent) published: bool,
    pub(in crate::task_consent) rolled_back: bool,
    pub(in crate::task_consent) phase: PrepPhase,
    pub(in crate::task_consent) run_id: String,
    pub(in crate::task_consent) target_id: String,
    pub(in crate::task_consent) task_id: String,
    pub(in crate::task_consent) surface: SurfaceKind,
    pub(in crate::task_consent) native: Option<NativeScopeProof>,
    pub(in crate::task_consent) members: Vec<ScopeMember>,
    pub(in crate::task_consent) selected_targets: Vec<(SurfaceKind, String)>,
    pub(in crate::task_consent) opened_run: bool,
    pub(in crate::task_consent) managed: Option<ManagedResourcePolicy>,
}

#[derive(Clone, PartialEq, Eq)]
pub(in crate::task_consent) struct ManagedResourcePolicy {
    pub(in crate::task_consent) selection_revision: u64,
    pub(in crate::task_consent) profile: String,
}

/// Host-only reservation for resources, deliberately not a PrepareTicket.
/// No target/native proof/execution permit exists until the original tab binds.
pub(crate) struct ManagedResourceTicket {
    pub(crate) id: u64,
    pub(crate) session_id: String,
    pub(crate) run_id: String,
    pub(crate) request_id: String,
    pub(crate) revision: u64,
    pub(crate) task_id: String,
    pub(crate) selection_revision: u64,
    pub(crate) profile: String,
}

pub(crate) enum ManagedResourceReady {
    Prepare(ManagedResourceTicket),
    Already(CommitOutcome),
}

pub(in crate::task_consent) struct Ticket {
    pub(in crate::task_consent) id: String,
    pub(in crate::task_consent) app_session_id: String,
    pub(in crate::task_consent) agent_session_id: String,
    pub(in crate::task_consent) incarnation: u64,
    pub(in crate::task_consent) connection_generation: u64,
    pub(in crate::task_consent) owner_nonce: u64,
    pub(in crate::task_consent) request_id: String,
    pub(in crate::task_consent) task_id: String,
    pub(in crate::task_consent) grant_revision: u64,
    pub(in crate::task_consent) feature_epoch: u64,
    /// One-shot scheduling identity. Distinct from a reused task or request id.
    pub(in crate::task_consent) dispatch_id: String,
    pub(in crate::task_consent) consumed: bool,
    pub(in crate::task_consent) invalid: bool,
}

pub(in crate::task_consent) struct Notice {
    pub(in crate::task_consent) id: String,
    pub(in crate::task_consent) dispatch_id: String,
    pub(in crate::task_consent) app_session_id: String,
    pub(in crate::task_consent) owner_nonce: u64,
    pub(in crate::task_consent) incarnation: u64,
    pub(in crate::task_consent) connection_generation: u64,
    pub(in crate::task_consent) agent_session_id: String,
    pub(in crate::task_consent) request_id: String,
    pub(in crate::task_consent) task_id: String,
    pub(in crate::task_consent) revision: u64,
    pub(in crate::task_consent) feature_epoch: u64,
    pub(in crate::task_consent) reason: &'static str,
}

/// Typed no-authority record. Taking it does not admit execution.
#[derive(Clone)]
pub struct NoAuthorityNotice {
    pub id: String,
    pub dispatch_id: String,
    pub app_session_id: String,
    pub owner_nonce: u64,
    pub incarnation: u64,
    pub connection_generation: u64,
    pub agent_session_id: String,
    pub request_id: String,
    pub task_id: String,
    pub revision: u64,
    pub feature_epoch: u64,
    pub reason: &'static str,
}

pub(in crate::task_consent) enum Decision {
    Continuation(Ticket),
    NoAuthority(Notice),
}

pub(in crate::task_consent) struct ScopeMemory {
    pub(in crate::task_consent) app_session_id: String,
    pub(in crate::task_consent) purpose: String,
    pub(in crate::task_consent) surfaces: Vec<SurfaceKind>,
    pub(in crate::task_consent) scope_intent: String,
    pub(in crate::task_consent) members: Vec<ScopeMember>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct TransportContext {
    pub app_session_id: String,
    pub incarnation: u64,
    pub connection_generation: u64,
    pub agent_session_id: Option<String>,
    pub feature_epoch_at_issue: u64,
    pub owner_nonce: u64,
}

pub struct IssuedTransport {
    token: String,
    pub incarnation: u64,
    pub connection_generation: u64,
    pub feature_epoch_at_issue: u64,
}

impl IssuedTransport {
    pub(in crate::task_consent) fn from_parts(
        token: String,
        incarnation: u64,
        connection_generation: u64,
        feature_epoch_at_issue: u64,
    ) -> Self {
        Self {
            token,
            incarnation,
            connection_generation,
            feature_epoch_at_issue,
        }
    }

    /// Host/MCP environment only. Tool results must not print this value.
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl fmt::Debug for IssuedTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IssuedTransport")
            .field("token", &"<redacted>")
            .field("incarnation", &self.incarnation)
            .field("connection_generation", &self.connection_generation)
            .field("feature_epoch_at_issue", &self.feature_epoch_at_issue)
            .finish()
    }
}

pub enum BearerClass {
    Unknown,
    Retired,
    Live(TransportContext),
}

#[derive(Clone)]
pub struct ConsentBinding {
    pub app_session_id: String,
    pub agent_session_id: String,
    pub owner_nonce: u64,
    pub incarnation: u64,
    pub connection_generation: u64,
    pub task_id: String,
    pub request_id: String,
    pub grant_revision: u64,
    pub feature_epoch: u64,
    pub run_id: String,
    pub target_id: String,
}

#[derive(Clone)]
pub struct AuthorityClaim {
    pub id: String,
    pub app_session_id: String,
    pub agent_session_id: String,
    pub incarnation: u64,
    pub connection_generation: u64,
    pub owner_nonce: u64,
    pub request_id: String,
    pub task_id: String,
    pub grant_revision: u64,
    pub feature_epoch: u64,
    /// One-shot scheduling identity. Consumption is once, by this id.
    pub dispatch_id: String,
}

#[derive(Clone)]
pub struct ExecutionPermit {
    pub session_id: String,
    pub run_id: String,
    pub task_id: String,
    pub request_id: String,
    pub grant_revision: u64,
    pub feature_epoch: u64,
    pub incarnation: u64,
    pub connection_generation: u64,
    pub owner_nonce: u64,
    pub agent_session_id: String,
    /// Approved member this call may touch.
    pub target_id: String,
    /// Broker execution generation, frozen when this call is admitted.
    pub execution_generation: u64,
    /// `observe`, `input`, or `stop`.
    pub capability: &'static str,
}

pub struct PrepareTicket {
    pub id: u64,
    pub session_id: String,
    pub run_id: String,
    pub target_id: String,
    pub surface: SurfaceKind,
    pub(crate) native: Option<NativeScopeProof>,
    pub(crate) members: Vec<ScopeMember>,
    pub request_id: String,
    pub revision: u64,
    pub task_id: String,
}

/// Host-only selection reservation. Its members are not execution targets.
pub struct WebViewSelectionTicket {
    pub(crate) id: u64,
    pub(crate) session_id: String,
    pub(crate) run_id: String,
    pub(crate) request_id: String,
    pub(crate) revision: u64,
    pub(crate) task_id: String,
    pub(crate) selection_revision: u64,
    pub(crate) members: Vec<ScopeMember>,
}

pub struct CommitOutcome {
    pub request_id: String,
    pub revision: u64,
    pub run_id: String,
    pub continuation_id: String,
    pub decision_class: &'static str,
    pub binding: ConsentBinding,
}

pub enum CommitReady {
    Prepare(PrepareTicket),
    WebViewSelection(WebViewSelectionTicket),
    Already(CommitOutcome),
}

#[derive(Clone, Copy, Default)]
pub struct ExecutionTargetClaim<'a> {
    pub run_id: Option<&'a str>,
    pub target_id: Option<&'a str>,
}

pub struct AbortEffect {
    pub feature: Option<(bool, u64)>,
    pub run_id: String,
    pub opened_run: bool,
}

#[derive(Clone)]
pub struct ScopeMemoryView {
    pub purpose: String,
    pub surfaces: Vec<String>,
    pub scope_intent: String,
}

pub enum ToolNeed {
    Access,
    Read,
    Write,
    Stop,
    Unknown,
}

pub fn tool_need(name: &str) -> ToolNeed {
    match name {
        "computer_request_access" | "computer_complete_task" => ToolNeed::Access,
        "computer_status"
        | "computer_list_targets"
        | "computer_open_target"
        | "computer_observe"
        | "browser_list_tabs"
        | "browser_observe" => ToolNeed::Read,
        "computer_act"
        | "computer_wait"
        | "computer_navigate"
        | "computer_download"
        | "computer_upload"
        | "browser_open"
        | "browser_act"
        | "computer_request_handoff" => ToolNeed::Write,
        "computer_stop" => ToolNeed::Stop,
        _ => ToolNeed::Unknown,
    }
}

pub(in crate::task_consent) enum ParsedAction {
    Request {
        purpose: String,
        surfaces: Vec<SurfaceKind>,
        capabilities: Vec<Capability>,
        scope_intent: String,
    },
    Status,
    Cancel,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "lowercase", deny_unknown_fields)]
pub(in crate::task_consent) enum AccessAction {
    Request {
        purpose: String,
        surfaces: Vec<String>,
        capabilities: Vec<String>,
        #[serde(rename = "scopeIntent")]
        scope_intent: String,
    },
    Status,
    Cancel,
}

pub(in crate::task_consent) fn schema(message: &str) -> BrokerError {
    BrokerError::Schema(message.to_string())
}

pub(in crate::task_consent) fn fresh_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub(in crate::task_consent) fn require_session(value: &str) -> Result<String, BrokerError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 256 {
        Err(BrokerError::IdentityMismatch("appSessionId"))
    } else {
        Ok(trimmed.to_string())
    }
}

pub(in crate::task_consent) fn bounded_text(value: &str) -> Result<String, BrokerError> {
    let trimmed = value.trim();
    let len = trimmed.chars().count();
    if (1..=MAX_TEXT).contains(&len) {
        Ok(trimmed.to_string())
    } else {
        Err(schema("request rejected"))
    }
}

pub(in crate::task_consent) fn intent_key(
    purpose: &str,
    surfaces: &[SurfaceKind],
    scope_intent: &str,
    generation: u64,
    nonce: u64,
) -> String {
    let mut surfaces: Vec<&str> = surfaces.iter().copied().map(SurfaceKind::as_wire).collect();
    surfaces.sort_unstable();
    format!(
        "{purpose}|{}|{scope_intent}|{generation}|{nonce}",
        surfaces.join(",")
    )
}

pub(in crate::task_consent) fn canonical_key(intent: &str, caps: &[Capability]) -> String {
    let mut caps: Vec<&str> = caps.iter().copied().map(Capability::as_str).collect();
    caps.sort_unstable();
    format!("{intent}|{}", caps.join(","))
}

pub(in crate::task_consent) fn request_body(request: &RequestRecord) -> Value {
    json!({
        "state": request.state.as_str(),
        "requestId": request.request_id,
        "taskId": request.task_id,
        "revision": request.revision,
        "authority": "none",
        "purpose": request.purpose,
        "scopeIntent": request.scope_intent,
        "surfaces": request.surfaces.iter().copied().map(SurfaceKind::as_wire).collect::<Vec<_>>(),
        "capabilities": request.capabilities.iter().copied().map(Capability::as_str).collect::<Vec<_>>(),
        "instruction": if request.state.is_pending() {
            "End this turn and wait for the Host decision. Do not poll, repeat this request, or call execution tools."
        } else {
            "This request record does not grant execution. Follow the original Host decision; do not repeat a denied or expired request."
        },
    })
}

pub(in crate::task_consent) fn claim_from_ticket(ticket: &Ticket) -> AuthorityClaim {
    AuthorityClaim {
        id: ticket.id.clone(),
        app_session_id: ticket.app_session_id.clone(),
        agent_session_id: ticket.agent_session_id.clone(),
        incarnation: ticket.incarnation,
        connection_generation: ticket.connection_generation,
        owner_nonce: ticket.owner_nonce,
        request_id: ticket.request_id.clone(),
        task_id: ticket.task_id.clone(),
        grant_revision: ticket.grant_revision,
        feature_epoch: ticket.feature_epoch,
        dispatch_id: ticket.dispatch_id.clone(),
    }
}

pub(in crate::task_consent) fn binding_from_grant(
    grant: &GrantRecord,
    run_id: &str,
) -> ConsentBinding {
    ConsentBinding {
        app_session_id: grant.app_session_id.clone(),
        agent_session_id: grant.agent_session_id.clone(),
        owner_nonce: grant.owner_nonce,
        incarnation: grant.incarnation,
        connection_generation: grant.connection_generation,
        task_id: grant.task_id.clone(),
        request_id: grant.request_id.clone(),
        grant_revision: grant.revision,
        feature_epoch: grant.feature_epoch,
        run_id: run_id.to_string(),
        target_id: grant.active_target_id.clone(),
    }
}

pub(in crate::task_consent) fn same_caps(left: &[Capability], right: &[Capability]) -> bool {
    let mut left: Vec<Capability> = left.to_vec();
    let mut right: Vec<Capability> = right.to_vec();
    left.sort_by_key(|cap| cap.as_str());
    right.sort_by_key(|cap| cap.as_str());
    left == right
}
