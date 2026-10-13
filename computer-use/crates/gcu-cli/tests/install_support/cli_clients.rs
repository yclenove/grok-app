//! Codex and Antigravity install tests.

use std::fs;
use std::path::PathBuf;

use gcu_cli::install::{installer, ClientId, InstallError, Scope, SERVER_NAME};

use super::{backup_paths, ctx, fix, gcu_str, tget, toml_at, walk, write};

#[test]
fn codex_install_missing_file_writes_server_and_notify() {
    let fix = fix("codex-missing");
    installer(ClientId::Codex)
        .apply(&ctx(&fix, Scope::User, false, false))
        .unwrap();
    let doc = toml_at(&fix.home.path.join(".codex/config.toml"));
    let server = tget(tget(&doc, "mcp_servers"), SERVER_NAME);
    assert_eq!(tget(server, "command").as_str(), Some(gcu_str(&fix)));
    assert_eq!(
        tget(server, "args").as_array().unwrap()[0].as_str(),
        Some("mcp")
    );
    assert!(server.get("enabled").is_none());
    let notify = doc.get("notify").unwrap().as_array().unwrap();
    assert_eq!(notify.len(), 2);
    assert_eq!(notify[0].as_str(), Some(gcu_str(&fix)));
    assert_eq!(notify[1].as_str(), Some("turn-ended"));
    let status = installer(ClientId::Codex)
        .status(&ctx(&fix, Scope::User, false, false))
        .unwrap();
    assert!(status.present && status.matches_gcu);
}

#[test]
fn codex_install_other_servers_preserves_them() {
    let fix = fix("codex-other");
    let path = fix.home.path.join(".codex/config.toml");
    let original = "# codex comment\n[mcp_servers.other]\ncommand = \"other\"\n";
    write(&path, original);
    installer(ClientId::Codex)
        .apply(&ctx(&fix, Scope::User, false, false))
        .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("# codex comment"));
    let doc = toml::from_str::<toml::Value>(&text).unwrap();
    assert_eq!(
        tget(tget(tget(&doc, "mcp_servers"), "other"), "command").as_str(),
        Some("other")
    );
    assert_eq!(
        tget(tget(tget(&doc, "mcp_servers"), SERVER_NAME), "args")
            .as_array()
            .unwrap()[0]
            .as_str(),
        Some("mcp")
    );
}

#[test]
fn codex_install_existing_entry_is_idempotent() {
    let fix = fix("codex-idem");
    let inst = installer(ClientId::Codex);
    let ctx = ctx(&fix, Scope::User, false, false);
    inst.apply(&ctx).unwrap();
    let before = walk(&fix.home.path);
    let outcome = inst.apply(&ctx).unwrap();
    assert!(outcome.noop);
    assert_eq!(walk(&fix.home.path), before);
}

#[test]
fn codex_uninstall_removes_only_computer_use() {
    let fix = fix("codex-rm");
    let path = fix.home.path.join(".codex/config.toml");
    let original = format!(
        "# codex comment\n[mcp_servers.other]\ncommand = \"other\"\n\n[mcp_servers.computer-use]\ncommand = \"{}\"\nargs = [\"mcp\"]\nnotify = [\"{}\", \"turn-ended\"]\n",
        gcu_str(&fix),
        gcu_str(&fix)
    );
    write(&path, &original);
    let inst = installer(ClientId::Codex);
    inst.uninstall(&ctx(&fix, Scope::User, false, false))
        .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("# codex comment"));
    assert!(text.contains("other"));
    assert!(!text.contains(SERVER_NAME));
    assert!(!text.contains("notify"));
    let again = inst
        .uninstall(&ctx(&fix, Scope::User, false, false))
        .unwrap();
    assert!(again.noop);
}

#[test]
fn codex_install_bad_toml_leaves_file_unchanged() {
    let fix = fix("codex-bad");
    let path = fix.home.path.join(".codex/config.toml");
    write(&path, b"[[[");
    let err = installer(ClientId::Codex)
        .apply(&ctx(&fix, Scope::User, false, false))
        .unwrap_err();
    assert!(matches!(
        err,
        InstallError::Unparseable { .. } | InstallError::UnexpectedShape { .. }
    ));
    assert_eq!(fs::read(&path).unwrap(), b"[[[");
    assert!(backup_paths(&fix.home.path).is_empty());
}

