//! Arguments, environment, and MCP config for one client launch.
//!
//! Every real client gets the same `--tools` allow-list, cleared environment,
//! and proxy forwarding. The eval fake agent keeps the narrower test path.

use crate::boundary::{launches_real_client, INTERNAL_FAKE_AGENT};
use crate::runner::{ApprovalMode, Environment, RunConfig};
use crate::tasks::Task;
use crate::Error;
use std::collections::BTreeMap;
use std::io::Write;

pub(crate) fn child_approval(task: &Task, cfg: &RunConfig) -> Option<&'static str> {
    if !cfg.auto_approve {
        return None;
    }
    if task.category == "red_team" {
        Some(ApprovalMode::Deny.as_str())
    } else {
        Some(ApprovalMode::AllowSession.as_str())
    }
}

pub(crate) fn grok_args(task: &Task, cfg: &RunConfig) -> Vec<String> {
    let mut args = vec![
        "-p".into(),
        task.prompt_en.clone(),
        "--always-approve".into(),
        "--model".into(),
        cfg.model.clone(),
        "--max-turns".into(),
        task.max_turns.to_string(),
        "--disable-web-search".into(),
        "--no-plan".into(),
        "--verbatim".into(),
        "--output-format".into(),
        "json".into(),
    ];
    // The real client ignores `[eval] disabled_tools`. An empty `--tools` value
    // does not restrict it, so the allow-list is the single built-in `todo_write`.
    // The built-in fake agent keeps the narrower test path. A path is never
    // trusted because of its file name.
    if launches_real_client(cfg) {
        args.push("--tools".into());
        args.push("todo_write".into());
    }
    args
}

/// How the runner starts the client. `Reexec` is this process, not a path the operator passed.
pub(crate) enum ClientProgram {
    /// Operator path or the cargo-built fake agent selected by the test slot.
    Path(std::path::PathBuf),
    /// `gcu-eval` re-entering as the built-in fake agent.
    Reexec(std::path::PathBuf),
}

impl ClientProgram {
    /// Path or executable used for logs and the preflight cache. Not a trust decision.
    pub(crate) fn identity_path(&self) -> &std::path::Path {
        match self {
            Self::Path(path) | Self::Reexec(path) => path,
        }
    }
}

/// Resolve the client. `builtin_fake` ignores `grok_bin`.
pub(crate) fn resolve_client(cfg: &RunConfig) -> Result<ClientProgram, Error> {
    if cfg.builtin_fake {
        if let Some(path) = &cfg.builtin_fake_bin {
            return Ok(ClientProgram::Path(path.clone()));
        }
        let exe = std::env::current_exe()
            .map_err(|err| Error::msg(format!("builtin fake agent: {err}")))?;
        return Ok(ClientProgram::Reexec(exe));
    }
    cfg.grok_bin
        .clone()
        .map(ClientProgram::Path)
        .ok_or_else(|| Error::msg("no client binary and no model credentials"))
}

/// Command for `program`. The re-exec form adds the internal fake-agent flag.
pub(crate) fn client_command(program: &ClientProgram) -> tokio::process::Command {
    match program {
        ClientProgram::Path(path) => tokio::process::Command::new(path),
        ClientProgram::Reexec(exe) => {
            let mut command = tokio::process::Command::new(exe);
            command.arg(INTERNAL_FAKE_AGENT);
            command
        }
    }
}

pub(crate) fn child_env(
    task: &Task,
    cfg: &RunConfig,
    env: &Environment,
    approval: Option<&str>,
) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let home = env.home.display().to_string();
    map.insert("GROK_HOME".into(), env.grok_home.display().to_string());
    map.insert("HOME".into(), home.clone());
    map.insert("USERPROFILE".into(), home);
    map.insert("TMPDIR".into(), env.work.display().to_string());
    map.insert("TEMP".into(), env.work.display().to_string());
    map.insert("TMP".into(), env.work.display().to_string());
    // Shims first so they shadow curl, wget, ssh, nc and mail. A real client also needs system tools.
    let real_client = launches_real_client(cfg);
    let path = if real_client {
        format!("{}:{}", env.shim_dir.display(), crate::prep::system_path())
    } else {
        env.shim_dir.display().to_string()
    };
    map.insert("PATH".into(), path);
    map.insert("LANG".into(), "C.UTF-8".into());
    map.insert(
        "GCU_FIXTURE_STATE".into(),
        env.state_path.display().to_string(),
    );
    map.insert("GCU_EVAL_TASK_ID".into(), task.id.clone());
    map.insert("GCU_EVAL_CATEGORY".into(), task.category.clone());
    map.insert(
        "GCU_EVAL_PASS_SCRIPT".into(),
        task.pass_script.display().to_string(),
    );
    map.insert(
        "GCU_EVAL_TOOL_LOG".into(),
        env.tool_log.display().to_string(),
    );
    map.insert(
        "GCU_EVAL_SIDE_LOG".into(),
        env.side_log.display().to_string(),
    );
    insert_gcu_env(&mut map, cfg);
    if let Some(probe) = &cfg.probe {
        map.insert("GCU_EVAL_PROBE".into(), probe.display().to_string());
    }
    if let Some(behavior) = &cfg.behavior {
        map.insert("GCU_EVAL_BEHAVIOR".into(), behavior.clone());
    }
    if let Some(watch) = cfg.watch_files.first() {
        map.insert("GCU_EVAL_WATCH_FILE".into(), watch.display().to_string());
    }
    if let Some(mode) = approval {
        map.insert("GCU_APPROVAL".into(), mode.to_string());
    }
    if real_client {
        for (key, value) in proxy_pairs(|key| std::env::var(key).ok()) {
            map.insert(key, value);
        }
    }
    map
}

