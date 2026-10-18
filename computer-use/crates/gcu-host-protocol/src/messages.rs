//! Strict JSON-RPC envelopes and versioned parameters. No persistent-grant write.
use crate::Error;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Host wire version. A major mismatch fails before any method is dispatched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Version {
    /// Incompatible protocol generation.
    pub major: u16,
    /// Backwards-compatible extension level.
    pub minor: u16,
}
/// Current protocol generation.
pub const VERSION: Version = Version { major: 1, minor: 1 };
/// Strict JSON-RPC request or notification. IDs are nonzero monotonic integers.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Exactly `2.0`.
    pub jsonrpc: String,
    /// Missing for a notification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// Method in the fixed table below.
    pub method: String,
    /// Strict method-specific parameters.
    pub params: Value,
}
/// Successful response. JSON null is an explicit result, never a missing field.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Success {
    /// Exactly `2.0`.
    pub jsonrpc: String,
    /// Request being answered.
    pub id: u64,
    /// Method-specific result.
    pub result: Value,
}
/// JSON-RPC error details. Never log arbitrary remote messages or data.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RpcError {
    /// Standard JSON-RPC or application error code.
    pub code: i32,
    /// Fixed diagnostic; no secrets or action payloads.
    pub message: String,
}
impl RpcError {
    /// Make a fixed protocol error. No IO or cancellation.
    pub fn new(code: i32, message: &str) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
/// Failed response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    /// Exactly `2.0`.
    pub jsonrpc: String,
    /// Request being answered.
    pub id: u64,
    /// Fixed error.
    pub error: RpcError,
}
/// A single JSON-RPC frame; batching is deliberately unsupported.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Envelope {
    /// Request or notification.
    Request(Request),
    /// Successful response.
    Success(Success),
    /// Failed response.
    Failure(Failure),
}
impl Envelope {
    /// Validate the JSON-RPC version and identifier; method validation happens
    /// before dispatch. Pure and nonblocking.
    pub fn validate(&self) -> Result<(), Error> {
        let (version, id) = match self {
            Self::Request(r) => (&r.jsonrpc, r.id),
            Self::Success(r) => (&r.jsonrpc, Some(r.id)),
            Self::Failure(r) => (&r.jsonrpc, Some(r.id)),
        };
        if version != "2.0" || id == Some(0) {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}

/// Explicit empty parameters/result.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

/// Open an App-owned local session.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionOpen {
    /// App session identity, scoped to this engine instance.
    pub session_id: String,
    /// Only local sessions may obtain endpoints.
    pub kind: SessionKind,
}

/// App session origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    /// Trusted local chat.
    Local,
    /// SSH session; refused.
    Ssh,
    /// Remote IM session; refused.
    RemoteIm,
    /// Scheduled session; refused.
    Automation,
}

/// Secret endpoint returned only over the authenticated Host connection.
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionOpened {
    /// Exact loopback URL.
    pub mcp_url: String,
    /// 32 random bytes, lowercase hex; never log or pass through argv/environment.
    pub token: String,
}

/// Target one App session.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionId {
    /// App-owned session identity.
    pub session_id: String,
}

/// Stop one or all sessions; applied before action/approval work.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Stop {
    /// None stops every session.
    pub session_id: Option<String>,
}

/// Trusted user event on the independent authenticated stop channel.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Resume {
    /// None resumes every session.
    pub session_id: Option<String>,
    /// Strictly increasing per stop connection; replay is refused.
    pub sequence: u64,
    /// Fresh event nonce generated by Host only after a real user gesture.
    pub user_event: [u8; 32],
}

/// A monotonic Host policy revision.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyChanged {
    /// Must exceed the last loaded policy version.
    pub version: u64,
}

/// Read a retained redacted image in its owning session.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotGet {
    /// Must own the requested image.
    pub session_id: String,
    /// Opaque delivered screenshot identifier.
    pub id: String,
}

/// In-memory safe image, never a path.
// Debug is metadata only and never formats `bytes`.
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Image {
    /// Encoded media type.
    pub mime: String,
    /// Physical pixel width.
    pub width: u32,
    /// Physical pixel height.
    pub height: u32,
    /// Redacted bytes; JSON byte array.
    pub bytes: Vec<u8>,
}
impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("mime", &self.mime)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("byte_count", &self.bytes.len())
            .finish()
    }
}

