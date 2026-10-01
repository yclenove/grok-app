//! Probe-only relay: apply a real worker resume, then hold or lose its reply.
//! Auth stays in memory; no production fault route or credential logging.
use axum::{
    body::{to_bytes, Body, Bytes},
    extract::{Request, State},
    http::{header, Method, StatusCode},
    response::{IntoResponse, Response},
    Router,
};
use std::{
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{oneshot, watch};

struct RelayState {
    upstream: String,
    authorization: String,
    client: reqwest::Client,
    lose_reply: bool,
    committed: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    release: watch::Sender<bool>,
}

async fn forward(State(state): State<Arc<RelayState>>, request: Request) -> Response {
    let path = request.uri().path().to_string();
    if request.method() != Method::POST
        || request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            != Some(state.authorization.as_str())
        || !matches!(
            path.as_str(),
            "/health"
                | "/open"
                | "/tabs"
                | "/observe"
                | "/pause-run"
                | "/resume-run"
                | "/run-status"
                | "/cancel-run"
        )
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let host_marker = request.headers().get("x-grok-cu-host").cloned();
    let Ok(body) = to_bytes(request.into_body(), 64 * 1024).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    if path == "/resume-run" {
        state.calls.fetch_add(1, Ordering::SeqCst);
    }
    let mut outgoing = state
        .client
        .post(format!("{}{path}", state.upstream))
        .header(header::AUTHORIZATION, &state.authorization)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body);
    if let Some(marker) = host_marker {
        outgoing = outgoing.header("x-grok-cu-host", marker);
    }
    let Ok(response) = outgoing.send().await else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    let status = response.status();
    let Ok(bytes) = response.bytes().await else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    if bytes.len() > 2 * 1024 * 1024 {
        return StatusCode::BAD_GATEWAY.into_response();
    }
    if path == "/resume-run" && status.is_success() {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return StatusCode::BAD_GATEWAY.into_response();
        };
        if value["runRevision"] != 2 || value["phase"] != "running" {
            return StatusCode::BAD_GATEWAY.into_response();
        }
        state.committed.store(true, Ordering::SeqCst);
        let _ = state.release.subscribe().wait_for(|ready| *ready).await;
        if state.lose_reply {
            // Drop the HTTP body mid-stream after the real transition. This
            // is an actual transport failure, not a fabricated WorkerError.
            return Response::builder().status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from_stream(async_stream::stream! {
                    yield Ok::<_, std::io::Error>(Bytes::from_static(b"{"));
                    yield Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "owned fixture reply lost"));
                })).expect("fixed relay response");
        }
    }
    (status, [(header::CONTENT_TYPE, "application/json")], bytes).into_response()
}

pub(super) struct Relay {
    pub base: String,
    pub committed: Arc<AtomicBool>,
    pub calls: Arc<AtomicUsize>,
    release: watch::Sender<bool>,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<Result<(), String>>>,
}

impl Relay {
    pub fn start(upstream: &str, token: &str, lose_reply: bool) -> Result<Self, String> {
        let upstream =
            grok_computer_use_core::browser::worker_http::require_loopback_base(upstream)
                .map_err(|e| e.to_string())?;
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let base = format!(
            "http://{}",
            listener.local_addr().map_err(|e| e.to_string())?
        );
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let committed = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let release = watch::channel(false).0;
        let state = Arc::new(RelayState {
            upstream,
            authorization: format!("Bearer {token}"),
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(|e| e.to_string())?,
            lose_reply,
            committed: committed.clone(),
            calls: calls.clone(),
            release: release.clone(),
        });
        let (stop, stopped) = oneshot::channel();
        let thread = std::thread::spawn(move || {
            runtime.block_on(async move {
                let listener =
                    tokio::net::TcpListener::from_std(listener).map_err(|e| e.to_string())?;
                axum::serve(listener, Router::new().fallback(forward).with_state(state))
                    .with_graceful_shutdown(async {
                        let _ = stopped.await;
                    })
                    .await
                    .map_err(|e| e.to_string())
            })
        });
        Ok(Self {
            base,
            committed,
            calls,
            release,
            stop: Some(stop),
            thread: Some(thread),
        })
    }
    pub fn release(&self) {
        self.release.send_replace(true);
    }
    pub fn close(&mut self) -> Result<(), String> {
        self.release();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| "resume relay panicked")??;
        }
        Ok(())
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
