//! Install and uninstall matrix. Homes are temporary directories.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gcu_cli::install::{
    installer, ClientBins, ClientId, InstallCtx, InstallError, RecordingExecutor, Scope,
    StaticConfirmer, SERVER_NAME,
};
use serde_json::Value;

const STAMP: &str = "20261013T180000Z";

struct Temp {
    path: PathBuf,
}

impl Temp {
    fn new(label: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("gcu-p405a-{label}-{n}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct Fix {
    home: Temp,
    project: Temp,
    gcu: PathBuf,
    agy: PathBuf,
    executor: RecordingExecutor,
    confirmer: StaticConfirmer,
}

fn fix(label: &str) -> Fix {
    let home = Temp::new(&format!("{label}-h"));
    let project = Temp::new(&format!("{label}-p"));
    let gcu = project.path.join("bin").join("gcu");
    let agy = project.path.join("bin").join("agy");
    fs::create_dir_all(gcu.parent().unwrap()).unwrap();
    fs::write(&gcu, b"#!/bin/sh\nexit 0\n").unwrap();
    fs::write(&agy, b"#!/bin/sh\nexit 0\n").unwrap();
    Fix {
        home,
        project,
        gcu,
        agy,
        executor: RecordingExecutor::new(),
        confirmer: StaticConfirmer { accept: false },
    }
}

fn ctx<'a>(fix: &'a Fix, scope: Scope, interactive: bool, yes: bool) -> InstallCtx<'a> {
    InstallCtx {
        home: &fix.home.path,
        project_dir: &fix.project.path,
        gcu_exe: &fix.gcu,
        scope,
        interactive,
        yes,
        backup_stamp: Some(STAMP),
        confirmer: &fix.confirmer,
        executor: &fix.executor,
        bins: ClientBins {
            agy: Some(&fix.agy),
        },
    }
}

/// Escape backslashes so a Windows path can sit inside a JSON or TOML basic string.
fn esc(text: &str) -> String {
    text.replace('\\', "\\\\")
}

fn gcu_str(fix: &Fix) -> &str {
    fix.gcu.to_str().unwrap()
}

fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

fn walk(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    fn rec(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        let Ok(read) = fs::read_dir(dir) else {
            return;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                rec(&path, out);
            } else {
                out.push((path.clone(), fs::read(&path).unwrap()));
            }
        }
    }
    rec(root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn backup_paths(root: &Path) -> Vec<PathBuf> {
    walk(root)
        .into_iter()
        .map(|(path, _)| path)
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(".gcu-backup-"))
        })
        .collect()
}

fn toml_at(path: &Path) -> toml::Value {
    toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn tget<'a>(value: &'a toml::Value, key: &str) -> &'a toml::Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}"))
}

#[derive(Clone, Copy)]
enum Kind {
    Command,
    Stdio,
    Local,
}

struct Case {
    id: ClientId,
    scope: Scope,
    parent: &'static str,
    kind: Kind,
    rel: &'static str,
}

fn json_cases() -> Vec<Case> {
    let specs = [
        (
            ClientId::Claude,
            "mcpServers",
            Kind::Command,
            ".claude.json",
            ".mcp.json",
        ),
        (
            ClientId::Gemini,
            "mcpServers",
            Kind::Command,
            ".gemini/settings.json",
            ".gemini/settings.json",
        ),
        (
            ClientId::Opencode,
            "mcp",
            Kind::Local,
            ".config/opencode/opencode.json",
            "opencode.json",
        ),
        (
            ClientId::Cursor,
            "mcpServers",
            Kind::Stdio,
            ".cursor/mcp.json",
            ".cursor/mcp.json",
        ),
        (
            ClientId::VsCode,
            "servers",
            Kind::Stdio,
            vscode_user_rel(),
            ".vscode/mcp.json",
        ),
        (
            ClientId::Antigravity,
            "mcpServers",
            Kind::Command,
            "",
            ".agents/mcp_config.json",
        ),
    ];
    let mut out = Vec::new();
    for (id, parent, kind, user, project) in specs {
        if !user.is_empty() {
            out.push(Case {
                id,
                scope: Scope::User,
                parent,
                kind,
                rel: user,
            });
        }
        out.push(Case {
            id,
            scope: Scope::Project,
            parent,
            kind,
            rel: project,
        });
    }
    out
}

