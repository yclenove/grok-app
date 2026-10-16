//! Local-session MCP injection. Tokens stay out of argv, environment, and logs.
//!
//! Scheme 1 (ADR 0001) is an ACP `type:"http"` entry whose bearer lives only in
//! the `Authorization` header. Scheme 2 passes the same secret through an
//! inherited `--token-fd` and never through `GCU_TOKEN`.

use crate::messages::{SessionKind, SessionOpened};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;

/// ACP server name for both injection schemes.
pub const SERVER_NAME: &str = "grok-computer-use";

/// Facts the App already has when it decides whether a chat may see Computer Use.
///
/// `ssh` is the existing non-empty OpenSSH alias check. `scheduled` is
/// `SessionMeta.scheduled`. `remote_im` is a bridge turn. `side_channel` is the
/// official-aux empty MCP list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InjectFacts {
    /// OpenSSH alias is present.
    pub ssh: bool,
    /// Host scheduler created the chat.
    pub scheduled: bool,
    /// Remote IM bridge turn.
    pub remote_im: bool,
    /// Official side job that must not receive App MCP servers.
    pub side_channel: bool,
}

/// One issued endpoint. Debug is omitted so a token cannot be printed by accident.
#[derive(Clone)]
pub struct Issued {
    /// Exact loopback MCP URL.
    pub mcp_url: String,
    /// 32-byte lowercase hex bearer.
    pub token: String,
}

/// Live endpoints plus every secret that must still be scrubbed after revoke.
#[derive(Default)]
pub struct TokenBook {
    live: HashMap<String, Issued>,
    redact: Vec<String>,
}

/// The four places a session secret is forbidden to appear.
pub struct Surfaces<'a> {
    /// Environment of this process.
    pub process_env: &'a [(String, String)],
    /// Environment explicitly given to a child. Inherited vars are `process_env`.
    pub child_env: &'a [(String, String)],
    /// Text written about an ACP payload.
    pub acp_log: &'a str,
    /// Text placed in a crash or exit report.
    pub crash_report: &'a str,
}

static KNOWN: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn lock(mutex: &Mutex<Vec<String>>) -> std::sync::MutexGuard<'_, Vec<String>> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

/// Session kind for `engine.session.open`. SSH wins over the other refusals.
pub fn session_kind(facts: InjectFacts) -> SessionKind {
    if facts.ssh {
        SessionKind::Ssh
    } else if facts.remote_im {
        SessionKind::RemoteIm
    } else if facts.scheduled {
        SessionKind::Automation
    } else {
        SessionKind::Local
    }
}

/// Existing ACP rule: SSH and official side jobs get an empty `mcpServers` list.
pub fn omit_local_mcp_catalog(facts: InjectFacts) -> bool {
    facts.ssh || facts.side_channel
}

/// Computer Use is added only for an enabled local chat that is not a side job.
pub fn should_inject(enabled: bool, facts: InjectFacts) -> bool {
    enabled && !facts.side_channel && session_kind(facts) == SessionKind::Local
}

/// Extra environment for the App and for `gcu`. Always empty: no `GCU_TOKEN`.
pub fn extra_env() -> Vec<(String, String)> {
    Vec::new()
}

/// Scheme 1. `token` is only the Authorization header value.
pub fn http_entry(url: &str, token: &str) -> Value {
    json!({
        "type": "http",
        "name": SERVER_NAME,
        "url": url,
        "headers": [{ "name": "Authorization", "value": format!("Bearer {token}") }],
    })
}

/// Scheme 2. `fd` is an inherited descriptor. The token is not in argv or env.
pub fn fd_entry(command: &str, url: &str, fd: u32) -> Value {
    json!({
        "name": SERVER_NAME,
        "command": command,
        "args": ["mcp", "--connect", url, "--token-fd", fd.to_string()],
    })
}

/// Log line for the MCP catalog. It has a count and no payload.
pub fn catalog_log(count: usize) -> String {
    format!("acp session open injecting mcpServers count={count}")
}

/// Redacted form of an ACP payload. Register secrets first, or pass them to [`redact`].
pub fn acp_payload_log(payload: &Value) -> String {
    scrub(&payload.to_string())
}

/// Exit text with registered secrets and bearer/`GCU_TOKEN` patterns removed.
pub fn crash_report(head: &str, stderr: &str) -> String {
    let tail = scrub(stderr);
    if tail.is_empty() {
        scrub(head)
    } else {
        let capped = if tail.len() > 800 {
            format!("…{}", &tail[tail.len() - 800..])
        } else {
            tail
        };
        scrub(&format!("{head}; stderr: {capped}"))
    }
}

/// Remember a bearer so later [`scrub`] calls remove it. Idempotent. Bounded.
pub fn remember_secret(token: &str) {
    if token.len() < 16 {
        return;
    }
    let mut known = lock(&KNOWN);
    if known.iter().any(|item| item == token) {
        return;
    }
    known.push(token.to_owned());
    if known.len() > 256 {
        known.remove(0);
    }
}

