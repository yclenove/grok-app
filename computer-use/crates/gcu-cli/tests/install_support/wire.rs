//! CLI install/uninstall wiring. Every home is a temporary directory.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use assert_cmd::Command;
use serde_json::Value;

const CLIENTS: &[&str] = &[
    "grok",
    "codex",
    "claude",
    "antigravity",
    "gemini",
    "opencode",
    "cursor",
    "vscode",
];

struct Layout {
    home: PathBuf,
    project: PathBuf,
    gcu: PathBuf,
    agy: PathBuf,
    agy_log: PathBuf,
}

struct Guard {
    files: Vec<(PathBuf, Option<Vec<u8>>)>,
}

impl Guard {
    fn new() -> Self {
        let mut paths = Vec::new();
        for key in ["HOME", "USERPROFILE"] {
            let Some(root) = std::env::var_os(key) else {
                continue;
            };
            if root.is_empty() {
                continue;
            }
            let root = PathBuf::from(root);
            for rel in [
                ".grok/config.toml",
                ".grok/hooks/computer-use-stop.json",
                ".grok/hooks/computer-use-turn-ended.sh",
                ".grok/hooks/computer-use-turn-ended.cmd",
                ".codex/config.toml",
                ".claude.json",
                ".gemini/settings.json",
                ".gemini/config/mcp_config.json",
                ".config/opencode/opencode.json",
                ".cursor/mcp.json",
            ] {
                paths.push(root.join(rel));
            }
            paths.push(vscode_user(&root));
        }
        paths.sort();
        paths.dedup();
        let files = paths
            .into_iter()
            .map(|path| {
                let bytes = fs::read(&path).ok();
                (path, bytes)
            })
            .collect();
        Self { files }
    }

    fn check(&self) {
        for (path, before) in &self.files {
            assert_eq!(
                fs::read(path).ok(),
                *before,
                "real config changed: {}",
                path.display()
            );
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.check();
        }
    }
}

fn temp(label: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("gcu{n}-{label}-{}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn layout(label: &str) -> Layout {
    let home = temp(&format!("{label}h"));
    let project = temp(&format!("{label}p"));
    let bin = project.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let gcu = bin.join(if cfg!(windows) { "gcu.exe" } else { "gcu" });
    fs::write(&gcu, b"gcu").unwrap();
    let agy_log = bin.join("agy.out");
    let agy = write_agy(&bin);
    Layout {
        home,
        project,
        gcu,
        agy,
        agy_log,
    }
}

#[cfg(windows)]
fn write_agy(_dir: &Path) -> PathBuf {
    windows_agy()
}

#[cfg(not(windows))]
fn write_agy(dir: &Path) -> PathBuf {
    let path = dir.join("agy");
    fs::write(
        &path,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$GCU_FAKE_AGY_LOG\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

#[cfg(windows)]
fn windows_agy() -> PathBuf {
    use std::sync::OnceLock;
    static EXE: OnceLock<PathBuf> = OnceLock::new();
    EXE.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("gcu-agy-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("agy.rs");
        fs::write(
            &src,
            r#"fn main() {
                let log = std::env::var("GCU_FAKE_AGY_LOG").expect("log");
                let mut out = std::fs::OpenOptions::new().create(true).append(true).open(log).unwrap();
                use std::io::Write;
                for arg in std::env::args().skip(1) { writeln!(out, "{arg}").unwrap(); }
            }"#,
        )
        .unwrap();
        let exe = dir.join("agy.exe");
        let status = std::process::Command::new("rustc")
            .arg(&src)
            .arg("-o")
            .arg(&exe)
            .status()
            .expect("rustc");
        assert!(status.success(), "rustc agy fixture failed");
        exe
    })
    .clone()
}

fn command(layout: &Layout) -> Command {
    let empty = layout.project.join("empty");
    fs::create_dir_all(&empty).unwrap();
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("gcu"));
    cmd.env("HOME", &layout.home)
        .env("USERPROFILE", &layout.home)
        .env("GCU_FAKE_AGY_LOG", &layout.agy_log)
        .env("PATH", &empty)
        .env_remove("GCU_HOME")
        .env_remove("GCU_PROJECT")
        .env_remove("GCU_YES")
        .env_remove("YES")
        .env_remove("GROK_YES")
        .env_remove("GCU_CONFIRM")
        .env_remove("GCU_APPROVAL")
        .timeout(Duration::from_secs(30));
    cmd
}

