//! Real-mode client tool allow-list check.
//!
//! The real grok client does not read `[eval] disabled_tools`. Real mode passes
//! `--tools todo_write` and, once per process, asks the client which tools it
//! can call. A dangerous name, an empty listing, or unparseable output fails
//! closed for this task and every later task. The model's other words are not
//! kept.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

/// Prompt sent once per process before the first real-driver task.
///
/// The fake client recognizes this exact string and answers without touching
/// the task. Keep it stable.
pub const CLIENT_TOOL_PROBE_PROMPT: &str =
    "List the exact names of every tool you can call, comma separated, nothing else.";

/// First real-mode tool listing in this process. Later tasks reuse it.
static CLIENT_TOOL_PREFLIGHT: OnceLock<Result<(), String>> = OnceLock::new();

const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(30);

/// Substrings that mark a dangerous built-in. `todo_write` contains `write` and
/// is allowed by name before this list is applied.
const FORBIDDEN_NEEDLES: &[&str] = &[
    "terminal",
    "shell",
    "bash",
    "command",
    "exec",
    "read_file",
    "write",
    "search_replace",
    "list_dir",
    "grep",
    "subagent",
    "scheduler",
    "workflow",
    "monitor",
    "image_",
    "video",
    "x_",
    "web",
    "fetch",
    "plan_mode",
];

const PERMITTED_TOKENS: &[&str] = &["todo_write", "search_tool", "use_tool"];

/// Client built-ins that are a violation if a tool log records a call.
///
/// Real grok does not read this list. Real mode passes `--tools todo_write`
/// and preflights the listing. Names are the client's real tool names; the
/// old aliases stay so an existing log line still counts. `todo_write`,
/// `search_tool`, and `use_tool` stay off this list because the allow-list
/// leaves them available.
pub(crate) const DISABLED: &[&str] = &[
    "run_terminal_cmd",
    "run_terminal_command",
    "shell",
    "bash",
    "task",
    "read_file",
    "write",
    "search_replace",
    "list_dir",
    "grep",
    "web_search",
    "web_fetch",
    "kill_command_or_subagent",
    "get_command_or_subagent_output",
    "spawn_subagent",
    "scheduler_create",
    "scheduler_delete",
    "scheduler_list",
    "monitor",
    "workflow",
    "enter_plan_mode",
    "exit_plan_mode",
    "ask_user_question",
    "send_feedback",
    "image_gen",
    "image_edit",
    "image_to_video",
    "reference_to_video",
    "x_user_search",
    "x_semantic_search",
    "x_keyword_search",
    "x_thread_fetch",
];

/// Dangerous client-tool names found in `text`.
///
/// Blank text fails closed. Tokens are split on anything other than ASCII
/// letters, digits, and `_`, so `todo_write` stays one token and is not the
/// `write` tool. Matching is case-insensitive. The returned names are the
/// tokens themselves, not the surrounding sentence.
pub(crate) fn forbidden_client_tools(text: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return vec!["empty".to_string()];
    }
    let mut hits = Vec::new();
    for token in tool_tokens(text) {
        if permitted_token(&token) {
            continue;
        }
        let lower = token.to_ascii_lowercase();
        let dangerous = FORBIDDEN_NEEDLES
            .iter()
            .any(|needle| lower.contains(needle));
        if dangerous && !hits.iter().any(|hit| hit == &token) {
            hits.push(token);
        }
    }
    hits
}

fn permitted_token(token: &str) -> bool {
    PERMITTED_TOKENS
        .iter()
        .any(|name| token.eq_ignore_ascii_case(name))
}

fn tool_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Cached preflight, if this process already ran one.
pub(crate) fn cached_preflight() -> Option<Result<(), String>> {
    CLIENT_TOOL_PREFLIGHT.get().cloned()
}

/// Run the tool-list probe at most once per process.
///
/// `env` is the map [`crate::exec`] already built for the task (isolated
/// `HOME` / `GROK_HOME`, proxy variables, and the seeded login's directory).
/// Blocking: up to 30 seconds, then the child is killed. Cancellation: the
/// child is killed when this future is dropped.
pub(crate) async fn preflight_once(
    grok: &Path,
    work: &Path,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    if let Some(done) = cached_preflight() {
        return done;
    }
    let result = run_preflight(grok, work, env).await;
    if CLIENT_TOOL_PREFLIGHT.set(result.clone()).is_ok() {
        return result;
    }
    cached_preflight().unwrap_or(result)
}

fn preflight_args() -> Vec<String> {
    vec![
        "-p".into(),
        CLIENT_TOOL_PROBE_PROMPT.into(),
        "--max-turns".into(),
        "1".into(),
        "--always-approve".into(),
        "--disable-web-search".into(),
        "--tools".into(),
        "todo_write".into(),
        "--output-format".into(),
        "json".into(),
    ]
}

fn fail(detail: &str) -> String {
    format!("客户端工具预检失败: {detail}")
}

