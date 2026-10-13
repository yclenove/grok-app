//! Real WebView2 download through the production TaskGrant, Broker and adapter.

use super::*;
use crate::computer_use::webview_host::HostOwnedWebView;
use sha2::{Digest, Sha256};
use std::io::{self, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::thread::{self, JoinHandle};

mod http;
pub(super) mod lifecycle;

const DOWNLOAD_BYTES: &[u8] = b"grok-app-webview-download-fixture-v1\n";
const OVERSIZE_BYTES: u64 = grok_computer_use_core::staging::MAX_DOWNLOAD_BYTES + 1;

struct FixtureServer {
    address: SocketAddr,
    stats: Arc<FixtureStats>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct FixtureStats {
    accepts: AtomicU64,
    read_failures: AtomicU64,
    last_read_failure: parking_lot::Mutex<Option<io::ErrorKind>>,
    page_requests: AtomicU64,
    write_failures: AtomicU64,
    last_write_failure: parking_lot::Mutex<Option<io::ErrorKind>>,
    downloads: AtomicU64,
    oversize_requests: AtomicU64,
    oversize_bytes_written: AtomicU64,
    oversize_write_failures: AtomicU64,
}

impl FixtureServer {
    fn spawn() -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(FixtureStats::default());
        let stop_thread = stop.clone();
        let stats_thread = stats.clone();
        let join = thread::spawn(move || {
            let mut workers = Vec::new();
            while !stop_thread.load(AtomicOrdering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stats_thread.accepts.fetch_add(1, AtomicOrdering::SeqCst);
                        let request_stats = stats_thread.clone();
                        workers.push(thread::spawn(move || serve(stream, &request_stats)));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
                reap_finished(&mut workers);
            }
            for worker in workers {
                let _ = worker.join();
            }
        });
        Ok(Self {
            address,
            stats,
            stop,
            join: Some(join),
        })
    }

    fn page_url(&self) -> String {
        format!("http://{}/page", self.address)
    }

    fn download_count(&self) -> u64 {
        self.stats.downloads.load(AtomicOrdering::SeqCst)
    }

    fn connection_stats(&self) -> String {
        format!(
            "accepts={} pages={} read_failures={} read_kind={:?} write_failures={} write_kind={:?}",
            self.stats.accepts.load(AtomicOrdering::SeqCst),
            self.stats.page_requests.load(AtomicOrdering::SeqCst),
            self.stats.read_failures.load(AtomicOrdering::SeqCst),
            *self.stats.last_read_failure.lock(),
            self.stats.write_failures.load(AtomicOrdering::SeqCst),
            *self.stats.last_write_failure.lock(),
        )
    }

    fn oversize_stats(&self) -> (u64, u64, u64) {
        (
            self.stats.oversize_requests.load(AtomicOrdering::SeqCst),
            self.stats
                .oversize_bytes_written
                .load(AtomicOrdering::SeqCst),
            self.stats
                .oversize_write_failures
                .load(AtomicOrdering::SeqCst),
        )
    }

    fn stop_and_join(&mut self) -> Result<(), String> {
        self.stop.store(true, AtomicOrdering::SeqCst);
        if let Some(join) = self.join.take() {
            join.join()
                .map_err(|_| "WebView download fixture server panicked".to_string())?;
        }
        Ok(())
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

fn reap_finished(workers: &mut Vec<JoinHandle<()>>) {
    let mut active = Vec::with_capacity(workers.len());
    for worker in workers.drain(..) {
        if worker.is_finished() {
            let _ = worker.join();
        } else {
            active.push(worker);
        }
    }
    *workers = active;
}

fn serve(mut stream: TcpStream, stats: &FixtureStats) {
    // Winsock accepted sockets inherit the listener's nonblocking mode. This
    // worker uses read/write timeouts and write_all, so restore blocking IO
    // explicitly before receiving headers or streaming the attachment body.
    if let Err(error) = stream.set_nonblocking(false) {
        stats.read_failures.fetch_add(1, AtomicOrdering::SeqCst);
        *stats.last_read_failure.lock() = Some(error.kind());
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));
    let path = match http::read_request_path(&mut stream) {
        Err(error) => {
            stats.read_failures.fetch_add(1, AtomicOrdering::SeqCst);
            *stats.last_read_failure.lock() = Some(error.kind());
            return;
        }
        Ok(path) => path,
    };
    let result = match path.as_str() {
        "/page" => {
            stats.page_requests.fetch_add(1, AtomicOrdering::SeqCst);
            let body = concat!(
                "<!doctype html><html lang=\"zh-CN\"><meta charset=\"utf-8\">",
                "<title>WebView download</title><body>",
                "<a id=\"download\" href=\"/fixture.bin\" download=\"fixture.bin\">",
                "下载测试文件</a>",
                "<a id=\"oversize\" href=\"/oversize.bin\" download=\"oversize.bin\">",
                "下载超限文件</a></body></html>"
            );
            write_response(
                &mut stream,
                "text/html; charset=utf-8",
                &[],
                body.as_bytes(),
            )
        }
        "/fixture.bin" => {
            stats.downloads.fetch_add(1, AtomicOrdering::SeqCst);
            write_response(
                &mut stream,
                "application/octet-stream",
                &["Content-Disposition: attachment; filename=fixture.bin"],
                DOWNLOAD_BYTES,
            )
        }
        "/oversize.bin" => {
            stats.downloads.fetch_add(1, AtomicOrdering::SeqCst);
            stats.oversize_requests.fetch_add(1, AtomicOrdering::SeqCst);
            let (written, failed) = write_repeated_response(
                &mut stream,
                "application/octet-stream",
                &["Content-Disposition: attachment; filename=oversize.bin"],
                OVERSIZE_BYTES,
            );
            stats
                .oversize_bytes_written
                .fetch_add(written, AtomicOrdering::SeqCst);
            if let Some(kind) = failed {
                stats
                    .oversize_write_failures
                    .fetch_add(1, AtomicOrdering::SeqCst);
                Err(io::Error::new(kind, "fixture response failed"))
            } else {
                Ok(())
            }
        }
        _ => write_status(&mut stream, "404 Not Found"),
    };
    if let Err(error) = result {
        stats.write_failures.fetch_add(1, AtomicOrdering::SeqCst);
        *stats.last_write_failure.lock() = Some(error.kind());
    }
}

fn write_response(
    stream: &mut TcpStream,
    content_type: &str,
    headers: &[&str],
    body: &[u8],
) -> io::Result<()> {
    let extra = if headers.is_empty() {
        String::new()
    } else {
        format!("{}\r\n", headers.join("\r\n"))
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\n{extra}Cache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.write_all(body)
}

fn write_repeated_response(
    stream: &mut TcpStream,
    content_type: &str,
    headers: &[&str],
    bytes: u64,
) -> (u64, Option<io::ErrorKind>) {
    let extra = if headers.is_empty() {
        String::new()
    } else {
        format!("{}\r\n", headers.join("\r\n"))
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\n{extra}Cache-Control: no-store\r\nContent-Length: {bytes}\r\nConnection: close\r\n\r\n"
    );
    if let Err(error) = stream.write_all(response.as_bytes()) {
        return (0, Some(error.kind()));
    }
    let chunk = [b'x'; 16 * 1024];
    let mut remaining = bytes;
    while remaining > 0 {
        let len = remaining.min(chunk.len() as u64) as usize;
        if let Err(error) = stream.write_all(&chunk[..len]) {
            return (bytes - remaining, Some(error.kind()));
        }
        remaining -= len as u64;
    }
    (bytes, None)
}

fn write_status(stream: &mut TcpStream, status: &str) -> io::Result<()> {
    stream.write_all(
        format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes(),
    )
}

pub(super) struct DownloadWorld {
    pub(super) broker: Arc<crate::computer_use::ComputerUseBroker>,
    pub(super) adapter: Arc<WebViewAdapter>,
    owner: String,
    staging_root: PathBuf,
}

#[derive(Clone)]
pub(super) struct DownloadGrant {
    pub(super) token: String,
    pub(super) run_id: String,
    pub(super) session_id: String,
}

impl DownloadWorld {
    pub(super) fn new(live: Arc<dyn LiveWebView>, staging_root: PathBuf) -> Result<Self, String> {
        let adapter = Arc::new(WebViewAdapter::new());
        adapter.install_scope_fixture(vec![live]);
        let broker = Arc::new(crate::computer_use::ComputerUseBroker::request_endpoint(
            crate::computer_use::BrokerOptions {
                feature_enabled: false,
                lease_path: staging_root.join("broker.lease"),
                ..Default::default()
            },
        ));
        broker.tabs().set_staging_root(staging_root.clone());
        broker
            .register_surface_adapter(crate::computer_use::SurfaceKind::WebView, adapter.clone())
            .map_err(|error| error.to_string())?;
        Ok(Self {
            broker,
            adapter,
            owner: format!("webview-download-probe-{}", uuid::Uuid::new_v4()),
            staging_root,
        })
    }

    pub(super) fn grant(&self) -> Result<DownloadGrant, String> {
        let connection = self
            .broker
            .reserve_transport_connection(&self.owner)
            .map_err(|error| error.to_string())?;
        let token = connection.token().to_string();
        self.broker
            .bind_transport_agent(&token, "webview-download-probe-agent")
            .map_err(|error| error.to_string())?;
        self.broker
            .submit_access(
                &token,
                serde_json::json!({
                    "action": "request",
                    "purpose": "real WebView2 download acceptance",
                    "scopeIntent": "the selected local fixture page only",
                    "surfaces": ["app-webview"],
                    "capabilities": ["observe", "input"]
                }),
            )
            .map_err(|error| error.to_string())?;
        let key = self
            .broker
            .host_task_view(&token)
            .map_err(|error| error.to_string())?
            .request
            .ok_or("WebView download access request was not published")?
            .key;
        let picker = self
            .broker
            .begin_task_scope_picker(&token, &key, crate::computer_use::SurfaceKind::WebView)
            .map_err(|error| error.to_string())?
            .discover()
            .map_err(|error| error.to_string())?;
        let candidates = picker.views().map_err(|error| error.to_string())?;
        if candidates.len() != 1 {
            return Err(format!(
                "WebView download fixture expected one candidate, got {}",
                candidates.len()
            ));
        }
        let selection = picker
            .select(&[candidates[0].candidate_id.clone()])
            .map_err(|error| error.to_string())?;
        let grok_computer_use_core::broker::AccessApprovalReady::Prepare(mut approval) = self
            .broker
            .reserve_selected_access_approval(&token, selection)
            .map_err(|error| error.to_string())?
        else {
            return Err("fresh WebView download grant skipped preparation".into());
        };
        approval
            .publish_feature()
            .map_err(|error| error.to_string())?;
        approval
            .prepare_execution()
            .map_err(|error| error.to_string())?;
        approval
            .prepare_publication()
            .map_err(|error| error.to_string())?;
        let outcome = approval
            .commit_for_transport(&token)
            .map_err(|error| error.to_string())?;
        Ok(DownloadGrant {
            token,
            run_id: outcome.run_id,
            session_id: self.owner.clone(),
        })
    }

    fn call(
        &self,
        grant: &DownloadGrant,
        tool: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.broker
            .preflight_tool_permission(&grant.token, tool, &args)
            .map_err(|error| error.to_string())?;
        Ok(grok_computer_use_core::tools::dispatch_with_transport(
            &self.broker,
            &grant.token,
            tool,
            args,
            None,
        ))
    }
}

impl Drop for DownloadWorld {
    fn drop(&mut self) {
        self.broker.fence_task_consent(&self.owner);
        for ticket in self.broker.take_cleanup_tickets() {
            let _ = self.broker.finish_stop_cleanup(&ticket);
        }
    }
}

struct ObservationCursor {
    target_id: String,
    target_generation: u64,
    snapshot_id: String,
    element_ref: String,
}

fn observation_diagnostics(
    live: &HostOwnedWebView,
    body: &serde_json::Value,
    expected_target_id: &str,
    expected_name: &str,
) -> String {
    let nodes = body.get("nodes").and_then(serde_json::Value::as_array);
    let node_summary = nodes
        .into_iter()
        .flatten()
        .take(8)
        .map(|node| {
            let role = node
                .get("role")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("missing");
            let name = match node.get("name").and_then(serde_json::Value::as_str) {
                Some(value) if value == expected_name => "expected".to_string(),
                Some(value) => format!("other-length-{}", value.chars().count()),
                None => "missing".to_string(),
            };
            let actions = node
                .get("actions")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .take(4)
                .collect::<Vec<_>>()
                .join(",");
            format!("role={role},name={name},actions=[{actions}]")
        })
        .collect::<Vec<_>>()
        .join(" | ");
    let observed_target = body.get("targetId").and_then(serde_json::Value::as_str);
    let observed_generation = body
        .get("targetGeneration")
        .and_then(serde_json::Value::as_u64);
    let snapshot_length = body
        .get("snapshotId")
        .and_then(serde_json::Value::as_str)
        .map(str::len);
    format!(
        "nodes={} summary=[{}] target_match={} observed_generation={:?} native_generation={} snapshot_length={:?} navigation={:?} downloads={:?}",
        nodes.map_or(0, Vec::len),
        node_summary,
        observed_target == Some(expected_target_id),
        observed_generation,
        live.fixture_generation(),
        snapshot_length,
        live.navigation_counts(),
        live.download_counts(),
    )
}

fn text_payload(result: &serde_json::Value, label: &str) -> Result<serde_json::Value, String> {
    if result.get("isError").and_then(serde_json::Value::as_bool) != Some(false) {
        return Err(format!("{label} failed: {result}"));
    }
    let text = result
        .get("content")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|part| part.get("type").and_then(serde_json::Value::as_str) == Some("text"))
        .and_then(|part| part.get("text"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("{label} text payload missing"))?;
    serde_json::from_str(text).map_err(|error| format!("{label} payload invalid: {error}"))
}

fn observe(
    stage: &str,
    live: &Arc<HostOwnedWebView>,
    world: &DownloadWorld,
    grant: &DownloadGrant,
    target_id: &str,
    accessible_name: &str,
) -> Result<ObservationCursor, String> {
    let result = world.call(
        grant,
        "computer_observe",
        serde_json::json!({"runId": grant.run_id, "targetId": target_id, "screenshot": false}),
    )?;
    let body = text_payload(&result, "WebView download observation")?;
    cursor_from_observation(&body, accessible_name).map_err(|error| {
        format!(
            "{error}; stage={stage}; {}",
            observation_diagnostics(live, &body, target_id, accessible_name)
        )
    })
}

fn cursor_from_observation(
    body: &serde_json::Value,
    accessible_name: &str,
) -> Result<ObservationCursor, String> {
    let node = body
        .get("nodes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|node| {
            node.get("name").and_then(serde_json::Value::as_str) == Some(accessible_name)
                && node
                    .get("actions")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|actions| {
                        actions
                            .iter()
                            .any(|action| action.as_str() == Some("download"))
                    })
        })
        .ok_or_else(|| {
            format!("WebView observation did not expose the typed download link {accessible_name}")
        })?;
    Ok(ObservationCursor {
        target_id: body
            .get("targetId")
            .and_then(serde_json::Value::as_str)
            .ok_or("WebView observation targetId missing")?
            .to_string(),
        target_generation: body
            .get("targetGeneration")
            .and_then(serde_json::Value::as_u64)
            .ok_or("WebView observation targetGeneration missing")?,
        snapshot_id: body
            .get("snapshotId")
            .and_then(serde_json::Value::as_str)
            .ok_or("WebView observation snapshotId missing")?
            .to_string(),
        element_ref: node
            .get("ref")
            .and_then(serde_json::Value::as_str)
            .ok_or("WebView observation element ref missing")?
            .to_string(),
    })
}

fn download_args(
    grant: &DownloadGrant,
    cursor: &ObservationCursor,
    action_id: &str,
    filename: &str,
) -> serde_json::Value {
    serde_json::json!({
        "runId": grant.run_id,
        "targetId": cursor.target_id,
        "targetGeneration": cursor.target_generation,
        "actionId": action_id,
        "snapshotId": cursor.snapshot_id,
        "elementRef": cursor.element_ref,
        "filename": filename
    })
}

fn completed_path(result: &serde_json::Value) -> Result<(PathBuf, u64), String> {
    let body = text_payload(result, "WebView download")?;
    let path = body
        .get("path")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("WebView download result path missing")?;
    let bytes = body
        .get("bytes")
        .and_then(serde_json::Value::as_u64)
        .ok_or("WebView download byte count missing")?;
    Ok((PathBuf::from(path), bytes))
}

fn require_tool_error(result: &serde_json::Value, label: &str) -> Result<(), String> {
    if result.get("isError").and_then(serde_json::Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(format!("{label} unexpectedly succeeded: {result}"))
    }
}

fn tool_error_kind(result: &serde_json::Value) -> &'static str {
    let text = result
        .get("content")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    if text.contains("did not reach a native terminal state") {
        "native-terminal-timeout"
    } else if text.contains("staged file size is not allowed") {
        "size-rejected"
    } else if text.contains("cancelled or interrupted") {
        "interrupted"
    } else if text.contains("outcome unknown") {
        "unknown"
    } else {
        "other"
    }
}