fn with_paths<'a>(
    cmd: &'a mut Command,
    layout: &Layout,
    verb: &str,
    client: &str,
) -> &'a mut Command {
    cmd.args([verb, "--client", client, "--home"])
        .arg(&layout.home)
        .arg("--project")
        .arg(&layout.project)
        .arg("--gcu")
        .arg(&layout.gcu)
        .arg("--agy")
        .arg(&layout.agy)
}

fn parent_key(client: &str) -> &'static str {
    match client {
        "opencode" => "mcp",
        "vscode" => "servers",
        "grok" | "codex" => "mcp_servers",
        _ => "mcpServers",
    }
}

fn vscode_user(home: &Path) -> PathBuf {
    let root = if cfg!(windows) {
        home.join("AppData/Roaming")
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        home.join(".config")
    };
    root.join("Code/User/mcp.json")
}

fn user_config(client: &str, home: &Path) -> PathBuf {
    match client {
        "grok" => home.join(".grok/config.toml"),
        "codex" => home.join(".codex/config.toml"),
        "claude" => home.join(".claude.json"),
        "antigravity" => home.join(".gemini/config/mcp_config.json"),
        "gemini" => home.join(".gemini/settings.json"),
        "opencode" => home.join(".config/opencode/opencode.json"),
        "cursor" => home.join(".cursor/mcp.json"),
        "vscode" => vscode_user(home),
        other => panic!("unknown client {other}"),
    }
}

fn write_file(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

fn seed_other(client: &str, home: &Path) -> String {
    let body = if matches!(client, "grok" | "codex") {
        "# keep\n\n[mcp_servers.other]\ncommand = \"other\"\n".to_owned()
    } else {
        format!(
            "{{\"keep\":true,\"{}\":{{\"other\":{{\"command\":\"other\"}}}}}}\n",
            parent_key(client)
        )
    };
    write_file(&user_config(client, home), &body);
    body
}

fn seed_matching(client: &str, home: &Path, gcu: &str) {
    let gcu = gcu.replace('\\', "\\\\");
    let body = if matches!(client, "grok" | "codex") {
        format!(
            "[mcp_servers.other]\ncommand = \"other\"\n\n[mcp_servers.grok-computer-use]\ncommand = \"{gcu}\"\nargs = [\"mcp\"]\n"
        )
    } else if client == "opencode" {
        format!(
            "{{\"keep\":true,\"mcp\":{{\"other\":{{\"command\":\"other\"}},\"grok-computer-use\":{{\"type\":\"local\",\"command\":[\"{gcu}\",\"mcp\"],\"enabled\":true}}}}}}\n"
        )
    } else if matches!(client, "cursor" | "vscode") {
        format!(
            "{{\"keep\":true,\"{}\":{{\"other\":{{\"command\":\"other\"}},\"grok-computer-use\":{{\"type\":\"stdio\",\"command\":\"{gcu}\",\"args\":[\"mcp\"]}}}}}}\n",
            parent_key(client)
        )
    } else {
        format!(
            "{{\"keep\":true,\"{}\":{{\"other\":{{\"command\":\"other\"}},\"grok-computer-use\":{{\"command\":\"{gcu}\",\"args\":[\"mcp\"]}}}}}}\n",
            parent_key(client)
        )
    };
    write_file(&user_config(client, home), &body);
}

fn files_under(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        let Ok(read) = fs::read_dir(dir) else {
            return;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push((path.clone(), fs::read(&path).unwrap()));
            }
        }
    }
    walk(root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn backups(root: &Path) -> Vec<PathBuf> {
    files_under(root)
        .into_iter()
        .map(|(path, _)| path)
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(".gcu-backup-"))
        })
        .collect()
}

#[cfg(unix)]
fn assert_owner_only(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "{}", path.display());
}