#[test]
fn codex_install_existing_notify_chains_previous() {
    let fix = fix("codex-chain");
    let path = fix.home.path.join(".codex/config.toml");
    write(&path, "notify = [\"old\", \"bin\"]\n# keep\n");
    installer(ClientId::Codex)
        .apply(&ctx(&fix, Scope::User, false, false))
        .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("# keep"));
    let doc = toml::from_str::<toml::Value>(&text).unwrap();
    let notify: Vec<_> = doc
        .get("notify")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        notify,
        vec![
            gcu_str(&fix),
            "turn-ended",
            "--previous-notify",
            "old",
            "bin"
        ]
    );
}

#[test]
fn codex_uninstall_restores_previous_notify() {
    let fix = fix("codex-restore");
    let inst = installer(ClientId::Codex);
    let ctx = ctx(&fix, Scope::User, false, false);
    let path = fix.home.path.join(".codex/config.toml");
    write(&path, "notify = [\"old\", \"bin\"]\n");
    inst.apply(&ctx).unwrap();
    inst.uninstall(&ctx).unwrap();
    let doc = toml_at(&path);
    assert!(doc
        .get("mcp_servers")
        .and_then(|v| v.get(SERVER_NAME))
        .is_none());
    let notify: Vec<_> = doc
        .get("notify")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(notify, vec!["old", "bin"]);
}

#[test]
fn codex_install_invalid_notify_does_not_write() {
    let fix = fix("codex-bad-notify");
    let path = fix.home.path.join(".codex/config.toml");
    let bytes = b"notify = \"nope\"\n# stay\n";
    write(&path, bytes);
    let err = installer(ClientId::Codex)
        .apply(&ctx(&fix, Scope::User, false, false))
        .unwrap_err();
    assert!(matches!(err, InstallError::UnexpectedShape { .. }), "{err}");
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(backup_paths(&fix.home.path).is_empty());
}

#[test]
fn codex_project_scope_keeps_notify_in_user_config() {
    let fix = fix("codex-proj");
    let inst = installer(ClientId::Codex);
    inst.apply(&ctx(&fix, Scope::Project, false, false))
        .unwrap();
    let project = toml_at(&fix.project.path.join(".codex/config.toml"));
    assert!(project.get("notify").is_none());
    assert_eq!(
        tget(tget(tget(&project, "mcp_servers"), SERVER_NAME), "command").as_str(),
        Some(gcu_str(&fix))
    );
    let user = toml_at(&fix.home.path.join(".codex/config.toml"));
    assert!(user.get("mcp_servers").is_none());
    assert_eq!(
        user.get("notify").unwrap().as_array().unwrap()[1].as_str(),
        Some("turn-ended")
    );
    inst.uninstall(&ctx(&fix, Scope::Project, false, false))
        .unwrap();
    assert!(fix.project.path.join(".codex/config.toml").exists());
    let project = toml_at(&fix.project.path.join(".codex/config.toml"));
    assert!(project
        .get("mcp_servers")
        .and_then(|v| v.get(SERVER_NAME))
        .is_none());
    let user_text = fs::read_to_string(fix.home.path.join(".codex/config.toml")).unwrap();
    assert!(!user_text.contains("notify"), "{user_text}");
}

#[test]
fn codex_plan_dry_run_does_not_write() {
    let fix = fix("codex-dry");
    let before = walk(&fix.home.path);
    let plan = installer(ClientId::Codex)
        .plan(&ctx(&fix, Scope::User, false, false))
        .unwrap();
    assert!(!plan.is_noop());
    assert!(plan.render_diff().contains("config.toml"));
    assert_eq!(walk(&fix.home.path), before);
}

#[test]
fn antigravity_user_install_missing_file_plans_agy_add() {
    let fix = fix("agy-missing");
    let inst = installer(ClientId::Antigravity);
    let ctx = ctx(&fix, Scope::User, false, false);
    let before = walk(&fix.home.path);
    let plan = inst.plan(&ctx).unwrap();
    assert!(fix.executor.calls().unwrap().is_empty());
    assert!(plan.render_diff().contains("mcp add"));
    assert_eq!(walk(&fix.home.path), before);
    inst.apply(&ctx).unwrap();
    let calls = fix.executor.calls().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].args,
        vec![
            "mcp".to_owned(),
            "add".to_owned(),
            SERVER_NAME.to_owned(),
            "--".to_owned(),
            gcu_str(&fix).to_owned(),
            "mcp".to_owned()
        ]
    );
    assert_eq!(calls[0].program, fix.agy);
    assert_eq!(calls[0].cwd.as_deref(), Some(fix.project.path.as_path()));
    assert!(calls[0]
        .env
        .iter()
        .any(|(k, v)| k == "HOME" && v == fix.home.path.to_str().unwrap()));
    assert!(calls[0]
        .env
        .iter()
        .any(|(k, v)| k == "USERPROFILE" && v == fix.home.path.to_str().unwrap()));
    assert!(!fix
        .home
        .path
        .join(".gemini/config/mcp_config.json")
        .exists());
}