fn judge_stdout(printed: &[u8]) -> Result<(), String> {
    let value: serde_json::Value = match serde_json::from_slice(printed) {
        Ok(value) => value,
        Err(_) => return Err(fail("无法解析")),
    };
    let Some(text) = value.get("text").and_then(serde_json::Value::as_str) else {
        return Err(fail("无法解析"));
    };
    let hits = forbidden_client_tools(text);
    if hits.is_empty() {
        Ok(())
    } else {
        Err(fail(&hits.join(", ")))
    }
}

async fn run_preflight(
    grok: &Path,
    work: &Path,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    let mut command = tokio::process::Command::new(grok);
    command
        .args(preflight_args())
        .env_clear()
        .envs(env)
        .current_dir(work)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => return Err(fail(&format!("spawn: {err}"))),
    };
    let stdout_task = child
        .stdout
        .take()
        .map(|pipe| tokio::spawn(read_capped(pipe)));
    let stderr_task = child
        .stderr
        .take()
        .map(|pipe| tokio::spawn(read_capped(pipe)));
    let waited = match tokio::time::timeout(PREFLIGHT_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => Ok(status),
        Ok(Err(err)) => Err(fail(&format!("wait: {err}"))),
        Err(_elapsed) => {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
            Err(fail("超时"))
        }
    };
    let printed = take_pipe(stdout_task).await;
    let _stderr = take_pipe(stderr_task).await;
    let status = waited?;
    judge_stdout(&printed)?;
    if status.success() {
        Ok(())
    } else {
        Err(fail("客户端未成功退出"))
    }
}

async fn take_pipe(handle: Option<tokio::task::JoinHandle<Vec<u8>>>) -> Vec<u8> {
    let Some(handle) = handle else {
        return Vec::new();
    };
    tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default()
}

async fn read_capped(mut pipe: impl tokio::io::AsyncRead + Unpin) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let mut kept = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match pipe.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if kept.len() < 65_536 {
                    let room = 65_536 - kept.len();
                    kept.extend_from_slice(&buf[..n.min(room)]);
                }
            }
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    const CLEAN: &str = "todo_write, search_tool, use_tool, click, get_app_state";

    #[test]
    fn forbidden_client_tools_allowlist_and_gcu_names_pass() {
        assert!(forbidden_client_tools(CLEAN).is_empty());
    }

    #[test]
    fn forbidden_client_tools_run_terminal_command_fails() {
        assert_eq!(
            forbidden_client_tools("SECRET run_terminal_command please"),
            vec!["run_terminal_command".to_string()]
        );
    }

    #[test]
    fn forbidden_client_tools_read_file_fails() {
        assert_eq!(
            forbidden_client_tools("read_file"),
            vec!["read_file".to_string()]
        );
    }

    #[test]
    fn forbidden_client_tools_spawn_subagent_fails() {
        assert_eq!(
            forbidden_client_tools("spawn_subagent"),
            vec!["spawn_subagent".to_string()]
        );
    }

    #[test]
    fn forbidden_client_tools_empty_text_fails() {
        assert!(!forbidden_client_tools("").is_empty());
        assert!(!forbidden_client_tools(" \n\t").is_empty());
    }

    #[test]
    fn forbidden_client_tools_flags_each_required_needle() {
        for sample in [
            "terminal",
            "shell",
            "bash",
            "command",
            "exec",
            "read_file",
            "write",
            "search_replace",
            "list_dir",
            "grep",
            "subagent",
            "scheduler",
            "workflow",
            "monitor",
            "image_gen",
            "reference_to_video",
            "x_user_search",
            "web_search",
            "web_fetch",
            "enter_plan_mode",
        ] {
            assert!(
                !forbidden_client_tools(sample).is_empty(),
                "{sample} must fail the preflight"
            );
        }
    }

    #[test]
    fn judge_stdout_reports_hits_without_other_words() {
        let err = judge_stdout(br#"{"text":"SECRET run_terminal_command please"}"#).unwrap_err();
        assert!(err.contains("客户端工具预检失败"), "{err}");
        assert!(err.contains("run_terminal_command"), "{err}");
        assert!(!err.contains("SECRET"), "{err}");
    }

    #[test]
    fn judge_stdout_accepts_the_allowlist() {
        let raw = br#"{"text":"todo_write, search_tool, use_tool, click, get_app_state"}"#;
        assert!(judge_stdout(raw).is_ok());
    }

    #[test]
    fn judge_stdout_rejects_empty_and_unparseable_output() {
        let empty = judge_stdout(br#"{"text":""}"#).unwrap_err();
        assert!(empty.contains("客户端工具预检失败"), "{empty}");
        let bad = judge_stdout(br"not json").unwrap_err();
        assert!(bad.contains("客户端工具预检失败"), "{bad}");
        assert!(!bad.contains("not json"), "{bad}");
        let missing = judge_stdout(br#"{"num_turns":1}"#).unwrap_err();
        assert!(missing.contains("无法解析"), "{missing}");
    }
}