fn agy_lines(layout: &Layout) -> Vec<String> {
    if !layout.agy_log.exists() {
        return Vec::new();
    }
    fs::read_to_string(&layout.agy_log)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn assert_server(client: &str, home: &Path, gcu: &str, other: bool) {
    let text = fs::read_to_string(user_config(client, home)).unwrap();
    if matches!(client, "grok" | "codex") {
        let doc: toml::Value = toml::from_str(&text).unwrap();
        if other {
            assert_eq!(
                doc["mcp_servers"]["other"]["command"].as_str(),
                Some("other")
            );
        }
        let server = &doc["mcp_servers"]["grok-computer-use"];
        assert_eq!(server["command"].as_str(), Some(gcu));
        assert_eq!(server["args"][0].as_str(), Some("mcp"));
        return;
    }
    let value: Value = serde_json::from_str(&text).unwrap();
    if other {
        assert_eq!(value["keep"], true);
        assert_eq!(value[parent_key(client)]["other"]["command"], "other");
    }
    if client == "antigravity" {
        return;
    }
    let server = &value[parent_key(client)]["grok-computer-use"];
    match client {
        "opencode" => {
            assert_eq!(server["type"], "local");
            assert_eq!(server["enabled"], true);
            assert_eq!(server["command"][0], gcu);
            assert_eq!(server["command"][1], "mcp");
        }
        "cursor" | "vscode" => {
            assert_eq!(server["type"], "stdio");
            assert_eq!(server["command"], gcu);
            assert_eq!(server["args"][0], "mcp");
        }
        _ => {
            assert_eq!(server["command"], gcu);
            assert_eq!(server["args"][0], "mcp");
        }
    }
}

fn assert_removed(client: &str, home: &Path) {
    let path = user_config(client, home);
    let text = fs::read_to_string(&path).unwrap();
    if matches!(client, "grok" | "codex") {
        let doc: toml::Value = toml::from_str(&text).unwrap();
        assert!(doc["mcp_servers"].get("grok-computer-use").is_none());
        assert_eq!(
            doc["mcp_servers"]["other"]["command"].as_str(),
            Some("other")
        );
    } else {
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["keep"], true);
        assert_eq!(value[parent_key(client)]["other"]["command"], "other");
        assert!(value[parent_key(client)].get("grok-computer-use").is_none());
    }
}

#[cfg(unix)]
fn normalize(text: &str, layout: &Layout) -> String {
    let mut pairs = [
        layout.gcu.display().to_string(),
        layout.agy.display().to_string(),
        layout.project.display().to_string(),
        layout.home.display().to_string(),
    ];
    for pair in &mut pairs {
        *pair = pair.replace('\\', "/");
    }
    let mut text = text.replace('\\', "/");
    let tokens = ["$GCU", "$AGY", "$PROJECT", "$HOME"];
    for (path, token) in pairs.iter().zip(tokens) {
        text = text.replace(path, token);
    }
    text = text.replace("computer-use-turn-ended.sh", "$HOOK_FILE");
    text = text.replace("computer-use-turn-ended.cmd", "$HOOK_FILE");
    text = text.replace("\\\"$HOME", "$HOME");
    text = text.replace("$HOOK_FILE\\\"", "$HOOK_FILE");
    redact_hook(&text)
}

