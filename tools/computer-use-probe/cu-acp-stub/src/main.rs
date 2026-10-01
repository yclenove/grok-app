//! Loopback ACP stdio stub for isolated App-shell E3.
//! Ignores grok CLI flags; speaks newline JSON-RPC on stdin/stdout.

use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};

fn record_mcp_catalog(message: &serde_json::Value) {
    let Ok(path) = std::env::var("GROK_CU_ACP_STUB_LEDGER") else {
        return;
    };
    let Some(servers) = message
        .pointer("/params/mcpServers")
        .and_then(serde_json::Value::as_array)
    else {
        return;
    };
    let cu_count = servers
        .iter()
        .filter(|server| {
            server.get("name").and_then(serde_json::Value::as_str)
                == Some("grok-computer-use")
        })
        .count();
    let row = serde_json::json!({
        "event": "mcp_catalog",
        "sessionId": message.pointer("/params/sessionId").and_then(serde_json::Value::as_str),
        "totalCount": servers.len(),
        "cuCount": cu_count,
        "pid": std::process::id(),
    });
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{row}");
    }
}

fn main() {
    if std::env::args().any(|a| a == "--version") {
        println!("grok 1.0.0");
        return;
    }
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut session_id = String::from("agent-cu-d6-1");
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if msg.get("method").is_none() {
            continue;
        }
        let Some(id) = msg.get("id").cloned() else {
            continue;
        };
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let result = match method {
            "initialize" => serde_json::json!({
                "protocolVersion": 1,
                "agentCapabilities": {
                    "loadSession": true,
                    "promptCapabilities": {
                        "image": false,
                        "audio": false,
                        "embeddedContext": false
                    }
                },
                "_meta": { "agentVersion": "1.0.0-cu-stub" }
            }),
            "authenticate" => serde_json::json!({}),
            "session/new" | "session/load" => {
                if let Some(sid) = msg
                    .pointer("/params/sessionId")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    session_id = sid.to_string();
                }
                serde_json::json!({ "sessionId": session_id })
            }
            "session/prompt" => serde_json::json!({ "stopReason": "end_turn" }),
            "session/set_model" | "session/set_mode" | "session/cancel" => {
                serde_json::json!({})
            }
            "_x.ai/session/update_mcp_servers" => {
                record_mcp_catalog(&msg);
                serde_json::json!({ "ok": true })
            }
            _ => serde_json::json!({}),
        };
        let reply = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        });
        if writeln!(stdout, "{reply}").is_err() {
            break;
        }
        let _ = stdout.flush();
    }
}
