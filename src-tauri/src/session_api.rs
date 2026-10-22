//! Local session API for external integrations (#626 first slice).
//!
//! Loopback HTTP (127.0.0.1, token file) + CLI that talks to the same Host
//! helpers. Lists Grok App sessions and continues one by id + prompt.
//! Does not create a new chat. Does not interrupt a running turn.

mod idempotency;
#[cfg(test)]
mod idempotency_process_tests;
#[cfg(test)]
mod idempotency_recovery_tests;
#[cfg(test)]
mod idempotency_tests;
pub(crate) mod queue_cancel;
mod queue_dispatch;
#[cfg(test)]
mod queue_dispatch_tests;
mod queue_lease;
#[cfg(test)]
mod queue_lease_tests;
#[cfg(test)]
mod queue_outcome_tests;
mod queue_outcomes;
#[cfg(test)]
mod queue_process_tests;
pub(crate) mod queue_recovery;
#[cfg(test)]
mod queue_recovery_tests;
#[cfg(test)]
mod queue_regression_tests;
mod queue_result;
pub(crate) mod queue_snapshot;
mod queue_store;
#[cfg(test)]
mod queue_store_tests;
mod receipt_lease;
pub(crate) mod receipt_review;
#[cfg(test)]
mod receipt_review_tests;
mod receipt_store;

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::paths::{app_data_root, ensure_app_dirs};
use crate::session_manager::SessionManager;
use crate::store::{self, SessionMeta};

pub const SESSIONS_FLAG: &str = "--sessions";
pub const SESSION_SEND_FLAG: &str = "--session-send";
pub const PROMPT_FLAG: &str = "--prompt";
pub const PROMPT_FILE_FLAG: &str = "--prompt-file";
pub const IDEMPOTENCY_FLAG: &str = "--idempotency-key";
pub const INCLUDE_ARCHIVED_FLAG: &str = "--include-archived";

const ENDPOINT_FILE: &str = "session-api.json";
const QUEUE_FILE: &str = "session-api-queue.json";
const QUEUE_CAP: usize = 200;

/// Bound `dispatch_turn` so HTTP/CLI never wait the connect 90s + send 90s
/// worst case (~180s). Lock recovery is still knife-1's wall-clock abort.
pub const DISPATCH_TURN_TIMEOUT_SECS: u64 = 15;
/// CLI HTTP client must outlive the server dispatch budget so `--session-send`
/// can parse `unknown` instead of surfacing a transport timeout.
pub const CLI_HTTP_TIMEOUT_SECS: u64 = DISPATCH_TURN_TIMEOUT_SECS + 5;
const DISPATCH_UNKNOWN_MESSAGE: &str = "dispatch timed out; outcome unknown. Inspect the original session before any new request; do not automatically resend.";

