use super::*;
use crate::acp_client::AcpEvent;
use crate::computer_use::ComputerUseBroker;
use crate::journal_throttle::JournalWriteThrottle;
use crate::permission::{PermissionPolicy, SessionAllowCache};
use crate::session_fsm::SessionFsm;
use crate::store::SessionMeta;
use grok_computer_use_core::adapter::{
    AdapterActResult, Capabilities, ComputerUseAdapter, DispatchRequest, SurfaceKind, TargetInfo,
};
use grok_computer_use_core::broker::{BrokerOptions, StopState};
use grok_computer_use_core::protocol::Observation;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

const MCP_TEST_TARGET: &str = "mcp-test:window:1";

#[derive(Default)]
struct McpTestAdapter {
    aborts: AtomicU64,
    abort_blocked: AtomicBool,
    abort_failures: AtomicU64,
    dispatches: AtomicU64,
    releases: AtomicU64,
}

impl McpTestAdapter {
    fn fail_next_aborts(&self, count: u64) {
        self.abort_failures.store(count, Ordering::SeqCst);
    }

    fn set_abort_blocked(&self, blocked: bool) {
        self.abort_blocked.store(blocked, Ordering::SeqCst);
    }
}

impl ComputerUseAdapter for McpTestAdapter {
    fn backend_id(&self) -> &'static str {
        "mcp-test"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::for_surface(SurfaceKind::Desktop, self.backend_id())
    }

    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        Ok(vec![TargetInfo {
            target_id: MCP_TEST_TARGET.into(),
            title: "MCP reconciliation fixture".into(),
            app_name: "mcp-test".into(),
            kind: "window".into(),
            pid: Some(std::process::id()),
            backend: self.backend_id().into(),
            execution_mode: "exclusive".into(),
            replay_policy: "never".into(),
            lifecycle_stamp: 1,
            display_id: "mcp-test-display".into(),
            coordinate_space: "image-pixels".into(),
            scope_label: "MCP reconciliation test target".into(),
        }])
    }

    fn target_alive(&self, target_id: &str) -> bool {
        target_id == MCP_TEST_TARGET
    }

    fn observe(&self, _target_id: &str) -> Result<Observation, String> {
        Err("unused in MCP reconciler test".into())
    }

    fn act(&self, _request: &DispatchRequest) -> Result<AdapterActResult, String> {
        self.dispatches.fetch_add(1, Ordering::SeqCst);
        Err("unused in MCP reconciler test".into())
    }

    fn abort(&self, _run_id: &str, _generation: u64) -> Result<(), String> {
        self.aborts.fetch_add(1, Ordering::SeqCst);
        while self.abort_blocked.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        if self
            .abort_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err("injected surface cleanup failure".into());
        }
        Ok(())
    }

    fn is_idle(&self, _run_id: &str) -> bool {
        true
    }

    fn start_periodic_preview(&self, _target_id: &str) {}

    fn stop_periodic_preview(&self) {}

    fn periodic_preview_active(&self) -> bool {
        false
    }

    fn release_target_for_run(&self, _run_id: &str, target_id: &str) {
        if target_id == MCP_TEST_TARGET {
            self.releases.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[derive(Debug)]
enum StubReply {
    Ok,
    Error(&'static str),
}

struct StubRequest {
    method: String,
    params: Value,
    reply: oneshot::Sender<StubReply>,
}

struct TcpStub {
    client: Arc<AcpClient>,
    requests: mpsc::UnboundedReceiver<StubRequest>,
    server: tokio::task::JoinHandle<()>,
    events: tokio::task::JoinHandle<()>,
}

impl TcpStub {
    async fn connect() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ACP stub");
        let addr = listener.local_addr().expect("ACP stub address");
        let (request_tx, requests) = mpsc::unbounded_channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept ACP client");
            let (read, mut write) = stream.into_split();
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            loop {
                line.clear();
                let Ok(read) = reader.read_line(&mut line).await else {
                    break;
                };
                if read == 0 {
                    break;
                }
                let message: Value = serde_json::from_str(line.trim()).expect("ACP JSON-RPC");
                let id = message
                    .get("id")
                    .and_then(Value::as_u64)
                    .expect("request id");
                let method = message
                    .get("method")
                    .and_then(Value::as_str)
                    .expect("request method")
                    .to_string();
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                let (reply, receive_reply) = oneshot::channel();
                if request_tx
                    .send(StubRequest {
                        method,
                        params,
                        reply,
                    })
                    .is_err()
                {
                    break;
                }
                let response = match receive_reply.await {
                    Ok(StubReply::Ok) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "applied": true }
                    }),
                    Ok(StubReply::Error(message)) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32000, "message": message }
                    }),
                    Err(_) => break,
                };
                let mut encoded = serde_json::to_vec(&response).expect("encode response");
                encoded.push(b'\n');
                if write.write_all(&encoded).await.is_err() {
                    break;
                }
            }
        });
        let (client, mut event_rx) = AcpClient::connect_tcp(
            &addr.to_string(),
            std::env::temp_dir().join("grok-cu-acp-stub"),
        )
        .await
        .expect("connect ACP stub");
        let events = tokio::spawn(async move {
            while let Some((_session, event)) = event_rx.recv().await {
                if matches!(event, AcpEvent::ProcessExited { .. }) {
                    break;
                }
            }
        });
        Self {
            client,
            requests,
            server,
            events,
        }
    }

    async fn next(&mut self) -> StubRequest {
        tokio::time::timeout(Duration::from_secs(2), self.requests.recv())
            .await
            .expect("ACP request timeout")
            .expect("ACP request channel closed")
    }

    async fn assert_no_request(&mut self) {
        match tokio::time::timeout(Duration::from_millis(120), self.requests.recv()).await {
            Err(_) | Ok(None) => {}
            Ok(Some(_)) => panic!("unexpected ACP catalog request"),
        }
    }

    async fn shutdown(self) {
        self.client.kill().await;
        let _ = tokio::time::timeout(Duration::from_secs(2), self.server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), self.events).await;
    }
}