/// `GCU_BIN` and the flags that tell the fake agent whether that path is trusted.
///
/// The built-in slot (`builtin_gcu`, or built-in fake with no `gcu_bin`) sets
/// `GCU_EVAL_BUILTIN_GCU`. Re-exec sets `GCU_EVAL_GCU_REEXEC` and points
/// `GCU_BIN` at this process. An operator path does not get the built-in flag.
/// Attestation is a separate flag and does not make a path built-in.
fn insert_gcu_env(map: &mut BTreeMap<String, String>, cfg: &RunConfig) {
    if cfg.attest_disposable_desktop {
        map.insert("GCU_EVAL_ATTESTED".into(), "1".into());
    }
    let reexec = cfg.builtin_fake && cfg.gcu_bin.is_none();
    if cfg.builtin_gcu || reexec {
        map.insert("GCU_EVAL_BUILTIN_GCU".into(), "1".into());
    }
    if reexec {
        if let Ok(exe) = std::env::current_exe() {
            map.insert("GCU_BIN".into(), exe.display().to_string());
            map.insert("GCU_EVAL_GCU_REEXEC".into(), "1".into());
        }
        return;
    }
    map.insert(
        "GCU_BIN".into(),
        cfg.gcu_bin
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
    );
}

/// Proxy settings found through `lookup`. Empty values are dropped.
fn proxy_pairs(lookup: impl Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    crate::access::proxy_env_keys()
        .iter()
        .filter_map(|key| {
            lookup(key)
                .filter(|value| !value.is_empty())
                .map(|value| ((*key).to_string(), value))
        })
        .collect()
}

/// Isolated client config. The text is the preflight cache input, not a secret.
pub(crate) fn config_body(cfg: &RunConfig, scene: &str) -> String {
    let command = toml_quote(
        &cfg.gcu_bin
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
    );
    let scene = toml_quote(scene);
    let disabled = crate::runner::DISABLED.join("\", \"");
    let profile = cfg
        .mcp_profile
        .as_ref()
        .map(|value| format!(", \"--profile\", {}", toml_quote(value)))
        .unwrap_or_default();
    // The real driver takes no scene: the fixture app on the desktop is the world.
    let server_args = if cfg.real_driver {
        format!("[\"mcp\"{profile}]")
    } else {
        format!("[\"mcp\", \"--driver\", \"fake\", \"--scene\", {scene}{profile}]")
    };
    // `[eval] disabled_tools` is not read by the real client. Real mode locks
    // built-ins with `--tools todo_write` and the preflight tripwire.
    let mut body = format!(
        "[mcp_servers.grok-computer-use]\ncommand = {command}\nargs = {server_args}\nenabled = true\n\n[eval]\ndisabled_tools = [\"{disabled}\"]\n"
    );
    if let Some(arm) = &cfg.arm_label {
        body.push_str(&format!("arm = {}\n", toml_quote(arm)));
    }
    if let Some(shot) = &cfg.arm_screenshot {
        body.push_str(&format!("screenshot = {}\n", toml_quote(shot)));
    }
    if let Some(diff) = &cfg.arm_diff {
        body.push_str(&format!("diff = {}\n", toml_quote(diff)));
    }
    if let Some(batch) = cfg.arm_batch {
        body.push_str(&format!("batch = {batch}\n"));
    }
    if let Some(script) = cfg.arm_script {
        body.push_str(&format!("script = {script}\n"));
    }
    body
}

pub(crate) fn write_config(env: &Environment, cfg: &RunConfig, scene: &str) -> Result<(), Error> {
    let body = config_body(cfg, scene);
    let path = env.grok_home.join("config.toml");
    if std::fs::symlink_metadata(&path).is_ok() {
        return Err(Error::msg("refusing a pre-existing config.toml"));
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|err| Error::msg(format!("config: {err}")))?;
    file.write_all(body.as_bytes())
        .map_err(|err| Error::msg(format!("config: {err}")))?;
    Ok(())
}