fn case_path(fix: &Fix, case: &Case) -> PathBuf {
    let root = match case.scope {
        Scope::User => &fix.home.path,
        Scope::Project => &fix.project.path,
    };
    root.join(case.rel)
}

fn assert_entry(value: &Value, case: &Case, gcu: &str) {
    let server = &value[case.parent][SERVER_NAME];
    match case.kind {
        Kind::Command => {
            assert_eq!(server["command"], gcu, "{}", case.id);
            assert_eq!(server["args"], serde_json::json!(["mcp"]));
            assert!(server.get("type").is_none());
        }
        Kind::Stdio => {
            assert_eq!(server["type"], "stdio");
            assert_eq!(server["command"], gcu);
            assert_eq!(server["args"], serde_json::json!(["mcp"]));
        }
        Kind::Local => {
            assert_eq!(server["type"], "local");
            assert_eq!(server["command"], serde_json::json!([gcu, "mcp"]));
            assert_eq!(server["enabled"], true);
        }
    }
}

fn entry_json(case: &Case, gcu: &str) -> String {
    let gcu = esc(gcu);
    match case.kind {
        Kind::Command => format!(r#"{{"note":"stay","command":"{gcu}","args":["mcp"]}}"#),
        Kind::Stdio => {
            format!(r#"{{"note":"stay","type":"stdio","command":"{gcu}","args":["mcp"]}}"#)
        }
        Kind::Local => {
            format!(r#"{{"note":"stay","type":"local","command":["{gcu}","mcp"],"enabled":true}}"#)
        }
    }
}

fn seeded(case: &Case, gcu: &str, include_self: bool) -> String {
    let ours = if include_self {
        format!(",\n    \"{SERVER_NAME}\": {}", entry_json(case, gcu))
    } else {
        String::new()
    };
    format!(
        "{{\n  \"keep\": true,\n  \"{}\": {{\n    \"other\": {{\"command\": \"other\"}}{ours}\n  }}\n}}\n",
        case.parent
    )
}

#[test]
fn json_install_missing_or_empty_file_writes_server_entry() {
    for case in json_cases() {
        for empty in [false, true] {
            let label = format!("json-empty-{}-{}", case.id, empty);
            let fix = fix(&label);
            let path = case_path(&fix, &case);
            if empty {
                write(&path, " \n");
            }
            let outcome = installer(case.id)
                .apply(&ctx(&fix, case.scope, false, false))
                .unwrap();
            assert!(!outcome.noop, "{}", case.id);
            if empty {
                assert_eq!(outcome.backups.len(), 1, "{}", case.id);
            } else {
                assert!(outcome.backups.is_empty(), "{}", case.id);
            }
            let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            assert_entry(&value, &case, gcu_str(&fix));
            assert_eq!(value["keep"], Value::Null);
            let status = installer(case.id)
                .status(&ctx(&fix, case.scope, false, false))
                .unwrap();
            assert!(status.present && status.matches_gcu, "{}", case.id);
        }
    }
}

#[test]
fn json_install_other_servers_preserves_them_and_key_order() {
    for case in json_cases() {
        let fix = fix(&format!("json-other-{}", case.id));
        let path = case_path(&fix, &case);
        let original = seeded(&case, gcu_str(&fix), false);
        write(&path, &original);
        installer(case.id)
            .apply(&ctx(&fix, case.scope, false, false))
            .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["keep"], true, "{}", case.id);
        assert_eq!(value[case.parent]["other"]["command"], "other");
        assert_entry(&value, &case, gcu_str(&fix));
        let keys: Vec<_> = value[case.parent]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(
            keys,
            vec!["other".to_owned(), SERVER_NAME.to_owned()],
            "{}",
            case.id
        );
        let backup = backup_paths(path.parent().unwrap());
        assert_eq!(backup.len(), 1, "{}", case.id);
        assert_eq!(fs::read_to_string(&backup[0]).unwrap(), original);
    }
}

#[test]
fn json_install_existing_entry_is_idempotent() {
    for case in json_cases() {
        let fix = fix(&format!("json-idem-{}", case.id));
        let path = case_path(&fix, &case);
        let original = seeded(&case, gcu_str(&fix), true);
        write(&path, &original);
        let outcome = installer(case.id)
            .apply(&ctx(&fix, case.scope, false, false))
            .unwrap();
        assert!(outcome.noop, "{}", case.id);
        assert!(outcome.backups.is_empty(), "{}", case.id);
        assert_eq!(fs::read_to_string(&path).unwrap(), original, "{}", case.id);
        assert!(backup_paths(&fix.home.path).is_empty());
        assert!(backup_paths(&fix.project.path).is_empty());
    }
}

#[test]
fn json_uninstall_removes_only_computer_use() {
    for case in json_cases() {
        let fix = fix(&format!("json-rm-{}", case.id));
        let path = case_path(&fix, &case);
        let original = seeded(&case, gcu_str(&fix), true);
        write(&path, &original);
        let inst = installer(case.id);
        let ctx = ctx(&fix, case.scope, false, false);
        let outcome = inst.uninstall(&ctx).unwrap();
        assert!(!outcome.noop, "{}", case.id);
        assert_eq!(fs::read(&outcome.backups[0]).unwrap(), original.as_bytes());
        let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["keep"], true);
        assert_eq!(value[case.parent]["other"]["command"], "other");
        assert!(value[case.parent].get(SERVER_NAME).is_none(), "{}", case.id);
        assert!(value[case.parent]
            .as_object()
            .unwrap()
            .contains_key("other"));
        let again = inst.uninstall(&ctx).unwrap();
        assert!(again.noop, "{}", case.id);
        let status = inst.status(&ctx).unwrap();
        assert!(!status.present && !status.matches_gcu);
    }
}

#[test]
fn json_install_bad_file_leaves_file_unchanged() {
    for case in json_cases() {
        let wrong = format!(r#"{{"{}":[]}}"#, case.parent);
        let samples = [b"{".to_vec(), b"[]".to_vec(), wrong.into_bytes()];
        for bytes in samples {
            let fix = fix(&format!("json-bad-{}", case.id));
            let path = case_path(&fix, &case);
            write(&path, &bytes);
            let err = installer(case.id)
                .apply(&ctx(&fix, case.scope, false, false))
                .unwrap_err();
            assert!(
                matches!(
                    err,
                    InstallError::Unparseable { .. } | InstallError::UnexpectedShape { .. }
                ),
                "{err}"
            );
            assert_eq!(fs::read(&path).unwrap(), bytes, "{}", case.id);
            assert!(backup_paths(&fix.home.path).is_empty());
            assert!(backup_paths(&fix.project.path).is_empty());
        }
    }
}

#[test]
fn json_plan_dry_run_does_not_write() {
    for case in json_cases() {
        let fix = fix(&format!("json-dry-{}", case.id));
        let before = (walk(&fix.home.path), walk(&fix.project.path));
        let plan = installer(case.id)
            .plan(&ctx(&fix, case.scope, false, false))
            .unwrap();
        assert!(!plan.is_noop());
        assert!(plan.render_diff().contains(case.rel) || plan.render_diff().contains(SERVER_NAME));
        assert_eq!(before, (walk(&fix.home.path), walk(&fix.project.path)));
        assert!(fix.executor.calls().unwrap().is_empty());
    }
}

#[test]
fn grok_install_missing_file_writes_server_and_stop_hook() {
    let fix = fix("grok-missing");
    let inst = installer(ClientId::Grok);
    let ctx = ctx(&fix, Scope::User, false, true);
    let outcome = inst.apply(&ctx).unwrap();
    assert!(!outcome.noop);
    assert!(outcome.backups.is_empty());
    let doc = toml_at(&fix.home.path.join(".grok/config.toml"));
    let server = tget(tget(&doc, "mcp_servers"), SERVER_NAME);
    assert_eq!(tget(server, "command").as_str(), Some(gcu_str(&fix)));
    assert_eq!(
        tget(server, "args").as_array().unwrap()[0].as_str(),
        Some("mcp")
    );
    assert!(server.get("enabled").is_none());
    let script =
        fs::read_to_string(fix.home.path.join(".grok/hooks/computer-use-turn-ended.sh")).unwrap();
    assert!(script.contains("# gcu-computer-use-stop"));
    assert!(script.contains("end_turn"));
    assert!(script.contains("turn-ended"));
    assert!(script.contains(gcu_str(&fix)));
    assert!(!script.contains("set -e"));
    let hook: Value = serde_json::from_str(
        &fs::read_to_string(fix.home.path.join(".grok/hooks/computer-use-stop.json")).unwrap(),
    )
    .unwrap();
    let command = hook["hooks"]["Stop"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    // The Stop hook script is a Unix shell script; Windows quotes the command differently.
    if cfg!(unix) {
        assert!(command.ends_with("computer-use-turn-ended.sh"));
    }
    assert_eq!(hook["hooks"]["Stop"][0]["hooks"][0]["timeout"], 10);
    assert_eq!(hook["hooks"]["Stop"][0]["hooks"][0]["type"], "command");
    let status = inst.status(&ctx).unwrap();
    assert!(status.present && status.matches_gcu);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(fix.home.path.join(".grok/hooks/computer-use-turn-ended.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111);
    }
}

#[test]
fn grok_install_project_scope_writes_project_config_and_user_hook() {
    let fix = fix("grok-project");
    installer(ClientId::Grok)
        .apply(&ctx(&fix, Scope::Project, false, true))
        .unwrap();
    assert!(!fix.home.path.join(".grok/config.toml").exists());
    assert!(fix.project.path.join(".grok/config.toml").exists());
    assert!(fix
        .home
        .path
        .join(".grok/hooks/computer-use-stop.json")
        .exists());
    let doc = toml_at(&fix.project.path.join(".grok/config.toml"));
    assert_eq!(
        tget(tget(tget(&doc, "mcp_servers"), SERVER_NAME), "command").as_str(),
        Some(gcu_str(&fix))
    );
}

#[test]
fn grok_install_other_servers_preserves_them() {
    let fix = fix("grok-other");
    let path = fix.home.path.join(".grok/config.toml");
    let original =
        "# user comment\n\n[mcp_servers.other]\ncommand = \"other\"\n# keep me\nargs = [\"x\"]\n"
            .to_string();
    write(&path, &original);
    installer(ClientId::Grok)
        .apply(&ctx(&fix, Scope::User, false, true))
        .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("# user comment"));
    assert!(text.contains("# keep me"));
    let doc = toml::from_str::<toml::Value>(&text).unwrap();
    assert_eq!(
        tget(tget(tget(&doc, "mcp_servers"), "other"), "command").as_str(),
        Some("other")
    );
    assert_eq!(
        tget(tget(tget(&doc, "mcp_servers"), SERVER_NAME), "command").as_str(),
        Some(gcu_str(&fix))
    );
    let backups = backup_paths(&fix.home.path);
    assert!(backups
        .iter()
        .any(|path| path.ends_with("config.toml.gcu-backup-20261013T180000Z")));
    let config_backup = backups.iter().find(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("config.toml.gcu-backup-"))
    });
    assert_eq!(
        fs::read_to_string(config_backup.unwrap()).unwrap(),
        original
    );
}

#[test]
fn grok_install_existing_entry_is_idempotent() {
    let fix = fix("grok-idem");
    let inst = installer(ClientId::Grok);
    inst.apply(&ctx(&fix, Scope::User, false, true)).unwrap();
    let before = walk(&fix.home.path);
    let outcome = inst.apply(&ctx(&fix, Scope::User, false, false)).unwrap();
    assert!(outcome.noop);
    assert!(outcome.backups.is_empty());
    assert_eq!(walk(&fix.home.path), before);
}

#[test]
fn grok_uninstall_removes_only_computer_use() {
    let fix = fix("grok-rm");
    let inst = installer(ClientId::Grok);
    let yes = ctx(&fix, Scope::User, false, true);
    inst.apply(&yes).unwrap();
    let path = fix.home.path.join(".grok/config.toml");
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("\n[mcp_servers.other]\ncommand = \"other\"\n");
    fs::write(&path, &text).unwrap();
    inst.uninstall(&yes).unwrap();
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.contains("other"));
    assert!(!after.contains(SERVER_NAME));
    assert!(!fix
        .home
        .path
        .join(".grok/hooks/computer-use-turn-ended.sh")
        .exists());
    assert!(!fix
        .home
        .path
        .join(".grok/hooks/computer-use-stop.json")
        .exists());
    let status = inst.status(&yes).unwrap();
    assert!(!status.present);
    let again = inst
        .uninstall(&ctx(&fix, Scope::User, false, false))
        .unwrap();
    assert!(again.noop);
}

