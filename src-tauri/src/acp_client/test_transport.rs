//! Shared in-memory ACP fixture. No child, credentials, provider or network IO.
use super::*;
use crate::session_migration::mcp_stdio::SessionMcpPolicy;

pub(crate) fn client_for(
    custom_route: bool,
    policy: SessionMcpPolicy,
    recent_agent_session: Option<&str>,
) -> (Arc<AcpClient>, tokio::io::DuplexStream) {
    let (client, peer, _) = client_with_events(custom_route, policy, recent_agent_session);
    (client, peer)
}

pub(crate) type EventReceiver = mpsc::UnboundedReceiver<(Option<String>, AcpEvent)>;

pub(crate) async fn request_with_timeout(
    client: &AcpClient,
    method: &str,
    timeout_secs: u64,
) -> Result<Value, String> {
    assert_eq!(client.cli_path, PathBuf::from("synthetic-acp"));
    assert!(client.child_pid.is_none());
    client
        .request_timeout(method, json!({}), timeout_secs)
        .await
}

#[derive(Clone, Copy)]
pub(crate) enum WriterPauseAt {
    Bytes(usize),
    Flush,
}

pub(crate) struct WriterPause {
    pub(crate) written: std::sync::atomic::AtomicUsize,
    entered: AtomicBool,
    reached: Notify,
    released: AtomicBool,
    waker: ParkingMutex<Option<std::task::Waker>>,
    at: WriterPauseAt,
}

impl WriterPause {
    pub(crate) async fn wait(&self) {
        while !self.entered.load(Ordering::SeqCst) {
            self.reached.notified().await;
        }
    }

    pub(crate) fn release(&self) {
        self.released.store(true, Ordering::SeqCst);
        if let Some(waker) = self.waker.lock().take() {
            waker.wake();
        }
    }

    fn hold(&self, context: &std::task::Context<'_>) -> bool {
        if self.released.load(Ordering::SeqCst) {
            return false;
        }
        *self.waker.lock() = Some(context.waker().clone());
        self.entered.store(true, Ordering::SeqCst);
        self.reached.notify_one();
        !self.released.load(Ordering::SeqCst)
    }
}

pub(crate) async fn install_writer_pause(
    client: &AcpClient,
    at: WriterPauseAt,
) -> Arc<WriterPause> {
    assert_eq!(client.cli_path, PathBuf::from("synthetic-acp"));
    assert!(client.child_pid.is_none());
    let mut writer = client.stdin.lock().await;
    let inner = writer.take().expect("synthetic writer missing");
    let control = Arc::new(WriterPause {
        written: std::sync::atomic::AtomicUsize::new(0),
        entered: AtomicBool::new(false),
        reached: Notify::new(),
        released: AtomicBool::new(false),
        waker: ParkingMutex::new(None),
        at,
    });
    *writer = Some(Box::new(PausedWriter {
        inner,
        control: control.clone(),
    }));
    control
}

struct PausedWriter {
    inner: Box<dyn AsyncWrite + Unpin + Send>,
    control: Arc<WriterPause>,
}

impl AsyncWrite for PausedWriter {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let writer = self.get_mut();
        let written = writer.control.written.load(Ordering::SeqCst);
        let limit = match writer.control.at {
            WriterPauseAt::Bytes(limit) if written >= limit && writer.control.hold(context) => {
                return std::task::Poll::Pending;
            }
            WriterPauseAt::Bytes(limit) if written < limit => bytes.len().min(limit - written),
            _ => bytes.len(),
        };
        match std::pin::Pin::new(&mut writer.inner).poll_write(context, &bytes[..limit]) {
            std::task::Poll::Ready(Ok(count)) => {
                writer.control.written.fetch_add(count, Ordering::SeqCst);
                std::task::Poll::Ready(Ok(count))
            }
            other => other,
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let writer = self.get_mut();
        if matches!(writer.control.at, WriterPauseAt::Flush) && writer.control.hold(context) {
            return std::task::Poll::Pending;
        }
        std::pin::Pin::new(&mut writer.inner).poll_flush(context)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_shutdown(context)
    }
}