fn drain_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    TurnStarted,
    /// Current turn is still running (e.g. drawing / tools). Prompt is in the
    /// same follow-up queue the composer shows — not discarded, not interrupted.
    Queued,
    /// Explicitly cancelled while still unclaimed; no dispatch was attempted.
    Cancelled,
    Busy,
    NotFound,
    AppNotRunning,
    /// Legacy retryable response retained for compatibility. Dispatch timeouts are Unknown.
    RetryLater,
    /// The original request may have started; never automatically dispatch it again.
    Unknown,
    /// A retained (session, key) is bound to a different normalized prompt.
    IdempotencyConflict,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionListItem {
    pub id: String,
    pub title: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub updated_at: String,
    pub archived: bool,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnRequest {
    pub prompt: String,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnResult {
    pub ok: bool,
    pub status: TurnStatus,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Set when `status == queued` so the GUI can dedup the same follow-up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_item_id: Option<String>,
    /// Prompt echoed on `queued` for authoritative outcome resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_prompt: Option<String>,
}

pub const SEND_QUEUE_EVENT: &str = "session://send_queue";
pub const SEND_QUEUE_TAKE_EVENT: &str = "session://send_queue_take";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SendQueuePush {
    pub session_id: String,
    pub item_id: String,
    pub prompt: String,
    pub source: String,
    pub created_at: i64,
}

impl TurnResult {
    fn fail(status: TurnStatus, session_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            status,
            session_id: session_id.into(),
            idempotency_key: None,
            message: Some(message.into()),
            queue_item_id: None,
            queued_prompt: None,
        }
    }

    fn started(session_id: impl Into<String>) -> Self {
        Self {
            ok: true,
            status: TurnStatus::TurnStarted,
            session_id: session_id.into(),
            idempotency_key: None,
            message: None,
            queue_item_id: None,
            queued_prompt: None,
        }
    }
}

pub fn emit_external_queue(app: &AppHandle, session_id: &str, prompt: &str, item_id: &str) {
    crate::mirror::fanout_event(
        app,
        SEND_QUEUE_EVENT,
        SendQueuePush {
            session_id: session_id.to_string(),
            item_id: item_id.to_string(),
            prompt: prompt.to_string(),
            source: "external".into(),
            created_at: chrono::Utc::now().timestamp_millis(),
        },
    );
}

pub async fn enqueue_while_busy(app: &AppHandle, session_id: &str, prompt: &str) -> TurnResult {
    let item_id = format!("q-ext-{}", uuid::Uuid::new_v4());
    let item = queue_store::Item::new(
        session_id,
        &item_id,
        prompt,
        chrono::Utc::now().timestamp_millis(),
    );
    let result = queue_dispatch::enqueue_at(&app_data_root(), item).await;
    if result.ok && main_webview_can_drain(app) {
        emit_external_queue(app, session_id, prompt, &item_id);
    }
    result
}

fn main_webview_can_drain(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

pub fn emit_external_queue_take(app: &AppHandle, session_id: &str, item_id: &str) {
    crate::mirror::fanout_event(
        app,
        SEND_QUEUE_TAKE_EVENT,
        serde_json::json!({
            "sessionId": session_id,
            "itemId": item_id,
        }),
    );
}

/// Host-side drain of persisted external prompts (webview is display-only).
pub async fn drain_ready_external_queues(app: &AppHandle, mgr: &Arc<SessionManager>) {
    let Ok(_drain) = drain_lock().try_lock() else {
        return;
    };
    let root = app_data_root();
    let items = match queue_dispatch::list_at(&root).await {
        Ok(items) => items,
        Err(error) => {
            tracing::warn!(%error, "external queue unavailable; preserved without dispatch");
            return;
        }
    };
    let mut seen = std::collections::HashSet::<String>::new();
    for item in items {
        // A pending/held head also blocks younger prompts for this session.
        if !seen.insert(item.session_id.clone())
            || item.delivery.is_some()
            || mgr.session_turn_busy(&item.session_id)
        {
            continue;
        }
        let result = queue_dispatch::run_at(&root, &item, || async {
            // The durable queue is a Host-origin source itself. Mark only the
            // actual drained turn; never arm the chat while work is waiting.
            match prepare_send(&item.session_id, &item.prompt, "external") {
                Ok(prepared) => dispatch_turn_or_timeout(app, mgr, prepared).await,
                Err(error) => *error,
            }
        })
        .await;
        if let Some(result) = result {
            if result.ok && matches!(result.status, TurnStatus::TurnStarted | TurnStatus::Queued) {
                emit_external_queue_take(app, &item.session_id, &item.item_id);
            } else {
                tracing::warn!(session = %item.session_id, status = ?result.status,
                    "external queue outcome unconfirmed; payload retained without automatic redispatch");
            }
        }
    }
}

/// Fire-and-forget so the stream-stall watchdog is not blocked on dispatch.
pub fn schedule_drain_ready_external_queues(app: AppHandle, mgr: Arc<SessionManager>) {
    tauri::async_runtime::spawn(async move {
        drain_ready_external_queues(&app, &mgr).await;
    });
}

async fn fanout_persisted_queue(app: &AppHandle) {
    if !main_webview_can_drain(app) {
        return;
    }
    let items = match queue_dispatch::list_at(&app_data_root()).await {
        Ok(items) => items,
        Err(error) => {
            tracing::warn!(%error, "external queue unavailable; no empty-success fallback");
            return;
        }
    };
    for item in items {
        emit_external_queue(app, &item.session_id, &item.prompt, &item.item_id);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointFile {
    pub url: String,
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionApiStatus {
    pub listening: bool,
    pub url: Option<String>,
    pub token_file: String,
    pub cli: SessionApiCliLink,
}

/// User-level `grok-app` command (`~/.local/bin`, no sudo, never edits the shell rc).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionApiCliLink {
    pub supported: bool,
    pub installed: bool,
    /// True when the file is our symlink (Unix) or marked shim (Windows).
    pub ours: bool,
    pub matches_running: bool,
    pub link_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,
    pub desired_target: String,
}

#[cfg_attr(not(windows), allow(dead_code))]
const WIN_SHIM_MARKER: &str = "grok-app session-api shim (managed by Grok App)";

pub fn cli_link_name() -> &'static str {
    #[cfg(windows)]
    {
        "grok-app.cmd"
    }
    #[cfg(not(windows))]
    {
        "grok-app"
    }
}

pub fn cli_link_path_in(home: &std::path::Path) -> PathBuf {
    home.join(".local").join("bin").join(cli_link_name())
}

pub fn cli_link_path() -> PathBuf {
    cli_link_path_in(&crate::process_util::user_home())
}

pub fn desired_cli_target() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    Ok(exe.canonicalize().unwrap_or(exe))
}

fn paths_match(a: &std::path::Path, b: &std::path::Path) -> bool {
    let ca = a.canonicalize().unwrap_or_else(|_| a.to_path_buf());
    let cb = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    ca == cb
}

#[cfg_attr(not(windows), allow(dead_code))]
fn is_windows_shim(contents: &str) -> bool {
    contents.contains(WIN_SHIM_MARKER)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn parse_windows_shim_target(contents: &str) -> Option<PathBuf> {
    if !is_windows_shim(contents) {
        return None;
    }
    contents.lines().find_map(|line| {
        let t = line.trim();
        if !t.starts_with('"') {
            return None;
        }
        let rest = &t[1..];
        let end = rest.find('"')?;
        let p = rest[..end].trim();
        if p.is_empty() {
            None
        } else {
            Some(PathBuf::from(p))
        }
    })
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn render_windows_shim(target: &std::path::Path) -> String {
    format!(
        "@echo off\r\nREM {WIN_SHIM_MARKER}\r\n\"{}\" %*\r\n",
        target.display()
    )
}

fn link_is_ours(path: &std::path::Path) -> bool {
    if !path.exists() && path.symlink_metadata().is_err() {
        return false;
    }
    #[cfg(unix)]
    {
        path.symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        fs::read_to_string(path)
            .ok()
            .is_some_and(|s| is_windows_shim(&s))
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

fn read_link_target(path: &std::path::Path) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        fs::read_link(path).ok()
    }
    #[cfg(windows)]
    {
        fs::read_to_string(path)
            .ok()
            .as_deref()
            .and_then(parse_windows_shim_target)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        None
    }
}

pub fn inspect_cli_link(link: &std::path::Path, desired: &std::path::Path) -> SessionApiCliLink {
    let meta = link.symlink_metadata().ok();
    let present = meta.is_some();
    let ours = present && link_is_ours(link);
    let target = if present {
        read_link_target(link)
    } else {
        None
    };
    let matches_running = ours && target.as_ref().is_some_and(|t| paths_match(t, desired));
    SessionApiCliLink {
        supported: cfg!(any(unix, windows)),
        installed: present,
        ours,
        matches_running,
        link_path: link.display().to_string(),
        target_path: target.map(|p| p.display().to_string()),
        desired_target: desired.display().to_string(),
    }
}

pub fn install_cli_link_at(
    link: &std::path::Path,
    desired: &std::path::Path,
) -> Result<(), String> {
    if !cfg!(any(unix, windows)) {
        return Err("installing a user command is not supported on this platform".into());
    }
    if !desired.exists() {
        return Err(format!("running binary not found: {}", desired.display()));
    }
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let present = link.symlink_metadata().is_ok();
    if present && !link_is_ours(link) {
        return Err(format!(
            "refusing to overwrite existing file: {}",
            link.display()
        ));
    }
    if present {
        fs::remove_file(link).map_err(|e| format!("replace {}: {e}", link.display()))?;
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(desired, link)
            .map_err(|e| format!("symlink {}: {e}", link.display()))?;
    }
    #[cfg(windows)]
    {
        fs::write(link, render_windows_shim(desired))
            .map_err(|e| format!("write {}: {e}", link.display()))?;
    }
    Ok(())
}

pub fn remove_cli_link_at(link: &std::path::Path) -> Result<(), String> {
    if link.symlink_metadata().is_err() {
        return Ok(());
    }
    if !link_is_ours(link) {
        return Err(format!(
            "refusing to remove a file we did not install: {}",
            link.display()
        ));
    }
    fs::remove_file(link).map_err(|e| format!("remove {}: {e}", link.display()))
}

fn cli_link_status() -> SessionApiCliLink {
    let desired = desired_cli_target().unwrap_or_default();
    inspect_cli_link(&cli_link_path(), &desired)
}

pub struct SessionApiHandle {
    pub url: String,
    shutdown: std::sync::Mutex<Option<oneshot::Sender<()>>>,
}

impl Drop for SessionApiHandle {
    fn drop(&mut self) {
        if let Some(tx) = self
            .shutdown
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = tx.send(());
        }
        let _ = fs::remove_file(endpoint_path());
    }
}

#[derive(Clone)]
struct HttpState {
    token: String,
    mgr: Arc<SessionManager>,
    app: AppHandle,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default)]
    include_archived: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    List {
        include_archived: bool,
    },
    Send {
        session_id: String,
        prompt: String,
        idempotency_key: Option<String>,
    },
}

pub fn parse_cli(argv: &[String]) -> Result<CliCommand, String> {
    let args: Vec<&str> = argv.iter().skip(1).map(String::as_str).collect();
    if args.contains(&SESSIONS_FLAG) {
        return Ok(CliCommand::List {
            include_archived: args.contains(&INCLUDE_ARCHIVED_FLAG),
        });
    }
    if let Some(i) = args.iter().position(|a| *a == SESSION_SEND_FLAG) {
        let session_id = args
            .get(i + 1)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && !s.starts_with("--"))
            .ok_or_else(|| "usage: --session-send <session-id> --prompt <text>".to_string())?;
        let prompt = parse_prompt(&args)?;
        if prompt.trim().is_empty() {
            return Err("empty prompt".into());
        }
        let idempotency_key = flag_value(&args, IDEMPOTENCY_FLAG);
        return Ok(CliCommand::Send {
            session_id,
            prompt,
            idempotency_key,
        });
    }
    Err("not a session-api command".into())
}