#[test]
fn grok_install_bad_toml_leaves_file_unchanged() {
    let fix = fix("grok-bad");
    let path = fix.home.path.join(".grok/config.toml");
    let bytes = b"this is [[[ not toml";
    write(&path, bytes);
    let err = installer(ClientId::Grok)
        .apply(&ctx(&fix, Scope::User, false, true))
        .unwrap_err();
    assert!(matches!(err, InstallError::Unparseable { .. }), "{err}");
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(backup_paths(&fix.home.path).is_empty());
    assert!(!fix.home.path.join(".grok/hooks").exists());
}

#[test]
fn grok_install_mcp_servers_wrong_type_leaves_file_unchanged() {
    let fix = fix("grok-shape");
    let path = fix.home.path.join(".grok/config.toml");
    let bytes = b"mcp_servers = \"nope\"\n";
    write(&path, bytes);
    let err = installer(ClientId::Grok)
        .apply(&ctx(&fix, Scope::User, false, true))
        .unwrap_err();
    assert!(matches!(err, InstallError::UnexpectedShape { .. }), "{err}");
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(backup_paths(&fix.home.path).is_empty());
}

#[test]
fn grok_apply_noninteractive_without_yes_does_not_write() {
    let fix = fix("grok-noyes");
    let err = installer(ClientId::Grok)
        .apply(&ctx(&fix, Scope::User, false, false))
        .unwrap_err();
    assert!(matches!(err, InstallError::ConfirmationRequired));
    assert!(!fix.home.path.join(".grok").exists());
    let err = installer(ClientId::Grok)
        .apply(&ctx(&fix, Scope::Project, false, false))
        .unwrap_err();
    assert!(matches!(err, InstallError::ConfirmationRequired));
    assert!(!fix.project.path.join(".grok").exists());
    assert!(!fix.home.path.join(".grok").exists());
}

