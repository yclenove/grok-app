//! Fail-first regressions: shipped stop/revoke must not create, block on or
//! drop the request-owned HTTP runtime inside a Tokio async worker.

use super::gates::enabled_opts;
use super::*;
use crate::browser::worker_http::bounded_loopback_post;
use crate::browser::{
    ManagedBrowserWorker, ManagedDownload, ManagedPage, ManagedProfile, ManagedWorkerAction,
    WorkerError,
};
use crate::fake::FakeAdapter;
use crate::session_grants::SessionGrants;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

#[path = "tests_http_cancellation.rs"]
mod http_cancellation;

struct LoopbackHttpWorker {
    base: String,
    token: String,
    staging: std::path::PathBuf,
}

impl ManagedBrowserWorker for LoopbackHttpWorker {
    fn bind_request(
        self: Arc<Self>,
        _identity: crate::browser::ManagedRequestIdentity,
    ) -> Result<Arc<dyn ManagedBrowserWorker>, WorkerError> {
        // Deliberately non-cooperative fixture: Stop must still wait for its
        // independent cancel endpoint and the original physical request.
        Ok(self)
    }
    fn goto(
        &self,
        _owner: &str,
        _profile: &str,
        _page_id: &str,
        _page_generation: u64,
        _action_id: &str,
        url: &str,
    ) -> Result<ManagedPage, WorkerError> {
        Ok(ManagedPage {
            page_id: "page-1".into(),
            page_generation: 1,
            url: url.into(),
            popup: false,
        })
    }

    fn download(
        &self,
        _owner: &str,
        _profile: &str,
        _page_id: &str,
        _page_generation: u64,
        _snapshot_id: &str,
        _action_id: &str,
        _filename: &str,
        _element_ref: Option<&str>,
    ) -> Result<ManagedDownload, WorkerError> {
        Err(WorkerError::transport("download unused in stop tests"))
    }

    fn open_profile(&self, _owner: &str, profile: &str) -> Result<ManagedProfile, WorkerError> {
        let dir = self.staging.join("profiles").join(profile);
        std::fs::create_dir_all(&dir).map_err(|e| WorkerError::transport(e.to_string()))?;
        Ok(ManagedProfile {
            dir,
            page: ManagedPage {
                page_id: "page-1".into(),
                page_generation: 1,
                url: "about:blank".into(),
                popup: false,
            },
        })
    }

    fn act_page(&self, _request: ManagedWorkerAction<'_>) -> Result<ManagedPage, WorkerError> {
        Err(WorkerError::transport("act unused in stop tests"))
    }

    fn list_pages(&self, _owner: &str, _profile: &str) -> Result<Vec<ManagedPage>, WorkerError> {
        // The fixture owns one static page; only observe/cancel use its gated
        // HTTP server. Reauthorization still runs the production page refresh.
        Ok(vec![ManagedPage {
            page_id: "page-1".into(),
            page_generation: 1,
            url: "about:blank".into(),
            popup: false,
        }])
    }

    fn capture_page(
        &self,
        owner: &str,
        profile: &str,
        page_id: &str,
        page_generation: u64,
        options: crate::adapter::CaptureOptions,
    ) -> Result<crate::browser::ManagedObservation, WorkerError> {
        let value = crate::browser::bounded_loopback_post_cancellable(
            &self.base,
            &self.token,
            "/observe",
            &json!({"owner": owner, "profile": profile, "pageId": page_id, "pageGeneration": page_generation}),
            &[],
            Duration::from_secs(4),
            &options.cancellation,
        )?;
        crate::browser::parse_managed_observation(&value)
    }

    fn cancel_run(&self, owner: &str) -> Result<(), WorkerError> {
        bounded_loopback_post(
            &self.base,
            &self.token,
            "/cancel-run",
            &json!({ "owner": owner }),
            &[("x-grok-cu-host", "1")],
            Duration::from_secs(2),
        )?;
        Ok(())
    }
}

fn spawn_http(
    status_line: &str,
    headers: &str,
    body: &[u8],
) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().expect("addr");
    let status_line = status_line.to_string();
    let headers = headers.to_string();
    let body = body.to_vec();
    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let response = format!(
                "{status_line}\r\n{headers}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    (format!("http://127.0.0.1:{}", addr.port()), handle)
}