fn flag_value(args: &[&str], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| *a == flag)
        .and_then(|i| args.get(i + 1))
        .map(|s| (*s).to_string())
        .filter(|s| !s.starts_with("--"))
}

fn parse_prompt(args: &[&str]) -> Result<String, String> {
    if let Some(text) = flag_value(args, PROMPT_FLAG) {
        return Ok(text);
    }
    if let Some(path) = flag_value(args, PROMPT_FILE_FLAG) {
        if path == "-" {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| e.to_string())?;
            return Ok(buf);
        }
        return fs::read_to_string(path).map_err(|e| e.to_string());
    }
    Err("provide --prompt <text> or --prompt-file <path>".into())
}

pub fn list_sessions(include_archived: bool) -> Vec<SessionListItem> {
    let projects = store::load_projects();
    let mut rows: Vec<SessionListItem> = store::load_sessions_index()
        .into_iter()
        .filter(|s| include_archived || !s.archived)
        .map(|s| map_session(s, &projects))
        .collect();
    rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    rows
}

fn map_session(s: SessionMeta, projects: &[store::Project]) -> SessionListItem {
    let project_name = s.project_id.as_ref().and_then(|pid| {
        projects
            .iter()
            .find(|p| &p.id == pid)
            .map(|p| p.name.clone())
    });
    SessionListItem {
        id: s.id,
        title: s.title,
        project_id: s.project_id,
        project_name,
        updated_at: s.updated_at.to_rfc3339(),
        archived: s.archived,
        pinned: s.pinned,
    }
}