fn require_preflight_identity_error(
    result: Result<serde_json::Value, String>,
    field: &str,
) -> Result<(), String> {
    match result {
        Err(error) if error == format!("identity mismatch: {field}") => Ok(()),
        Err(error) => Err(format!(
            "wrong {field} WebView download returned the wrong preflight error: {error}"
        )),
        Ok(result) => Err(format!(
            "wrong {field} WebView download reached tool dispatch: {result}"
        )),
    }
}

fn staged_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let mut pending = vec![root.join("computer-use-staging")];
    while let Some(directory) = pending.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.to_string()),
        };
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let file_type = entry.file_type().map_err(|error| error.to_string())?;
            if file_type.is_symlink() {
                return Err("WebView download staging unexpectedly contains a symlink".into());
            }
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}

struct PublishedFiles {
    staging_root: PathBuf,
    paths: Vec<PathBuf>,
}

impl PublishedFiles {
    fn new(staging_root: PathBuf) -> Self {
        Self {
            staging_root,
            paths: Vec::new(),
        }
    }

    fn retain(&mut self, path: PathBuf) {
        self.paths.push(path);
    }
}

impl Drop for PublishedFiles {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = std::fs::remove_file(path);
        }
        for path in &self.paths {
            if let Some(directory) = path.parent() {
                let _ = std::fs::remove_dir(directory);
            }
        }
        if let Some(run) = self
            .paths
            .first()
            .and_then(|path| path.parent())
            .and_then(Path::parent)
            .and_then(Path::parent)
        {
            let _ = std::fs::remove_dir(run.join("downloads"));
            let _ = std::fs::remove_dir(run);
        }
        let _ = std::fs::remove_dir(self.staging_root.join("computer-use-staging"));
    }
}

