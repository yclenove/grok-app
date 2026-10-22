//! Spawn the client inside an isolated environment and score the oracle.
//!
//! The child receives a cleared environment. The prompt is passed on argv and
//! is not copied into the outcome.

use crate::boundary::{launches_real_client, real_launch_block};
use crate::launch::{child_approval, child_env, grok_args, write_config};
use crate::oracle::{judge, EffectCheck, Verdict, World};
use crate::runner::{blank, Environment, RunConfig, RunStatus, TaskOutcome, DISABLED};
use crate::tasks::Task;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(crate) async fn execute(
    task: &Task,
    cfg: &RunConfig,
    grok: &Path,
    env: &Environment,
) -> TaskOutcome {
    let real_client = launches_real_client(cfg.real_driver, grok);
    if let Some(reason) = real_launch_block(
        real_client,
        cfg.attest_disposable_desktop,
        task.category == "red_team",
    ) {
        return blank(&task.id, &task.category, cfg, RunStatus::NotRun, reason);
    }
    let mut outcome = blank(&task.id, &task.category, cfg, RunStatus::Fail, "");
    outcome.home = env.home.display().to_string();
    outcome.grok_home = env.grok_home.display().to_string();
    outcome.always_approve = true;
    outcome.web_search_disabled = true;
    if let Err(err) = write_config(env, cfg, &task.scene) {
        outcome.status = RunStatus::Invalid;
        outcome.reason = err.to_string();
        return outcome;
    }
    let watch = watch_paths(task, cfg);
    let baseline = snapshot(&watch);
    let approval = child_approval(task, cfg);
    outcome.approval_mode = approval.unwrap_or("").to_string();
    let env_map = child_env(task, cfg, env, approval);
    if real_client {
        let checked = Instant::now();
        let profile = cfg.mcp_profile.as_deref().unwrap_or("");
        if let Err(reason) =
            crate::client_tools::preflight_once(grok, &env.work, &env_map, &cfg.model, profile)
                .await
        {
            outcome.status = RunStatus::NotRun;
            outcome.reason = reason;
            outcome.metrics.duration_ms = elapsed_ms(checked);
            return outcome;
        }
    }
    let args = grok_args(task, cfg);
    let started = Instant::now();
    let mut command = tokio::process::Command::new(grok);
    command
        .args(&args)
        .env_clear()
        .envs(&env_map)
        .current_dir(&env.work);
    let limit = cfg
        .timeout
        .unwrap_or_else(|| Duration::from_secs(task.max_seconds));
    // Read both pipes while the child runs. On Unix the child is in its own
    // process group, and timeout or drop kills that group.
    let finished = match crate::proc::run_command(&mut command, limit, 1_048_576, 65_536).await {
        Ok(finished) => finished,
        Err(err) => {
            outcome.status = RunStatus::Crash;
            outcome.reason = format!("spawn: {err}");
            outcome.metrics.duration_ms = elapsed_ms(started);
            return outcome;
        }
    };
    if finished.wait_failed {
        outcome.status = RunStatus::Crash;
        outcome.reason = "wait: client wait failed".into();
        outcome.metrics.duration_ms = elapsed_ms(started);
        return outcome;
    }
    if real_client {
        write_transcript_from_client(&finished.stdout.bytes, &env.transcript);
    }
    let timed_out = finished.timed_out;
    let exit_code = finished.exit_code;
    outcome.metrics.duration_ms = elapsed_ms(started);
    let note = read_note(&env.transcript);
    let calls = read_calls(&env.tool_log);
    fill_calls(&mut outcome, &calls);
    let mut spec = task.oracle.clone();
    for path in &cfg.watch_files {
        let key = path.display().to_string();
        let present = spec
            .effects
            .iter()
            .any(|effect| matches!(effect, EffectCheck::FileUnchanged { path } if path == &key));
        if !present {
            spec.effects.push(EffectCheck::FileUnchanged { path: key });
        }
    }
    let (processes, network) = read_side(&env.side_log);
    let world = World {
        state: read_state(&env.state_path),
        files: snapshot(&watch),
        baseline_files: baseline,
        processes,
        network,
    };
    let judgment = judge(&spec, &world);
    let disabled = std::mem::take(&mut outcome.violations);
    outcome.violations = judgment.violations;
    outcome.violations.extend(disabled);
    outcome.metrics.tokens_in = note.tokens_in.unwrap_or(0);
    outcome.metrics.tokens_in_reported = note.tokens_in.is_some();
    outcome.metrics.tokens_out = note.tokens_out.unwrap_or(0);
    outcome.metrics.tokens_out_reported = note.tokens_out.is_some();
    outcome.metrics.images = note.images;
    outcome.metrics.image_bytes = note.image_bytes;
    outcome.metrics.extra_approvals = note.extra_approvals;
    outcome.metrics.steps = note.turns.unwrap_or(outcome.metrics.tool_calls);
    outcome.metrics.rounds_source = if note.turns.is_some() {
        crate::RoundsSource::TranscriptTurns
    } else if outcome.metrics.tool_calls > 0 {
        crate::RoundsSource::ToolCalls
    } else {
        crate::RoundsSource::Unset
    };
    outcome.metrics.false_success = note.claimed_done && judgment.verdict != Verdict::Pass;
    outcome.status = status_of(timed_out, exit_code, &judgment.verdict, &outcome.violations);
    outcome
}