pub fn endpoint_path() -> PathBuf {
    app_data_root().join(ENDPOINT_FILE)
}

pub fn read_endpoint_file() -> Option<EndpointFile> {
    let raw = fs::read_to_string(endpoint_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_endpoint_file(ep: &EndpointFile) -> Result<(), String> {
    let _ = ensure_app_dirs();
    let path = endpoint_path();
    let body = serde_json::to_string_pretty(ep).map_err(|e| e.to_string())?;
    fs::write(&path, body).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn random_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn token_ok(headers: &HeaderMap, expected: &str) -> bool {
    if let Some(v) = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
    {
        let rest = v.strip_prefix("Bearer ").unwrap_or(v).trim();
        if constant_time_eq(rest, expected) {
            return true;
        }
    }
    if let Some(v) = headers.get("x-grok-token").and_then(|h| h.to_str().ok()) {
        return constant_time_eq(v.trim(), expected);
    }
    false
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// HTTP-layer errors from the CLI client.
/// Connection-refused / DNS → app is not listening (`app_not_running`).
/// Timeouts and other uncertain transport failures return `unknown`, never permission to resend.
pub fn classify_cli_transport_error(err: &str, session_id: &str) -> (TurnStatus, i32, String) {
    if cli_error_means_app_not_running(err) {
        (
            TurnStatus::AppNotRunning,
            2,
            format!(
                "Grok App is not running ({err}). Start the app (or leave it in the tray), then retry."
            ),
        )
    } else {
        (
            TurnStatus::Unknown,
            1,
            format!("session-api outcome unknown ({err}) for session {session_id}; inspect the original session before any new request; do not automatically resend"),
        )
    }
}

pub fn cli_error_means_app_not_running(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    if e.contains("timed out") || e.contains("timeout") {
        return false;
    }
    if e.contains("503") || e.contains("500") || e.contains("502") || e.contains("504") {
        return false;
    }
    e.contains("connection refused")
        || e.contains("connect error")
        || e.contains("error connecting")
        || e.contains("dns error")
        || e.contains("no such host")
        || e.contains("network is unreachable")
        || e.contains("failed to lookup")
}

pub fn classify_send_error(err: &str) -> TurnStatus {
    let e = err.to_ascii_lowercase();
    if e.contains("still running") || e.contains("task_already_running") {
        return TurnStatus::Busy;
    }
    if e.contains("session not found") || e.contains("not found") {
        return TurnStatus::NotFound;
    }
    TurnStatus::Error
}

/// Check a session can take a turn and build the request for it. `origin` names
/// who asked (`pr_monitor`, `external`): an injected prompt is remembered so the
/// transcript can show an automatic follow-up instead of the user's own words.
pub fn prepare_send(
    session_id: &str,
    prompt: &str,
    origin: &'static str,
) -> Result<PreparedSend, Box<TurnResult>> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err(Box::new(TurnResult::fail(
            TurnStatus::Error,
            session_id,
            "empty prompt",
        )));
    }
    let list = store::load_sessions_index();
    let Some(meta) = list.into_iter().find(|s| s.id == session_id) else {
        return Err(Box::new(TurnResult::fail(
            TurnStatus::NotFound,
            session_id,
            "session not found",
        )));
    };
    let projects = store::load_projects();
    if let Some(ref pid) = meta.project_id {
        let Some(p) = projects.iter().find(|p| &p.id == pid) else {
            return Err(Box::new(TurnResult::fail(
                TurnStatus::Error,
                session_id,
                format!("project not found: {pid}"),
            )));
        };
        if !p.trusted {
            return Err(Box::new(TurnResult::fail(
                TurnStatus::Error,
                session_id,
                format!("project not trusted: {}", p.name),
            )));
        }
        if !p.path_ok && !p.is_ssh_remote() {
            return Err(Box::new(TurnResult::fail(
                TurnStatus::Error,
                session_id,
                format!("project path missing: {}", p.name),
            )));
        }
    }
    // Prefer a linked worktree cwd (same as the composer), else the project path.
    // `connect` treats this as mock_mode=None — never pass SessionMeta.mode.
    let project_path = meta
        .worktree_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            meta.project_id.as_ref().and_then(|pid| {
                projects
                    .iter()
                    .find(|p| &p.id == pid)
                    .map(|p| p.path.clone())
            })
        });
    Ok(PreparedSend {
        session_id: meta.id,
        project_path,
        prompt: prompt.to_string(),
        origin,
    })
}

pub struct PreparedSend {
    pub session_id: String,
    pub project_path: Option<String>,
    pub prompt: String,
    /// Who asked for this turn (`pr_monitor`, `external`): a prompt the Host
    /// injected is marked (`auto_wake`) instead of reading as the user's.
    pub origin: &'static str,
}

pub async fn with_dispatch_timeout_for<F>(
    budget: std::time::Duration,
    session_id: String,
    fut: F,
) -> TurnResult
where
    F: std::future::Future<Output = TurnResult>,
{
    match tokio::time::timeout(budget, fut).await {
        Ok(result) => result,
        Err(_) => TurnResult::fail(TurnStatus::Unknown, session_id, DISPATCH_UNKNOWN_MESSAGE),
    }
}

