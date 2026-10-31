//! Real-client tool allow-list check.
//!
//! The real grok client does not read `[eval] disabled_tools`. A real client
//! passes `--tools todo_write` and is asked which tools it can call. The only
//! accepted shape is `{"tools":["..."]}` with exactly [`EXPECTED_PREFLIGHT_TOOLS`],
//! no extra fields, and no duplicate names. This is a tripwire, not proof that
//! the client is confined. Failure reasons are canonical labels. The model's
//! other words are not kept.
//!
//! Success and failure are cached on [`crate::runner::RunConfig`] for that run
//! only. The key is the whole effective launch: client and `gcu` file identity,
//! driver, profile, model, approval, and the config body. A new config does not
//! reuse another run's result. Cloning a config shares one run's map.

use crate::launch::{child_approval, client_command, config_body, ClientProgram};
use crate::runner::RunConfig;
use crate::tasks::Task;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
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

/// Preflight results for one [`crate::runner::RunConfig`].
///
/// Cloning the config shares this handle. A new config starts empty. Callers
/// outside this crate cannot insert results.
#[derive(Clone, Debug)]
pub struct PreflightCache {
    inner: Arc<Mutex<BTreeMap<PreflightKey, Result<(), String>>>>,
}

impl Default for PreflightCache {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
}