#[test]
fn grok_install_interactive_decline_does_not_write() {
    let fix = fix("grok-no");
    let err = installer(ClientId::Grok)
        .apply(&ctx(&fix, Scope::User, true, false))
        .unwrap_err();
    assert!(matches!(err, InstallError::ConfirmationDeclined));
    assert!(!fix.home.path.join(".grok").exists());
}

#[test]
fn grok_install_interactive_accept_writes() {
    let fix = fix("grok-yes");
    let confirmer = StaticConfirmer { accept: true };
    let ctx = InstallCtx {
        confirmer: &confirmer,
        interactive: true,
        yes: false,
        ..ctx(&fix, Scope::User, true, false)
    };
    installer(ClientId::Grok).apply(&ctx).unwrap();
    assert!(fix.home.path.join(".grok/config.toml").exists());
}

#[test]
fn grok_plan_dry_run_does_not_write() {
    let fix = fix("grok-dry");
    let before = (walk(&fix.home.path), walk(&fix.project.path));
    let plan = installer(ClientId::Grok)
        .plan(&ctx(&fix, Scope::User, false, false))
        .unwrap();
    assert!(!plan.is_noop());
    let diff = plan.render_diff();
    assert!(diff.contains("config.toml"), "{diff}");
    assert!(diff.contains(SERVER_NAME) || diff.contains("mcp"));
    assert_eq!(before, (walk(&fix.home.path), walk(&fix.project.path)));
    let removal = installer(ClientId::Grok)
        .plan_removal(&ctx(&fix, Scope::User, false, false))
        .unwrap();
    assert!(removal.is_noop(), "{}", removal.render_diff());
    assert_eq!(before, (walk(&fix.home.path), walk(&fix.project.path)));
}