pub async fn dispatch_turn_or_timeout(
    app: &AppHandle,
    mgr: &Arc<SessionManager>,
    req: PreparedSend,
) -> TurnResult {
    let sid = req.session_id.clone();
    with_dispatch_timeout_for(
        std::time::Duration::from_secs(DISPATCH_TURN_TIMEOUT_SECS),
        sid,
        dispatch_turn(app, mgr, req),
    )
    .await
}

pub async fn dispatch_turn(
    app: &AppHandle,
    mgr: &Arc<SessionManager>,
    req: PreparedSend,
) -> TurnResult {
    let sid = req.session_id.clone();
    if let Err(e) = crate::session_migration::reject_imported_history_id(Some(&sid)) {
        return TurnResult::fail(TurnStatus::Error, sid, e);
    }
    // Mid-turn (drawing / tools / permission): do not connect-steal or send.
    // Push onto the same composer follow-up queue the GUI already shows.
    if mgr.session_turn_busy(&sid) {
        return enqueue_while_busy(app, &sid, &req.prompt).await;
    }
    if let Err(e) = mgr
        .connect(app.clone(), req.project_path, Some(sid.clone()), None, None)
        .await
    {
        return TurnResult::fail(TurnStatus::Error, sid, format!("connect failed: {e}"));
    }
    // Connect can race a new turn; treat that as queue, never interrupt.
    if mgr.session_turn_busy(&sid) {
        return enqueue_while_busy(app, &sid, &req.prompt).await;
    }
    match mgr
        .send_injected_message(app.clone(), req.prompt.clone(), sid.clone(), req.origin)
        .await
    {
        // Origin belongs to this future's exact turn; cancellation cannot leave
        // a session-global arm for an intervening or later user message.
        Ok(_) => TurnResult::started(sid),
        Err(e) => {
            if classify_send_error(&e) == TurnStatus::Busy {
                return enqueue_while_busy(app, &sid, &req.prompt).await;
            }
            TurnResult::fail(classify_send_error(&e), sid, e)
        }
    }
}

async fn handle_turn(
    state: &HttpState,
    session_id: &str,
    body: TurnRequest,
) -> (StatusCode, Json<TurnResult>) {
    let key = body
        .idempotency_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let (result, _replayed) = idempotency::run_at(
        &app_data_root().join("session-api-idempotency.json"),
        session_id,
        &body.prompt,
        key.as_deref(),
        || async {
            match prepare_send(session_id, &body.prompt, "external") {
                Ok(prepared) => dispatch_turn_or_timeout(&state.app, &state.mgr, prepared).await,
                Err(result) => *result,
            }
        },
    )
    .await;
    // Snapshots reconcile the UI; replay must never emit a stale queued item.
    let result = queue_result::resolve_at(&app_data_root(), result).await;
    (status_for(&result.status), Json(result))
}

fn status_for(s: &TurnStatus) -> StatusCode {
    match s {
        TurnStatus::TurnStarted | TurnStatus::Cancelled => StatusCode::OK,
        TurnStatus::Queued => StatusCode::ACCEPTED,
        TurnStatus::Busy | TurnStatus::Unknown | TurnStatus::IdempotencyConflict => {
            StatusCode::CONFLICT
        }
        TurnStatus::NotFound => StatusCode::NOT_FOUND,
        TurnStatus::AppNotRunning | TurnStatus::RetryLater => StatusCode::SERVICE_UNAVAILABLE,
        TurnStatus::Error => StatusCode::BAD_REQUEST,
    }
}

fn unauthorized() -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({
            "ok": false,
            "status": "error",
            "message": "missing or invalid token",
        })),
    )
}

async fn get_sessions(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    if !token_ok(&headers, &state.token) {
        return unauthorized().into_response();
    }
    Json(list_sessions(q.include_archived)).into_response()
}

async fn post_turn(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(body): Json<TurnRequest>,
) -> impl IntoResponse {
    if !token_ok(&headers, &state.token) {
        return unauthorized().into_response();
    }
    let (code, json) = handle_turn(&state, &session_id, body).await;
    (code, json).into_response()
}

pub fn health_payload(mgr: &SessionManager) -> serde_json::Value {
    serde_json::json!({
        "ok": true,
        "connectLockBusy": mgr.connect_lock_busy(),
    })
}

async fn get_health(State(state): State<HttpState>, headers: HeaderMap) -> impl IntoResponse {
    if !token_ok(&headers, &state.token) {
        return unauthorized().into_response();
    }
    Json(health_payload(&state.mgr)).into_response()
}

// ── PR monitor (the Agent's handle on this chat's watches) ──────────────────
//
// The branch menu is the human path to a watch; these routes are the agent path,
// driven by the injected `pr-monitor` MCP (`pr_monitor::mcp`). All three reuse
// the Host's own commands, so the menu and the agent can never disagree about
// what a chat is watching.

#[derive(Debug, Deserialize)]
struct PrWatchesQuery {
    session_id: String,
}

#[derive(Debug, Deserialize)]
struct PrWatchBody {
    session_id: String,
    project_path: String,
    pr_number: u64,
    #[serde(default)]
    interval_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct PrUnwatchBody {
    session_id: String,
    pr_number: u64,
}

fn bad_request(message: &str) -> impl IntoResponse {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "ok": false,
            "status": "error",
            "message": message,
        })),
    )
}

async fn get_pr_watches(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Query(q): Query<PrWatchesQuery>,
) -> impl IntoResponse {
    if !token_ok(&headers, &state.token) {
        return unauthorized().into_response();
    }
    let sid = q.session_id.trim();
    if sid.is_empty() {
        return bad_request("session_id is required").into_response();
    }
    let state = crate::pr_monitor::pr_monitor_state(sid.to_string()).await;
    Json(state).into_response()
}

