//! Stand-in for `gcu call` and `gcu mcp`. It logs the tool name and exits.
//!
//! The real `gcu mcp` server waits for stdin EOF. This binary does not read
//! stdin, so an evaluation client cannot hang on it.

use std::process::ExitCode;

/// Run the built-in fake gcu and return a process status code.
#[must_use]
pub fn builtin_fake_gcu_entry() -> u8 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let Some(log) = std::env::var_os("GCU_EVAL_TOOL_LOG") else {
        eprintln!("eval-fake-gcu: missing tool log");
        return 1;
    };
    match run(&args, std::path::Path::new(&log)) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("eval-fake-gcu: {err}");
            1
        }
    }
}

pub(crate) fn run(args: &[&str], path: &std::path::Path) -> Result<(), String> {
    let tool = tool_name(args);
    let line = serde_json::json!({"tool": tool, "result": "ok"});
    let text = serde_json::to_string(&line).map_err(|err| err.to_string())?;
    let existing = crate::fake_io::read_optional(path)?;
    let mut body = String::new();
    if existing.as_ref().is_none_or(|bytes| bytes.is_empty()) {
        body.push_str(&format!("{}\n", crate::exec::TELEMETRY_HANDSHAKE));
    }
    body.push_str(&text);
    body.push('\n');
    crate::fake_io::append(path, body.as_bytes())
}

fn tool_name(args: &[&str]) -> String {
    let args: Vec<&str> = args
        .iter()
        .copied()
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