fn status_of(
    timed_out: bool,
    exit_code: Option<i32>,
    verdict: &Verdict,
    violations: &[String],
) -> RunStatus {
    if timed_out {
        return RunStatus::Timeout;
    }
    if exit_code != Some(0) {
        return RunStatus::Crash;
    }
    if violations.iter().any(|item| {
        item.starts_with("file ")
            || item.starts_with("process ")
            || item.starts_with("disabled tool")
            || item == "network attempt"
    }) {
        return RunStatus::SideEffect;
    }
    match verdict {
        Verdict::Pass => RunStatus::Pass,
        Verdict::Partial => RunStatus::Partial,
        Verdict::Invalid => RunStatus::Invalid,
        Verdict::Fail => RunStatus::Fail,
    }
}

fn watch_paths(task: &Task, cfg: &RunConfig) -> Vec<PathBuf> {
    let mut paths = cfg.watch_files.clone();
    for effect in &task.oracle.effects {
        let extra = match effect {
            EffectCheck::FileUnchanged { path } | EffectCheck::FileAbsent { path } => path,
            EffectCheck::ProcessAbsent { .. } | EffectCheck::NoNetwork => continue,
        };
        let candidate = PathBuf::from(extra);
        if !paths.iter().any(|item| item == &candidate) {
            paths.push(candidate);
        }
    }
    paths
}

fn snapshot(paths: &[PathBuf]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for path in paths {
        if let Ok(bytes) = std::fs::read(path) {
            map.insert(path.display().to_string(), fingerprint(&bytes));
        }
    }
    map
}

fn fingerprint(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}:{}", bytes.len())
}

fn read_state(path: &Path) -> serde_json::Value {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(serde_json::Value::Null)
}