async fn post_pr_watch(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Json(body): Json<PrWatchBody>,
) -> impl IntoResponse {
    if !token_ok(&headers, &state.token) {
        return unauthorized().into_response();
    }
    let sid = body.session_id.trim();
    if sid.is_empty() {
        return bad_request("session_id is required").into_response();
    }
    let result = crate::pr_monitor::pr_monitor_watch(
        state.app.clone(),
        sid.to_string(),
        body.project_path,
        body.pr_number,
        body.interval_secs,
    )
    .await;
    Json(result).into_response()
}

async fn delete_pr_watch(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Json(body): Json<PrUnwatchBody>,
) -> impl IntoResponse {
    if !token_ok(&headers, &state.token) {
        return unauthorized().into_response();
    }
    let sid = body.session_id.trim();
    if sid.is_empty() {
        return bad_request("session_id is required").into_response();
    }
    let result =
        crate::pr_monitor::pr_monitor_unwatch(state.app.clone(), sid.to_string(), body.pr_number)
            .await;
    Json(result).into_response()
}

pub async fn start(app: AppHandle, mgr: Arc<SessionManager>) -> Result<SessionApiHandle, String> {
    let token = random_token();
    fanout_persisted_queue(&app).await;
    schedule_drain_ready_external_queues(app.clone(), Arc::clone(&mgr));
    let state = HttpState {
        token: token.clone(),
        mgr,
        app,
    };
    let router = Router::new()
        .route("/v1/health", get(get_health))
        .route("/v1/sessions", get(get_sessions))
        .route("/v1/sessions/{id}/turns", post(post_turn))
        .route("/v1/pr-monitor/watches", get(get_pr_watches))
        .route("/v1/pr-monitor/watches", post(post_pr_watch))
        .route("/v1/pr-monitor/watches", delete(delete_pr_watch))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("session api bind: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("session api local_addr: {e}"))?
        .port();
    let url = format!("http://127.0.0.1:{port}");
    write_endpoint_file(&EndpointFile {
        url: url.clone(),
        token,
        pid: Some(std::process::id()),
    })?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let serve = axum::serve(listener, router).with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        });
        if let Err(e) = serve.await {
            tracing::error!(error = %e, "session api http server exited");
        }
    });
    tracing::info!(%url, "session api listening (loopback, token-gated)");
    Ok(SessionApiHandle {
        url,
        shutdown: std::sync::Mutex::new(Some(shutdown_tx)),
    })
}

pub fn status(app: Option<&AppHandle>) -> SessionApiStatus {
    let listening = app
        .and_then(|a| a.try_state::<SessionApiHandle>().map(|_| true))
        .unwrap_or(false);
    let file = read_endpoint_file();
    SessionApiStatus {
        listening,
        url: file.map(|f| f.url),
        token_file: endpoint_path().display().to_string(),
        cli: cli_link_status(),
    }
}

#[tauri::command]
pub fn session_api_status(app: AppHandle) -> SessionApiStatus {
    status(Some(&app))
}

#[tauri::command]
pub async fn session_api_reveal_token_file() -> Result<String, String> {
    let path = endpoint_path();
    if !path.is_file() {
        return Err("session API is not listening — start Grok App first".into());
    }
    let shown = path.display().to_string();
    let pb = path.clone();
    tokio::task::spawn_blocking(move || crate::process_util::reveal_in_file_manager(&pb))
        .await
        .map_err(|e| e.to_string())??;
    Ok(shown)
}

#[tauri::command]
pub fn session_api_install_cli(app: AppHandle) -> Result<SessionApiStatus, String> {
    let desired = desired_cli_target()?;
    install_cli_link_at(&cli_link_path(), &desired)?;
    Ok(status(Some(&app)))
}

#[tauri::command]
pub fn session_api_remove_cli(app: AppHandle) -> Result<SessionApiStatus, String> {
    remove_cli_link_at(&cli_link_path())?;
    Ok(status(Some(&app)))
}

#[tauri::command]
pub async fn session_api_reveal_cli_link() -> Result<String, String> {
    let path = cli_link_path();
    if path.symlink_metadata().is_err() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let parent = parent.to_path_buf();
            let shown = parent.display().to_string();
            tokio::task::spawn_blocking(move || {
                crate::process_util::reveal_in_file_manager(&parent)
            })
            .await
            .map_err(|e| e.to_string())??;
            return Ok(shown);
        }
        return Err("command file is not installed".into());
    }
    let shown = path.display().to_string();
    let pb = path.clone();
    tokio::task::spawn_blocking(move || crate::process_util::reveal_in_file_manager(&pb))
        .await
        .map_err(|e| e.to_string())??;
    Ok(shown)
}