fn base_catalog(version: u64) -> Value {
    json!([{
        "name": format!("base-{version}"),
        "command": "base-command",
        "args": [],
        "env": []
    }])
}

fn deps(base_version: Arc<AtomicU64>, timeout: Duration) -> McpReconcileDeps {
    McpReconcileDeps {
        timeout,
        build_base: Arc::new(move |_| base_catalog(base_version.load(Ordering::SeqCst))),
        build_entry: Arc::new(|session, run| {
            Ok(json!({
                "name": crate::computer_use::MCP_SERVER_NAME,
                "command": "private-runtime",
                "args": ["computer-use-mcp"],
                "env": [
                    { "name": "GROK_APP_CU_SESSION", "value": session },
                    { "name": "GROK_APP_CU_RUN", "value": run }
                ]
            }))
        }),
    }
}

fn ready_live(
    app_session_id: &str,
    agent_session_id: &str,
    process_id: &str,
    acp: Arc<AcpClient>,
) -> LiveSession {
    let mut fsm = SessionFsm::new();
    let _ = fsm.start_connect();
    let _ = fsm.handshake_ok();
    let now = std::time::Instant::now();
    LiveSession {
        app_session_id: app_session_id.into(),
        process_id: process_id.into(),
        meta: SessionMeta {
            id: app_session_id.into(),
            project_id: None,
            title: "MCP TCP test".into(),
            agent_session_id: Some(agent_session_id.into()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            model_id: None,
            archived: false,
            pinned: false,
            effort: None,
            mode: None,
            permission_policy: None,
            json_schema: None,
            scheduled: false,
            worktree_path: None,
            worktree_branch: None,
            is_worktree_session: false,
            plugin_dirs: Vec::new(),
            extra_rules: None,
            max_agent_turns: None,
            system_prompt_override: None,
            fork_agent_session: false,
            fork_rewind_prompt_index: None,
            no_ask_user: None,
            workspace_id: None,
            workspace_root_snapshot: None,
            workspace_capability: None,
            provider_id: None,
        },
        fsm,
        backend: "tcp-acp-stub".into(),
        acp: Some(acp),
        mock_stream: None,
        streaming_message_id: None,
        active_turn_id: None,
        stream_message_id_locked: false,
        stream_buf: String::new(),
        stream_thought: String::new(),
        stream_last_was_assistant: false,
        stream_attachments: Vec::new(),
        model_id: None,
        effort: None,
        product_mode: None,
        project_path: None,
        allow_cache: SessionAllowCache::default(),
        policy: PermissionPolicy::default(),
        provider_retry_attempt: 0,
        provider_retry_aborted: false,
        needs_history_bootstrap: false,
        pending_plan_rpc_id: None,
        pending_permission_rpc_id: None,
        pending_permission_options: None,
        pending_permission_tool_name: None,
        pending_permission_ui: None,
        pending_ask_user_rpc_id: None,
        pending_ask_user_ui: None,
        last_activity: now,
        last_stream_progress: now,
        last_stall_emit: None,
        stall_soft_emits: 0,
        journal_throttle: JournalWriteThrottle::with_default_interval(),
        open_tool_ids: HashSet::new(),
        open_tool_seen_at: HashMap::new(),
        terminal_tool_ids: HashSet::new(),
        deferred_prompt_complete: None,
        tools_this_turn: 0,
        saw_model_output: false,
        prompt_in_flight: false,
        sent_prompt_this_visit: false,
        pending_stream_emit: None,
        stream_emit_flush_gen: 0,
        last_tool_heartbeat_emit: None,
    }
}

fn manager_with_live(session: &str, agent: &str, client: Arc<AcpClient>) -> Arc<SessionManager> {
    let manager = Arc::new(SessionManager::new());
    *manager.inner.lock() = Some(ready_live(session, agent, "process-shared", client));
    manager
}

fn desired_present(
    session: &str,
    attempt: &str,
) -> (ComputerUseBroker, sessions::AuthorizationTicket) {
    let adapter = Arc::new(McpTestAdapter::default());
    let broker = ComputerUseBroker::new(
        adapter,
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir()
                .join(format!("cu-mcp-reconcile-{}.lease", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    );
    let ticket = sessions::begin(&broker, session, None, attempt, 1).expect("begin attempt");
    broker
        .authorize_target(&ticket.run_id, MCP_TEST_TARGET)
        .expect("authorize fixture");
    sessions::activate(&broker, &ticket).expect("request MCP attach");
    (broker, ticket)
}

fn catalogs(request: &StubRequest) -> &[Value] {
    assert_eq!(request.method, "_x.ai/session/update_mcp_servers");
    request
        .params
        .get("mcpServers")
        .and_then(Value::as_array)
        .expect("MCP catalog array")
}

fn assert_catalog(request: &StubRequest, base: &str, cu_count: usize) {
    let rows = catalogs(request);
    assert!(rows.iter().any(|row| row.get("name") == Some(&json!(base))));
    assert_eq!(
        rows.iter()
            .filter(|row| {
                row.get("name").and_then(Value::as_str)
                    == Some(crate::computer_use::MCP_SERVER_NAME)
            })
            .count(),
        cu_count
    );
}

fn noop_stop_plan() -> FencedComputerUseStop {
    FencedComputerUseStop {
        desired: sessions::McpDesiredState {
            generation: 0,
            run_id: None,
            attempt_generation: None,
        },
        cleanup: None,
        fence_error: None,
    }
}

fn activate_test_run(
    broker: &ComputerUseBroker,
    session: &str,
    attempt: &str,
) -> sessions::AuthorizationTicket {
    let ticket = sessions::begin(broker, session, None, attempt, 1).expect("begin test run");
    broker
        .authorize_target(&ticket.run_id, MCP_TEST_TARGET)
        .expect("authorize test target");
    sessions::activate(broker, &ticket).expect("activate test run");
    sessions::complete(broker, &ticket).expect("complete test run");
    ticket
}

async fn call_ipc(url: &str, token: &str, name: &str, arguments: Value) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("{url}/cu/tool"))
        .bearer_auth(token)
        .json(&json!({"name": name, "arguments": arguments}))
        .send()
        .await
        .expect("loopback request");
    let status = response.status().as_u16();
    let body = response.json().await.unwrap_or(Value::Null);
    (status, body)
}

fn detach_handshake_acp(manager: &SessionManager, session: &str) -> FencedAcpTermination {
    manager
        .with_session_mut(session, FencedAcpTermination::capture)
        .flatten()
        .expect("handshake ACP is captured exactly once")
}

mod catalog;
mod lifecycle;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn production_ipc_revokes_stop_and_process_exit_bearers_before_cleanup() {
    let suffix = uuid::Uuid::new_v4().to_string();
    let adapter = Arc::new(McpTestAdapter::default());
    let broker = Arc::new(ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("cu-ipc-exit-{suffix}.lease")),
            ..BrokerOptions::default()
        },
    ));
    let manager = Arc::new(SessionManager::new());
    crate::computer_use::ipc::register_session_manager(&manager);
    let endpoint = crate::computer_use::ipc::ensure(Arc::clone(&broker))
        .expect("production Computer Use IPC endpoint");

    // Model Stop must close the credential before its deliberately blocked
    // adapter cleanup completes. A repeated Host Stop shares the same
    // generation-bound cleanup ticket and cannot dispatch it twice.
    let model_session = format!("model-stop-{suffix}");
    let model_ticket = activate_test_run(&broker, &model_session, "model-stop-attempt");
    let model_token = endpoint
        .issue_session(&model_session, &model_ticket.run_id)
        .expect("issue model bearer");
    assert_eq!(
        call_ipc(&endpoint.url, &model_token, "computer_status", json!({}))
            .await
            .0,
        200
    );

    adapter.set_abort_blocked(true);
    let (stop_status, stop_body) =
        call_ipc(&endpoint.url, &model_token, "computer_stop", json!({})).await;
    assert_eq!(stop_status, 200, "{stop_body}");
    assert_eq!(
        broker.stop_state(&model_ticket.run_id).unwrap(),
        StopState::StopRequested
    );
    assert!(sessions::mcp_desired(&model_session).run_id.is_none());
    assert_eq!(
        call_ipc(&endpoint.url, &model_token, "computer_status", json!({}))
            .await
            .0,
        401,
        "the stopped bearer must be invalid before Stop returns"
    );
    let stopped_generation = sessions::mcp_desired(&model_session).generation;
    let repeated = manager.fence_computer_use_stop_with_broker(Arc::clone(&broker), &model_session);
    assert_eq!(repeated.desired.generation, stopped_generation);
    manager.spawn_fenced_computer_use_stop(model_session.clone(), repeated, None);

    adapter.set_abort_blocked(false);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let status = manager.mcp_catalog_status(&model_session);
            if adapter.aborts.load(Ordering::SeqCst) == 1
                && adapter.releases.load(Ordering::SeqCst) == 1
                && status.as_ref().is_some_and(|status| {
                    !status.pending
                        && !status.desired_present
                        && !status.resource_cleanup_pending
                        && status.applied_generation == Some(status.desired_generation)
                })
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("model Stop cleanup converged");
    assert_eq!(
        sessions::mcp_desired(&model_session).generation,
        stopped_generation
    );
    sessions::forget_session(&model_session);

    // ProcessExited first fences every App session on the exact ACP
    // incarnation. The ACP transport is still alive in this fixture only
    // so the test can prove that no catalog request is needed on this path.
    let live_id = format!("exit-live-{suffix}");
    let background_id = format!("exit-background-{suffix}");
    let parked_id = format!("exit-parked-{suffix}");
    let live_ticket = activate_test_run(&broker, &live_id, "exit-live-attempt");
    let background_ticket = activate_test_run(&broker, &background_id, "exit-background-attempt");
    let parked_ticket = activate_test_run(&broker, &parked_id, "exit-parked-attempt");
    let process_tokens = [
        (
            live_id.clone(),
            live_ticket.run_id.clone(),
            endpoint
                .issue_session(&live_id, &live_ticket.run_id)
                .expect("issue live bearer"),
        ),
        (
            background_id.clone(),
            background_ticket.run_id.clone(),
            endpoint
                .issue_session(&background_id, &background_ticket.run_id)
                .expect("issue background bearer"),
        ),
        (
            parked_id.clone(),
            parked_ticket.run_id.clone(),
            endpoint
                .issue_session(&parked_id, &parked_ticket.run_id)
                .expect("issue parked bearer"),
        ),
    ];
    for (_, _, token) in &process_tokens {
        assert_eq!(
            call_ipc(&endpoint.url, token, "computer_status", json!({}))
                .await
                .0,
            200
        );
    }

    let stub = TcpStub::connect().await;
    *manager.inner.lock() = Some(ready_live(
        &live_id,
        "agent-exit-live",
        "process-exit-ipc",
        Arc::clone(&stub.client),
    ));
    manager.background.lock().insert(
        background_id.clone(),
        ready_live(
            &background_id,
            "agent-exit-background",
            "process-exit-ipc",
            Arc::clone(&stub.client),
        ),
    );
    let parked_live = ready_live(
        &parked_id,
        "agent-exit-parked",
        "process-exit-ipc",
        Arc::clone(&stub.client),
    );
    manager.parked.lock().insert(
        parked_id.clone(),
        ParkedAgent {
            process_id: parked_live.process_id,
            app_session_id: parked_live.app_session_id,
            meta: parked_live.meta,
            acp: parked_live.acp.expect("parked ACP"),
            last_activity: parked_live.last_activity,
            model_id: parked_live.model_id,
            effort: parked_live.effort,
            product_mode: parked_live.product_mode,
            project_path: parked_live.project_path,
            policy: parked_live.policy,
            sandbox_profile: None,
            needs_history_bootstrap: parked_live.needs_history_bootstrap,
            backend: parked_live.backend,
        },
    );

    let plans = manager
        .fence_computer_use_for_process_exit_with("process-exit-ipc", Some(Arc::clone(&broker)));
    assert_eq!(plans.len(), 3, "all shared-process tenants must be fenced");
    assert!(manager
        .fence_computer_use_for_process_exit_with("process-exit-ipc", Some(Arc::clone(&broker)),)
        .is_empty());

    for (_, _, token) in &process_tokens {
        assert_eq!(
            call_ipc(&endpoint.url, token, "computer_status", json!({}))
                .await
                .0,
            401,
            "ProcessExited must revoke each old bearer before cleanup"
        );
        assert_eq!(
            call_ipc(&endpoint.url, token, "computer_act", json!({}))
                .await
                .0,
            401,
            "an old action credential must fail at the HTTP boundary"
        );
    }
    assert_eq!(
        adapter.dispatches.load(Ordering::SeqCst),
        0,
        "no post-exit action reached the adapter"
    );

    let baseline_aborts = adapter.aborts.load(Ordering::SeqCst);
    let baseline_releases = adapter.releases.load(Ordering::SeqCst);
    adapter.fail_next_aborts(1);
    let mut failed_cleanup = Vec::new();
    for (session, plan) in plans {
        if manager
            .finish_process_exit_cleanup(&session, plan)
            .await
            .is_err()
        {
            failed_cleanup.push(session);
        }
    }
    assert_eq!(failed_cleanup.len(), 1, "one injected cleanup must fail");
    for session in &failed_cleanup {
        let status = manager
            .mcp_catalog_status(session)
            .expect("failed cleanup status");
        assert!(!status.pending && !status.desired_present);
        assert!(status.resource_cleanup_pending);
        manager
            .retry_computer_use_cleanup(session)
            .await
            .expect("same-generation process-exit cleanup retry");
    }
    for (session, _, _) in &process_tokens {
        let status = manager
            .mcp_catalog_status(session)
            .expect("settled process-exit status");
        assert!(!status.pending);
        assert!(!status.desired_present);
        assert!(!status.resource_cleanup_pending);
        sessions::forget_session(session);
    }
    assert_eq!(
        adapter.aborts.load(Ordering::SeqCst) - baseline_aborts,
        4,
        "three cleanups plus one retry"
    );
    assert_eq!(
        adapter.releases.load(Ordering::SeqCst) - baseline_releases,
        3,
        "each process-owned target is released once"
    );
    *manager.inner.lock() = None;
    manager.background.lock().clear();
    manager.parked.lock().clear();
    stub.shutdown().await;
}