#[cfg(unix)]
fn redact_hook(text: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in text.lines() {
        let body = line.trim_start_matches(['+', ' ', '-']);
        if body.starts_with("#!/bin/sh") || body.starts_with("@echo off") {
            skipping = true;
            out.push_str("+$HOOK\n");
            continue;
        }
        if skipping {
            if body.starts_with("#!/") || line.starts_with("---") || line.starts_with("# ") {
                skipping = false;
            } else {
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn snap(name: &str, text: &str) {
    insta::with_settings!({
        prepend_module_to_snapshot => false,
        snapshot_path => "snapshots",
        omit_expression => true,
    }, {
        insta::assert_snapshot!(name, text);
    });
}

#[test]
fn install_help_lists_dry_run_yes_and_json() {
    let _guard = Guard::new();
    for verb in ["install", "uninstall"] {
        let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("gcu"));
        let output = cmd
            .arg(verb)
            .arg("--help")
            .env_remove("GCU_APPROVAL")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("--dry-run"), "{verb}: {text}");
        assert!(text.contains("--yes"), "{verb}: {text}");
        assert!(text.contains("--json"), "{verb}: {text}");
        assert!(text.contains("--client"), "{verb}: {text}");
        assert!(text.contains("--home"), "{verb}: {text}");
    }
}

#[test]
fn install_without_client_exits_two_and_unknown_client_is_rejected() {
    let _guard = Guard::new();
    Command::new(assert_cmd::cargo::cargo_bin!("gcu"))
        .arg("install")
        .env_remove("GCU_APPROVAL")
        .assert()
        .code(2);
    Command::new(assert_cmd::cargo::cargo_bin!("gcu"))
        .args(["install", "--client", "emacs"])
        .env_remove("GCU_APPROVAL")
        .assert()
        .code(2);
}

#[test]
fn cli_matrix_empty_other_idempotent_uninstall_dry_run_backup_and_confirm() {
    let _guard = Guard::new();
    for client in CLIENTS {
        empty_install(client);
        other_entries_backup_and_uninstall(client);
        idempotent(client);
        dry_run_writes_nothing(client);
        confirm_missing(client);
    }
}

fn empty_install(client: &str) {
    let layout = layout(&format!("empty-{client}"));
    let mut cmd = command(&layout);
    with_paths(&mut cmd, &layout, "install", client)
        .arg("--yes")
        .arg("--json");
    let output = cmd.assert().success().get_output().stdout.clone();
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["client"], client);
    assert_eq!(value["action"], "install");
    assert_eq!(value["dry_run"], false);
    assert_eq!(value["noop"], false);
    if client == "antigravity" {
        assert_eq!(
            agy_lines(&layout),
            vec![
                "mcp".to_owned(),
                "add".to_owned(),
                "grok-computer-use".to_owned(),
                "--".to_owned(),
                layout.gcu.display().to_string(),
                "mcp".to_owned(),
            ]
        );
        return;
    }
    assert_server(client, &layout.home, layout.gcu.to_str().unwrap(), false);
    if client == "grok" {
        let script = fs::read_to_string(
            layout
                .home
                .join(".grok/hooks")
                .join(gcu_cli::install::stop_hook_file_name()),
        )
        .unwrap();
        assert!(script.contains("turn-ended"), "{script}");
        assert!(script.contains("end_turn"), "{script}");
        assert!(script.contains("gcu-computer-use-stop"), "{script}");
    }
}

fn other_entries_backup_and_uninstall(client: &str) {
    let layout = layout(&format!("other-{client}"));
    let original = seed_other(client, &layout.home);
    let mut cmd = command(&layout);
    with_paths(&mut cmd, &layout, "install", client).arg("--yes");
    cmd.assert().success();
    let copies = backups(&layout.home);
    assert_eq!(copies.len(), 1, "{client}");
    assert_eq!(fs::read_to_string(&copies[0]).unwrap(), original);
    #[cfg(unix)]
    assert_owner_only(&copies[0]);
    if client == "antigravity" {
        assert_eq!(
            fs::read_to_string(user_config(client, &layout.home)).unwrap(),
            original
        );
        assert!(agy_lines(&layout).contains(&"add".to_owned()));
        // The fake `agy` does not write JSON. Seed the entry a real `agy` would
        // have added, then uninstall must only invoke `agy mcp remove`.
        seed_matching(client, &layout.home, layout.gcu.to_str().unwrap());
        let present = fs::read_to_string(user_config(client, &layout.home)).unwrap();
        let mut remove = command(&layout);
        with_paths(&mut remove, &layout, "uninstall", client)
            .arg("--yes")
            .arg("--json");
        remove.assert().success();
        let lines = agy_lines(&layout);
        assert_eq!(
            lines[lines.len() - 3..],
            [
                "mcp".to_owned(),
                "remove".to_owned(),
                "grok-computer-use".to_owned()
            ]
        );
        assert_eq!(
            fs::read_to_string(user_config(client, &layout.home)).unwrap(),
            present
        );
        return;
    }
    assert_server(client, &layout.home, layout.gcu.to_str().unwrap(), true);
    if matches!(client, "grok" | "codex") {
        let text = fs::read_to_string(user_config(client, &layout.home)).unwrap();
        assert!(text.contains("# keep"), "{text}");
    }
    let mut remove = command(&layout);
    with_paths(&mut remove, &layout, "uninstall", client)
        .arg("--yes")
        .arg("--json");
    remove.assert().success();
    assert_removed(client, &layout.home);
    if client == "grok" {
        assert!(!layout
            .home
            .join(".grok/hooks")
            .join(gcu_cli::install::stop_hook_file_name())
            .exists());
        assert!(!layout
            .home
            .join(".grok/hooks/computer-use-stop.json")
            .exists());
    }
}

fn idempotent(client: &str) {
    let layout = layout(&format!("idem-{client}"));
    if client == "antigravity" {
        seed_matching(client, &layout.home, layout.gcu.to_str().unwrap());
    } else {
        let mut cmd = command(&layout);
        with_paths(&mut cmd, &layout, "install", client).arg("--yes");
        cmd.assert().success();
    }
    let before = files_under(&layout.home);
    let backup_count = backups(&layout.home).len();
    let mut again = command(&layout);
    with_paths(&mut again, &layout, "install", client).arg("--json");
    let output = again.assert().success().get_output().stdout.clone();
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["noop"], true, "{client}");
    assert_eq!(value["backups"].as_array().unwrap().len(), 0);
    assert_eq!(files_under(&layout.home), before, "{client}");
    assert_eq!(backups(&layout.home).len(), backup_count, "{client}");
    if client == "antigravity" {
        assert!(agy_lines(&layout).is_empty(), "{client}");
    }
}