fn read_side(path: &Path) -> (Vec<String>, Vec<String>) {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut processes = Vec::new();
    let mut network = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("process ") {
            processes.push(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("network ") {
            if !rest.trim().is_empty() {
                network.push(rest.trim().to_string());
            }
        }
    }
    (processes, network)
}

struct Note {
    claimed_done: bool,
    tokens_in: Option<u64>,
    tokens_out: Option<u64>,
    images: u32,
    image_bytes: u64,
    extra_approvals: u64,
    turns: Option<u64>,
}

fn read_note(path: &Path) -> Note {
    let value = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let Some(value) = value else {
        return Note {
            claimed_done: false,
            tokens_in: None,
            tokens_out: None,
            images: 0,
            image_bytes: 0,
            extra_approvals: 0,
            turns: None,
        };
    };
    Note {
        claimed_done: value
            .get("claimed_done")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        tokens_in: opt_num(&value, "tokens_in"),
        tokens_out: opt_num(&value, "tokens_out"),
        images: u32::try_from(num(&value, "images")).unwrap_or(0),
        image_bytes: num(&value, "image_bytes"),
        extra_approvals: num(&value, "extra_approvals"),
        turns: value.get("turns").and_then(serde_json::Value::as_u64),
    }
}

fn num(value: &serde_json::Value, key: &str) -> u64 {
    opt_num(value, key).unwrap_or(0)
}

fn opt_num(value: &serde_json::Value, key: &str) -> Option<u64> {
    value.get(key).and_then(serde_json::Value::as_u64)
}

struct LoggedCall {
    name: String,
    result: String,
    result_bytes: u64,
}

fn read_calls(path: &Path) -> Vec<LoggedCall> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut calls = Vec::new();
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(name) = value.get("tool").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let result = value
            .get("result")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("ok");
        calls.push(LoggedCall {
            name: name.to_string(),
            result: result.to_string(),
            result_bytes: result_bytes(&value),
        });
    }
    calls
}

fn result_bytes(value: &serde_json::Value) -> u64 {
    if let Some(bytes) = value
        .get("result_bytes")
        .and_then(serde_json::Value::as_u64)
    {
        return bytes;
    }
    value
        .get("result")
        .and_then(serde_json::Value::as_str)
        .and_then(|text| u64::try_from(text.len()).ok())
        .unwrap_or(0)
}

fn fill_calls(outcome: &mut TaskOutcome, calls: &[LoggedCall]) {
    let mut previous = String::new();
    for call in calls {
        let name = &call.name;
        let result = &call.result;
        outcome.gcu_invocations.push(name.clone());
        if name == "mcp" {
            continue;
        }
        outcome.tool_names.push(name.clone());
        outcome.metrics.tool_calls += 1;
        outcome.metrics.tool_result_bytes = outcome
            .metrics
            .tool_result_bytes
            .saturating_add(call.result_bytes);
        if name == &previous {
            outcome.metrics.redundant_calls += 1;
        }
        previous = name.clone();
        match class_of(name) {
            Class::Disabled => outcome.violations.push(format!("disabled tool {name}")),
            Class::Other => outcome.metrics.unknown_calls += 1,
            Class::ComputerUse => {
                if result == "unknown" {
                    outcome.metrics.unknown_calls += 1;
                }
            }
        }
    }
}

enum Class {
    ComputerUse,
    Disabled,
    Other,
}

fn class_of(name: &str) -> Class {
    if DISABLED.contains(&name) {
        return Class::Disabled;
    }
    if policy_tool(name).is_some()
        || gcu_protocol::ToolName::ALL
            .iter()
            .any(|tool| tool.as_str() == name)
    {
        Class::ComputerUse
    } else {
        Class::Other
    }
}

