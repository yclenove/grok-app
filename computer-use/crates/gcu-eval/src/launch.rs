//! Arguments, environment, and MCP config for one client launch.
//!
//! Every real client gets the same `--tools` allow-list, cleared environment,
//! and proxy forwarding. The eval fake agent keeps the narrower test path.

use crate::boundary::{admit_launch, launches_real_client};
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

/// Fake selection carries no executable path.
#[derive(Debug)]
pub(crate) enum ClientProgram {
    BuiltinFake,
    External(std::path::PathBuf),
}

impl ClientProgram {
    /// External identity for preflight caching; fake code has no file identity.
    pub(crate) fn identity_path(&self) -> Option<&std::path::Path> {
        match self {
            Self::BuiltinFake => None,
            Self::External(path) => Some(path),
        }
    }
}

/// Resolve only after the same admission decision used by the runner/helper.
pub(crate) fn resolve_client(cfg: &RunConfig) -> Result<ClientProgram, Error> {
    admit_launch(
        cfg.grok_bin.as_deref(),
        cfg.gcu_bin.as_deref(),
        cfg.fake_scope.is_some(),
        cfg.real_driver,
    )
    .map_err(Error::msg)?;
    if let Some(path) = &cfg.grok_bin {
        return Ok(ClientProgram::External(path.clone()));
    }
    if cfg.builtin_fake {
        return Ok(ClientProgram::BuiltinFake);
    }
    Err(Error::msg("no client binary and no model credentials"))
}

/// External commands require admission even if resolution was bypassed.
/// No fake selection can be converted into a process command.
pub(crate) fn client_command(
    program: &ClientProgram,
) -> Result<tokio::process::Command, &'static str> {
    match program {
        ClientProgram::BuiltinFake => Err("the fake client runs in process"),
        ClientProgram::External(path) => {
            admit_launch(Some(path), None, false, false)?;
            Ok(tokio::process::Command::new(path))
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

/// Forward a supplied backend path; fake agents refuse it using shared admission.
/// An absent path selects the in-process fake backend and needs no marker.
fn insert_gcu_env(map: &mut BTreeMap<String, String>, cfg: &RunConfig) {
    if let Some(path) = &cfg.gcu_bin {
        map.insert("GCU_BIN".into(), path.display().to_string());
    }
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
    config_body_for(cfg, scene, None)
}
fn config_body_for(
    cfg: &RunConfig,
    scene: &str,
    data_address: Option<std::net::SocketAddr>,
) -> String {
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
    let server_args = if let Some(address) = data_address {
        format!(
            "[\"scope-relay\", \"--data-address\", {}]",
            toml_quote(&address.to_string())
        )
    } else if cfg.real_driver {
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

pub(crate) fn write_config_for(
    env: &Environment,
    cfg: &RunConfig,
    scene: &str,
    data_address: Option<std::net::SocketAddr>,
) -> Result<(), Error> {
    let body = config_body_for(cfg, scene, data_address);
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
    fn fake_selection_has_no_path_and_unverified_paths_never_build_commands() {
        let cfg = RunConfig {
            builtin_fake: true,
            ..RunConfig::default()
        };
        let program = resolve_client(&cfg).unwrap();
        assert!(matches!(program, ClientProgram::BuiltinFake));
        assert!(program.identity_path().is_none());
        assert!(client_command(&program).is_err());
        for path in [
            "eval-fake-agent",
            "eval-fake-agent.exe",
            "eval-fake-gcu",
            "gcu-eval",
            "",
        ] {
            let path = std::path::PathBuf::from(path);
            // Even bypassing resolution cannot build a Command from a fake-named path.
            assert!(client_command(&ClientProgram::External(path.clone())).is_err());
            assert!(resolve_client(&RunConfig {
                grok_bin: Some(path.clone()),
                ..cfg.clone()
            })
            .is_err());
            assert!(resolve_client(&RunConfig {
                gcu_bin: Some(path),
                ..cfg.clone()
            })
            .is_err());
        }
        // Keep both in-process implementations free of subprocess launch APIs.
        for source in [include_str!("fake_agent.rs"), include_str!("fake_gcu.rs")] {
            for forbidden in ["Command", "std::process::", "tokio::process::"] {
                // ExitCode is just the standalone binary's status adapter.
                let source = source.replace("use std::process::ExitCode;", "");
                assert!(
                    !source.contains(forbidden),
                    "fake implementation gained {forbidden}"
                );
            }
        }
    }

    #[test]
    fn scoped_config_contains_only_fixed_relay_and_public_data_address() {
        let cfg = RunConfig {
            gcu_bin: Some(std::path::PathBuf::from("/fixtures/gcu")),
            ..RunConfig::default()
        };
        let body = config_body_for(&cfg, "form", Some("127.0.0.1:23456".parse().unwrap()));
        assert!(body.contains("args = [\"scope-relay\", \"--data-address\", \"127.0.0.1:23456\"]"));
        for private in [
            "Install",
            "scope_digest",
            "control_lease",
            "--scene",
            "--scope-fd",
        ] {
            assert!(
                !body.contains(private),
                "control content in MCP config: {private}"
            );
        }
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
        assert!(
            !marked.contains_key("GCU_EVAL_BUILTIN_GCU"),
            "an arbitrary gcu path is not the built-in artifact"
        );
        let reexec = RunConfig {
            builtin_fake: true,
            builtin_gcu: true,
            gcu_bin: None,
            ..RunConfig::default()
        };
        let reexec = child_env(&task, &reexec, &env, None);
        assert!(!reexec.contains_key("GCU_EVAL_BUILTIN_GCU"));
        assert!(!reexec.contains_key("GCU_EVAL_ATTESTED"));
        assert!(!reexec.contains_key("GCU_BIN"));
        assert!(!reexec.contains_key("GCU_EVAL_GCU_REEXEC"));
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