fn dry_run_writes_nothing(client: &str) {
    let layout = layout(&format!("dry-{client}"));
    let before = (files_under(&layout.home), files_under(&layout.project));
    let mut cmd = command(&layout);
    with_paths(&mut cmd, &layout, "install", client)
        .arg("--dry-run")
        .arg("--json");
    let output = cmd.assert().success().get_output().stdout.clone();
    assert_eq!(
        before,
        (files_under(&layout.home), files_under(&layout.project))
    );
    assert!(!layout.agy_log.exists());
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["noop"], false);
    assert!(value["backups"].as_array().unwrap().is_empty());
    #[cfg(unix)]
    {
        let diff = value["diff"].as_str().unwrap();
        snap(&format!("dry_run_{client}"), &normalize(diff, &layout));
    }
    let mut text = command(&layout);
    with_paths(&mut text, &layout, "install", client).arg("--dry-run");
    let stdout = String::from_utf8(text.assert().success().get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("dry-run: no files written"), "{stdout}");
    assert!(stdout.contains(client), "{stdout}");
}

fn confirm_missing(client: &str) {
    let layout = layout(&format!("ask-{client}"));
    let mut cmd = command(&layout);
    with_paths(&mut cmd, &layout, "install", client)
        .env("GCU_YES", "1")
        .env("YES", "1")
        .env("GROK_YES", "1")
        .env("GCU_CONFIRM", "yes")
        .write_stdin("y\nyes\n");
    if client == "grok" {
        let output = cmd.assert().code(2).get_output().clone();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("confirmation"), "{stderr}");
        assert!(output.stdout.is_empty());
        assert!(!layout.home.join(".grok").exists());
        return;
    }
    cmd.assert().success();
}

#[test]
fn home_flag_overrides_gcu_home_and_process_home() {
    let _guard = Guard::new();
    let layout = layout("flag");
    let gcu_home = temp("gcu-home");
    let process_home = temp("proc-home");
    let mut cmd = command(&layout);
    cmd.env("HOME", &process_home)
        .env("USERPROFILE", &process_home)
        .env("GCU_HOME", &gcu_home);
    with_paths(&mut cmd, &layout, "install", "claude").arg("--yes");
    cmd.assert().success();
    assert!(user_config("claude", &layout.home).exists());
    assert!(!user_config("claude", &gcu_home).exists());
    assert!(!user_config("claude", &process_home).exists());
}

#[test]
fn gcu_home_overrides_process_home() {
    let _guard = Guard::new();
    let layout = layout("env");
    let process_home = temp("proc-home");
    let mut cmd = command(&layout);
    cmd.env("HOME", &process_home)
        .env("USERPROFILE", &process_home)
        .env("GCU_HOME", &layout.home)
        .args(["install", "claude", "--gcu"])
        .arg(&layout.gcu)
        .arg("--project")
        .arg(&layout.project)
        .arg("--yes");
    cmd.assert().success();
    assert!(user_config("claude", &layout.home).exists());
    assert!(!user_config("claude", &process_home).exists());
}

#[test]
fn grok_home_redirects_config_and_still_requires_confirmation() {
    let _guard = Guard::new();
    let layout = layout("grok-home");
    let grok_home = temp("grok-root");
    let mut refused = command(&layout);
    with_paths(&mut refused, &layout, "install", "grok")
        .arg("--grok-home")
        .arg(&grok_home)
        .write_stdin("yes\n");
    refused.assert().code(2);
    assert!(!grok_home.join("config.toml").exists());
    assert!(!layout.home.join(".grok").exists());
    let mut cmd = command(&layout);
    with_paths(&mut cmd, &layout, "install", "grok")
        .arg("--grok-home")
        .arg(&grok_home)
        .arg("--yes");
    cmd.assert().success();
    assert!(grok_home.join("config.toml").exists());
    assert!(grok_home
        .join("hooks")
        .join(gcu_cli::install::stop_hook_file_name())
        .exists());
    assert!(!layout.home.join(".grok").exists());
}

#[test]
fn positional_client_matches_flag() {
    let _guard = Guard::new();
    let layout = layout("pos");
    let mut cmd = command(&layout);
    cmd.args(["install", "gemini", "--home"])
        .arg(&layout.home)
        .arg("--gcu")
        .arg(&layout.gcu)
        .arg("--yes");
    cmd.assert().success();
    assert_server("gemini", &layout.home, layout.gcu.to_str().unwrap(), false);
}

