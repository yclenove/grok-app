//! Conditional `browser_navigate`. The static catalog never includes it.
//!
//! Visibility is the App session id on [`crate::ServerConfig::browser_session`].
//! A call checks the binding again even when the client cached the tool list.

use std::sync::{Arc, Mutex};

use gcu_host_protocol::server::{Peer as HostPeer, RPC_TIMEOUT};
use gcu_protocol::builtin_browser::{
    classify_navigated, current_platform, l1_open, model_error, with_shared, ApplyFault,
    BrowserNavigate, BrowserSync, NavFault, NavRequest, Plan,
};
use gcu_protocol::{browser_navigate_tool_json, ErrorCode, ToolError};
use rmcp::model::{CallToolResult, ContentBlock, Tool};
use rmcp::service::Peer;
use rmcp::RoleServer;
use serde_json::{json, Value};

use crate::catalog::Profile;
use crate::result;
use crate::ServerConfig;

/// MCP name. Not a [`gcu_protocol::ToolName`] variant.
pub const NAME: &str = "browser_navigate";

struct Hub {
    peers: Vec<(String, u64, Peer<RoleServer>)>,
}

fn hub() -> std::sync::MutexGuard<'static, Hub> {
    static HUB: Mutex<Hub> = Mutex::new(Hub { peers: Vec::new() });
    HUB.lock().unwrap_or_else(|error| error.into_inner())
}

static NAV: Mutex<Option<Arc<dyn Navigator>>> = Mutex::new(None);

