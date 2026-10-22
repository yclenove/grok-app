//! Application-binary stdio MCP for in-range import history. No UI, no plugins.

#[cfg(test)]
use std::collections::HashSet;
use std::io::{BufRead, Read, Write};

use rusqlite::OptionalExtension;
use serde_json::{json, Value};

use super::continuation::AllowedSelection;
use super::errors::ErrorCode;
#[cfg(test)]
use super::search::search;
use super::search::{read_with_selection, search_with_selection};

pub const GRANT_ENV: &str = "GROK_IMPORT_HISTORY_GRANT";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportHistoryGrant {
    pub token: String,
    pub app_session_id: String,
    pub import_id: String,
    pub revision_id: String,
    pub branch_leaf: String,
    pub allowed_event_ids: Vec<String>,
    pub allowed_selection_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<super::continuation::AllowedSelection>,
}

impl ImportHistoryGrant {
    #[cfg(test)]
    pub fn allowed_set(&self) -> HashSet<String> {
        self.allowed_event_ids.iter().cloned().collect()
    }

    pub fn matches_selection(&self) -> bool {
        self.granted_selection().is_ok()
    }

    pub fn granted_selection(&self) -> Result<AllowedSelection, String> {
        super::continuation_scope::bound_selection(
            &self.import_id,
            &self.revision_id,
            &self.branch_leaf,
            self.selection.as_ref(),
            &self.allowed_selection_hash,
        )
        .map_err(|_| "HISTORY_ACCESS_UNAVAILABLE: grant selection mismatch".to_string())
    }
}

pub fn load_grant_from_env() -> Result<ImportHistoryGrant, String> {
    let raw = std::env::var(GRANT_ENV).map_err(|_| "HISTORY_ACCESS_UNAVAILABLE: missing grant")?;
    let launch: Value =
        serde_json::from_str(&raw).map_err(|_| "HISTORY_ACCESS_UNAVAILABLE: invalid grant")?;
    let sid = launch
        .get("appSessionId")
        .and_then(Value::as_str)
        .ok_or("HISTORY_ACCESS_UNAVAILABLE: missing session")?;
    uuid::Uuid::parse_str(sid).map_err(|_| "HISTORY_ACCESS_UNAVAILABLE: invalid session")?;
    let grant = super::engine::grant_for_session(sid)
        .ok_or("HISTORY_ACCESS_UNAVAILABLE: missing continuation")?;
    if launch.get("token").and_then(Value::as_str) != Some(grant.token.as_str()) {
        return Err("HISTORY_ACCESS_UNAVAILABLE: revoked grant".into());
    }
    validate_grant(&grant)?;
    Ok(grant)
}

fn validate_grant(grant: &ImportHistoryGrant) -> Result<(), String> {
    let unavailable = || "HISTORY_ACCESS_UNAVAILABLE: grant is invalid or revoked".to_string();
    if !grant.matches_selection() || grant.token.is_empty() {
        return Err(unavailable());
    }
    let current =
        super::engine::grant_for_session(&grant.app_session_id).ok_or_else(unavailable)?;
    if current.token != grant.token
        || current.import_id != grant.import_id
        || current.revision_id != grant.revision_id
        || current.branch_leaf != grant.branch_leaf
        || current.allowed_selection_hash != grant.allowed_selection_hash
    {
        return Err(unavailable());
    }
    let conn = super::catalog::open_catalog_readonly().map_err(|_| unavailable())?;
    let bound: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM continuation_refs WHERE app_session_id=?1 AND import_id=?2 AND revision_id=?3 AND branch_leaf=?4 AND allowed_selection_hash=?5)",
        rusqlite::params![grant.app_session_id, grant.import_id, grant.revision_id, grant.branch_leaf, grant.allowed_selection_hash],
        |r| r.get(0),
    ).map_err(|_| unavailable())?;
    if !bound {
        return Err(unavailable());
    }
    let hidden: Option<i64> = conn
        .query_row(
            "SELECT hidden FROM entries WHERE import_id=?1",
            rusqlite::params![grant.import_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| unavailable())?;
    if hidden != Some(0) {
        return Err(unavailable());
    }
    let bound_revision: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM revisions WHERE import_id=?1 AND revision_id=?2)",
            rusqlite::params![grant.import_id, grant.revision_id],
            |r| r.get(0),
        )
        .map_err(|_| unavailable())?;
    if !bound_revision {
        return Err(unavailable());
    }
    Ok(())
}

pub fn run_stdio() -> i32 {
    let grant = match load_grant_from_env() {
        Ok(g) => g,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "{e}");
            return 2;
        }
    };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut reader = stdin.lock();
    loop {
        let msg = match read_rpc(&mut reader) {
            Ok(Some(v)) => v,
            Ok(None) => break,
            Err(_) => break,
        };
        // JSON-RPC notifications never receive a reply. In particular, replying
        // to notifications/initialized confuses clients during tool discovery.
        let Some(id) = msg.get("id").cloned() else {
            continue;
        };
        let method = msg.get("method").and_then(|x| x.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result = handle_method(&grant, method, &params);
        let resp = match result {
            Ok(v) => json!({"jsonrpc":"2.0","id": id, "result": v}),
            Err(e) => json!({"jsonrpc":"2.0","id": id, "error": {"code": -32000, "message": e}}),
        };
        if write_rpc(&mut stdout, &resp).is_err() {
            break;
        }
    }
    0
}