/// Wire projection of the existing core doctor fields.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DoctorReport {
    /// Actual engine capabilities; user_activity_guard is false when unavailable.
    pub capabilities: gcu_protocol::Capabilities,
    /// Ready for the declared supported operations.
    pub ready: bool,
    /// Metadata only, including user_activity_guard: unavailable when applicable.
    pub issues: Vec<String>,
}

pub use crate::types::*;

/// Overlay cursor appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CursorKind {
    /// Pointer cursor.
    Pointer,
    /// Caret cursor.
    Caret,
    /// Drag cursor.
    Drag,
    /// Scroll cursor.
    Scroll,
}

/// Overlay placement in physical screen pixels.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OverlayShow {
    /// Owning App session.
    pub session_id: String,
    /// Target window.
    pub window_id: String,
    /// OS title.
    pub title: String,
    /// Screen-space window rectangle.
    pub frame: gcu_protocol::Rect,
    /// Backing scale.
    pub scale: f64,
    /// Screen-space x.
    pub x: f64,
    /// Screen-space y.
    pub y: f64,
    /// Cursor style.
    pub kind: CursorKind,
}

/// Overlay capsule update.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OverlayStatus {
    /// Owning App session.
    pub session_id: String,
    /// Engine-written status.
    pub message: String,
    /// Foreground route is active.
    pub foreground: bool,
}

/// Action timeline notification.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Event {
    /// Owning App session.
    pub session_id: String,
    /// Tool that ran.
    pub tool: gcu_protocol::ToolName,
    /// Safe short target label.
    pub target: String,
    /// Execution outcome.
    pub outcome: gcu_protocol::result::Outcome,
    /// Wall time.
    pub duration_ms: u64,
    /// Redacted thumbnail only.
    pub thumbnail: Option<Image>,
}

/// Engine resource heartbeat.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Health {
    /// Monotonic uptime.
    pub uptime_ms: u64,
    /// Open App session count.
    pub sessions: u32,
}

/// Fixed method descriptor used for dispatch and schema generation.
pub struct Method {
    /// Exact domain.subdomain.camelCase method.
    pub name: &'static str,
    /// True for engine-to-Host fire-and-forget notifications.
    pub notification: bool,
    /// Lowest negotiated minor that may dispatch this method.
    pub min_minor: u16,
}
const fn method(name: &'static str, notification: bool, min_minor: u16) -> Method {
    Method {
        name,
        notification,
        min_minor,
    }
}
/// Exhaustive method table. Browser methods require minor 1 and a request id.
pub const METHODS: &[Method] = &[
    method("host.approval.requestApp", false, 0),
    method("host.approval.confirmAction", false, 0),
    method("host.approval.handoff", false, 0),
    method("host.policy.load", false, 0),
    method("host.overlay.show", true, 0),
    method("host.overlay.status", true, 0),
    method("host.overlay.hide", true, 0),
    method("host.event.emit", true, 0),
    method("host.engine.health", true, 0),
    method("engine.session.open", false, 0),
    method("engine.session.turnEnded", false, 0),
    method("engine.session.close", false, 0),
    method("engine.session.stop", false, 0),
    method("engine.session.resume", false, 0),
    method("engine.policy.changed", false, 0),
    method("engine.system.doctor", false, 0),
    method("engine.screenshot.get", false, 0),
    method("engine.system.shutdown", false, 0),
    method("engine.browser.sync", false, 1),
    method("host.browser.navigate", false, 1),
];
/// Generate schemas from the exact Rust wire types. Pure and nonblocking.
pub fn schemas() -> Value {
    serde_json::json!({
        "envelope": schemars::schema_for!(Envelope),
        "methods": {
            "host.approval.requestApp": { "params": schemars::schema_for!(RequestApp), "result": schemars::schema_for!(AppDecision), "notification": false },
            "host.approval.confirmAction": { "params": schemars::schema_for!(ConfirmAction), "result": schemars::schema_for!(Confirmed), "notification": false },
            "host.approval.handoff": { "params": schemars::schema_for!(Handoff), "result": schemars::schema_for!(Empty), "notification": false },
            "host.policy.load": { "params": schemars::schema_for!(Empty), "result": schemars::schema_for!(Policy), "notification": false },
            "host.overlay.show": { "params": schemars::schema_for!(OverlayShow), "result": schemars::schema_for!(Empty), "notification": true },
            "host.overlay.status": { "params": schemars::schema_for!(OverlayStatus), "result": schemars::schema_for!(Empty), "notification": true },
            "host.overlay.hide": { "params": schemars::schema_for!(SessionId), "result": schemars::schema_for!(Empty), "notification": true },
            "host.event.emit": { "params": schemars::schema_for!(Event), "result": schemars::schema_for!(Empty), "notification": true },
            "host.engine.health": { "params": schemars::schema_for!(Health), "result": schemars::schema_for!(Empty), "notification": true },
            "engine.session.open": { "params": schemars::schema_for!(SessionOpen), "result": schemars::schema_for!(SessionOpened), "notification": false },
            "engine.session.turnEnded": { "params": schemars::schema_for!(SessionId), "result": schemars::schema_for!(Empty), "notification": false },
            "engine.session.close": { "params": schemars::schema_for!(SessionId), "result": schemars::schema_for!(Empty), "notification": false },
            "engine.session.stop": { "params": schemars::schema_for!(Stop), "result": schemars::schema_for!(Empty), "notification": false },
            "engine.session.resume": { "params": schemars::schema_for!(Resume), "result": schemars::schema_for!(Empty), "notification": false },
            "engine.policy.changed": { "params": schemars::schema_for!(PolicyChanged), "result": schemars::schema_for!(Empty), "notification": false },
            "engine.system.doctor": { "params": schemars::schema_for!(Empty), "result": schemars::schema_for!(DoctorReport), "notification": false },
            "engine.screenshot.get": { "params": schemars::schema_for!(ScreenshotGet), "result": schemars::schema_for!(Image), "notification": false },
            "engine.system.shutdown": { "params": schemars::schema_for!(Empty), "result": schemars::schema_for!(Empty), "notification": false },
            "engine.browser.sync": { "params": schemars::schema_for!(gcu_protocol::builtin_browser::BrowserSync), "result": schemars::schema_for!(gcu_protocol::builtin_browser::BrowserAck), "notification": false },
            "host.browser.navigate": { "params": schemars::schema_for!(gcu_protocol::builtin_browser::BrowserNavigate), "result": schemars::schema_for!(gcu_protocol::builtin_browser::BrowserNavigated), "notification": false },
        }
    })
}