fn navigator() -> Option<Arc<dyn Navigator>> {
    NAV.lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// Remember one peer per MCP connection so a binding change emits one
/// `tools/list_changed`. Repeat calls from the same connection replace it.
pub fn observe(config: &ServerConfig, connection: u64, peer: Peer<RoleServer>) {
    let Some(session) = config.browser_session.as_deref() else {
        return;
    };
    if matches!(config.profile, Profile::Minimal) {
        return;
    }
    let mut peers = hub();
    if let Some(row) = peers
        .peers
        .iter_mut()
        .find(|(id, existing, _)| id == session && *existing == connection)
    {
        *row = (session.to_owned(), connection, peer);
        return;
    }
    if peers.peers.len() >= 32 {
        peers.peers.remove(0);
    }
    peers.peers.push((session.to_owned(), connection, peer));
}

/// Append the tool only for a non-minimal profile with a live owner match.
pub fn augment(config: &ServerConfig, tools: &mut Vec<Tool>) {
    if matches!(config.profile, Profile::Minimal) {
        return;
    }
    let Some(session) = config.browser_session.as_deref() else {
        return;
    };
    if !visible(session) {
        return;
    }
    if let Ok(tool) = serde_json::from_value(browser_navigate_tool_json()) {
        tools.push(tool);
    }
}

/// Apply one sync and notify peers when visibility changes.
pub async fn apply_sync(sync: BrowserSync, proven: bool) -> Result<u64, ApplyFault> {
    let (result, notify_session) = with_shared(|book| {
        let before = book.owner().map(str::to_owned);
        let result = book.apply(&sync, proven).map(|()| sync.revision);
        let after = book.owner().map(str::to_owned);
        let notify = (before != after).then(|| before.or(after)).flatten();
        (result, notify)
    });
    if let Some(session) = notify_session {
        notify(&session).await;
    }
    result
}

/// Unregister the tool when `session` owns the live window.
pub async fn clear_owner(session: &str) -> Result<(), ApplyFault> {
    let changed = with_shared(|book| book.clear_owner(session))?;
    if changed {
        notify(session).await;
    }
    Ok(())
}

/// Install the Host navigator used when a plan is [`Plan::Ready`].
pub fn install_remote(peer: HostPeer) {
    *NAV.lock().unwrap_or_else(|error| error.into_inner()) =
        Some(Arc::new(RemoteNavigator { peer }));
}

/// Handle one call. The catalog cache is not trusted.
pub async fn call(
    config: &ServerConfig,
    stopped: bool,
    arguments: Value,
) -> Result<CallToolResult, Box<ToolError>> {
    let Some(session) = config.browser_session.clone() else {
        return Err(Box::new(unavailable()));
    };
    if matches!(config.profile, Profile::Minimal) || !visible(&session) {
        return Err(Box::new(unavailable()));
    }
    let url = url_arg(arguments)?;
    let plan = with_shared(|book| {
        let Some((instance, revision, document, policy)) = book.nav_context() else {
            return Plan::Refuse(NavFault::Unavailable);
        };
        book.plan(
            NavRequest {
                url: &url,
                session_id: &session,
                instance_id: &instance,
                revision,
                document_revision: document,
                policy_version: policy.unwrap_or(0),
                stopped,
            },
            l1_open(current_platform()),
        )
    });
    match plan {
        Plan::Ready(canonical) => dispatch_ready(session, canonical).await,
        Plan::Refuse(NavFault::Unknown) => Ok(unknown_result()),
        Plan::Refuse(fault) => Err(Box::new(tool_error(fault))),
    }
}

async fn dispatch_ready(
    session: String,
    canonical: String,
) -> Result<CallToolResult, Box<ToolError>> {
    let Some(nav) = navigator() else {
        return Err(Box::new(unavailable()));
    };
    let Some(body) = with_shared(|book| {
        book.nav_context()
            .map(|(instance, revision, document, policy)| BrowserNavigate {
                url: canonical.clone(),
                instance_id: instance,
                revision,
                document_revision: document,
                policy_version: policy.unwrap_or(0),
                session_id: session.clone(),
            })
    }) else {
        return Err(Box::new(unavailable()));
    };
    match nav.navigate(body).await {
        Ok(()) => Ok(handed_off()),
        Err(NavFault::Unknown) => Ok(unknown_result()),
        Err(fault) => Err(Box::new(tool_error(fault))),
    }
}

fn visible(session: &str) -> bool {
    with_shared(|book| book.owner() == Some(session))
}

fn url_arg(arguments: Value) -> Result<String, Box<ToolError>> {
    let Value::Object(map) = arguments else {
        return Err(Box::new(invalid_args()));
    };
    if map.len() != 1 || !map.contains_key("url") {
        return Err(Box::new(invalid_args()));
    }
    match map.get("url") {
        Some(Value::String(url))
            if !url.is_empty() && url.len() <= gcu_protocol::builtin_browser::URL_MAX_BYTES =>
        {
            Ok(url.clone())
        }
        _ => Err(Box::new(invalid_args())),
    }
}

fn unavailable() -> ToolError {
    let mut error = result::error(ErrorCode::CapabilityUnavailable);
    error.message = "This tool will be available in a later release or another profile.".into();
    error
}

fn invalid_args() -> ToolError {
    ToolError {
        code: ErrorCode::CapabilityUnavailable,
        message: "Invalid arguments; check the registered inputSchema.".into(),
        hint: "Correct the arguments before calling again.".into(),
        retryable: false,
        state: None,
    }
}

fn tool_error(fault: NavFault) -> ToolError {
    let code = model_error(fault).unwrap_or(ErrorCode::CapabilityUnavailable);
    let mut error = result::error(code);
    if fault == NavFault::Unavailable {
        error.message = "This tool will be available in a later release or another profile.".into();
    }
    error
}

fn handed_off() -> CallToolResult {
    // Delivery to native navigation succeeded. Page load is not claimed.
    outcome_result("done")
}

fn unknown_result() -> CallToolResult {
    outcome_result("unknown")
}

fn outcome_result(outcome: &str) -> CallToolResult {
    let mut result = CallToolResult::success(vec![ContentBlock::text(format!(
        "outcome: {outcome}\neffect: not_checked\n"
    ))]);
    result.structured_content = Some(json!({ "outcome": outcome, "effect": "not_checked" }));
    result.is_error = Some(false);
    result
}

async fn notify(session: &str) {
    let peers: Vec<_> = hub()
        .peers
        .iter()
        .filter(|(id, _, _)| id == session)
        .map(|(_, _, peer)| peer.clone())
        .collect();
    for peer in peers {
        let _ = peer.notify_tool_list_changed().await;
    }
}

#[async_trait::async_trait]
trait Navigator: Send + Sync {
    async fn navigate(&self, body: BrowserNavigate) -> Result<(), NavFault>;
}

struct RemoteNavigator {
    peer: HostPeer,
}

#[async_trait::async_trait]
impl Navigator for RemoteNavigator {
    async fn navigate(&self, body: BrowserNavigate) -> Result<(), NavFault> {
        match self
            .peer
            .call::<_, serde_json::Value>("host.browser.navigate", &body, RPC_TIMEOUT)
            .await
        {
            // dispatched:true is delivery. Do not record that body as unknown.
            Ok(value) => classify_navigated(&value),
            Err(gcu_host_protocol::Error::Remote(code)) => Err(fault_from_code(code)),
            // The frame may already have left. Do not claim it was not delivered.
            Err(gcu_host_protocol::Error::Timeout | gcu_host_protocol::Error::Disconnected) => {
                Err(NavFault::Unknown)
            }
            // These two fail before the frame is queued.
            Err(gcu_host_protocol::Error::Busy) => Err(NavFault::Busy),
            Err(gcu_host_protocol::Error::Invalid) => Err(NavFault::Internal),
            Err(_) => Err(NavFault::Unknown),
        }
    }
}

fn fault_from_code(code: i32) -> NavFault {
    match code {
        -32010 | -32601 => NavFault::Unavailable,
        -32011 | -32004 => NavFault::Owner,
        -32012 => NavFault::Gone,
        -32013 => NavFault::Stale,
        -32014 => NavFault::UrlBlocked,
        -32015 => NavFault::Stopped,
        -32602 => NavFault::Invalid,
        -32002 => NavFault::Busy,
        // Host proved the URL was not handed off.
        -32603 => NavFault::Internal,
        -32016 => NavFault::Unknown,
        _ => NavFault::Unknown,
    }
}
