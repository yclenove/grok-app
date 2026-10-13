//! Private session IPC. Credentials bind to a Host-created session/run, not request JSON.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{oneshot, Semaphore};

use crate::broker::ComputerUseBroker;

#[derive(Clone)]
pub struct SessionBinding {
    pub session_id: String,
    pub run_id: String,
}

#[derive(Default)]
struct Credentials(Mutex<HashMap<String, SessionBinding>>);

#[derive(Clone)]
struct Service {
    broker: Arc<ComputerUseBroker>,
    stop_handler: Option<Arc<crate::tools::ModelStopHandler>>,
    credentials: Arc<Credentials>,
    slots: Arc<Semaphore>,
    stop_slots: Arc<Semaphore>,
    handoff_slots: Arc<Semaphore>,
    request_slots: Arc<Semaphore>,
    completion_slots: Arc<Semaphore>,
    extension_poll_slot: Arc<Semaphore>,
    extension_result_slot: Arc<Semaphore>,
    rate: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
    upload_calls: upload::UploadCalls,
}

const RATE_MAX: usize = 8;
const RATE_WINDOW: Duration = Duration::from_millis(800);
const CONTROL_RATE_MAX: usize = 4;
const CONTROL_SLOTS: usize = 2;
const REQUEST_RATE_MAX: usize = 4;
const REQUEST_SLOTS: usize = 2;

impl Service {
    fn tool_admission(&self, name: &str) -> (&Arc<Semaphore>, usize, &'static str) {
        // Cancellation cannot wait behind the work it must cancel. Stop and
        // handoff also have separate bounded budgets: handoff flooding must not
        // consume Stop's reserved admission. Authentication is checked first.
        match name {
            "computer_stop" => (&self.stop_slots, CONTROL_RATE_MAX, "stop:"),
            "computer_request_handoff" => (&self.handoff_slots, CONTROL_RATE_MAX, "handoff:"),
            "computer_request_access" => (&self.request_slots, REQUEST_RATE_MAX, "request:"),
            "computer_complete_task" => (&self.completion_slots, CONTROL_RATE_MAX, "complete:"),
            _ => (&self.slots, RATE_MAX, ""),
        }
    }
}

fn forget_retired_rates(service: &Service) {
    let retired = service.broker.take_retired_tokens();
    if retired.is_empty() {
        return;
    }
    service
        .rate
        .lock()
        .retain(|key, _| !retired.iter().any(|token| key.ends_with(token.as_str())));
}

fn tool_rate_limited(hits: &[Instant], now: Instant) -> Value {
    let remaining = hits
        .first()
        .map(|oldest| RATE_WINDOW.saturating_sub(now.saturating_duration_since(*oldest)))
        .unwrap_or(RATE_WINDOW);
    // Round up so a client following this explicit backpressure signal cannot
    // re-enter before the oldest admission has actually left the window.
    json!({"isError": true, "code": "rate_limited", "completion": "not_started",
        "retryAfterMs": remaining.as_millis() as u64 + 1,
        "content": [{"type": "text", "text": "rate limited"}]})
}

pub(crate) fn admit_rate(
    hits: &mut Vec<Instant>,
    now: Instant,
    max: usize,
    window: Duration,
) -> bool {
    hits.retain(|t| now.saturating_duration_since(*t) < window);
    if hits.len() >= max {
        return false;
    }
    hits.push(now);
    true
}

pub fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get("authorization")?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if scheme.eq_ignore_ascii_case("bearer") {
        let token = token.trim();
        if token.is_empty() {
            None
        } else {
            Some(token)
        }
    } else {
        None
    }
}

/// DNS-rebinding defense: the Host header must name the loopback listener.
pub fn host_is_loopback(host: &str) -> bool {
    let host = host.trim();
    if host.is_empty() {
        return false;
    }
    let name = if let Some(inner) = host.strip_prefix('[') {
        inner.split(']').next().unwrap_or("")
    } else if let Some((name, port)) = host.rsplit_once(':') {
        if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
            name
        } else {
            host
        }
    } else {
        host
    };
    let name = name.trim().trim_end_matches('.');
    matches!(name, "127.0.0.1" | "localhost" | "::1")
}

pub struct IpcServer {
    pub url: String,
    service: Service,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
}

