//! Stand-in for `gcu call` and `gcu mcp`. It logs the tool name and exits.
//!
//! The real `gcu mcp` server waits for stdin EOF. This binary does not read
//! stdin, so an evaluation client cannot hang on it.

use std::io::Write;
use std::process::ExitCode;

/// Run the built-in fake gcu and return a process status code.
#[must_use]
pub fn builtin_fake_gcu_entry() -> u8 {
    match run() {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("eval-fake-gcu: {err}");
            1
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let tool = tool_name(&args);
    let path = std::env::var("GCU_EVAL_TOOL_LOG").map_err(|err| err.to_string())?;
    let line = serde_json::json!({"tool": tool, "result": "ok"});
    let text = serde_json::to_string(&line).map_err(|err| err.to_string())?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| err.to_string())?;
    if existing.is_empty() {
        writeln!(file, "{}", crate::exec::TELEMETRY_HANDSHAKE).map_err(|err| err.to_string())?;
    }
    writeln!(file, "{text}").map_err(|err| err.to_string())
}

fn tool_name(args: &[String]) -> String {
    let args: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|arg| *arg != crate::INTERNAL_FAKE_GCU)
        .collect();
    if let Some(index) = args.iter().position(|arg| *arg == "call") {
        return args.get(index + 1).copied().unwrap_or("call").to_string();
    }
    if args.contains(&"mcp") {
        return "mcp".into();
    }
    args.first().copied().unwrap_or("gcu").to_string()
}

/// Process status adapter for the `eval-fake-gcu` binary.
#[must_use]
pub fn builtin_fake_gcu_exit() -> ExitCode {
    ExitCode::from(builtin_fake_gcu_entry())
}
