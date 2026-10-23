//! The registered surface projects protocol types onto the existing core router.
use crate::{
    catalog, elicitation,
    files::{blocking, SessionFiles},
    instructions, params,
    result::{self, Results},
    script::ScriptRunner,
    Profile, ServeError,
};
use gcu_core::{
    action::ActionRouter,
    driver::Driver,
    scope::{InstanceEvidence, ScopeLease},
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
#[derive(Clone, Default)]
pub struct ServerConfig {
    /// Tools visible to the client. Every profile filters IMPLEMENTED_TOOLS.
    pub profile: Profile,
    /// App session that may see `browser_navigate`. None keeps the static catalog.
    pub browser_session: Option<String>,
    /// Same sentence doctor prints. Empty leaves instructions unchanged.
    pub route_note: String,
    /// Real script runner. `None` keeps `computer_script` unregistered.
    pub script: Option<Arc<dyn ScriptRunner>>,
}
impl std::fmt::Debug for ServerConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServerConfig")
            .field("profile", &self.profile)
            .field("browser_session", &self.browser_session.is_some())
            .field("route_note", &self.route_note)
            .field("script", &self.script.is_some())
            .finish()
    }
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
    /// MCP connection id. Browser notifications keep one peer per connection.
    connection: u64,
}
/// Maximum concurrently running tool calls per session.
const SLOTS: usize = 32;

fn shared_locks() -> WindowLocks {
    static LOCKS: OnceLock<WindowLocks> = OnceLock::new();
    LOCKS.get_or_init(WindowLocks::new).clone()
}
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
        let policy = host.policy.load();
        let activity = host.activity.clone();
        let router = ActionRouter::new(
            driver.clone(),
            if elicitation_enabled {
                Arc::new(elicitation::Approvals(host.approvals))
            } else {
                host.approvals
            },
            host.policy,
            host.events,
            shared_locks(),
        );
        Self::assemble(driver, router, activity, config, files, policy)
    }

    /// Trusted scoped session. The lease is not stored on the server, and no
    /// browser hub or scope-editing tool is installed.
    ///
    /// File setup is the caller's. This constructor does not block, wait, or
    /// take a timeout. Cancellation does not apply. Elicitation stays off, the
    /// same as [`crate::Connection::open_host_only`].
    pub(crate) fn new_scoped<D>(
        driver: D,
        lease: ScopeLease,
        host: Host,
        mut config: ServerConfig,
        files: SessionFiles,
    ) -> Result<Self, ServeError>
    where
        D: Driver + InstanceEvidence + 'static,
    {
        config.browser_session = None;
        let policy = host.policy.load();
        let activity = host.activity.clone();
        let router = ActionRouter::with_fixture_scope(
            driver,
            lease,
            activity.clone(),
            host.approvals,
            host.policy,
            host.events,
            shared_locks(),
        );
        let driver = router.bound_driver();
        Self::assemble(driver, router, activity, config, files, policy)
    }

    fn assemble(
        driver: Arc<dyn Driver>,
        router: ActionRouter,
        activity: Arc<dyn UserActivity>,
        config: ServerConfig,
        files: SessionFiles,
        policy: gcu_host_api::PolicyConfig,
    ) -> Result<Self, ServeError> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let connection = NEXT.fetch_add(1, Ordering::Relaxed);
        let session = Arc::new(Session::open(
            format!("mcp-{connection}"),
            activity,
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
            scope: router.fixture_lease(),
        };
        let script_ready = cfg!(feature = "script") && config.script.is_some();
        let tools =
            catalog::tools(config.profile, script_ready).map_err(|_| ServeError::Protocol)?;
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
            connection,
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
        if name == crate::browser_tool::NAME {
            let stopped =
                this.session.activity().latched_stop() || this.session.activity().stop_requested();
            return crate::browser_tool::call(&this.config, stopped, arguments).await;
        }
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
            "computer_script" => {
                let params: ComputerScriptParams = params::parse(arguments)?;
                params
                    .validate()
                    .map_err(|err| Box::new(params::invalid(err.0)))?;
                let Some(runner) = this.config.script.clone() else {
                    return Err(Box::new(result::error(ErrorCode::CapabilityUnavailable)));
                };
                let body = runner.run(&this.router, &this.session, &id, params).await;
                Ok(this.results.script(&body.output, body.error).await)
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
        McpConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .build(),
        )
        .with_server_info(Implementation::new(
            "grok-computer-use",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(instructions::instructions(
            self.0.driver.platform(),
            self.0.config.profile,
            &self.0.tools,
            &self.0.config.route_note,
        ))
    }
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut tools = self.0.tools.clone();
        crate::browser_tool::augment(&self.0.config, &mut tools);
        crate::browser_tool::observe(&self.0.config, self.0.connection, _context.peer);
        Ok(ListToolsResult::with_all_items(tools))
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
        crate::browser_tool::observe(&self.0.config, self.0.connection, context.peer.clone());
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