fn ok_cancel_body() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "ok": true,
        "pageId": "page-1",
        "pageGeneration": 1,
        "snapshotId": "snap-1",
        "nodes": []
    }))
    .unwrap()
}

fn bind_loopback(broker: &ComputerUseBroker, base: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("cu-async-stop-{}", Uuid::new_v4()));
    let worker = Arc::new(LoopbackHttpWorker {
        base: base.to_string(),
        token: "test-token".into(),
        staging: root.clone(),
    });
    broker.tabs().set_profile_root(root.join("profiles"));
    broker.tabs().set_staging_root(root.clone());
    broker.tabs().set_worker(worker);
    root
}

#[tokio::test(flavor = "multi_thread")]
async fn bounded_loopback_post_from_tokio_worker_does_not_panic() {
    let (base, join) = spawn_http(
        "HTTP/1.1 200 OK",
        "Content-Type: application/json",
        &ok_cancel_body(),
    );
    let result = bounded_loopback_post(
        &base,
        "test-token",
        "/cancel-run",
        &json!({ "owner": "run-a" }),
        &[("x-grok-cu-host", "1")],
        Duration::from_secs(2),
    );
    let _ = join.join();
    result.expect("shipped loopback POST must complete on a Tokio worker");
}

#[tokio::test(flavor = "multi_thread")]
async fn request_stop_from_tokio_worker_does_not_panic() {
    let (base, join) = spawn_http(
        "HTTP/1.1 200 OK",
        "Content-Type: application/json",
        &ok_cancel_body(),
    );
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake, enabled_opts(400));
    broker.open_run("sess-a", "run-a").expect("open");
    let root = bind_loopback(&broker, &base);
    broker
        .tabs()
        .open_managed_profile("sess-a", "run-a", "p1")
        .expect("profile");
    let state = broker
        .request_stop("run-a")
        .expect("shipped request_stop must not panic on Tokio");
    assert!(
        state == StopState::StopRequested || state == StopState::Stopped,
        "{state:?}"
    );
    let again = broker.request_stop("run-a").expect("idempotent stop");
    assert!(
        again == StopState::StopRequested || again == StopState::Stopped,
        "{again:?}"
    );
    let _ = join.join();
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread")]
async fn session_revoke_from_tokio_worker_does_not_panic() {
    let (base, join) = spawn_http(
        "HTTP/1.1 200 OK",
        "Content-Type: application/json",
        &ok_cancel_body(),
    );
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake, enabled_opts(400));
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "chat", None).expect("begin");
    let root = bind_loopback(&broker, &base);
    broker
        .tabs()
        .open_managed_profile("chat", &ticket.run_id, "p1")
        .expect("profile");
    grants.revoke(&broker, "chat", || {});
    grants.revoke(&broker, "chat", || {});
    let _ = join.join();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn stop_fence_acknowledges_before_blocked_cleanup_and_dedupes_owner() {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_abort_blocked(true);
    let broker = Arc::new(ComputerUseBroker::new(fake.clone(), enabled_opts(400)));
    broker.open_run("sess-a", "run-a").expect("open");
    let target = fake.fixture_id();
    broker
        .authorize_target("run-a", &target)
        .expect("authorize");

    // The synchronous phase returns before abort() is even entered and the
    // fenced run rejects new model work immediately.
    let ticket = broker
        .fence_stop("run-a")
        .expect("fence")
        .expect("cleanup ticket");
    assert_eq!(
        broker.stop_state("run-a").unwrap(),
        StopState::StopRequested
    );
    assert!(matches!(
        broker.observe("run-a"),
        Err(BrokerError::StopRequested)
    ));
    assert_eq!(fake.abort_called(), 0);

    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let cleanup_broker = Arc::clone(&broker);
    let cleanup_ticket = ticket.clone();
    let cleanup = std::thread::spawn(move || {
        let result = cleanup_broker.finish_stop_cleanup(&cleanup_ticket);
        let _ = finished_tx.send(result);
    });
    assert!(fake.wait_for_abort_call(Duration::from_secs(2)));
    assert!(matches!(
        finished_rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));

    // A duplicate Stop while cleanup is owned returns promptly and neither
    // starts another abort nor releases the target early.
    let duplicate = broker
        .fence_stop("run-a")
        .expect("duplicate fence")
        .expect("same cleanup ticket");
    assert_eq!(duplicate.generation(), ticket.generation());
    assert_eq!(
        broker.finish_stop_cleanup(&duplicate).unwrap(),
        StopState::StopRequested
    );
    assert_eq!(fake.abort_called(), 1);
    assert_eq!(fake.release_called(), 0);

    fake.set_abort_blocked(false);
    let result = finished_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("cleanup completion")
        .expect("cleanup succeeds");
    cleanup.join().expect("cleanup thread");
    assert_eq!(result, StopState::Stopped);
    assert!(!broker.stop_cleanup_pending("run-a").unwrap());
    assert_eq!(fake.abort_called(), 1);
    assert_eq!(fake.release_called(), 1);
}