impl IpcServer {
    #[cfg(test)]
    pub(crate) fn test_upload_waiter_counts(&self) -> (usize, usize) {
        self.service.upload_calls.waiter_counts()
    }
    fn validate_session_binding(&self, session: &str, run: &str) -> Result<(), String> {
        self.service
            .broker
            .require_owner(session, run)
            .map_err(|e| e.to_string())?;
        self.service
            .broker
            .authorized_target(run)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn fresh_token() -> String {
        format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        )
    }

    fn remove_rate_entries(&self, tokens: &[String]) {
        if tokens.is_empty() {
            return;
        }
        let mut rate = self.service.rate.lock();
        for token in tokens {
            rate.remove(token);
            rate.remove(&format!("stop:{token}"));
            rate.remove(&format!("handoff:{token}"));
            rate.remove(&format!("request:{token}"));
        }
    }

    /// Return the stable credential for the current session/run.
    ///
    /// Full MCP catalog replacement is retried after ambiguous ACP failures and
    /// whenever the base extension catalog changes. Those retries must render
    /// the same credential; rotating here would invalidate the MCP child that
    /// ACP still considers current before the replacement is committed.
    pub fn credential_for_session(&self, session: &str, run: &str) -> Result<String, String> {
        self.validate_session_binding(session, run)?;
        let mut entries = self.service.credentials.0.lock();
        if let Some(token) = entries.iter().find_map(|(token, binding)| {
            (binding.session_id == session && binding.run_id == run).then(|| token.clone())
        }) {
            return Ok(token);
        }

        let removed = entries
            .iter()
            .filter(|(_, binding)| binding.session_id == session)
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();
        entries.retain(|_, binding| binding.session_id != session);
        let token = Self::fresh_token();
        entries.insert(
            token.clone(),
            SessionBinding {
                session_id: session.into(),
                run_id: run.into(),
            },
        );
        drop(entries);
        self.remove_rate_entries(&removed);
        Ok(token)
    }

    /// Stable pre-approval credential. Does not read or replace legacy run tokens.
    pub fn issue_transport_credential(&self, app_session_id: &str) -> Result<String, String> {
        self.service
            .broker
            .issue_transport_credential(app_session_id)
            .map(|issued| issued.token().to_string())
            .map_err(|err| err.to_string())
    }

    /// Explicitly rotate a live credential. Catalog reconciliation must use
    /// `credential_for_session`; rotation is reserved for a deliberate
    /// security boundary where the caller also replaces the ACP catalog.
    pub fn rotate_session_credential(&self, session: &str, run: &str) -> Result<String, String> {
        self.validate_session_binding(session, run)?;
        let token = Self::fresh_token();
        let mut entries = self.service.credentials.0.lock();
        let removed = entries
            .iter()
            .filter(|(_, binding)| binding.session_id == session)
            .map(|(existing, _)| existing.clone())
            .collect::<Vec<_>>();
        entries.retain(|_, binding| binding.session_id != session);
        entries.insert(
            token.clone(),
            SessionBinding {
                session_id: session.into(),
                run_id: run.into(),
            },
        );
        drop(entries);
        self.remove_rate_entries(&removed);
        Ok(token)
    }

    /// Issue a fresh credential and retire the previous one. Kept for explicit
    /// probe/security-rotation callers; MCP catalog construction must use the
    /// stable `credential_for_session` API instead.
    pub fn issue_session(&self, session: &str, run: &str) -> Result<String, String> {
        self.rotate_session_credential(session, run)
    }

    pub fn revoke_session(&self, session: &str) {
        let tokens = {
            let mut entries = self.service.credentials.0.lock();
            let tokens = entries
                .iter()
                .filter(|(_, binding)| binding.session_id == session)
                .map(|(token, _)| token.clone())
                .collect::<Vec<_>>();
            entries.retain(|_, b| b.session_id != session);
            tokens
        };
        self.remove_rate_entries(&tokens);
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        // Listener shutdown must retire its original upload waiters before
        // graceful HTTP shutdown waits for their still-connected responses.
        self.service.upload_calls.close_all();
        self.service.broker.tabs().revoke_pairing();
        let bindings = std::mem::take(&mut *self.service.credentials.0.lock());
        for binding in bindings.values() {
            // Destructors must not perform adapter, filesystem, or loopback
            // network cleanup. The Host shutdown coordinator owns that work;
            // this fallback still fences every dispatch immediately.
            let _ = self.service.broker.fence_stop(&binding.run_id);
        }
        if let Some(shutdown) = self.shutdown.lock().take() {
            let _ = shutdown.send(());
        }
    }
}