pub fn handle_method(
    grant: &ImportHistoryGrant,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "import-history", "version": "1.0.0" }
        })),
        "notifications/initialized" | "initialized" => Ok(json!({})),
        "tools/list" => Ok(json!({
            "tools": [
                {
                    "name": "import_history_search",
                    "description": "Literal search over the allowed imported snapshot.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" },
                            "cursor": { "type": "string" },
                            "limit": { "type": "integer" }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "import_history_read",
                    "description": "Read one allowed imported event by citation id.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "citation": { "type": "string" },
                            "cursor": { "type": "string" },
                            "maxBytes": { "type": "integer" }
                        },
                        "required": ["citation"]
                    }
                },
                {
                    "name": "import_history_asset_read",
                    "description": "Read one allowed stored attachment by asset id.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "assetId": { "type": "string" },
                            "maxBytes": { "type": "integer" }
                        },
                        "required": ["assetId"]
                    }
                }
            ]
        })),
        "tools/call" => {
            let name = params.get("name").and_then(|x| x.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            call_tool(grant, name, &args)
        }
        "ping" => Ok(json!({})),
        other => Err(format!("unknown method {other}")),
    }
}

pub fn call_tool(grant: &ImportHistoryGrant, name: &str, args: &Value) -> Result<Value, String> {
    validate_grant(grant)?;
    if grant.app_session_id.trim().is_empty() || grant.token.trim().is_empty() {
        return Err("HISTORY_ACCESS_UNAVAILABLE: grant is not session-bound".into());
    }
    if let Some(sid) = args.get("appSessionId").and_then(|x| x.as_str()) {
        if sid != grant.app_session_id {
            return Err("HISTORY_ACCESS_UNAVAILABLE: grant session mismatch".into());
        }
    }
    let dir = super::catalog::revision_dir(&grant.import_id, &grant.revision_id)
        .map_err(|e| e.into_command_err())?;
    let selection = grant.granted_selection()?;
    let relations = super::continuation::load_relations(&grant.import_id, &grant.revision_id)
        .map_err(|e| e.into_command_err())?;
    match name {
        "import_history_search" | "search" => {
            let query = args.get("query").and_then(|x| x.as_str()).unwrap_or("");
            if query.contains("SELECT ") || query.contains(".*") && query.contains('(') {
                // Literal only; do not execute as SQL/regex. grams() already tokenizes.
            }
            let cursor = args.get("cursor").and_then(|x| x.as_str());
            let limit = args
                .get("limit")
                .and_then(|x| x.as_u64())
                .map(|n| n as usize);
            let (hits, next) =
                search_with_selection(&dir, query, cursor, limit, &selection, &relations)
                    .map_err(|e| e.into_command_err())?;
            Ok(json!({
                "content": [{ "type": "text", "text": serde_json::to_string(&json!({
                    "hits": hits,
                    "nextCursor": next,
                    "integrity": "allowed-selection"
                })).unwrap_or_default() }]
            }))
        }
        "import_history_read" | "read" => {
            let citation = args
                .get("citation")
                .and_then(|x| x.as_str())
                .ok_or_else(|| "citation required".to_string())?;
            if citation.contains("..") || citation.contains('/') || citation.contains('\\') {
                return Err(ErrorCode::HistoryAccessUnavailable.as_str().into());
            }
            let cursor = args.get("cursor").and_then(|x| x.as_str());
            let max_bytes = args
                .get("maxBytes")
                .and_then(|x| x.as_u64())
                .map(|n| n as usize);
            let (text, next_cursor) =
                read_with_selection(&dir, citation, cursor, max_bytes, &selection, &relations)
                    .map_err(|e| e.into_command_err())?;
            Ok(json!({
                "content": [{ "type": "text", "text": serde_json::to_string(&json!({
                    "citation": citation,
                    "text": text,
                    "truncated": next_cursor.is_some(),
                    "nextCursor": next_cursor,
                    "revision": grant.revision_id
                })).unwrap_or_default() }]
            }))
        }
        "import_history_asset_read" | "asset_read" => {
            let asset_id = args
                .get("assetId")
                .and_then(|x| x.as_str())
                .ok_or_else(|| "assetId required".to_string())?;
            if asset_id.contains("..")
                || asset_id.contains('/')
                || asset_id.contains('\\')
                || args.get("path").is_some()
            {
                return Err(ErrorCode::HistoryAccessUnavailable.as_str().into());
            }
            if !selection.permits_asset(asset_id) {
                return Err(
                    "HISTORY_ACCESS_UNAVAILABLE: asset is outside the allowed selection".into(),
                );
            }
            let max_bytes = args
                .get("maxBytes")
                .and_then(|x| x.as_u64())
                .map(|n| n as usize)
                .unwrap_or(super::budget::READ_MAX_BYTES)
                .min(super::budget::READ_MAX_BYTES);
            let raw = std::fs::read(dir.join("assets.json"))
                .map_err(|_| "HISTORY_ACCESS_UNAVAILABLE: asset catalog missing".to_string())?;
            let assets: Vec<super::types::AssetRecord> = serde_json::from_slice(&raw)
                .map_err(|_| "HISTORY_ACCESS_UNAVAILABLE: asset catalog invalid".to_string())?;
            let asset = assets
                .iter()
                .find(|a| a.asset_id == asset_id)
                .ok_or_else(|| "HISTORY_ACCESS_UNAVAILABLE: asset not found".to_string())?;
            if asset.state != super::types::AssetState::Stored {
                return Err("HISTORY_ACCESS_UNAVAILABLE: asset is not stored".into());
            }
            let digest = asset
                .digest
                .as_deref()
                .ok_or_else(|| "HISTORY_ACCESS_UNAVAILABLE: asset digest missing".to_string())?;
            if digest.contains("..") || digest.contains('/') || digest.contains('\\') {
                return Err(ErrorCode::HistoryAccessUnavailable.as_str().into());
            }
            let blob = dir.join("blobs").join(digest);
            super::catalog::ensure_inside_imports(&blob).map_err(|e| e.into_command_err())?;
            let bytes = std::fs::read(&blob)
                .map_err(|_| "HISTORY_ACCESS_UNAVAILABLE: stored blob missing".to_string())?;
            let truncated = bytes.len() > max_bytes;
            let slice = if truncated {
                &bytes[..max_bytes]
            } else {
                &bytes
            };
            use base64::{engine::general_purpose::STANDARD, Engine as _};
            let encoded = STANDARD.encode(slice);
            Ok(json!({
                "content": [{ "type": "text", "text": serde_json::to_string(&json!({
                    "assetId": asset_id,
                    "mime": asset.mime,
                    "size": bytes.len(),
                    "truncated": truncated,
                    "bytes": encoded
                })).unwrap_or_default() }]
            }))
        }
        _ => Err("unknown tool".into()),
    }
}