#[test]
fn feature_off_can_fence_all_runs_before_any_blocking_cleanup() {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_abort_blocked(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("sess-a", "run-a").expect("open A");
    broker.open_run("sess-b", "run-b").expect("open B");
    let target = fake.fixture_id();
    broker
        .authorize_target("run-a", &target)
        .expect("authorize A");
    broker
        .authorize_target("run-b", &target)
        .expect("authorize B");

    let tickets = broker.fence_feature_enabled(false);
    assert_eq!(tickets.len(), 2);
    assert!(!broker.feature_enabled());
    assert_eq!(fake.abort_called(), 0);
    for run in ["run-a", "run-b"] {
        assert_eq!(broker.stop_state(run).unwrap(), StopState::StopRequested);
        assert!(matches!(
            broker.observe(run),
            Err(BrokerError::FeatureDisabled)
        ));
    }

    fake.set_abort_blocked(false);
    for ticket in tickets {
        broker.finish_stop_cleanup(&ticket).unwrap();
    }
    assert_eq!(fake.abort_called(), 2);
    assert_eq!(fake.release_called(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn worker_timeout_is_unknown_without_replay() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let hang = std::thread::spawn(move || {
        let _hold = listener.accept();
        std::thread::sleep(Duration::from_secs(4));
    });
    let base = format!("http://127.0.0.1:{}", addr.port());
    let err = bounded_loopback_post(
        &base,
        "test-token",
        "/cancel-run",
        &json!({ "owner": "run-a" }),
        &[],
        Duration::from_millis(200),
    )
    .expect_err("timeout");
    assert_eq!(err.code, "worker_timeout", "{}", err.code);
    assert_eq!(err.completion, crate::browser::WorkerCompletion::Unknown);
    let _ = hang.join();
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_and_disconnect_are_diagnosable() {
    let (base, join) = spawn_http("HTTP/1.1 200 OK", "Content-Type: text/plain", b"not-json");
    let err = bounded_loopback_post(
        &base,
        "test-token",
        "/cancel-run",
        &json!({}),
        &[],
        Duration::from_secs(2),
    )
    .expect_err("malformed");
    assert_eq!(err.code, "invalid_worker_response", "{}", err.code);
    let _ = join.join();

    let err = bounded_loopback_post(
        "http://127.0.0.1:1",
        "test-token",
        "/cancel-run",
        &json!({}),
        &[],
        Duration::from_millis(400),
    )
    .expect_err("disconnect");
    assert!(
        err.code == "worker_transport" || err.code == "worker_timeout",
        "{}",
        err.code
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn request_stop_timeout_stays_stop_requested() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let hang = std::thread::spawn(move || {
        let _hold = listener.accept();
        std::thread::sleep(Duration::from_secs(4));
    });
    let base = format!("http://127.0.0.1:{}", addr.port());
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake, enabled_opts(400));
    broker.open_run("sess-a", "run-a").expect("open");
    let root = bind_loopback(&broker, &base);
    broker
        .tabs()
        .open_managed_profile("sess-a", "run-a", "p1")
        .expect("profile");
    let err = broker.request_stop("run-a").expect_err("timeout cleanup");
    assert!(err.to_string().contains("browser cancellation"), "{err}");
    assert_eq!(
        broker.stop_state("run-a").expect("state"),
        StopState::StopRequested
    );
    let _ = hang.join();
    let _ = std::fs::remove_dir_all(root);
}
