//! Real-client tool allow-list check.
//!
//! The real grok client does not read `[eval] disabled_tools`. A real client
//! passes `--tools todo_write` and is asked which tools it can call. The
//! listing must be JSON whose tool names are exactly [`EXPECTED_PREFLIGHT_TOOLS`].
//! This is a tripwire, not a proof that the client is confined. Failure reasons
//! are canonical labels from this module. The model's other words are not kept.
//!
//! A success or failure is cached per client path, MCP profile, and model.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// Prompt sent before the first real-client task for a cache key.
///
/// The fake client recognizes this exact string and answers without touching
/// the task. Keep it stable.
pub const CLIENT_TOOL_PROBE_PROMPT: &str =
    "List the exact names of every tool you can call, comma separated, nothing else.";

/// Tool names the preflight tripwire accepts, and no others.
///
/// `list_apps` and `list_windows` are the discovery entry: they are
/// `gcu_protocol::ToolName::ListApps` and `ListWindows`, and the first two
/// entries of the minimal MCP profile. `click` and `get_app_state` are also
/// exposed by that profile and are intentionally absent here. `todo_write`,
/// `search_tool`, and `use_tool` are the client built-ins `--tools todo_write`
/// leaves available. Matching this set does not prove the client cannot call
/// anything else.
pub const EXPECTED_PREFLIGHT_TOOLS: &[&str] = &[
    "list_apps",
    "list_windows",
    "search_tool",
    "todo_write",
    "use_tool",
];

/// Preflight result for one client path, profile, and model.
static PREFLIGHT_CACHE: Mutex<BTreeMap<PreflightKey, Result<(), String>>> =
    Mutex::new(BTreeMap::new());

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PreflightKey {
    client: PathBuf,
    profile: String,
    model: String,
}

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

#[cfg(test)]
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
#[cfg(test)]
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

#[cfg(test)]
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

fn preflight_key(client: &Path, profile: &str, model: &str) -> PreflightKey {
    PreflightKey {
        client: client.to_path_buf(),
        profile: profile.to_string(),
        model: model.to_string(),
    }
}

fn lock_cache() -> std::sync::MutexGuard<'static, BTreeMap<PreflightKey, Result<(), String>>> {
    PREFLIGHT_CACHE
        .lock()
        .unwrap_or_else(|err| err.into_inner())
}

/// Cached preflight for this client, profile, and model.
pub(crate) fn cached_preflight(
    client: &Path,
    profile: &str,
    model: &str,
) -> Option<Result<(), String>> {
    lock_cache()
        .get(&preflight_key(client, profile, model))
        .cloned()
}

/// Run the tool-list probe unless this client, profile, and model were cached.
///
/// `env` is the map [`crate::exec`] already built for the task (isolated
/// `HOME` / `GROK_HOME`, proxy variables, and the seeded login's directory).
/// Blocking: up to 30 seconds, then the child process group is killed.
/// Cancellation: the child is killed when this future is dropped.
pub(crate) async fn preflight_once(
    grok: &Path,
    work: &Path,
    env: &BTreeMap<String, String>,
    model: &str,
    profile: &str,
) -> Result<(), String> {
    let key = preflight_key(grok, profile, model);
    if let Some(done) = lock_cache().get(&key).cloned() {
        return done;
    }
    let result = run_preflight(grok, work, env, model).await;
    lock_cache().insert(key, result.clone());
    result
}

/// Stdout cap. One more byte, even after a valid prefix, is `output_overflow`.
const PREFLIGHT_STDOUT_LIMIT: usize = 65_536;

fn preflight_args(model: &str) -> Vec<String> {
    vec![
        "-p".into(),
        CLIENT_TOOL_PROBE_PROMPT.into(),
        "--max-turns".into(),
        "1".into(),
        "--always-approve".into(),
        "--model".into(),
        model.into(),
        "--disable-web-search".into(),
        "--tools".into(),
        "todo_write".into(),
        "--no-plan".into(),
        "--verbatim".into(),
        "--output-format".into(),
        "json".into(),
    ]
}

#[cfg(test)]
fn judge_stdout(printed: &[u8]) -> Result<(), String> {
    decide_preflight(printed, false)
}

fn decide_preflight(printed: &[u8], overflow: bool) -> Result<(), String> {
    if overflow {
        return Err("output_overflow".into());
    }
    let value: serde_json::Value = match serde_json::from_slice(printed) {
        Ok(value) => value,
        Err(_) => return Err("unparseable".into()),
    };
    let names = match tool_names(&value) {
        Some(names) => names,
        None => return Err("unparseable".into()),
    };
    if names.is_empty() {
        return Err("empty".into());
    }
    let got: BTreeSet<&str> = names.iter().map(String::as_str).collect();
    let expected: BTreeSet<&str> = EXPECTED_PREFLIGHT_TOOLS.iter().copied().collect();
    if got == expected {
        return Ok(());
    }
    if let Some(label) = forbidden_label(&names) {
        return Err(label);
    }
    Err("tool_set_mismatch".into())
}