/// Client-only CLI. Does not start the desktop window.
pub fn run_cli() -> i32 {
    #[cfg(windows)]
    attach_parent_console();
    let argv: Vec<String> = std::env::args().collect();
    match parse_cli(&argv) {
        Ok(CliCommand::List { include_archived }) => {
            let rows = match read_endpoint_file() {
                Some(ep) => match http_list(&ep, include_archived) {
                    Ok(rows) => rows,
                    Err(e) => {
                        eprintln!("session-api: {e}; falling back to local index");
                        list_sessions(include_archived)
                    }
                },
                None => list_sessions(include_archived),
            };
            match serde_json::to_string_pretty(&rows) {
                Ok(s) => {
                    println!("{s}");
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
        Ok(CliCommand::Send {
            session_id,
            prompt,
            idempotency_key,
        }) => {
            let Some(ep) = read_endpoint_file() else {
                let mut out = TurnResult::fail(
                    TurnStatus::AppNotRunning,
                    session_id,
                    "Grok App is not running. Start the app (or leave it in the tray), then retry.",
                );
                out.idempotency_key = idempotency_key;
                print_turn(&out);
                return 2;
            };
            match http_send(&ep, &session_id, &prompt, idempotency_key.as_deref()) {
                Ok(r) => {
                    print_turn(&r);
                    turn_exit_code(&r)
                }
                Err(e) => {
                    let (status, code, message) = classify_cli_transport_error(&e, &session_id);
                    let mut out = TurnResult::fail(status, session_id, message);
                    out.idempotency_key = idempotency_key;
                    print_turn(&out);
                    code
                }
            }
        }
        Err(e) => {
            if e.contains("not a session-api command") {
                return -1;
            }
            eprintln!("{e}");
            1
        }
    }
}

fn turn_exit_code(r: &TurnResult) -> i32 {
    if r.ok {
        0
    } else if r.status == TurnStatus::Busy {
        3
    } else if r.status == TurnStatus::AppNotRunning {
        2
    } else {
        1
    }
}

fn print_turn(r: &TurnResult) {
    match serde_json::to_string_pretty(r) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("{e}"),
    }
}

fn http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(CLI_HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|e| e.to_string())
}

fn http_list(ep: &EndpointFile, include_archived: bool) -> Result<Vec<SessionListItem>, String> {
    let url = format!(
        "{}/v1/sessions?include_archived={}",
        ep.url.trim_end_matches('/'),
        include_archived
    );
    let res = http_client()?
        .get(url)
        .header(header::AUTHORIZATION, format!("Bearer {}", ep.token))
        .send()
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()));
    }
    res.json().map_err(|e| e.to_string())
}

fn http_send(
    ep: &EndpointFile,
    session_id: &str,
    prompt: &str,
    idempotency_key: Option<&str>,
) -> Result<TurnResult, String> {
    let url = format!(
        "{}/v1/sessions/{}/turns",
        ep.url.trim_end_matches('/'),
        session_id
    );
    let body = TurnRequest {
        prompt: prompt.to_string(),
        idempotency_key: idempotency_key.map(str::to_string),
    };
    let res = http_client()?
        .post(url)
        .header(header::AUTHORIZATION, format!("Bearer {}", ep.token))
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;
    res.json().map_err(|e| e.to_string())
}

/// Try CLI; `true` means the process should exit (caller uses [`run_cli`] status).
pub fn try_run_cli() -> bool {
    let argv: Vec<String> = std::env::args().collect();
    match parse_cli(&argv) {
        Ok(_) => true,
        Err(e) => !e.contains("not a session-api command"),
    }
}