/// Reject unknown fields and malformed method parameters before callback dispatch.
/// Pure, bounded by the outer frame limit; never executes a method.
pub fn validate_params(request: &Request) -> Result<(), Error> {
    match request.method.as_str() {
        "host.approval.requestApp" => serde_json::from_value::<RequestApp>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "host.approval.confirmAction" => {
            serde_json::from_value::<ConfirmAction>(request.params.clone())
                .map(|_| ())
                .map_err(|_| Error::Invalid)
        }
        "host.approval.handoff" => serde_json::from_value::<Handoff>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "host.policy.load" => serde_json::from_value::<Empty>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "host.overlay.show" => serde_json::from_value::<OverlayShow>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "host.overlay.status" => serde_json::from_value::<OverlayStatus>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "host.overlay.hide" => serde_json::from_value::<SessionId>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "host.event.emit" => serde_json::from_value::<Event>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "host.engine.health" => serde_json::from_value::<Health>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "engine.session.open" => serde_json::from_value::<SessionOpen>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "engine.session.turnEnded" => serde_json::from_value::<SessionId>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "engine.session.close" => serde_json::from_value::<SessionId>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "engine.session.stop" => serde_json::from_value::<Stop>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "engine.session.resume" => serde_json::from_value::<Resume>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "engine.policy.changed" => serde_json::from_value::<PolicyChanged>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "engine.system.doctor" => serde_json::from_value::<Empty>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "engine.screenshot.get" => serde_json::from_value::<ScreenshotGet>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "engine.system.shutdown" => serde_json::from_value::<Empty>(request.params.clone())
            .map(|_| ())
            .map_err(|_| Error::Invalid),
        "engine.browser.sync" => crate::browser::sync_params(&request.params),
        "host.browser.navigate" => crate::browser::navigate_params(&request.params),
        _ => Err(Error::Invalid),
    }
}

/// Direction, channel, minor, and id gate. Pure. Browser methods need an id.
pub fn inbound_permitted(is_host: bool, stop_channel: bool, minor: u16, request: &Request) -> bool {
    let direction = if is_host { "host." } else { "engine." };
    let control = matches!(
        request.method.as_str(),
        "engine.session.stop" | "engine.session.resume"
    );
    METHODS.iter().any(|method| {
        method.name == request.method
            && request.method.starts_with(direction)
            && (!method.notification || request.id.is_none())
            && (method.notification || request.id.is_some() || !is_host)
            && control == stop_channel
            && minor >= method.min_minor
            && (method.min_minor == 0 || request.id.is_some())
    })
}
