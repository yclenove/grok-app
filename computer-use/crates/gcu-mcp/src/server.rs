//! The registered surface projects protocol types onto the existing core router.
use crate::{
    catalog, elicitation,
    files::{blocking, SessionFiles},
    instructions, params,
    result::{self, Results},
    Profile, ServeError,
};
use gcu_core::{
    action::ActionRouter,
    driver::Driver,
    session::{Session, WindowLocks},
};
use gcu_host_api::{ApprovalProvider, EventSink, PolicyStore, UserActivity};
use gcu_protocol::*;
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CustomNotification, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig as McpConfig,
        Tool,
    },
    service::{NotificationContext, RequestContext},
    ErrorData, RoleServer, ServerHandler,
};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use tokio::{sync::Semaphore, time::Instant};

/// Trusted in-process Host adapters. No environment variable can grant access.
#[derive(Clone)]
pub struct Host {
    /// App authorization, sensitive confirmation, and handoff UI.
    pub approvals: Arc<dyn ApprovalProvider>,
    /// Read-only engine view of Host policy; Host persists Always decisions.
    pub policy: Arc<dyn PolicyStore>,
    /// Metadata-only action events.
    pub events: Arc<dyn EventSink>,
    /// Live user-activity/Stop latch, never cleared by MCP tools.
    pub activity: Arc<dyn UserActivity>,
}
/// Per-connection exposure settings. Driver/Host selection belongs to the caller.
#[derive(Clone, Debug, Default)]
pub struct ServerConfig {
    /// Tools visible to the client. Every profile filters IMPLEMENTED_TOOLS.
    pub profile: Profile,
}
#[derive(Clone)]
pub(crate) struct Server(Arc<Inner>);
struct Inner {
    driver: Arc<dyn Driver>,
    router: ActionRouter,
    session: Arc<Session>,
    results: Results,
    tools: Vec<Tool>,
    config: ServerConfig,
    slots: Semaphore,
    // Includes encoding/delivery-size bookkeeping in session response order.
    delivery: tokio::sync::Mutex<()>,
    calls: AtomicU64,
    active: AtomicUsize,
    last: Mutex<Instant>,
    closed: gcu_host_api::Cancel,
}
impl Server {
    pub(crate) fn session(&self) -> Arc<Session> {
        self.0.session.clone()
    }
    pub(crate) fn new(
        driver: Arc<dyn Driver>,
        host: Host,
        config: ServerConfig,
        files: SessionFiles,
    ) -> Result<Self, ServeError> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        static LOCKS: OnceLock<WindowLocks> = OnceLock::new();
        let router = ActionRouter::new(
            driver.clone(),
            Arc::new(elicitation::Approvals(host.approvals)),
            host.policy,
            host.events,
            LOCKS.get_or_init(WindowLocks::new).clone(),
        );
        let session = Arc::new(Session::open(
            format!("mcp-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
            host.activity,
            router.input_release(),
        ));
        let results = Results {
            files: Arc::new(files),
            session: session.clone(),
        };
        let tools = catalog::tools(config.profile).map_err(|_| ServeError::Protocol)?;
        Ok(Self(Arc::new(Inner {
            driver,
            router,
            session,
            results,
            tools,
            config,
            slots: Semaphore::new(32),
            delivery: tokio::sync::Mutex::new(()),
            calls: AtomicU64::new(1),
            active: AtomicUsize::new(0),
            last: Mutex::new(Instant::now()),
            closed: gcu_host_api::Cancel::new(),
        })))
    }
    async fn call(&self, name: &str, arguments: Value) -> CallToolResponse {
        let _delivery = self.0.delivery.lock().await;
        let result = self.dispatch(name, arguments).await;
        match result {
            Ok(result) => result,
            Err(error) => self.0.results.failure(*error).await,
        }
        .into()
    }
    async fn dispatch(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<rmcp::model::CallToolResult, Box<ToolError>> {
        let this = &self.0;
        if !this.tools.iter().any(|tool| tool.name == name) {
            let mut error = result::error(ErrorCode::CapabilityUnavailable);
            error.message =
                "This tool will be available in a later release or another profile.".into();
            return Err(Box::new(error));
        }
        this.session
            .begin_turn()
            .map_err(|_| Box::new(result::error(ErrorCode::ApprovalDenied)))?;
        let id = format!("mcp_{}", this.calls.fetch_add(1, Ordering::Relaxed));
        match name {
            "list_apps" => {
                let _: ListAppsParams = params::parse(arguments)?;
                let apps = this.router.list_apps(&this.session).await?;
                Ok(this.results.discovery(json!({"apps":apps})).await)
            }
            "list_windows" => {
                let p: ListWindowsParams = params::parse(arguments)?;
                let windows = this
                    .router
                    .list_windows(&this.session, p.app.as_deref())
                    .await?;
                Ok(this.results.discovery(json!({"windows":windows})).await)
            }
            "launch_app" => {
                let p: LaunchAppParams = params::parse(arguments)?;
                Ok(this
                    .results
                    .outcome(this.router.launch_app(&this.session, &id, p).await)
                    .await)
            }
            "get_app_state" => {
                let p: GetAppStateParams = params::parse(arguments)?;
                let state = this.router.observe(&this.session, p).await?;
                Ok(this.results.state(state).await)
            }
            "capabilities" => {
                let _: CapabilitiesParams = params::parse(arguments)?;
                let caps = this
                    .session
                    .enqueue(async { this.driver.capabilities().clone() })
                    .await
                    .map_err(|_| Box::new(result::error(ErrorCode::ApprovalDenied)))?;
                Ok(this.results.discovery(json!(caps)).await)
            }
            _ => {
                let call = params::action(name, arguments, id)?;
                Ok(this
                    .results
                    .outcome(this.router.execute(&this.session, call).await)
                    .await)
            }
        }
    }
    async fn close(&self) -> Result<(), ServeError> {
        self.0.closed.cancel();
        self.0.session.session_closed().await;
        let files = self.0.results.files.clone();
        blocking(move || files.close()).await?;
        Ok(())
    }
    async fn idle(self) {
        loop {
            tokio::select! {
                _ = self.0.closed.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    let elapsed = self.0.last.lock().unwrap_or_else(|e| e.into_inner()).elapsed();
                    if self.0.active.load(Ordering::Acquire) == 0 && elapsed >= Duration::from_secs(45) {
                        self.0.session.turn_ended().await;
                        *self.0.last.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
                    }
                }
            }
        }
    }
}
impl ServerHandler for Server {
    fn get_info(&self) -> McpConfig {
        McpConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "grok-computer-use",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(instructions::instructions(
                self.0.driver.platform(),
                self.0.config.profile,
            ))
    }
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.0.tools.clone()))
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let Ok(_slot) = self.0.slots.try_acquire() else {
            return Ok(self
                .0
                .results
                .failure(result::error(ErrorCode::Busy))
                .await
                .into());
        };
        self.0.active.fetch_add(1, Ordering::AcqRel);
        let _active = Active(self.clone());
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        Ok(elicitation::REQUEST_PEER
            .scope(context.peer, self.call(&request.name, arguments))
            .await)
    }
    async fn on_custom_notification(
        &self,
        notification: CustomNotification,
        _context: NotificationContext<RoleServer>,
    ) {
        if notification.method == "notifications/turn-ended" {
            self.0.session.turn_ended().await;
        }
    }
}
struct Active(Server);
impl Drop for Active {
    fn drop(&mut self) {
        *self.0 .0.last.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
        self.0 .0.active.fetch_sub(1, Ordering::AcqRel);
    }
}
pub(crate) struct Cleanup {
    server: Option<Server>,
    idle: tokio::task::JoinHandle<()>,
}
impl Cleanup {
    pub(crate) fn new(server: Server) -> Self {
        let idle = tokio::spawn(server.clone().idle());
        Self {
            server: Some(server),
            idle,
        }
    }
    pub(crate) async fn close(&mut self) -> Result<(), ServeError> {
        self.idle.abort();
        if let Some(server) = &self.server {
            server.close().await?;
        }
        self.server = None;
        Ok(())
    }
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        self.idle.abort();
        if let Some(server) = self.server.take() {
            server.0.closed.cancel();
            drop(tokio::spawn(async move {
                let _ = server.close().await;
            }));
        }
    }
}