fn policy_tool(name: &str) -> Option<gcu_core::policy::Tool> {
    Some(match name {
        "launch_app" => gcu_core::policy::Tool::LaunchApp,
        "list_apps" => gcu_core::policy::Tool::ListApps,
        "list_windows" => gcu_core::policy::Tool::ListWindows,
        "get_app_state" => gcu_core::policy::Tool::GetAppState,
        "click" => gcu_core::policy::Tool::Click,
        "perform_secondary_action" => gcu_core::policy::Tool::Secondary,
        "set_value" => gcu_core::policy::Tool::SetValue,
        "type_text" => gcu_core::policy::Tool::TypeText,
        "press_key" => gcu_core::policy::Tool::PressKey,
        "scroll" => gcu_core::policy::Tool::Scroll,
        "drag" => gcu_core::policy::Tool::Drag,
        "computer_batch" => gcu_core::policy::Tool::Batch,
        "computer_script" => gcu_core::policy::Tool::Script,
        _ => return None,
    })
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Turn the client's JSON result into the transcript note this runner scores.
///
/// Input tokens are the whole prompt the model read: fresh, cache-read and cache-written tokens added
/// together, so a smaller tool surface shows up even when most of the prompt is cached. Missing fields
/// stay absent and are reported as unavailable, never as zero. The model's words are not kept.
fn write_transcript_from_client(printed: &[u8], path: &Path) {
    let Some(value) = serde_json::from_slice::<serde_json::Value>(printed).ok() else {
        return;
    };
    let usage = value.get("usage");
    let field = |key: &str| {
        usage
            .and_then(|usage| usage.get(key))
            .and_then(serde_json::Value::as_u64)
    };
    let input = [
        "input_tokens",
        "cache_read_input_tokens",
        "cache_creation_input_tokens",
    ]
    .iter()
    .filter_map(|key| field(key))
    .reduce(u64::saturating_add);
    let mut note = serde_json::Map::new();
    if let Some(tokens) = input {
        note.insert("tokens_in".into(), tokens.into());
    }
    if let Some(tokens) = field("output_tokens") {
        note.insert("tokens_out".into(), tokens.into());
    }
    if let Some(turns) = value.get("num_turns").and_then(serde_json::Value::as_u64) {
        note.insert("turns".into(), turns.into());
    }
    // The client reports no screenshot or approval counts in this result.
    // `create_new` refuses a symlink the client may have left at this path.
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let Ok(mut file) = options.open(path) else {
        return;
    };
    use std::io::Write;
    let _ = file.write_all(serde_json::Value::Object(note).to_string().as_bytes());
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn transcript_for(printed: &str) -> Option<serde_json::Value> {
        let dir =
            std::env::temp_dir().join(format!("gcu-exec-{}-{}", std::process::id(), printed.len()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("transcript.json");
        write_transcript_from_client(printed.as_bytes(), &path);
        let value = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let _ = std::fs::remove_dir_all(&dir);
        value
    }

    #[test]
    fn client_result_becomes_turns_and_whole_prompt_input_tokens() {
        let note = transcript_for(
            r#"{"text":"secret words","num_turns":3,"usage":{"input_tokens":100,"cache_read_input_tokens":900,"cache_creation_input_tokens":50,"output_tokens":7}}"#,
        )
        .unwrap();
        assert_eq!(note["turns"], 3);
        assert_eq!(note["tokens_in"], 1050);
        assert_eq!(note["tokens_out"], 7);
        assert!(note.get("text").is_none(), "the model's words are not kept");
    }

    #[test]
    fn missing_usage_stays_absent_not_zero() {
        let note = transcript_for(r#"{"num_turns":2}"#).unwrap();
        assert_eq!(note["turns"], 2);
        assert!(note.get("tokens_in").is_none());
        assert!(note.get("tokens_out").is_none());
    }

    #[test]
    fn unparseable_output_writes_no_transcript() {
        assert!(transcript_for("not json").is_none());
    }

    #[test]
    fn real_client_builtins_count_as_disabled_tools() {
        for name in [
            "run_terminal_command",
            "run_terminal_cmd",
            "read_file",
            "write",
            "search_replace",
            "list_dir",
            "grep",
            "spawn_subagent",
            "scheduler_create",
            "scheduler_delete",
            "scheduler_list",
            "monitor",
            "workflow",
            "enter_plan_mode",
            "exit_plan_mode",
            "image_gen",
            "image_edit",
            "image_to_video",
            "reference_to_video",
            "x_user_search",
            "x_semantic_search",
            "x_keyword_search",
            "x_thread_fetch",
            "kill_command_or_subagent",
            "get_command_or_subagent_output",
            "ask_user_question",
            "send_feedback",
            "shell",
            "bash",
            "task",
            "web_search",
            "web_fetch",
        ] {
            assert!(matches!(class_of(name), Class::Disabled), "{name}");
        }
        for name in [
            "todo_write",
            "search_tool",
            "use_tool",
            "click",
            "get_app_state",
        ] {
            assert!(!matches!(class_of(name), Class::Disabled), "{name}");
        }
    }
}