/// Inject transport faults around the real duplex writer, not an answer stub.
pub(crate) async fn install_writer_fault(
    client: &AcpClient,
    fail_after: Option<usize>,
    fail_flush: bool,
    max_chunk: usize,
) -> Arc<std::sync::atomic::AtomicUsize> {
    assert_eq!(client.cli_path, PathBuf::from("synthetic-acp"));
    assert!(client.child_pid.is_none());
    assert!(max_chunk > 0);
    let mut writer = client.stdin.lock().await;
    let inner = writer.take().expect("synthetic writer missing");
    let written = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    *writer = Some(Box::new(FaultWriter {
        inner,
        written: written.clone(),
        fail_after,
        fail_flush,
        max_chunk,
    }));
    written
}

struct FaultWriter {
    inner: Box<dyn AsyncWrite + Unpin + Send>,
    written: Arc<std::sync::atomic::AtomicUsize>,
    fail_after: Option<usize>,
    fail_flush: bool,
    max_chunk: usize,
}

impl AsyncWrite for FaultWriter {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let writer = self.get_mut();
        let count = writer.written.load(Ordering::SeqCst);
        let remaining = writer
            .fail_after
            .map(|limit| limit.saturating_sub(count))
            .unwrap_or(usize::MAX);
        if remaining == 0 {
            return std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "synthetic permission writer failed",
            )));
        }
        let limit = bytes.len().min(remaining).min(writer.max_chunk);
        match std::pin::Pin::new(&mut writer.inner).poll_write(context, &bytes[..limit]) {
            std::task::Poll::Ready(Ok(count)) => {
                writer.written.fetch_add(count, Ordering::SeqCst);
                std::task::Poll::Ready(Ok(count))
            }
            other => other,
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let writer = self.get_mut();
        if writer.fail_flush {
            return std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "synthetic permission flush failed",
            )));
        }
        std::pin::Pin::new(&mut writer.inner).poll_flush(context)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_shutdown(context)
    }
}

pub(crate) fn client_with_events(
    custom_route: bool,
    policy: SessionMcpPolicy,
    recent_agent_session: Option<&str>,
) -> (Arc<AcpClient>, tokio::io::DuplexStream, EventReceiver) {
    // History protocol fixtures need no configured provider; model-control
    // fixtures below opt into the actual production capture explicitly.
    let binding = if custom_route {
        crate::providers::ProcessModelBinding::Generic {
            provider_id: "synthetic-history-route".into(),
            request_model: "synthetic-history-model".into(),
        }
    } else {
        crate::providers::ProcessModelBinding::catalog()
    };
    client_with_binding(custom_route, policy, recent_agent_session, binding, None)
}

pub(crate) fn client_for_current_route(
    custom_route: bool,
    policy: SessionMcpPolicy,
    recent_agent_session: Option<&str>,
) -> (Arc<AcpClient>, tokio::io::DuplexStream) {
    let (binding, configuration) = if policy.tcp {
        (crate::providers::ProcessModelBinding::catalog(), None)
    } else {
        let settings = crate::store::load_settings();
        let home =
            crate::paths::resolve_inference_grok_home(&settings.session_data_mode, custom_route);
        let (binding, _, configuration) = crate::providers::ProcessModelBinding::capture_for_spawn(
            custom_route,
            policy.ssh,
            policy.official_aux,
            "grok-4.6",
            &home,
            &settings,
        )
        .unwrap();
        (binding, configuration)
    };
    let (client, peer, _) = client_with_binding(
        custom_route,
        policy,
        recent_agent_session,
        binding,
        configuration,
    );
    (client, peer)
}