pub fn spawn(broker: Arc<ComputerUseBroker>) -> Result<IpcServer, String> {
    spawn_with_stop_handler(broker, None)
}

pub fn spawn_with_stop_handler(
    broker: Arc<ComputerUseBroker>,
    stop_handler: Option<Arc<crate::tools::ModelStopHandler>>,
) -> Result<IpcServer, String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let url = format!(
        "http://{}",
        listener.local_addr().map_err(|e| e.to_string())?
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let service = Service {
        broker,
        stop_handler,
        credentials: Arc::default(),
        slots: Arc::new(Semaphore::new(8)),
        stop_slots: Arc::new(Semaphore::new(CONTROL_SLOTS)),
        handoff_slots: Arc::new(Semaphore::new(CONTROL_SLOTS)),
        request_slots: Arc::new(Semaphore::new(REQUEST_SLOTS)),
        completion_slots: Arc::new(Semaphore::new(CONTROL_SLOTS)),
        extension_poll_slot: Arc::new(Semaphore::new(1)),
        extension_result_slot: Arc::new(Semaphore::new(1)),
        rate: Arc::default(),
        upload_calls: Arc::default(),
    };
    let app = Router::new()
        .route("/cu/tool", post(tool_call))
        .route("/cu/upload", post(upload::call))
        .route(
            "/cu/pairing-challenge",
            get(pairing_challenge)
                .post(pairing_challenge)
                .options(pairing_preflight),
        )
        .route(
            "/cu/pairing-confirm",
            post(pairing_confirm).options(pairing_preflight),
        )
        .route(
            "/cu/extension-status",
            post(extension_status).options(extension_lease_preflight),
        )
        .route(
            "/cu/extension-disconnect",
            post(extension_disconnect).options(extension_retirement_preflight),
        )
        .route(
            "/cu/extension-heartbeat",
            post(extension_heartbeat).options(extension_lease_preflight),
        )
        .route(
            "/cu/browser-restart",
            post(browser_restart).options(extension_lease_preflight),
        )
        .route("/cu/tab-offer", post(tab_offer).options(pairing_preflight))
        .route(
            "/cu/tab-unoffer",
            post(tab_unoffer).options(extension_retirement_preflight),
        )
        .route(
            "/cu/extension-poll",
            post(extension_transport::poll).options(extension_transport::preflight),
        )
        .route(
            "/cu/extension-result",
            post(extension_transport::result)
                .options(extension_transport::preflight)
                .layer(DefaultBodyLimit::max(
                    crate::browser::extension_protocol::EXTENSION_RESULT_BYTES,
                )),
        )
        .nest(
            "/cu/extension-completion",
            Router::new()
                .route(
                    "/claim",
                    post(extension_completion::claim).options(extension_completion::preflight),
                )
                .route(
                    "/status",
                    post(extension_completion::status).options(extension_completion::preflight),
                )
                .route(
                    "/settle",
                    post(extension_completion::settle).options(extension_completion::preflight),
                )
                .route(
                    "/retirement",
                    post(extension_completion::retirement).options(extension_completion::preflight),
                )
                .route(
                    "/host-lifetime",
                    post(extension_completion::host_lifetime)
                        .options(extension_completion::preflight),
                )
                .route(
                    "/host-retirement",
                    post(extension_completion::host_retirement)
                        .options(extension_completion::preflight),
                )
                .layer(DefaultBodyLimit::max(
                    crate::browser::extension_completion::COMPLETION_REQUEST_BYTES,
                )),
        )
        .nest("/cu/extension-actions", extension_actions::routes())
        .nest("/cu/extension-observation", extension_observation::routes())
        .layer(DefaultBodyLimit::max(64 * 1024))
        .with_state(service.clone());
    let (shutdown, stop) = oneshot::channel();
    let lease_broker = service.broker.clone();
    std::thread::Builder::new()
        .name("computer-use-ipc".into())
        .spawn(move || {
            runtime.block_on(async move {
                if let Ok(listener) = tokio::net::TcpListener::from_std(listener) {
                    let serve = async {
                        let _ = axum::serve(listener, app)
                            .with_graceful_shutdown(async {
                                let _ = stop.await;
                            })
                            .await;
                    };
                    let sweep = async {
                        let mut tick = tokio::time::interval(Duration::from_secs(1));
                        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                        loop {
                            tick.tick().await;
                            lease_broker.expire_existing_share_preparation();
                            lease_broker.tabs().expire_pairing_connection();
                        }
                    };
                    // Maintenance belongs to this listener, not a detached immortal task.
                    tokio::select! { _ = serve => {}, _ = sweep => {} }
                }
            });
        })
        .map_err(|e| e.to_string())?;
    Ok(IpcServer {
        url,
        service,
        shutdown: Mutex::new(Some(shutdown)),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolCall {
    name: String,
    #[serde(default)]
    session: Option<String>,
    #[serde(default = "empty_tool_arguments")]
    arguments: Value,
}

fn empty_tool_arguments() -> Value {
    Value::Object(Default::default())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SharedTabOfferRequest {
    instance_id: String,
    connection_nonce: String,
    generation: u64,
    sequence: u64,
    tab_id: String,
    title: String,
    url: String,
    browser_id: String,
    profile_id: String,
    document_generation: u64,
    #[serde(default)]
    document_id: Option<String>,
    focused: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SharedTabUnofferRequest {
    instance_id: String,
    connection_nonce: String,
    generation: u64,
    sequence: u64,
    tab_id: String,
    document_generation: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct BrowserRestartRequest {
    connection: crate::pairing::PairingConnection,
    current_id: String,
    previous_id: Option<String>,
    #[serde(default)]
    cleanups: Vec<BrowserExitCleanupBody>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct BrowserExitCleanupBody {
    /// Shipped `durableCleanupRecord` always includes `version: 1`.
    version: u32,
    browser_id: String,
    request_id: String,
    document_id: String,
    phase: String,
}

fn transport_session_override(call: &ToolCall, session: &str) -> bool {
    if call.session.as_ref().is_some_and(|value| value != session) {
        return true;
    }
    for key in ["session", "appSessionId", "app_session_id"] {
        if call
            .arguments
            .get(key)
            .is_some_and(|value| value.as_str() != Some(session))
        {
            return true;
        }
    }
    false
}

fn identity_overrides_binding(call: &ToolCall, binding: &SessionBinding) -> bool {
    if call
        .session
        .as_ref()
        .is_some_and(|s| s != &binding.session_id)
    {
        return true;
    }
    let args = &call.arguments;
    for (key, expected) in [
        ("runId", binding.run_id.as_str()),
        ("run_id", binding.run_id.as_str()),
        ("session", binding.session_id.as_str()),
        ("appSessionId", binding.session_id.as_str()),
        ("app_session_id", binding.session_id.as_str()),
    ] {
        if args
            .get(key)
            .is_some_and(|value| value.as_str() != Some(expected))
        {
            return true;
        }
    }
    false
}

async fn tool_call(
    State(service): State<Service>,
    headers: HeaderMap,
    Json(call): Json<ToolCall>,
) -> (StatusCode, Json<Value>) {
    if let Err(error) = tool_headers(&headers) {
        return error;
    }
    let presented = bearer_token(&headers).unwrap_or("");
    if presented.is_empty() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(error("invalid session credential")),
        );
    }
    tool_call_authenticated(service, presented.to_string(), call).await
}

fn tool_headers(headers: &HeaderMap) -> Result<(), (StatusCode, Json<Value>)> {
    // CORS is not authentication. A browser page must never reach the control endpoint.
    if headers.contains_key("origin")
        || headers.contains_key("referer")
        || headers.contains_key("forwarded")
        || headers.contains_key("x-forwarded-for")
        || headers.contains_key("x-forwarded-host")
        || headers.contains_key("sec-fetch-site")
        || headers.contains_key("sec-fetch-mode")
        || headers.contains_key("sec-fetch-dest")
        || headers.contains_key("cookie")
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error("browser origin is not allowed")),
        ));
    }
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !host_is_loopback(host)
        || headers.get_all("host").iter().count() != 1
        || headers.get_all("authorization").iter().count() > 1
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error("host is not loopback or headers are ambiguous")),
        ));
    }
    Ok(())
}

