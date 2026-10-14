//! Stand-in for `gcu call` and `gcu mcp`. It logs the tool name and exits.
//!
//! The real `gcu mcp` server waits for stdin EOF. This binary does not read
//! stdin, so an evaluation client cannot hang on it.

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("eval-fake-gcu: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let tool = tool_name(&args);
    let path = std::env::var("GCU_EVAL_TOOL_LOG").map_err(|err| err.to_string())?;
    let line = serde_json::json!({"tool": tool, "result": "ok"});
    let text = serde_json::to_string(&line).map_err(|err| err.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| err.to_string())?;
    writeln!(file, "{text}").map_err(|err| err.to_string())
}

fn tool_name(args: &[String]) -> String {
    if let Some(index) = args.iter().position(|arg| arg == "call") {
        return args
            .get(index + 1)
            .cloned()
            .unwrap_or_else(|| "call".into());
    }
    if args.iter().any(|arg| arg == "mcp") {
        return "mcp".into();
    }
    args.first().cloned().unwrap_or_else(|| "gcu".into())
}
