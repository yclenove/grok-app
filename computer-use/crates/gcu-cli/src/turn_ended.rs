//! Nonfatal Grok Stop / Codex notify hook. Payloads never enter diagnostics.
use crate::{exit::Failure, private, resume};
use std::{ffi::OsString, path::PathBuf, time::Duration};
use tokio::io::AsyncReadExt;

const MAX_INPUT: usize = 1024 * 1024;
fn diagnostic(message: &str) {
    eprintln!("gcu turn-ended: {message}");
}
fn chain_diagnostic(previous_failed: bool, message: &str) {
    if previous_failed {
        diagnostic(&format!(
            "cannot execute previous notify command; {message}"
        ));
    } else {
        diagnostic(message);
    }
}

/// Hooks bypass ordinary clap/environment gates; their errors are nonfatal.
pub(crate) fn entry(args: Vec<OsString>) -> ! {
    std::panic::set_hook(Box::new(|_| diagnostic("internal hook failure")));
    let result = std::panic::catch_unwind(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        let Ok(runtime) = runtime else {
            diagnostic("cannot start hook runtime");
            return None;
        };
        let status = runtime.block_on(run(args));
        runtime.shutdown_timeout(Duration::from_secs(1));
        status
    });
    if let Ok(Some(status)) = result {
        private::unsafe_platform::exit_like(status);
    }
    std::process::exit(0)
}

pub(crate) async fn run(mut args: Vec<OsString>) -> Option<std::process::ExitStatus> {
    if args == [OsString::from("--help")] || args == [OsString::from("-h")] {
        use clap::CommandFactory;
        if let Err(error) =
            crate::args::Cli::command().try_get_matches_from_mut(["gcu", "turn-ended", "--help"])
        {
            let _ = error.print();
        }
        return None;
    }
    let mut selected = None;
    if args.first().is_some_and(|arg| arg == "--session") {
        if args.len() < 2 {
            diagnostic("--session requires a private directory");
            return None;
        }
        selected = Some(PathBuf::from(args.remove(1)));
        args.remove(0);
    }
    let mut previous_status = None;
    let mut previous_failed = false;
    let payload = if args.first().is_some_and(|arg| arg == "--previous-notify") {
        args.remove(0);
        if args.is_empty() {
            diagnostic("notify chain requires a command and final JSON argument");
            return None;
        }
        // Codex appends one JSON argument. Forward original argv and inherited
        // stdin/stdout/stderr FIRST; no shell parsing or rewriting of arguments.
        // A configured empty notify array has no previous program to invoke.
        if args.len() > 1 {
            match tokio::process::Command::new(&args[0])
                .args(&args[1..])
                .status()
                .await
            {
                Ok(status) => previous_status = Some(status),
                Err(_) => previous_failed = true,
            }
        }
        args.pop()
            .and_then(|arg| arg.into_string().ok())
            .map(String::into_bytes)
    } else if args.len() == 1 && !args[0].to_string_lossy().starts_with("--") {
        args.pop()
            .and_then(|arg| arg.into_string().ok())
            .map(String::into_bytes)
    } else if args.is_empty() {
        let input = tokio::time::timeout(Duration::from_secs(5), async {
            let mut input = Vec::new();
            tokio::io::stdin()
                .take((MAX_INPUT + 1) as u64)
                .read_to_end(&mut input)
                .await?;
            Ok::<_, std::io::Error>(input)
        })
        .await;
        match input {
            Ok(Ok(input)) => Some(input),
            _ => {
                diagnostic("cannot read hook stdin (five-second deadline)");
                return previous_status;
            }
        }
    } else {
        diagnostic("expected stdin JSON or one notify JSON argument");
        return previous_status;
    };
    let Some(payload) = payload else {
        chain_diagnostic(previous_failed, "hook JSON argument must be UTF-8");
        return previous_status;
    };
    match notify(selected, &payload).await {
        Err(error) => chain_diagnostic(previous_failed, &error.message),
        Ok(()) if previous_failed => diagnostic("cannot execute previous notify command"),
        Ok(()) => (),
    }
    previous_status
}

async fn notify(selected: Option<PathBuf>, payload: &[u8]) -> Result<(), Failure> {
    if payload.len() > MAX_INPUT {
        return Err(Failure::arguments("hook JSON exceeds one MiB"));
    }
    let value: serde_json::Value =
        serde_json::from_slice(payload).map_err(|_| Failure::arguments("malformed hook JSON"))?;
    let object = value
        .as_object()
        .ok_or_else(|| Failure::arguments("hook JSON must be an object"))?;
    let event = if object.contains_key("hookEventName") {
        let name = value["hookEventName"]
            .as_str()
            .ok_or_else(|| Failure::arguments("invalid hookEventName"))?;
        let reason = value["reason"]
            .as_str()
            .ok_or_else(|| Failure::arguments("invalid hook reason"))?;
        if name != "stop" || reason != "end_turn" {
            return Ok(());
        }
        None
    } else if object.contains_key("type") {
        let kind = value["type"]
            .as_str()
            .ok_or_else(|| Failure::arguments("invalid notify type"))?;
        if kind != "agent-turn-complete" {
            return Ok(());
        }
        match (value.get("thread-id"), value.get("turn-id")) {
            (Some(thread), Some(turn)) if thread.is_string() && turn.is_string() => Some(
                serde_json::to_string(&(thread, turn))
                    .map_err(|_| Failure::internal("cannot encode turn identifier"))?,
            ),
            (None, None) => None,
            _ => return Err(Failure::arguments("invalid notify thread/turn identifier")),
        }
    } else {
        return Err(Failure::arguments(
            "unrecognized hook event (expected Grok Stop or Codex notify)",
        ));
    };
    if event.as_ref().is_some_and(|id| id.len() > 128) {
        return Err(Failure::arguments(
            "notify turn identifier exceeds 128 bytes",
        ));
    }
    let directory = private::work(move || resume::discover(selected)).await?;
    let token = resume::read_token(&directory).await?;
    resume::send(
        &directory,
        &format!("turn-ended {token} {}", event.unwrap_or_default()),
    )
    .await
}

#[cfg(test)]
#[path = "turn_ended_tests.rs"]
mod tests;