#[test]
fn antigravity_project_uninstall_removes_only_our_key() {
    let _guard = Guard::new();
    let layout = layout("agy-proj");
    let path = layout.project.join(".agents/mcp_config.json");
    let gcu = layout.gcu.to_str().unwrap().replace('\\', "\\\\");
    write_file(
        &path,
        &format!(
            "{{\"keep\":true,\"mcpServers\":{{\"other\":{{\"command\":\"other\"}},\"grok-computer-use\":{{\"command\":\"{gcu}\",\"args\":[\"mcp\"]}}}}}}\n"
        ),
    );
    let mut cmd = command(&layout);
    with_paths(&mut cmd, &layout, "uninstall", "antigravity")
        .arg("--scope")
        .arg("project")
        .arg("--yes");
    cmd.assert().success();
    let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value["keep"], true);
    assert_eq!(value["mcpServers"]["other"]["command"], "other");
    assert!(value["mcpServers"].get("grok-computer-use").is_none());
    assert!(agy_lines(&layout).is_empty());
}

#[test]
fn windows_stop_hook_calls_turn_ended_and_quotes_paths() {
    let script = gcu_cli::install::windows_stop_hook_script(r"C:\Program Files\gcu.exe");
    assert!(script.contains("turn-ended"), "{script}");
    assert!(script.contains("end_turn"), "{script}");
    assert!(script.contains("rem gcu-computer-use-stop"), "{script}");
    assert!(script.contains(r"C:\Program Files\gcu.exe"), "{script}");
    assert!(script.contains("\"%GCU%\" turn-ended"), "{script}");
    let percent = gcu_cli::install::windows_stop_hook_script(r"C:\a%b\gcu.exe");
    assert!(percent.contains(r"C:\a%%b\gcu.exe"), "{percent}");
    assert!(script.contains("\r\n"), "{script:?}");
    assert!(script.split("\r\n").all(|line| !line.contains('\n')));
    snap("windows_stop_hook", &script.replace("\r\n", "\n"));
}

#[cfg(unix)]
#[test]
fn grok_install_tty_yes_writes_and_other_answers_do_not() {
    let _guard = Guard::new();
    for answer in ["n\n", "y\n"] {
        let layout = layout("pty");
        let (mut master, slave) = open_pty().unwrap();
        let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin!("gcu"))
            .args(["install", "--client", "grok", "--home"])
            .arg(&layout.home)
            .arg("--project")
            .arg(&layout.project)
            .arg("--gcu")
            .arg(&layout.gcu)
            .env("HOME", &layout.home)
            .env("USERPROFILE", &layout.home)
            .env_remove("GCU_HOME")
            .env_remove("GCU_YES")
            .env_remove("YES")
            .env_remove("GCU_APPROVAL")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave))
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut seen = String::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !seen.contains("[y/N]") {
            assert!(Instant::now() < deadline, "prompt timed out: {seen}");
            let mut buf = [0_u8; 1024];
            match std::io::Read::read(&mut master, &mut buf) {
                Ok(0) => break,
                Ok(n) => seen.push_str(&String::from_utf8_lossy(&buf[..n])),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("{err}"),
            }
        }
        use std::io::Write;
        write!(master, "{answer}").unwrap();
        let status = child.wait().unwrap();
        let wrote = layout.home.join(".grok/config.toml").exists();
        if answer.starts_with('y') {
            assert!(status.success(), "{seen}");
            assert!(wrote);
        } else {
            assert!(!wrote);
            assert!(!status.success());
        }
    }
}

#[cfg(unix)]
fn open_pty() -> std::io::Result<(std::fs::File, std::fs::File)> {
    use std::os::fd::FromRawFd;
    let (mut master, mut slave) = (-1, -1);
    // SAFETY: the integers are writable outputs; null termios and winsize ask
    // the system for a fresh test-only terminal.
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: openpty returned two distinct owned descriptors.
    let (master_file, slave_file) = unsafe {
        (
            std::fs::File::from_raw_fd(master),
            std::fs::File::from_raw_fd(slave),
        )
    };
    // SAFETY: master stays open; F_SETFL only sets its nonblocking flag.
    if unsafe { libc::fcntl(master, libc::F_SETFL, libc::O_NONBLOCK) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((master_file, slave_file))
}