fn admit_tool(
    service: &Service,
    token: &str,
    name: &str,
) -> Result<tokio::sync::OwnedSemaphorePermit, (StatusCode, Json<Value>)> {
    let (slots, rate_max, prefix) = service.tool_admission(name);
    {
        let mut hits = service.rate.lock();
        let queue = hits.entry(format!("{prefix}{token}")).or_default();
        let now = Instant::now();
        if !admit_rate(queue, now, rate_max, RATE_WINDOW) {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(tool_rate_limited(queue, now)),
            ));
        }
    }
    Arc::clone(slots).try_acquire_owned().map_err(|_| {
        (
            StatusCode::TOO_MANY_REQUESTS,
            Json(error("too many pending calls")),
        )
    })
}

async fn tool_call_authenticated(
    service: Service,
    presented: String,
    call: ToolCall,
) -> (StatusCode, Json<Value>) {
    if let crate::task_consent::BearerClass::Live(ctx) = service.broker.classify_bearer(&presented)
    {
        if transport_session_override(&call, &ctx.app_session_id) {
            return (
                StatusCode::FORBIDDEN,
                Json(error("session/run does not match credential")),
            );
        }
        let token = presented;
        let permit = match admit_tool(&service, &token, &call.name) {
            Ok(permit) => permit,
            Err(error) => return error,
        };
        let result = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let result = crate::tools::dispatch_with_transport(
                &service.broker,
                &token,
                &call.name,
                call.arguments,
                service.stop_handler.as_deref(),
            );
            forget_retired_rates(&service);
            result
        })
        .await
        .unwrap_or_else(|_| error("tool worker failed; outcome unknown"));
        return (StatusCode::OK, Json(result));
    }
    if matches!(
        service.broker.classify_bearer(&presented),
        crate::task_consent::BearerClass::Retired
    ) {
        forget_retired_rates(&service);
        return (
            StatusCode::UNAUTHORIZED,
            Json(error("invalid session credential")),
        );
    }
    let binding = service.credentials.0.lock().get(&presented).cloned();
    let Some(binding) = binding else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(error("invalid session credential")),
        );
    };
    if identity_overrides_binding(&call, &binding) {
        return (
            StatusCode::FORBIDDEN,
            Json(error("session/run does not match credential")),
        );
    }
    let permit = match admit_tool(&service, &presented, &call.name) {
        Ok(permit) => permit,
        Err(error) => return error,
    };
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        crate::tools::dispatch_with_stop_handler(
            &service.broker,
            &binding,
            None,
            &call.name,
            call.arguments,
            service.stop_handler.as_deref(),
        )
    })
    .await
    .unwrap_or_else(|_| error("tool worker failed; outcome unknown"));
    (StatusCode::OK, Json(result))
}