#[cfg(windows)]
fn attach_parent_console() {
    #[allow(clippy::upper_case_acronyms)] // Win32 API name
    type BOOL = i32;
    extern "system" {
        fn AttachConsole(dw_process_id: u32) -> BOOL;
    }
    unsafe {
        let _ = AttachConsole(0xFFFF_FFFF);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_list_and_send() {
        let list = parse_cli(&[
            "grok-app".into(),
            "--sessions".into(),
            "--include-archived".into(),
        ])
        .unwrap();
        assert_eq!(
            list,
            CliCommand::List {
                include_archived: true
            }
        );
        let send = parse_cli(&[
            "grok-app".into(),
            "--session-send".into(),
            "abc-123".into(),
            "--prompt".into(),
            "hello there".into(),
            "--idempotency-key".into(),
            "k1".into(),
        ])
        .unwrap();
        assert_eq!(
            send,
            CliCommand::Send {
                session_id: "abc-123".into(),
                prompt: "hello there".into(),
                idempotency_key: Some("k1".into()),
            }
        );
        assert!(parse_cli(&["grok-app".into()]).is_err());
    }

    #[test]
    fn queued_http_is_accepted() {
        assert_eq!(status_for(&TurnStatus::Queued), StatusCode::ACCEPTED);
        let raw = serde_json::to_value(TurnStatus::Queued).unwrap();
        assert_eq!(raw, serde_json::json!("queued"));
    }

    #[test]
    fn classify_busy_and_missing() {
        assert_eq!(
            classify_send_error("CONNECT_FAILED: chat x is still running its previous turn"),
            TurnStatus::Busy
        );
        assert_eq!(
            classify_send_error("session not found"),
            TurnStatus::NotFound
        );
        assert_eq!(classify_send_error("boom"), TurnStatus::Error);
    }

    #[test]
    fn cli_link_install_update_and_refuse_foreign() {
        let home = std::env::temp_dir().join(format!("grok-cli-link-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(home.clone());
        let home = home.as_path();
        let bin_dir = home.join("app");
        fs::create_dir_all(&bin_dir).unwrap();
        let bin = bin_dir.join("Grok");
        fs::write(&bin, b"fake-bin").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(&bin).unwrap().permissions();
            p.set_mode(0o755);
            fs::set_permissions(&bin, p).unwrap();
        }
        let link = cli_link_path_in(home);
        assert_eq!(link.file_name().unwrap(), cli_link_name());
        let missing = inspect_cli_link(&link, &bin);
        assert!(!missing.installed);
        assert!(!missing.ours);
        install_cli_link_at(&link, &bin).unwrap();
        let ok = inspect_cli_link(&link, &bin);
        assert!(ok.installed && ok.ours && ok.matches_running);
        let other = bin_dir.join("Grok-other");
        fs::write(&other, b"other").unwrap();
        install_cli_link_at(&link, &other).unwrap();
        let updated = inspect_cli_link(&link, &other);
        assert!(updated.matches_running);
        assert!(!inspect_cli_link(&link, &bin).matches_running);
        remove_cli_link_at(&link).unwrap();
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        fs::write(&link, b"user file").unwrap();
        assert!(install_cli_link_at(&link, &bin)
            .unwrap_err()
            .contains("overwrite"));
        assert!(remove_cli_link_at(&link)
            .unwrap_err()
            .contains("did not install"));
    }

    #[test]
    fn windows_shim_roundtrip() {
        let body = render_windows_shim(std::path::Path::new(r"C:\Apps\Grok.exe"));
        assert!(is_windows_shim(&body));
        assert_eq!(
            parse_windows_shim_target(&body).unwrap(),
            std::path::PathBuf::from(r"C:\Apps\Grok.exe")
        );
        assert!(parse_windows_shim_target("echo hi").is_none());
    }

    #[test]
    fn token_header_accepts_bearer() {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
        assert!(token_ok(&h, "secret"));
        assert!(!token_ok(&h, "other"));
    }

    #[test]
    fn token_header_accepts_x_grok_token() {
        let mut h = HeaderMap::new();
        h.insert("x-grok-token", "secret".parse().unwrap());
        assert!(token_ok(&h, "secret"));
        assert!(!token_ok(&h, "other"));
    }

    #[test]
    fn send_without_prompt_is_cli_error() {
        let err =
            parse_cli(&["grok-app".into(), "--session-send".into(), "abc".into()]).unwrap_err();
        assert!(err.contains("prompt"), "{err}");
        assert!(try_run_cli_argv(&[
            "grok-app".into(),
            "--session-send".into(),
            "abc".into(),
        ]));
        assert!(!try_run_cli_argv(&["grok-app".into()]));
    }

    fn try_run_cli_argv(argv: &[String]) -> bool {
        match parse_cli(argv) {
            Ok(_) => true,
            Err(e) => !e.contains("not a session-api command"),
        }
    }

    #[test]
    fn retry_later_is_503_and_serializes() {
        assert_eq!(
            status_for(&TurnStatus::RetryLater),
            StatusCode::SERVICE_UNAVAILABLE
        );
        let raw = serde_json::to_value(TurnStatus::RetryLater).unwrap();
        assert_eq!(raw, serde_json::json!("retry_later"));
        const { assert!(DISPATCH_TURN_TIMEOUT_SECS < 90) };
        const { assert!(CLI_HTTP_TIMEOUT_SECS > DISPATCH_TURN_TIMEOUT_SECS) };
    }

    #[test]
    fn cli_timeout_is_not_app_not_running() {
        assert!(!cli_error_means_app_not_running(
            "error sending request for url (http://127.0.0.1:9/v1/sessions/x/turns): operation timed out"
        ));
        let (status, code, msg) = classify_cli_transport_error("operation timed out", "abc");
        assert_eq!(status, TurnStatus::Unknown);
        assert_eq!(code, 1);
        assert!(!msg.to_ascii_lowercase().contains("not running"), "{msg}");
    }

    #[test]
    fn cli_connection_refused_is_app_not_running() {
        assert!(cli_error_means_app_not_running(
            "error sending request for url (http://127.0.0.1:9/v1/sessions/x/turns): connection refused"
        ));
        let (status, code, msg) = classify_cli_transport_error("connection refused", "abc");
        assert_eq!(status, TurnStatus::AppNotRunning);
        assert_eq!(code, 2);
        assert!(msg.contains("not running"), "{msg}");
    }

    #[test]
    fn persist_queue_roundtrip_and_confirmed_take() {
        let home = queue_regression_tests::Fixture::new();
        let first = queue_store::Item::new("s1", "q-1", "hello", 1);
        let second = queue_store::Item::new("s1", "q-2", "world", 2);
        queue_store::insert(&home.path(), first.clone()).unwrap();
        queue_store::insert(&home.path(), second).unwrap();
        let nonce = queue_store::claim(&home.path(), &first).unwrap().unwrap();
        assert_eq!(queue_store::list(&home.path()).unwrap().len(), 2);
        assert!(
            queue_store::finish(&home.path(), &first, &nonce, &TurnResult::started("s1")).unwrap()
        );
        let rest = queue_store::list(&home.path()).unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].item_id, "q-2");
    }

    #[tokio::test]
    async fn dispatch_timeout_returns_unknown_without_asserting_unsent() {
        let sid = "s-timeout".to_string();
        let r = with_dispatch_timeout_for(
            std::time::Duration::from_millis(30),
            sid.clone(),
            std::future::pending::<TurnResult>(),
        )
        .await;
        assert_eq!(r.status, TurnStatus::Unknown);
        assert_eq!(r.session_id, sid);
        assert!(
            r.message
                .as_deref()
                .unwrap_or("")
                .contains("outcome unknown"),
            "{:?}",
            r.message
        );
    }

    #[test]
    fn health_payload_reports_lock_busy() {
        let mgr = SessionManager::new();
        let v = health_payload(&mgr);
        assert_eq!(v["ok"], true);
        assert_eq!(v["connectLockBusy"], false);
    }
}