struct ProbeRoot(PathBuf);

impl Drop for ProbeRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0.join("broker.lease"));
        let _ = std::fs::remove_dir(&self.0);
    }
}

fn verify_file(path: &Path, bytes: u64) -> Result<(), String> {
    let actual = std::fs::read(path).map_err(|error| error.to_string())?;
    if bytes != DOWNLOAD_BYTES.len() as u64 || actual != DOWNLOAD_BYTES {
        return Err(format!(
            "WebView staged bytes mismatch: reported={bytes} actual={}",
            actual.len()
        ));
    }
    let actual_hash = Sha256::digest(&actual);
    let expected_hash = Sha256::digest(DOWNLOAD_BYTES);
    if actual_hash != expected_hash {
        return Err("WebView staged download hash mismatch".into());
    }
    Ok(())
}

fn check(live: &Arc<HostOwnedWebView>, server: &FixtureServer) -> Result<(), String> {
    live.fixture_ready()?;
    let staging_root = std::env::temp_dir().join(format!(
        "grok-cu-webview-download-probe-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&staging_root).map_err(|error| error.to_string())?;
    let _root = ProbeRoot(staging_root.clone());
    let world = DownloadWorld::new(live.clone(), staging_root)?;
    let grant = world.grant()?;
    let (target_id, _) = world
        .broker
        .authorized_target(&grant.run_id)
        .map_err(|error| error.to_string())?;
    let mut files = PublishedFiles::new(world.staging_root.clone());

    let first_cursor = observe("initial", live, &world, &grant, &target_id, "下载测试文件")?;

    let mut wrong_generation = download_args(
        &grant,
        &first_cursor,
        "wrong-generation",
        "wrong-generation.bin",
    );
    wrong_generation["targetGeneration"] = serde_json::json!(first_cursor.target_generation + 1);
    require_preflight_identity_error(
        world.call(&grant, "computer_download", wrong_generation),
        "targetGeneration",
    )?;
    let mut wrong_snapshot = download_args(
        &grant,
        &first_cursor,
        "wrong-snapshot",
        "wrong-snapshot.bin",
    );
    wrong_snapshot["snapshotId"] = serde_json::json!("forged-snapshot");
    require_preflight_identity_error(
        world.call(&grant, "computer_download", wrong_snapshot),
        "snapshotId",
    )?;
    let mut wrong_ref = download_args(&grant, &first_cursor, "wrong-ref", "wrong-ref.bin");
    wrong_ref["elementRef"] = serde_json::json!("forged-ref");
    require_preflight_identity_error(
        world.call(&grant, "computer_download", wrong_ref),
        "elementRef",
    )?;
    if server.download_count() != 0
        || live.download_counts() != (0, 0, 0)
        || !staged_files(&world.staging_root)?.is_empty()
    {
        return Err(format!(
            "invalid WebView download identity reached a side effect: network={} native={:?}",
            server.download_count(),
            live.download_counts()
        ));
    }

    let first_args = download_args(&grant, &first_cursor, "native-download-1", "first.bin");
    let first = world.call(&grant, "computer_download", first_args.clone())?;
    let (first_path, first_bytes) = completed_path(&first).map_err(|error| {
        format!(
            "{error}; stage=first network={} native={:?} server={:?}",
            server.download_count(),
            live.download_counts(),
            server.connection_stats(),
        )
    })?;
    files.retain(first_path.clone());
    verify_file(&first_path, first_bytes)?;
    if server.download_count() != 1 || live.download_counts() != (1, 1, 1) {
        return Err(format!(
            "first WebView download was not exactly-once: network={} native={:?}",
            server.download_count(),
            live.download_counts()
        ));
    }

    let replay = world.call(&grant, "computer_download", first_args)?;
    if replay != first || server.download_count() != 1 || live.download_counts() != (1, 1, 1) {
        return Err("successful WebView download replay dispatched a second request".into());
    }

    let second_cursor = observe(
        "after-first",
        live,
        &world,
        &grant,
        &target_id,
        "下载测试文件",
    )?;
    let second = world.call(
        &grant,
        "computer_download",
        download_args(&grant, &second_cursor, "native-download-2", "second.bin"),
    )?;
    let (second_path, second_bytes) = completed_path(&second).map_err(|error| {
        format!(
            "{error}; stage=second network={} native={:?} server={:?}",
            server.download_count(),
            live.download_counts(),
            server.connection_stats(),
        )
    })?;
    files.retain(second_path.clone());
    verify_file(&second_path, second_bytes)?;
    if second_path == first_path
        || server.download_count() != 2
        || live.download_counts() != (2, 2, 2)
    {
        return Err(format!(
            "second WebView download did not use a fresh completed expectation: network={} native={:?}",
            server.download_count(),
            live.download_counts()
        ));
    }

    let retained = staged_files(&world.staging_root)?;
    let oversize_cursor = observe("oversize", live, &world, &grant, &target_id, "下载超限文件")?;
    let oversize_args = download_args(
        &grant,
        &oversize_cursor,
        "native-download-oversize",
        "oversize.bin",
    );
    let oversize_started = Instant::now();
    let oversize = world.call(&grant, "computer_download", oversize_args.clone())?;
    let oversize_elapsed = oversize_started.elapsed();
    let oversize_error_kind = tool_error_kind(&oversize);
    require_tool_error(&oversize, "oversize WebView download")?;
    let oversize_stats = server.oversize_stats();
    if server.download_count() != 3
        || live.download_counts() != (3, 3, 3)
        || oversize_stats != (1, OVERSIZE_BYTES, 0)
        || staged_files(&world.staging_root)? != retained
    {
        return Err(format!(
            "oversize WebView download was published or not exactly-once: network={} native={:?} connections={:?} server={oversize_stats:?} elapsed_ms={} error_kind={oversize_error_kind}",
            server.download_count(),
            live.download_counts(),
            server.connection_stats(),
            oversize_elapsed.as_millis(),
        ));
    }
    let oversize_replay = world.call(&grant, "computer_download", oversize_args)?;
    require_tool_error(&oversize_replay, "oversize WebView download replay")?;
    if server.download_count() != 3
        || live.download_counts() != (3, 3, 3)
        || staged_files(&world.staging_root)? != retained
    {
        return Err("rejected WebView download replay dispatched or published again".into());
    }

    println!(
        "gate: webview_download bytes={} network=3 claimed=3 finished=3 invalid=0 replay=0 second=1 oversize_rejected=1",
        DOWNLOAD_BYTES.len()
    );
    Ok(())
}

pub(crate) fn run() -> Result<(), String> {
    let mut server = FixtureServer::spawn()?;
    // An explicit fixture-only profile enables a controlled comparison with
    // the ordinary shared environment. Retain it as evidence; BrowserExit must
    // be observed before the test owner considers it eligible for cleanup.
    let profile = std::env::var_os("GROK_CU_DOWNLOAD_PROFILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let spawn = if let Some(profile) = &profile {
        HostOwnedWebView::spawn_disposable(
            "resource-browser-download",
            "download",
            &server.page_url(),
            profile,
        )
    } else {
        HostOwnedWebView::spawn("resource-browser-download", "download", &server.page_url())
    };
    let live = spawn.map_err(|error| format!("{error}; server={}", server.connection_stats()))?;
    let result = check(&live, &server);
    let result = live.finish_probe(result);
    let server_result = server.stop_and_join();
    result.and(server_result)
}
