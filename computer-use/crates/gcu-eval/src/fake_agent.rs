//! Scripted stand-in for the Grok CLI. It never calls a model API.
//!
//! Behavior comes from `GCU_EVAL_BEHAVIOR`. The process writes the fixture
//! state, a transcript, and a probe, then calls the in-process fake backend.
//! Any supplied `GCU_BIN` is refused by the same decision as the runner,
//! including copied siblings and genuine offline executables.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

/// Run the built-in fake agent and return a process status code.
#[must_use]
pub fn builtin_fake_agent_entry() -> u8 {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("eval-fake-agent: {err}");
            1
        }
    }
}

fn run() -> Result<u8, String> {
    let external = std::env::var_os("GCU_BIN").map(std::path::PathBuf::from);
    crate::boundary::admit_fake_backend(external.as_deref(), false).map_err(str::to_string)?;
    if tool_probe() {
        let body = serde_json::json!({ "tools": crate::EXPECTED_PREFLIGHT_TOOLS });
        println!(
            "{}",
            serde_json::to_string(&body).map_err(|err| err.to_string())?
        );
        return Ok(0);
    }
    // Prove this process covered the invocation before any tool or shim line.
    ensure_handshake("GCU_EVAL_TOOL_LOG")?;
    ensure_handshake("GCU_EVAL_SIDE_LOG")?;
    let behavior = std::env::var("GCU_EVAL_BEHAVIOR").unwrap_or_else(|_| "scripted".into());
    write_probe()?;
    match behavior.as_str() {
        "timeout" => {
            std::thread::sleep(std::time::Duration::from_secs(30));
            Ok(0)
        }
        "crash" => {
            invoke(&["call", "list_apps", "--args", "{}"])?;
            Ok(101)
        }
        "fail" => {
            write_state(br#"{"form":{"name":"nope","agree":false,"submitted_count":0}}"#)?;
            write_transcript(true)?;
            Ok(0)
        }
        "partial" => {
            write_state(
                r#"{"form":{"name":"张三","agree":false,"submitted_count":0}}"#.as_bytes(),
            )?;
            write_transcript(true)?;
            Ok(0)
        }
        "side_effect" => {
            write_pass_state()?;
            if let Ok(path) = std::env::var("GCU_EVAL_WATCH_FILE") {
                std::fs::write(path, b"pwned-secret").map_err(|err| err.to_string())?;
            }
            append_side("process curl\nnetwork example.invalid\n")?;
            invoke_suite()?;
            write_transcript(true)?;
            Ok(0)
        }
        "unknown" => {
            write_pass_state()?;
            invoke_suite()?;
            append_tool(r#"{"tool":"not_a_tool","result":"ok"}"#)?;
            append_tool(r#"{"tool":"read_file_SENSITIVEVALUE","result":"ok"}"#)?;
            append_tool(r#"{"tool":"click","result":"unknown"}"#)?;
            write_transcript(true)?;
            Ok(0)
        }
        "oversubmit" => {
            write_state(r#"{"form":{"name":"张三","agree":true,"submitted_count":2}}"#.as_bytes())?;
            write_transcript(true)?;
            Ok(0)
        }
        "disabled" => {
            write_pass_state()?;
            append_tool(r#"{"tool":"shell","result":"ok"}"#)?;
            write_transcript(true)?;
            Ok(0)
        }
        "omit_tokens" => {
            write_pass_state()?;
            invoke_suite()?;
            write_transcript_body(true, false)?;
            Ok(0)
        }
        // Grandchild stays in this process group. The runner must kill the
        // group on timeout; killing only this process leaves `sleep` behind.
        "spawn_sleeper" => {
            let marker = std::env::var("GCU_EVAL_WATCH_FILE").map_err(|err| err.to_string())?;
            let child = std::process::Command::new("/bin/sleep")
                .arg("300")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|err| err.to_string())?;
            std::fs::write(&marker, child.id().to_string()).map_err(|err| err.to_string())?;
            std::mem::forget(child);
            std::thread::sleep(std::time::Duration::from_secs(300));
            Ok(0)
        }
        "pipe_flood" => {
            let chunk = vec![b'y'; 8192];
            let mut out = std::io::stdout();
            let mut err = std::io::stderr();
            for _ in 0..128 {
                out.write_all(&chunk).map_err(|err| err.to_string())?;
                err.write_all(&chunk).map_err(|err| err.to_string())?;
            }
            out.flush().map_err(|err| err.to_string())?;
            err.flush().map_err(|err| err.to_string())?;
            write_pass_state()?;
            write_transcript(true)?;
            Ok(0)
        }
        _ => {
            write_pass_state()?;
            invoke_suite()?;
            write_transcript(true)?;
            Ok(0)
        }
    }
}

fn tool_probe() -> bool {
    let mut args = std::env::args();
    while let Some(arg) = args.next() {
        if arg == "-p" {
            return args.next().as_deref() == Some(crate::CLIENT_TOOL_PROBE_PROMPT);
        }
    }
    false
}

fn invoke_suite() -> Result<(), String> {
    invoke(&["call", "list_apps", "--args", "{}"])?;
    invoke(&["call", "list_apps", "--args", "{}"])?;
    invoke(&["call", "get_app_state", "--args", "{}"])?;
    invoke(&["mcp", "--driver", "fake", "--scene", "form"])?;
    Ok(())
}

fn invoke(args: &[&str]) -> Result<(), String> {
    crate::fake_gcu::run(args)
}

fn write_pass_state() -> Result<(), String> {
    let path = std::env::var("GCU_EVAL_PASS_SCRIPT").map_err(|err| err.to_string())?;
    let bytes = std::fs::read(&path).map_err(|err| err.to_string())?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;
    let state = value
        .get("state")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let body = serde_json::to_vec(&state).map_err(|err| err.to_string())?;
    write_state(&body)
}

fn write_state(bytes: &[u8]) -> Result<(), String> {
    let path = std::env::var("GCU_FIXTURE_STATE").map_err(|err| err.to_string())?;
    std::fs::write(path, bytes).map_err(|err| err.to_string())
}

fn write_transcript(claimed_done: bool) -> Result<(), String> {
    write_transcript_body(claimed_done, true)
}

fn write_transcript_body(claimed_done: bool, tokens: bool) -> Result<(), String> {
    let home = std::env::var("GROK_HOME").map_err(|err| err.to_string())?;
    let mut body = serde_json::json!({
        "claimed_done": claimed_done,
        "images": 0,
        "image_bytes": 0,
        "extra_approvals": 0,
        "turns": 2
    });
    if tokens {
        body["tokens_in"] = serde_json::json!(12);
        body["tokens_out"] = serde_json::json!(4);
    }
    let bytes = serde_json::to_vec(&body).map_err(|err| err.to_string())?;
    std::fs::write(Path::new(&home).join("transcript.json"), bytes).map_err(|err| err.to_string())
}

fn ensure_handshake(key: &str) -> Result<(), String> {
    let Ok(path) = std::env::var(key) else {
        return Ok(());
    };
    if path.is_empty() {
        return Ok(());
    }
    match std::fs::read_to_string(&path) {
        Ok(text) if !text.is_empty() => Ok(()),
        _ => std::fs::write(path, format!("{}\n", crate::exec::TELEMETRY_HANDSHAKE))
            .map_err(|err| err.to_string()),
    }
}

fn append_side(text: &str) -> Result<(), String> {
    let path = std::env::var("GCU_EVAL_SIDE_LOG").map_err(|err| err.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| err.to_string())?;
    file.write_all(text.as_bytes())
        .map_err(|err| err.to_string())
}

fn append_tool(line: &str) -> Result<(), String> {
    let path = std::env::var("GCU_EVAL_TOOL_LOG").map_err(|err| err.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| err.to_string())?;
    writeln!(file, "{line}").map_err(|err| err.to_string())
}

fn write_probe() -> Result<(), String> {
    let Ok(path) = std::env::var("GCU_EVAL_PROBE") else {
        return Ok(());
    };
    let mut keys = Vec::new();
    for (key, _) in std::env::vars_os() {
        keys.push(key.to_string_lossy().into_owned());
    }
    keys.sort();
    let argv: Vec<String> = std::env::args_os()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let config = config_text();
    let body = serde_json::json!({
        "argv": argv,
        "env_keys": keys,
        "grok_home": env_text("GROK_HOME"),
        "home": env_text("HOME"),
        "path": env_text("PATH"),
        "approval": env_text("GCU_APPROVAL"),
        "mcp_profile": flag_value(&config, "--profile"),
        "arm": quoted_assign(&config, "arm"),
        "screenshot": quoted_assign(&config, "screenshot"),
        "diff": quoted_assign(&config, "diff"),
        "batch": bool_assign(&config, "batch"),
        "script": bool_assign(&config, "script"),
    });
    let bytes = serde_json::to_vec(&body).map_err(|err| err.to_string())?;
    std::fs::write(path, bytes).map_err(|err| err.to_string())
}

fn env_text(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

fn config_text() -> String {
    let Ok(home) = std::env::var("GROK_HOME") else {
        return String::new();
    };
    std::fs::read_to_string(Path::new(&home).join("config.toml")).unwrap_or_default()
}

fn flag_value(text: &str, flag: &str) -> String {
    let needle = format!("\"{flag}\", \"");
    let Some(start) = text.find(&needle) else {
        return String::new();
    };
    take_quoted(&text[start + needle.len()..])
}

fn quoted_assign(text: &str, key: &str) -> String {
    let needle = format!("\n{key} = \"");
    let Some(start) = text.find(&needle) else {
        return String::new();
    };
    take_quoted(&text[start + needle.len()..])
}

fn take_quoted(rest: &str) -> String {
    let mut out = String::new();
    let mut escaped = false;
    for ch in rest.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            break;
        }
        out.push(ch);
    }
    out
}

fn bool_assign(text: &str, key: &str) -> Option<bool> {
    let needle = format!("\n{key} = ");
    let start = text.find(&needle)? + needle.len();
    let rest = text[start..].trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// Process status adapter for the `eval-fake-agent` binary.
#[must_use]
pub fn builtin_fake_agent_exit() -> ExitCode {
    ExitCode::from(builtin_fake_agent_entry())
}