/// Pairing HTTP is loopback-only and still requires the exact installed
/// extension Origin. Origin is defense in depth; the one-time HMAC proof is
/// the identity boundary against native loopback callers that can forge it.
pub fn pairing_headers_ok(headers: &HeaderMap) -> bool {
    pairing_headers_allow(headers, None)
}

pub fn pairing_headers_ok_for(headers: &HeaderMap, installed: Option<&str>) -> bool {
    pairing_headers_allow(headers, installed)
}

fn pairing_headers_allow(headers: &HeaderMap, installed: Option<&str>) -> bool {
    if headers.get_all("origin").iter().count() != 1
        || headers.get_all("host").iter().count() != 1
        || headers.get_all("authorization").iter().count() > 1
        || headers.contains_key("forwarded")
        || headers.contains_key("x-forwarded-for")
        || headers.contains_key("x-forwarded-host")
    {
        return false;
    }
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !host_is_loopback(host) {
        return false;
    }
    headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|origin| pairing_origin_ok(origin, installed))
}

fn pairing_origin_ok(origin: &str, installed: Option<&str>) -> bool {
    installed.is_some_and(|id| !id.is_empty() && origin == format!("chrome-extension://{id}"))
}

fn pairing_gate(
    service: &Service,
    headers: &HeaderMap,
) -> Result<HeaderMap, Box<(StatusCode, HeaderMap, Json<Value>)>> {
    pairing_gate_for(service, headers, "pairing")
}

