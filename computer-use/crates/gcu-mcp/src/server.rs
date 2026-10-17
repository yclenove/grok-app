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
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, OnceLock,
};
use tokio::sync::Semaphore;

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
    default_tree_nodes: u32,
    slots: Semaphore,
    // Includes encoding/delivery-size bookkeeping in session response order.
    delivery: tokio::sync::Mutex<()>,
    calls: AtomicU64,
    lifecycle: crate::lifecycle::Lifecycle,
}
/// Maximum concurrently running tool calls per session.
const SLOTS: usize = 32;
impl Server {
    pub(crate) fn lifecycle(&self) -> crate::lifecycle::Lifecycle {
        self.0.lifecycle.clone()
    }
    /// Wait until no `call_tool` is still running, or `limit` passes. A call that was
    /// cancelled by close (for example a pending approval) returns its Deny result only
    /// after `close` has returned, so transports wait here before they tear down.
    pub(crate) async fn calls_finished(&self, limit: std::time::Duration) {
        let end = tokio::time::Instant::now() + limit;
        while self.0.slots.available_permits() < SLOTS && tokio::time::Instant::now() < end {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }
    pub(crate) fn session(&self) -> Arc<Session> {
        self.0.session.clone()
    }
    pub(crate) fn screenshot(&self, id: &str) -> Option<gcu_host_protocol::messages::Image> {
        self.0.results.images.get(id)
    }
    pub(crate) fn files(&self) -> Arc<SessionFiles> {
        self.0.results.files.clone()
    }
    pub(crate) fn new(
        driver: Arc<dyn Driver>,
        host: Host,
        config: ServerConfig,
        files: SessionFiles,
    ) -> Result<Self, ServeError> {
        Self::new_with_approvals(driver, host, config, files, true)
    }
    pub(crate) fn new_with_approvals(
        driver: Arc<dyn Driver>,
        host: Host,
        config: ServerConfig,
        files: SessionFiles,
        elicitation_enabled: bool,
    ) -> Result<Self, ServeError> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        static LOCKS: OnceLock<WindowLocks> = OnceLock::new();
        let policy = host.policy.load();
        let router = ActionRouter::new(
            driver.clone(),
            if elicitation_enabled {
                Arc::new(elicitation::Approvals(host.approvals))
            } else {
                host.approvals
            },
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
            images: Arc::new(crate::image_cache::ImageCache::default()),
            files: Arc::new(files),
            session: session.clone(),
            encoding: gcu_core::redact::EncodeOptions {
                max_edge: policy.max_image_edge,
                format: if policy.image_format == "jpeg" {
                    gcu_core::redact::ImageFormat::Jpeg
                } else {
                    gcu_core::redact::ImageFormat::Png
                },
                ..Default::default()
            },
        };
        let tools = catalog::tools(config.profile).map_err(|_| ServeError::Protocol)?;
        let lifecycle = crate::lifecycle::Lifecycle::new(session.clone());
        Ok(Self(Arc::new(Inner {
            driver,
            router,
            session,
            results,
            tools,
            config,
            default_tree_nodes: policy.max_tree_nodes.clamp(1, 5000),
            slots: Semaphore::new(SLOTS),
            delivery: tokio::sync::Mutex::new(()),
            calls: AtomicU64::new(1),
            lifecycle,
        })))
    }
    pub(crate) async fn call_result(
        &self,
        name: &str,
        arguments: Value,
    ) -> rmcp::model::CallToolResult {
        let _active = self.0.lifecycle.active();
        let _delivery = self.0.delivery.lock().await;
        let result = self.dispatch(name, arguments).await;
        match result {
            Ok(result) => result,
            Err(error) => self.0.results.failure(*error).await,
        }
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
        this.lifecycle
            .begin_turn()
            .await
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
                let mut arguments = arguments;
                if let Some(object) = arguments.as_object_mut() {
                    object
                        .entry("max_tree_nodes")
                        .or_insert_with(|| json!(this.default_tree_nodes));
                }
                let p: GetAppStateParams = params::parse(arguments)?;
                let state = this.router.observe(&this.session, p).await?;
                Ok(this.results.state(state).await)
            }
            "computer_batch" => {
                let params: ComputerBatchParams = params::parse(arguments)?;
                let report = this.router.run_batch(&this.session, &id, params).await;
                Ok(this.results.batch(report).await)
            }
            "capabilities" => {
                let _: CapabilitiesParams = params::parse(arguments)?;
                let caps = this
                    .session
                    .enqueue(async {
                        let mut capabilities = this.driver.capabilities().clone();
                        capabilities.user_activity_guard &=
                            this.session.activity().ms_since_physical_input().is_some();
                        capabilities
                    })
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
        self.0.lifecycle.close();
        self.0.results.images.clear();
        self.0.session.session_closed().await;
        let files = self.0.results.files.clone();
        blocking(move || files.close()).await?;
        Ok(())
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
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        Ok(elicitation::REQUEST_PEER
            .scope(context.peer, self.call_result(&request.name, arguments))
            .await
            .into())
    }
    async fn on_custom_notification(
        &self,
        notification: CustomNotification,
        _context: NotificationContext<RoleServer>,
    ) {
        if notification.method == "notifications/turn-ended" {
            self.0.lifecycle.turn_ended(None).await;
        }
    }
}

pub(crate) struct Cleanup {
    server: Option<Server>,
    idle: tokio::task::JoinHandle<()>,
}
impl Cleanup {
    pub(crate) fn new(server: Server) -> Self {
        let idle = tokio::spawn(server.lifecycle().idle());
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
            server.0.lifecycle.close();
            drop(tokio::spawn(async move {
                let _ = server.close().await;
            }));
        }
    }
}