/// Remove registered secrets, `Bearer <64 hex>`, and `GCU_TOKEN=` assignments.
pub fn scrub(text: &str) -> String {
    let known = lock(&KNOWN);
    redact(text, &known)
}

/// Remove `tokens` and the bearer / `GCU_TOKEN` patterns. Does not touch the registry.
pub fn redact(text: &str, tokens: &[String]) -> String {
    let mut out = text.to_owned();
    for token in tokens {
        if token.len() >= 16 {
            out = out.replace(token, "[redacted]");
        }
    }
    redact_markers(&out)
}

/// Names of surfaces that still contain `token` or a `GCU_TOKEN` assignment.
pub fn leak_sites(token: &str, surfaces: Surfaces<'_>) -> Vec<&'static str> {
    let mut hits = Vec::new();
    if env_leaks(surfaces.process_env, token) {
        hits.push("process_env");
    }
    if env_leaks(surfaces.child_env, token) {
        hits.push("child_env");
    }
    if text_leaks(surfaces.acp_log, token) {
        hits.push("acp_log");
    }
    if text_leaks(surfaces.crash_report, token) {
        hits.push("crash_report");
    }
    hits
}

fn env_leaks(pairs: &[(String, String)], token: &str) -> bool {
    pairs.iter().any(|(key, value)| {
        key == "GCU_TOKEN"
            || key.contains(token)
            || value.contains(token)
            || value.contains("GCU_TOKEN=")
    })
}

fn text_leaks(text: &str, token: &str) -> bool {
    text.contains(token) || text.contains("GCU_TOKEN=")
}

fn redact_markers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let Some((at, skip)) = find_marker(rest) else {
            out.push_str(rest);
            break;
        };
        if skip == 0 {
            let next = at.saturating_add(1).min(rest.len());
            out.push_str(&rest[..next]);
            rest = &rest[next..];
            continue;
        }
        out.push_str(&rest[..at]);
        out.push_str("[redacted]");
        rest = &rest[at + skip..];
    }
    out
}

/// Index of the earliest marker and how many bytes it consumes.
/// A zero length means the match is not a 64-hex bearer; the caller skips one byte.
fn find_marker(text: &str) -> Option<(usize, usize)> {
    let bearer = text.find("Bearer ").map(|at| (at, bearer_len(&text[at..])));
    let env = text
        .find("GCU_TOKEN=")
        .map(|at| (at, gcu_token_len(&text[at..])));
    match (bearer, env) {
        (Some((left, left_len)), Some((right, right_len))) => {
            if left <= right {
                Some((left, left_len))
            } else {
                Some((right, right_len))
            }
        }
        (Some(pair), None) | (None, Some(pair)) => Some(pair),
        (None, None) => None,
    }
}

fn bearer_len(text: &str) -> usize {
    let Some(hex) = text.strip_prefix("Bearer ") else {
        return 0;
    };
    let digits = hex.bytes().take_while(u8::is_ascii_hexdigit).count();
    if digits == 64 {
        "Bearer ".len() + 64
    } else {
        0
    }
}

fn gcu_token_len(text: &str) -> usize {
    let Some(value) = text.strip_prefix("GCU_TOKEN=") else {
        return 0;
    };
    let tail = value
        .find(|ch: char| ch.is_whitespace() || ch == '"' || ch == '\'')
        .unwrap_or(value.len());
    "GCU_TOKEN=".len() + tail
}

impl TokenBook {
    /// Empty book.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new endpoint. A replaced secret stays in the redact list.
    pub fn issue(&mut self, session_id: &str, issued: Issued) {
        remember_secret(&issued.token);
        self.keep(&issued.token);
        if let Some(previous) = self.live.insert(session_id.to_owned(), issued) {
            self.keep(&previous.token);
        }
    }

    /// Drop the live endpoint. The secret remains scrubbed.
    pub fn revoke(&mut self, session_id: &str) -> Option<Issued> {
        let issued = self.live.remove(session_id)?;
        self.keep(&issued.token);
        Some(issued)
    }

    /// Forget every live endpoint and keep their secrets for scrubbing.
    pub fn revoke_all(&mut self) {
        let live = std::mem::take(&mut self.live);
        for issued in live.into_values() {
            self.keep(&issued.token);
        }
    }

    /// Endpoint currently accepted for this session.
    pub fn current(&self, session_id: &str) -> Option<&Issued> {
        self.live.get(session_id)
    }

    /// Live session count for the supervisor idle clock.
    pub fn live_count(&self) -> u32 {
        u32::try_from(self.live.len()).unwrap_or(u32::MAX)
    }

    /// Live session ids. Order is not significant.
    pub fn live_ids(&self) -> Vec<String> {
        self.live.keys().cloned().collect()
    }

    fn keep(&mut self, token: &str) {
        if token.len() >= 16 && !self.redact.iter().any(|item| item == token) {
            self.redact.push(token.to_owned());
        }
    }
}

impl From<&SessionOpened> for Issued {
    fn from(opened: &SessionOpened) -> Self {
        Self {
            mcp_url: opened.mcp_url.clone(),
            token: opened.token.clone(),
        }
    }
}