#[test]
fn grok_install_preserves_comments() {
    let fix = fix("grok-comments");
    let path = fix.home.path.join(".grok/config.toml");
    let original = format!(
        "# user comment\n[mcp_servers.grok-computer-use]\ncommand = \"{}\"\nargs = [\"mcp\"]\n# trailing\n",
        esc(gcu_str(&fix))
    );
    write(&path, &original);
    installer(ClientId::Grok)
        .apply(&ctx(&fix, Scope::User, false, true))
        .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    assert!(fix
        .home
        .path
        .join(".grok/hooks/computer-use-stop.json")
        .exists());
    assert!(backup_paths(&fix.home.path).iter().all(|path| !path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("config.toml.gcu-backup-")));
}

#[test]
fn grok_install_bad_hook_json_leaves_file_unchanged() {
    let fix = fix("grok-bad-hook");
    let path = fix.home.path.join(".grok/hooks/computer-use-stop.json");
    write(&path, b"{");
    let err = installer(ClientId::Grok)
        .apply(&ctx(&fix, Scope::User, false, true))
        .unwrap_err();
    assert!(matches!(err, InstallError::Unparseable { .. }), "{err}");
    assert_eq!(fs::read(&path).unwrap(), b"{");
    assert!(!fix.home.path.join(".grok/config.toml").exists());
    assert!(backup_paths(&fix.home.path).is_empty());
}