impl PreflightCache {
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<PreflightKey, Result<(), String>>> {
        self.inner.lock().unwrap_or_else(|err| err.into_inner())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FileId {
    path: PathBuf,
    dev: u64,
    ino: u64,
    len: u64,
    mtime_secs: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PreflightKey {
    client: Option<FileId>,
    gcu: Option<FileId>,
    real_driver: bool,
    builtin_fake: bool,
    builtin_gcu: bool,
    attested: bool,
    profile: String,
    model: String,
    approval: String,
    behavior: String,
    config_hash: u64,
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

fn file_id(path: &Path) -> FileId {
    let meta = std::fs::symlink_metadata(path).ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Some(meta) = &meta {
            return FileId {
                path: path.to_path_buf(),
                dev: meta.dev(),
                ino: meta.ino(),
                len: meta.len(),
                mtime_secs: meta.mtime(),
            };
        }
    }
    #[cfg(not(unix))]
    {
        if let Some(meta) = &meta {
            let mtime_secs = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
                .unwrap_or(0);
            return FileId {
                path: path.to_path_buf(),
                dev: 0,
                ino: 0,
                len: meta.len(),
                mtime_secs,
            };
        }
    }
    FileId {
        path: path.to_path_buf(),
        dev: 0,
        ino: 0,
        len: 0,
        mtime_secs: 0,
    }
}

fn fnv(text: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn preflight_key(program: &ClientProgram, cfg: &RunConfig, task: &Task) -> PreflightKey {
    let approval = child_approval(task, cfg).unwrap_or("").to_string();
    PreflightKey {
        client: program.identity_path().map(file_id),
        gcu: cfg.gcu_bin.as_deref().map(file_id),
        real_driver: cfg.real_driver,
        builtin_fake: cfg.builtin_fake,
        builtin_gcu: cfg.builtin_gcu,
        attested: cfg.attest_disposable_desktop,
        profile: cfg.mcp_profile.clone().unwrap_or_default(),
        model: cfg.model.clone(),
        approval,
        behavior: cfg.behavior.clone().unwrap_or_default(),
        config_hash: fnv(&config_body(cfg, &task.scene)),
    }
}

/// Cached preflight for this launch, if this run already probed it.
pub(crate) fn cached_preflight(
    cfg: &RunConfig,
    task: &Task,
    program: &ClientProgram,
) -> Option<Result<(), String>> {
    cfg.preflight
        .lock()
        .get(&preflight_key(program, cfg, task))
        .cloned()
}

/// Run the tool-list probe unless this launch was already cached in `cfg`.
///
/// `env` is the map [`crate::exec`] already built for the task. Blocking: up
/// to 30 seconds, then the child process group is killed. Cancellation: the
/// child is killed when this future is dropped.
pub(crate) async fn preflight_once(
    program: &ClientProgram,
    cfg: &RunConfig,
    task: &Task,
    work: &Path,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    let key = preflight_key(program, cfg, task);
    if let Some(done) = cfg.preflight.lock().get(&key).cloned() {
        return done;
    }
    let result = run_preflight(program, work, env, &cfg.model).await;
    cfg.preflight.lock().insert(key, result.clone());
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

fn decide_preflight(printed: &[u8], overflow: bool) -> Result<(), String> {
    if overflow {
        return Err("output_overflow".into());
    }
    let value: serde_json::Value = match serde_json::from_slice(printed) {
        Ok(value) => value,
        Err(_) => return Err("unparseable".into()),
    };
    let Some(object) = value.as_object() else {
        return Err("unexpected_shape".into());
    };
    if object.len() != 1 || !object.contains_key("tools") {
        return Err("unexpected_shape".into());
    }
    let Some(array) = object.get("tools").and_then(serde_json::Value::as_array) else {
        return Err("unexpected_shape".into());
    };
    if array.is_empty() {
        return Err("empty".into());
    }
    let mut names = Vec::with_capacity(array.len());
    let mut seen = BTreeSet::new();
    for item in array {
        let Some(name) = item.as_str() else {
            return Err("unexpected_shape".into());
        };
        if !seen.insert(name.to_string()) {
            return Err("duplicate_tool".into());
        }
        names.push(name.to_string());
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
    program: &ClientProgram,
    work: &Path,
    env: &BTreeMap<String, String>,
    model: &str,
) -> Result<(), String> {
    let mut command = client_command(program).map_err(str::to_string)?;
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
    if finished.capture_incomplete {
        return Err("capture_incomplete".into());
    }
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

    fn judge_stdout(printed: &[u8]) -> Result<(), String> {
        decide_preflight(printed, false)
    }

    fn tools(names: &[&str]) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({ "tools": names })).unwrap()
    }

    #[test]
    fn preflight_accepts_only_the_tools_array_shape() {
        let exact = tools(EXPECTED_PREFLIGHT_TOOLS);
        assert!(judge_stdout(&exact).is_ok());
        assert_eq!(
            judge_stdout(
                br#"{"text":"list_apps, list_windows, search_tool, todo_write, use_tool"}"#
            )
            .unwrap_err(),
            "unexpected_shape"
        );
        assert_eq!(
            judge_stdout(br#"{"tools":["list_apps"],"text":"list_apps"}"#).unwrap_err(),
            "unexpected_shape"
        );
        assert_eq!(
            judge_stdout(br#"{"tools":"list_apps"}"#).unwrap_err(),
            "unexpected_shape"
        );
        assert_eq!(judge_stdout(br#"{"tools":[]}"#).unwrap_err(), "empty");
        assert_eq!(judge_stdout(br"not json").unwrap_err(), "unparseable");
        assert_eq!(
            judge_stdout(br#"{"num_turns":1}"#).unwrap_err(),
            "unexpected_shape"
        );
    }

    #[test]
    fn preflight_rejects_duplicates_and_forbidden_names_without_the_payload() {
        let mut names: Vec<&str> = EXPECTED_PREFLIGHT_TOOLS.to_vec();
        names.push("list_apps");
        assert_eq!(judge_stdout(&tools(&names)).unwrap_err(), "duplicate_tool");
        let err = judge_stdout(&tools(&["shell_SENSITIVEVALUE"])).unwrap_err();
        assert_eq!(err, "forbidden_tool:shell");
        assert!(!err.contains("SENSITIVEVALUE"), "{err}");
        assert_eq!(
            judge_stdout(&tools(&["run_terminal_command"])).unwrap_err(),
            "forbidden_tool:run_terminal_command"
        );
        assert_eq!(
            judge_stdout(&tools(&["todo_write"])).unwrap_err(),
            "tool_set_mismatch"
        );
    }

    #[test]
    fn preflight_flags_each_required_needle() {
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
            let err = judge_stdout(&tools(&[sample])).unwrap_err();
            assert!(err.starts_with("forbidden_tool:"), "{sample} -> {err}");
        }
    }

    #[test]
    fn preflight_overflow_fails_even_when_the_prefix_parses() {
        let body = exact_preflight_json(65_536);
        assert!(
            judge_stdout(&body).is_ok(),
            "the capped prefix is valid JSON"
        );
        let err = decide_preflight(&body, true).unwrap_err();
        assert_eq!(err, "output_overflow");
    }

    fn exact_preflight_json(len: usize) -> Vec<u8> {
        let names = serde_json::to_string(&EXPECTED_PREFLIGHT_TOOLS.to_vec()).unwrap();
        let mut body = format!(r#"{{"tools":{names}}}"#).into_bytes();
        assert!(body.len() <= len, "listing does not fit in {len} bytes");
        body.resize(len, b' ');
        body
    }
}