fn tool_names(value: &serde_json::Value) -> Option<Vec<String>> {
    if let Some(tools) = value.get("tools") {
        let array = tools.as_array()?;
        let mut names = Vec::with_capacity(array.len());
        for item in array {
            names.push(item.as_str()?.to_string());
        }
        return Some(names);
    }
    let text = value.get("text").and_then(serde_json::Value::as_str)?;
    if text.trim().is_empty() {
        return Some(Vec::new());
    }
    Some(tool_tokens(text))
}

/// Canonical `forbidden_tool:` label. The returned text never contains `names`.
fn forbidden_label(names: &[String]) -> Option<String> {
    for item in DISABLED {
        if names
            .iter()
            .any(|name| !is_expected(name) && name.eq_ignore_ascii_case(item))
        {
            return Some(format!("forbidden_tool:{item}"));
        }
    }
    for needle in FORBIDDEN_NEEDLES {
        if names
            .iter()
            .any(|name| !is_expected(name) && name.to_ascii_lowercase().contains(needle))
        {
            return Some(format!("forbidden_tool:{needle}"));
        }
    }
    None
}

fn is_expected(name: &str) -> bool {
    EXPECTED_PREFLIGHT_TOOLS
        .iter()
        .any(|item| item.eq_ignore_ascii_case(name))
}

async fn run_preflight(
    grok: &Path,
    work: &Path,
    env: &BTreeMap<String, String>,
    model: &str,
) -> Result<(), String> {
    let mut command = tokio::process::Command::new(grok);
    command
        .args(preflight_args(model))
        .env_clear()
        .envs(env)
        .current_dir(work);
    let finished = match crate::proc::run_command(
        &mut command,
        PREFLIGHT_TIMEOUT,
        PREFLIGHT_STDOUT_LIMIT,
        PREFLIGHT_STDOUT_LIMIT,
    )
    .await
    {
        Ok(finished) => finished,
        Err(_) => return Err("spawn_failed".into()),
    };
    if finished.timed_out {
        return Err("timeout".into());
    }
    if finished.wait_failed {
        return Err("client_failed".into());
    }
    decide_preflight(&finished.stdout.bytes, finished.stdout.overflow)?;
    if finished.exit_code == Some(0) {
        Ok(())
    } else {
        Err("client_failed".into())
    }
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
        assert_eq!(err, "forbidden_tool:run_terminal_command");
        assert!(!err.contains("SECRET"), "{err}");
    }

    #[test]
    fn judge_stdout_accepts_the_exact_allowlist() {
        let raw = br#"{"text":"todo_write, search_tool, use_tool, list_apps, list_windows"}"#;
        assert!(judge_stdout(raw).is_ok());
        let tools =
            br#"{"tools":["use_tool","list_windows","todo_write","search_tool","list_apps"]}"#;
        assert!(judge_stdout(tools).is_ok());
    }

    #[test]
    fn judge_stdout_rejects_empty_and_unparseable_output() {
        assert_eq!(judge_stdout(br#"{"text":""}"#).unwrap_err(), "empty");
        let bad = judge_stdout(br"not json").unwrap_err();
        assert_eq!(bad, "unparseable");
        assert!(!bad.contains("not json"), "{bad}");
        assert_eq!(
            judge_stdout(br#"{"num_turns":1}"#).unwrap_err(),
            "unparseable"
        );
    }

    #[test]
    fn judge_stdout_shell_sensitive_value_is_a_canonical_label() {
        let err = judge_stdout(br#"{"text":"shell_SENSITIVEVALUE"}"#).unwrap_err();
        assert_eq!(err, "forbidden_tool:shell");
        assert!(
            !err.contains("SENSITIVEVALUE"),
            "model-written token leaked into the preflight reason: {err}"
        );
    }

    #[test]
    fn judge_stdout_overflow_fails_even_when_the_prefix_parses() {
        let mut body = exact_preflight_json(65_536);
        assert!(
            judge_stdout(&body).is_ok(),
            "the capped prefix is valid JSON"
        );
        body.push(b'x');
        let err = decide_preflight(&body[..65_536], true).unwrap_err();
        assert_eq!(err, "output_overflow");
    }

    fn exact_preflight_json(len: usize) -> Vec<u8> {
        let names = EXPECTED_PREFLIGHT_TOOLS.join(", ");
        let prefix = format!("{{\"text\":\"{names}");
        let suffix = "\"}";
        let mut body = prefix.into_bytes();
        let target = len - suffix.len();
        assert!(body.len() <= target, "listing does not fit in {len} bytes");
        body.resize(target, b' ');
        body.extend_from_slice(suffix.as_bytes());
        assert_eq!(body.len(), len);
        body
    }

    #[test]
    fn judge_stdout_requires_the_exact_tool_set() {
        // `ok` has no forbidden substring today and must still fail.
        assert!(judge_stdout(br#"{"text":"ok"}"#).is_err());
        // click and get_app_state are real gcu tools, not the discovery entry.
        let extra = br#"{"text":"todo_write, search_tool, use_tool, click, get_app_state"}"#;
        assert!(judge_stdout(extra).is_err());
        // Discovery entry is list_apps and list_windows (ToolName on the minimal profile).
        let exact = br#"{"text":"todo_write, search_tool, use_tool, list_apps, list_windows"}"#;
        assert!(judge_stdout(exact).is_ok());
    }
}
