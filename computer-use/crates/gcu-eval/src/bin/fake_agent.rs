//! Scripted stand-in for the Grok CLI. It never calls a model API.
//!
//! Behavior comes from `GCU_EVAL_BEHAVIOR`. The process writes the fixture
//! state, a transcript, and a probe, then optionally invokes `GCU_BIN`.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("eval-fake-agent: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let behavior = std::env::var("GCU_EVAL_BEHAVIOR").unwrap_or_else(|_| "scripted".into());
    write_probe()?;
    match behavior.as_str() {
        "timeout" => {
            std::thread::sleep(std::time::Duration::from_secs(30));
            Ok(ExitCode::SUCCESS)
        }
        "crash" => {
            invoke(&["call", "list_apps", "--args", "{}"])?;
            Ok(ExitCode::from(101))
        }
        "fail" => {
            write_state(br#"{"form":{"name":"nope","agree":false,"submitted_count":0}}"#)?;
            write_transcript(true)?;
            Ok(ExitCode::SUCCESS)
        }
        "partial" => {
            write_state(
                r#"{"form":{"name":"张三","agree":false,"submitted_count":0}}"#.as_bytes(),
            )?;
            write_transcript(true)?;
            Ok(ExitCode::SUCCESS)
        }
        "side_effect" => {
            write_pass_state()?;
            if let Ok(path) = std::env::var("GCU_EVAL_WATCH_FILE") {
                std::fs::write(path, b"pwned-secret").map_err(|err| err.to_string())?;
            }
            append_side("process curl\nnetwork example.invalid\n")?;
            invoke_suite()?;
            write_transcript(true)?;
            Ok(ExitCode::SUCCESS)
        }
        "unknown" => {
            write_pass_state()?;
            invoke_suite()?;
            append_tool(r#"{"tool":"not_a_tool","result":"ok"}"#)?;
            append_tool(r#"{"tool":"click","result":"unknown"}"#)?;
            write_transcript(true)?;
            Ok(ExitCode::SUCCESS)
        }
        "oversubmit" => {
            write_state(r#"{"form":{"name":"张三","agree":true,"submitted_count":2}}"#.as_bytes())?;
            write_transcript(true)?;
            Ok(ExitCode::SUCCESS)
        }
        "disabled" => {
            write_pass_state()?;
            append_tool(r#"{"tool":"shell","result":"ok"}"#)?;
            write_transcript(true)?;
            Ok(ExitCode::SUCCESS)
        }
        _ => {
            write_pass_state()?;
            invoke_suite()?;
            write_transcript(true)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn invoke_suite() -> Result<(), String> {
    invoke(&["call", "list_apps", "--args", "{}"])?;
    invoke(&["call", "list_apps", "--args", "{}"])?;
    invoke(&["call", "get_app_state", "--args", "{}"])?;
    invoke(&["mcp", "--driver", "fake", "--scene", "form"])?;
    Ok(())
}

fn invoke(args: &[&str]) -> Result<(), String> {
    let bin = std::env::var("GCU_BIN").map_err(|_| "GCU_BIN missing".to_string())?;
    if bin.is_empty() {
        return Err("GCU_BIN missing".into());
    }
    let status = std::process::Command::new(bin)
        .args(args)
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("gcu status {status}"))
    }
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
    let home = std::env::var("GROK_HOME").map_err(|err| err.to_string())?;
    let body = serde_json::json!({
        "claimed_done": claimed_done,
        "tokens_in": 12,
        "tokens_out": 4,
        "images": 0,
        "image_bytes": 0,
        "extra_approvals": 0,
        "turns": 2
    });
    let bytes = serde_json::to_vec(&body).map_err(|err| err.to_string())?;
    std::fs::write(Path::new(&home).join("transcript.json"), bytes).map_err(|err| err.to_string())
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
    let body = serde_json::json!({
        "argv": argv,
        "env_keys": keys,
        "grok_home": env_text("GROK_HOME"),
        "home": env_text("HOME"),
        "path": env_text("PATH"),
        "approval": env_text("GCU_APPROVAL"),
    });
    let bytes = serde_json::to_vec(&body).map_err(|err| err.to_string())?;
    std::fs::write(path, bytes).map_err(|err| err.to_string())
}

fn env_text(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}