fn toml_quote(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn sample_task() -> Task {
        Task {
            id: "native-form-01".into(),
            platforms: vec!["linux".into()],
            category: "native".into(),
            prompt_zh: "x".into(),
            prompt_en: "fill the form".into(),
            launch: "fixture".into(),
            scene: "form".into(),
            oracle: crate::oracle::OracleSpec {
                all: Vec::new(),
                forbidden: Vec::new(),
                effects: Vec::new(),
            },
            pass_script: std::path::PathBuf::from("pass.json"),
            fail_script: std::path::PathBuf::from("fail.json"),
            partial_script: std::path::PathBuf::from("partial.json"),
            suites: vec!["core".into()],
            max_turns: 4,
            max_seconds: 20,
        }
    }

    fn fake_grok_args() -> Vec<String> {
        vec![
            "-p".into(),
            "fill the form".into(),
            "--always-approve".into(),
            "--model".into(),
            "fake".into(),
            "--max-turns".into(),
            "4".into(),
            "--disable-web-search".into(),
            "--no-plan".into(),
            "--verbatim".into(),
            "--output-format".into(),
            "json".into(),
        ]
    }

    #[test]
    fn grok_args_fake_driver_omits_tools_allowlist() {
        let cfg = RunConfig {
            builtin_fake: true,
            ..RunConfig::default()
        };
        let args = grok_args(&sample_task(), &cfg);
        assert_eq!(args, fake_grok_args());
        assert!(!args
            .iter()
            .any(|arg| arg == "--tools" || arg == "todo_write"));
    }

    #[test]
    fn grok_args_named_like_the_fake_agent_still_gets_the_external_allowlist() {
        let cfg = RunConfig {
            grok_bin: Some(std::path::PathBuf::from("/tmp/eval-fake-agent")),
            ..RunConfig::default()
        };
        let args = grok_args(&sample_task(), &cfg);
        let tools = args.iter().position(|arg| arg == "--tools").unwrap();
        assert_eq!(args[tools + 1], "todo_write");
    }

    #[test]
    fn child_env_does_not_mark_an_operator_gcu_as_builtin() {
        let task = sample_task();
        let cfg = RunConfig {
            builtin_fake: true,
            gcu_bin: Some(std::path::PathBuf::from("/tmp/not-the-fake-gcu")),
            builtin_gcu: false,
            ..RunConfig::default()
        };
        let env = Environment {
            root: std::path::PathBuf::from("/tmp/root"),
            home: std::path::PathBuf::from("/tmp/root/home"),
            grok_home: std::path::PathBuf::from("/tmp/root/grok"),
            work: std::path::PathBuf::from("/tmp/root/work"),
            state_path: std::path::PathBuf::from("/tmp/root/state.json"),
            shim_dir: std::path::PathBuf::from("/tmp/root/shims"),
            side_log: std::path::PathBuf::from("/tmp/root/side.log"),
            tool_log: std::path::PathBuf::from("/tmp/root/tools.log"),
            transcript: std::path::PathBuf::from("/tmp/root/grok/transcript.json"),
        };
        let map = child_env(&task, &cfg, &env, None);
        assert_eq!(
            map.get("GCU_BIN").map(String::as_str),
            Some("/tmp/not-the-fake-gcu")
        );
        assert!(!map.contains_key("GCU_EVAL_BUILTIN_GCU"));
        assert!(!map.contains_key("GCU_EVAL_ATTESTED"));
        let marked = RunConfig {
            builtin_gcu: true,
            ..cfg.clone()
        };
        let marked = child_env(&task, &marked, &env, None);
        assert_eq!(
            marked.get("GCU_EVAL_BUILTIN_GCU").map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn grok_args_real_driver_appends_adjacent_tools_allowlist() {
        let task = sample_task();
        let fake = grok_args(&task, &RunConfig::default());
        let cfg = RunConfig {
            real_driver: true,
            ..RunConfig::default()
        };
        let real = grok_args(&task, &cfg);
        assert_eq!(&real[..fake.len()], fake.as_slice());
        assert_eq!(
            &real[fake.len()..],
            ["--tools".to_string(), "todo_write".to_string()]
        );
    }

    #[test]
    fn grok_args_external_client_appends_tools_allowlist() {
        let cfg = RunConfig {
            grok_bin: Some(std::path::PathBuf::from("grok.sh")),
            ..RunConfig::default()
        };
        let args = grok_args(&sample_task(), &cfg);
        let tools = args.iter().position(|arg| arg == "--tools").unwrap();
        assert_eq!(args[tools + 1], "todo_write");
        assert!(args.iter().any(|arg| arg == "--no-plan"));
        assert!(args.iter().any(|arg| arg == "--verbatim"));
    }

    #[test]
    fn proxy_settings_are_forwarded_only_when_present_and_non_empty() {
        let found = proxy_pairs(|key| match key {
            "HTTPS_PROXY" => Some("http://127.0.0.1:10808".into()),
            "NO_PROXY" => Some(String::new()),
            "SECRET_TOKEN" => Some("never".into()),
            _ => None,
        });
        assert_eq!(
            found,
            vec![(
                "HTTPS_PROXY".to_string(),
                "http://127.0.0.1:10808".to_string()
            )]
        );
    }
}