fn pairing_gate_for(
    service: &Service,
    headers: &HeaderMap,
    bucket: &'static str,
) -> Result<HeaderMap, Box<(StatusCode, HeaderMap, Json<Value>)>> {
    let installed = service.broker.tabs().installed_extension_id();
    let cors = pairing_cors_for(headers, Some(installed.as_str()));
    if !pairing_headers_allow(headers, Some(installed.as_str())) {
        return Err(Box::new((
            StatusCode::FORBIDDEN,
            cors,
            Json(json!({"error": "pairing host is not loopback"})),
        )));
    }
    {
        let mut hits = service.rate.lock();
        // Fixed bounded buckets: user pairing traffic cannot delay retirement or renewal.
        let queue = hits.entry(bucket.into()).or_default();
        if !admit_rate(queue, Instant::now(), RATE_MAX, RATE_WINDOW) {
            return Err(Box::new((
                StatusCode::TOO_MANY_REQUESTS,
                cors,
                Json(json!({"error": "rate limited"})),
            )));
        }
    }
    Ok(cors)
}

/// Reflect only the exact installed extension Origin.
fn pairing_cors_for(headers: &HeaderMap, installed: Option<&str>) -> HeaderMap {
    let mut out = HeaderMap::new();
    out.insert(
        axum::http::header::CACHE_CONTROL,
        "no-store".parse().unwrap(),
    );
    if !pairing_headers_allow(headers, installed) {
        return out;
    }
    let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) else {
        return out;
    };
    if !pairing_origin_ok(origin, installed) {
        return out;
    }
    if let Ok(value) = origin.parse() {
        out.insert(axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
    }
    out.insert(
        axum::http::header::ACCESS_CONTROL_ALLOW_METHODS,
        "GET, POST, OPTIONS".parse().unwrap(),
    );
    out.insert(
        axum::http::header::ACCESS_CONTROL_ALLOW_HEADERS,
        "authorization, content-type".parse().unwrap(),
    );
    out.insert(axum::http::header::VARY, "Origin".parse().unwrap());
    out
}

async fn pairing_preflight(
    State(service): State<Service>,
    headers: HeaderMap,
) -> (StatusCode, HeaderMap) {
    preflight_for(&service, &headers, "pairing")
}

async fn extension_lease_preflight(
    State(service): State<Service>,
    headers: HeaderMap,
) -> (StatusCode, HeaderMap) {
    preflight_for(&service, &headers, "extension-lease")
}

async fn extension_retirement_preflight(
    State(service): State<Service>,
    headers: HeaderMap,
) -> (StatusCode, HeaderMap) {
    preflight_for(&service, &headers, "extension-retirement")
}

fn preflight_for(
    service: &Service,
    headers: &HeaderMap,
    bucket: &'static str,
) -> (StatusCode, HeaderMap) {
    match pairing_gate_for(service, headers, bucket) {
        Ok(cors) => (StatusCode::NO_CONTENT, cors),
        Err(err) => {
            let (status, cors, _) = *err;
            (status, cors)
        }
    }
}