pub(crate) async fn open_with_empty_mcp(
    client: &AcpClient,
    resume: Option<&str>,
    fork: bool,
) -> Result<(String, bool), AgentError> {
    let cwd = crate::paths::general_workspace_dir();
    client
        .open_session_with_mcp_servers(resume, fork, &cwd.to_string_lossy(), json!([]))
        .await
}

pub(crate) fn client_with_binding(
    custom_route: bool,
    policy: SessionMcpPolicy,
    recent_agent_session: Option<&str>,
    model_binding: crate::providers::ProcessModelBinding,
    process_config: Option<crate::providers::ProcessConfigBinding>,
) -> (Arc<AcpClient>, tokio::io::DuplexStream, EventReceiver) {
    client_with_profile_binding(
        custom_route,
        policy,
        recent_agent_session,
        model_binding,
        process_config,
        None,
    )
}

pub(crate) fn client_from_main_profile(
    profile: crate::providers::PreparedMainProfile,
    cwd: PathBuf,
) -> (Arc<AcpClient>, tokio::io::DuplexStream, EventReceiver) {
    client_with_profile_binding(
        profile.custom,
        SessionMcpPolicy::default(),
        None,
        crate::providers::ProcessModelBinding::catalog(),
        None,
        Some((profile, cwd)),
    )
}

fn client_with_profile_binding(
    custom_route: bool,
    policy: SessionMcpPolicy,
    recent_agent_session: Option<&str>,
    model_binding: crate::providers::ProcessModelBinding,
    process_config: Option<crate::providers::ProcessConfigBinding>,
    profile: Option<(crate::providers::PreparedMainProfile, PathBuf)>,
) -> (Arc<AcpClient>, tokio::io::DuplexStream, EventReceiver) {
    let (model_binding, process_config, main_migration, main_runtime, cwd) =
        if let Some((profile, cwd)) = profile {
            (
                profile.model_binding,
                profile.configuration,
                profile.migration.map(super::main_migration::Binding::new),
                Some(profile.runtime),
                cwd,
            )
        } else {
            (
                model_binding,
                process_config,
                None,
                None,
                crate::paths::general_workspace_dir(),
            )
        };
    let (host, peer) = tokio::io::duplex(64 * 1024);
    let (reader, writer) = tokio::io::split(host);
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let client = Arc::new_cyclic(|client| AcpClient {
        self_weak: client.clone(),
        child: AsyncMutex::new(None),
        child_pid: None,
        owns_local_process_tree: false,
        stdin: AsyncMutex::new(Some(Box::new(writer))),
        next_id: AtomicU64::new(1),
        pending: ParkingMutex::new(HashMap::new()),
        event_tx,
        agent_session_id: ParkingMutex::new(recent_agent_session.map(str::to_owned)),
        observed_agent_models: ParkingMutex::new(HashMap::new()),
        cli_path: PathBuf::from("synthetic-acp"),
        cwd,
        stopped: AtomicBool::new(false),
        reader_alive: AtomicBool::new(true),
        reader_stop: Arc::new(Notify::new()),
        stderr_tail: ParkingMutex::new(Vec::new()),
        last_update_by_session: ParkingMutex::new(HashMap::new()),
        last_update_unstamped: ParkingMutex::new(None),
        empty_mcp_servers: policy.official_aux,
        app_session_id: None,
        sandbox_profile: ParkingMutex::new(None),
        custom_route,
        route_provider_id: if custom_route {
            "synthetic-custom".into()
        } else {
            crate::providers::SESSION_PROVIDER_OFFICIAL.into()
        },
        model_binding,
        process_config,
        main_migration,
        main_runtime,
        rewind_supported: ParkingMutex::new(None),
        ssh_alias: policy.ssh.then(|| "synthetic-ssh".into()),
        wsl: policy.wsl,
        tcp: policy.tcp,
        proc_home: std::sync::Mutex::new(None),
        import_history_grant: ParkingMutex::new(Default::default()),
    });
    client.start_read_loop(Box::new(reader));
    (client, peer, event_rx)
}