#[test]
fn antigravity_user_install_other_servers_preserves_bytes_and_backs_up() {
    let fix = fix("agy-other");
    let path = fix.home.path.join(".gemini/config/mcp_config.json");
    let original = "{\n  \"mcpServers\": {\"other\": {\"command\": \"other\"}}\n}\n";
    write(&path, original);
    let outcome = installer(ClientId::Antigravity)
        .apply(&ctx(&fix, Scope::User, false, false))
        .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    assert_eq!(outcome.backups.len(), 1);
    assert_eq!(fs::read_to_string(&outcome.backups[0]).unwrap(), original);
    assert_eq!(fix.executor.calls().unwrap()[0].args[1], "add");
}

#[test]
fn antigravity_user_install_existing_entry_is_idempotent() {
    let fix = fix("agy-idem");
    let path = fix.home.path.join(".gemini/config/mcp_config.json");
    let original = format!(
        "{{\n  \"mcpServers\": {{\"other\": {{\"command\": \"other\"}}, \"computer-use\": {{\"command\": \"{}\", \"args\": [\"mcp\"], \"note\": \"stay\"}}}}\n}}\n",
        gcu_str(&fix)
    );
    write(&path, &original);
    let outcome = installer(ClientId::Antigravity)
        .apply(&ctx(&fix, Scope::User, false, false))
        .unwrap();
    assert!(outcome.noop);
    assert!(outcome.backups.is_empty());
    assert!(fix.executor.calls().unwrap().is_empty());
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    let status = installer(ClientId::Antigravity)
        .status(&ctx(&fix, Scope::User, false, false))
        .unwrap();
    assert!(status.present && status.matches_gcu);
}

#[test]
fn antigravity_user_uninstall_present_entry_plans_agy_remove() {
    let fix = fix("agy-rm");
    let path = fix.home.path.join(".gemini/config/mcp_config.json");
    let original = "{\"mcpServers\":{\"other\":{\"command\":\"other\"},\"computer-use\":{\"command\":\"not-gcu\"}}}\n"
        .to_string();
    write(&path, &original);
    let inst = installer(ClientId::Antigravity);
    let outcome = inst
        .uninstall(&ctx(&fix, Scope::User, false, false))
        .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    assert_eq!(outcome.backups.len(), 1);
    let calls = fix.executor.calls().unwrap();
    assert_eq!(
        calls[0].args,
        vec![
            "mcp".to_owned(),
            "remove".to_owned(),
            SERVER_NAME.to_owned()
        ]
    );
}

#[test]
fn antigravity_user_install_bad_json_does_not_run() {
    let fix = fix("agy-bad");
    let path = fix.home.path.join(".gemini/config/mcp_config.json");
    write(&path, b"{");
    let err = installer(ClientId::Antigravity)
        .apply(&ctx(&fix, Scope::User, false, false))
        .unwrap_err();
    assert!(
        matches!(
            err,
            InstallError::Unparseable { .. } | InstallError::UnexpectedShape { .. }
        ),
        "{err}"
    );
    assert_eq!(fs::read(&path).unwrap(), b"{");
    assert!(fix.executor.calls().unwrap().is_empty());
    assert!(backup_paths(&fix.home.path).is_empty());
}

#[test]
fn antigravity_user_uninstall_missing_is_noop() {
    let fix = fix("agy-rm-miss");
    let outcome = installer(ClientId::Antigravity)
        .uninstall(&ctx(&fix, Scope::User, false, false))
        .unwrap();
    assert!(outcome.noop);
    assert!(fix.executor.calls().unwrap().is_empty());
}

#[test]
fn antigravity_user_install_without_binary_does_not_write() {
    let fix = fix("agy-nobin");
    let mut ctx = ctx(&fix, Scope::User, false, false);
    ctx.bins.agy = None;
    let err = installer(ClientId::Antigravity).apply(&ctx).unwrap_err();
    assert!(matches!(err, InstallError::MissingClientBinary { .. }));
    assert!(!fix.home.path.join(".gemini").exists());
}

#[test]
fn install_relative_gcu_path_returns_error() {
    let fix = fix("rel");
    let mut ctx = ctx(&fix, Scope::User, false, true);
    let relative = PathBuf::from("gcu");
    ctx.gcu_exe = &relative;
    let err = installer(ClientId::Grok).plan(&ctx).unwrap_err();
    assert!(matches!(err, InstallError::GcuPathNotAbsolute));
    assert_eq!(walk(&fix.home.path), Vec::<(PathBuf, Vec<u8>)>::new());
}