async fn pairing_challenge(
    State(service): State<Service>,
    headers: HeaderMap,
) -> (StatusCode, HeaderMap, Json<Value>) {
    let cors = match pairing_gate(&service, &headers) {
        Ok(cors) => cors,
        Err(err) => return *err,
    };
    let ch = match service.broker.public_pairing_challenge() {
        Ok(ch) => ch,
        Err(_) => {
            return (
                StatusCode::FORBIDDEN,
                cors,
                Json(json!({"error": "pairing preparation unavailable"})),
            )
        }
    };
    let Some(ch) = ch else {
        return (
            StatusCode::NOT_FOUND,
            cors,
            Json(json!({"error": "no pairing challenge"})),
        );
    };
    if now_ms() >= ch.expires_at_ms {
        return (
            StatusCode::GONE,
            cors,
            Json(json!({"error": "pairing challenge expired"})),
        );
    }
    (
        StatusCode::OK,
        cors,
        Json(json!({
            "protocol": crate::pairing::PAIRING_PROTOCOL,
            "nonce": ch.nonce,
            "instance": ch.instance_id,
            "ext": ch.installed_extension_id,
            "expiresAt": ch.expires_at_ms,
        })),
    )
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

async fn pairing_confirm(
    State(service): State<Service>,
    headers: HeaderMap,
    Json(body): Json<crate::pairing::PairingProof>,
) -> (StatusCode, HeaderMap, Json<Value>) {
    let cors = match pairing_gate(&service, &headers) {
        Ok(cors) => cors,
        Err(err) => return *err,
    };
    // Identity, MAC verification and one-shot consumption share one Host lock.
    match service.broker.complete_pairing(&body) {
        Ok(session) => (StatusCode::OK, cors, Json(json!(session))),
        Err(e) => (StatusCode::FORBIDDEN, cors, Json(json!({"error": e}))),
    }
}

async fn extension_status(
    State(service): State<Service>,
    headers: HeaderMap,
    Json(connection): Json<crate::pairing::PairingConnection>,
) -> (StatusCode, HeaderMap, Json<Value>) {
    extension_connection(
        &service,
        &headers,
        &connection,
        ExtensionConnectionOperation::Status,
    )
}

async fn extension_disconnect(
    State(service): State<Service>,
    headers: HeaderMap,
    Json(connection): Json<crate::pairing::PairingConnection>,
) -> (StatusCode, HeaderMap, Json<Value>) {
    extension_connection(
        &service,
        &headers,
        &connection,
        ExtensionConnectionOperation::Disconnect,
    )
}

async fn extension_heartbeat(
    State(service): State<Service>,
    headers: HeaderMap,
    Json(connection): Json<crate::pairing::PairingConnection>,
) -> (StatusCode, HeaderMap, Json<Value>) {
    extension_connection(
        &service,
        &headers,
        &connection,
        ExtensionConnectionOperation::Heartbeat,
    )
}

async fn browser_restart(
    State(service): State<Service>,
    headers: HeaderMap,
    Json(body): Json<BrowserRestartRequest>,
) -> (StatusCode, HeaderMap, Json<Value>) {
    // Browser lifecycle registration is an authenticated extension lease operation,
    // not a user pairing handshake. Keep it on the lease bucket so a rapid sequence
    // of challenge/confirm/share calls cannot reject a legitimate onStartup update.
    let cors = match pairing_gate_for(&service, &headers, "extension-lease") {
        Ok(cors) => cors,
        Err(error) => return *error,
    };
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let cleanups = body
        .cleanups
        .iter()
        .filter(|cleanup| cleanup.version == 1)
        .map(
            |cleanup| crate::browser::extension_completion::BrowserExitCleanup {
                browser_id: cleanup.browser_id.clone(),
                request_id: cleanup.request_id.clone(),
                document_id: cleanup.document_id.clone(),
                phase: cleanup.phase.clone(),
            },
        )
        .collect::<Vec<_>>();
    match service.broker.register_extension_browser(
        origin,
        bearer_token(&headers).unwrap_or_default(),
        &body.connection,
        &body.current_id,
        body.previous_id.as_deref(),
        &cleanups,
    ) {
        Ok(released) => (
            StatusCode::OK,
            cors,
            Json(json!({
                "ok": true,
                "abandoned": released.len(),
                "results": released.iter().map(|(request_id, result)| json!({
                    "requestId": request_id,
                    "result": result,
                })).collect::<Vec<_>>(),
            })),
        ),
        Err(_) => (
            StatusCode::FORBIDDEN,
            cors,
            Json(json!({"error":"browser restart unavailable"})),
        ),
    }
}

async fn tab_offer(
    State(service): State<Service>,
    headers: HeaderMap,
    Json(body): Json<SharedTabOfferRequest>,
) -> (StatusCode, HeaderMap, Json<Value>) {
    let cors = match pairing_gate(&service, &headers) {
        Ok(cors) => cors,
        Err(error) => return *error,
    };
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let token = bearer_token(&headers).unwrap_or_default();
    let connection = crate::pairing::PairingConnection {
        instance_id: body.instance_id.clone(),
        connection_nonce: body.connection_nonce.clone(),
        generation: body.generation,
    };
    let extension_id = service.broker.tabs().installed_extension_id();
    match service.broker.offer_connected_tab_document(
        &connection,
        body.sequence,
        crate::browser::SharedTabOffer {
            pairing_token: token,
            origin,
            extension_id: Some(extension_id.as_str()),
            tab_id: &body.tab_id,
            title: &body.title,
            url: &body.url,
            browser_id: &body.browser_id,
            profile_id: &body.profile_id,
            document_generation: body.document_generation,
            connection_generation: body.generation,
            focused: body.focused,
        },
        body.document_id.as_deref(),
    ) {
        Ok(()) => (
            StatusCode::OK,
            cors,
            Json(json!({"ok": true, "tabId": body.tab_id})),
        ),
        Err(error) => (StatusCode::FORBIDDEN, cors, Json(json!({"error": error}))),
    }
}

async fn tab_unoffer(
    State(service): State<Service>,
    headers: HeaderMap,
    Json(body): Json<SharedTabUnofferRequest>,
) -> (StatusCode, HeaderMap, Json<Value>) {
    let cors = match pairing_gate_for(&service, &headers, "extension-retirement") {
        Ok(cors) => cors,
        Err(error) => return *error,
    };
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let token = bearer_token(&headers).unwrap_or_default();
    let connection = crate::pairing::PairingConnection {
        instance_id: body.instance_id,
        connection_nonce: body.connection_nonce,
        generation: body.generation,
    };
    match service.broker.unoffer_connected_tab(
        origin,
        token,
        &connection,
        body.sequence,
        &body.tab_id,
        body.document_generation,
    ) {
        Ok(removed) => (
            StatusCode::OK,
            cors,
            Json(json!({"ok": true, "removed": removed})),
        ),
        Err(error) => (StatusCode::FORBIDDEN, cors, Json(json!({"error": error}))),
    }
}

enum ExtensionConnectionOperation {
    Status,
    Heartbeat,
    Disconnect,
}

fn extension_connection(
    service: &Service,
    headers: &HeaderMap,
    connection: &crate::pairing::PairingConnection,
    operation: ExtensionConnectionOperation,
) -> (StatusCode, HeaderMap, Json<Value>) {
    let bucket = if matches!(operation, ExtensionConnectionOperation::Disconnect) {
        "extension-retirement"
    } else {
        "extension-lease"
    };
    let cors = match pairing_gate_for(service, headers, bucket) {
        Ok(cors) => cors,
        Err(error) => return *error,
    };
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let token = bearer_token(headers).unwrap_or_default();
    let result = match operation {
        ExtensionConnectionOperation::Status => service
            .broker
            .pairing_connection_status(origin, token, connection),
        ExtensionConnectionOperation::Heartbeat | ExtensionConnectionOperation::Disconnect => {
            service.broker.check_pairing_connection(
                origin,
                token,
                connection,
                matches!(operation, ExtensionConnectionOperation::Disconnect),
            )
        }
    };
    match result {
        Ok(ready) => (
            StatusCode::OK,
            cors,
            Json(json!({"ok": true, "executionReady": ready})),
        ),
        Err(_) => (
            StatusCode::FORBIDDEN,
            cors,
            Json(json!({"error": "pairing connection unavailable"})),
        ),
    }
}

pub(crate) fn error(message: impl ToString) -> Value {
    json!({"isError":true,"code":"schema","content":[{"type":"text","text":message.to_string()}]})
}

pub(crate) fn fail(err: &crate::error::BrokerError) -> Value {
    json!({"isError":true,"code":err.code(),"content":[{"type":"text","text":err.to_string()}]})
}

pub(crate) fn success(value: Value) -> Value {
    json!({"isError":false,"content":[{"type":"text","text":value.to_string()}]})
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_control_admission;

#[cfg(test)]
mod tests_sharing;

#[cfg(test)]
mod tests_result_upload;
#[cfg(test)]
mod tests_transport;

mod extension_actions;
mod extension_completion;
mod extension_observation;
mod extension_transport;
mod result_body;
mod upload;

#[cfg(test)]
mod tests_actions;
#[cfg(test)]
mod tests_completion;
#[cfg(test)]
mod tests_existing_first_use;
#[cfg(test)]
mod tests_task_consent;