fn read_rpc<R: BufRead>(r: &mut R) -> Result<Option<Value>, ()> {
    loop {
        const RPC_MAX_BYTES: usize = 1024 * 1024;
        let mut line = Vec::new();
        let n = (&mut *r)
            .take((RPC_MAX_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| ())?;
        if n == 0 {
            return Ok(None);
        }
        if n > RPC_MAX_BYTES {
            return Err(());
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        return serde_json::from_slice(&line).map(Some).map_err(|_| ());
    }
}

fn write_rpc<W: Write>(w: &mut W, v: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(v)?;
    w.write_all(&body)?;
    w.write_all(b"\n")?;
    w.flush()
}

/// Per-open MCP inject policy. SSH/WSL/TCP/official-aux never get the local binary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMcpPolicy {
    pub ssh: bool,
    pub wsl: bool,
    pub official_aux: bool,
    pub tcp: bool,
}

impl SessionMcpPolicy {
    pub fn allows_local_import_mcp(self) -> bool {
        !self.ssh && !self.wsl && !self.official_aux && !self.tcp
    }
}

/// Merge user MCP servers with the current session's import-history grant.
/// Previous `import-history` entries are dropped so process reuse A→B→A
/// cannot keep the last tenant's grant.
pub fn compose_session_mcp_servers(
    mut user_servers: Value,
    grant: Option<&ImportHistoryGrant>,
    policy: SessionMcpPolicy,
) -> Value {
    let Some(arr) = user_servers.as_array_mut() else {
        return user_servers;
    };
    arr.retain(|v| v.get("name").and_then(|n| n.as_str()) != Some("import-history"));
    if !policy.allows_local_import_mcp() {
        return user_servers;
    }
    if let Some(grant) = grant {
        if let Ok(entry) = mcp_server_entry(grant) {
            arr.push(entry);
        }
    }
    user_servers
}

pub fn mcp_server_entry(grant: &ImportHistoryGrant) -> Result<Value, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(json!({
        "name": "import-history",
        "command": exe.to_string_lossy(),
        "args": [super::IMPORT_HISTORY_MCP_FLAG],
        "env": [
            // Pass only a bounded launch capability. Large record selections stay
            // in the protected continuation sidecar, not the Windows environment.
            {"name": GRANT_ENV, "value": json!({"appSessionId": grant.app_session_id, "token": grant.token}).to_string()},
            {"name": "GROK_APP_HOME", "value": crate::paths::app_data_root().to_string_lossy()}
        ]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("mcp_stdio_tests.rs");
}