#[test]
fn grok_install_foreign_stop_script_leaves_files_unchanged() {
    let fix = fix("grok-foreign");
    let path = fix.home.path.join(".grok/hooks/computer-use-turn-ended.sh");
    write(&path, b"#!/bin/sh\necho user\n");
    let err = installer(ClientId::Grok)
        .apply(&ctx(&fix, Scope::User, false, true))
        .unwrap_err();
    assert!(matches!(err, InstallError::ForeignFile { .. }), "{err}");
    assert_eq!(fs::read(&path).unwrap(), b"#!/bin/sh\necho user\n");
    assert!(!fix.home.path.join(".grok/config.toml").exists());
}

#[test]
fn grok_install_gcu_path_with_space_is_shell_quoted() {
    let mut fix = fix("grok-space");
    fix.gcu = fix.project.path.join("My Tools").join("gcu");
    fs::create_dir_all(fix.gcu.parent().unwrap()).unwrap();
    fs::write(&fix.gcu, b"#!/bin/sh\n").unwrap();
    installer(ClientId::Grok)
        .apply(&ctx(&fix, Scope::User, false, true))
        .unwrap();
    let script =
        fs::read_to_string(fix.home.path.join(".grok/hooks/computer-use-turn-ended.sh")).unwrap();
    assert!(script.contains(&format!("'{}'", gcu_str(&fix))), "{script}");
}

#[path = "install_support/cli_clients.rs"]
mod cli_clients;

/// VS Code keeps its user `mcp.json` in a per-OS directory under the home directory.
fn vscode_user_rel() -> &'static str {
    if cfg!(target_os = "windows") {
        "AppData/Roaming/Code/User/mcp.json"
    } else if cfg!(target_os = "macos") {
        "Library/Application Support/Code/User/mcp.json"
    } else {
        ".config/Code/User/mcp.json"
    }
}
