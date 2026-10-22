use super::*;
use crate::providers::runtime_config::runtime_text;

fn scalar(text: &str, path: &[&str]) -> String {
    let snapshot = ConfigSnapshot::parse(text).unwrap();
    let mut current = &snapshot.root;
    for name in &path[..path.len() - 1] {
        let Value::Table(next) = &current.fields[*name] else {
            panic!("expected table");
        };
        current = next;
    }
    match &current.fields[path[path.len() - 1]] {
        Value::String(value) | Value::Atom(value) => value.clone(),
        _ => panic!("expected scalar"),
    }
}

#[test]
fn runtime_flags_and_permission_modes_have_the_right_scope_and_scalar_types() {
    let settings = AppSettings {
        subagents_enabled: false,
        experimental_memory: false,
        todo_gate_enabled: true,
        todo_gate_max_fires_per_prompt: 99,
        subagent_worktree_snapshot_enabled: true,
        two_pass_compaction_enabled: true,
        auto_wake_enabled: true,
        ..AppSettings::default()
    };
    let text = "ui = { yolo_mode = false, custom = { label = '中文' } }\nfeatures = { custom = true }\ncompat = { claude = { skills = true }, cursor = { rules = true } }\n";
    let next = runtime_text(
        text,
        &settings,
        "always_approve",
        false,
        &ExtensionsPrefs::default(),
    )
    .unwrap();
    for (path, expected) in [
        (vec!["ui", "permission_mode"], "always-approve"),
        (vec!["ui", "yolo"], "true"),
        (vec!["ui", "yolo_mode"], "false"),
        (vec!["ui", "custom", "label"], "中文"),
        (vec!["subagents", "enabled"], "false"),
        (vec!["memory", "enabled"], "false"),
        (vec!["todo_gate_enabled"], "true"),
        (vec!["todo_gate_max_fires_per_prompt"], "20"),
        (vec!["subagent_worktree_snapshot_enabled"], "true"),
        (vec!["two_pass_compaction_enabled"], "true"),
        (vec!["auto_wake_enabled"], "true"),
        (vec!["features", "auto_wake"], "true"),
        (vec!["features", "custom"], "true"),
        (vec!["compat", "claude", "mcps"], "false"),
        (vec!["compat", "claude", "skills"], "true"),
        (vec!["compat", "cursor", "mcps"], "false"),
        (vec!["compat", "cursor", "rules"], "true"),
    ] {
        assert_eq!(scalar(&next, &path), expected);
    }
    assert_eq!(
        runtime_text(
            &next,
            &settings,
            "always_approve",
            false,
            &ExtensionsPrefs::default()
        )
        .unwrap(),
        next
    );
    let mut settings = settings;
    settings.todo_gate_max_fires_per_prompt = 0;
    let next = runtime_text(&next, &settings, "auto", false, &ExtensionsPrefs::default()).unwrap();
    assert_eq!(scalar(&next, &["todo_gate_max_fires_per_prompt"]), "3");
    assert_eq!(scalar(&next, &["ui", "permission_mode"]), "auto");
    assert_eq!(scalar(&next, &["ui", "yolo"]), "false");
}

#[test]
fn mcp_policy_uses_literal_names_preserves_definitions_and_never_synthesizes_servers() {
    let text = "[mcp_servers.'chat.cut']\ncommand = 'synthetic-unexecuted'\nargs = ['[ui]', '中文']\n[mcp_servers.'chat.cut'.env]\nTOKEN = 'synthetic-env'\n[mcp_servers.'chat.cut'.headers]\n'X-Label' = 'literal'\n[mcp_servers.normal]\nurl = 'https://synthetic.invalid/mcp'\n[mcp_servers.official-aux]\ncommand = 'synthetic-aux-unexecuted'\n";
    let mut prefs = ExtensionsPrefs::default();
    prefs.mcp.insert("chat.cut".into(), false);
    let mut settings = AppSettings::default();
    for (inject, with_users, chat, normal, auxiliary) in [
        (false, false, "false", "true", "false"),
        (true, false, "false", "false", "true"),
        (true, true, "false", "true", "true"),
    ] {
        settings.official_aux_with_user_mcp = with_users;
        let next = runtime_text(text, &settings, "ask", inject, &prefs).unwrap();
        for (server, expected) in [
            ("chat.cut", chat),
            ("normal", normal),
            ("official-aux", auxiliary),
        ] {
            assert_eq!(scalar(&next, &["mcp_servers", server, "enabled"]), expected);
        }
        assert_eq!(
            scalar(&next, &["mcp_servers", "chat.cut", "env", "TOKEN"]),
            "synthetic-env"
        );
        assert_eq!(
            scalar(&next, &["mcp_servers", "chat.cut", "headers", "X-Label"]),
            "literal"
        );
        assert_eq!(
            scalar(&next, &["mcp_servers", "normal", "url"]),
            "https://synthetic.invalid/mcp"
        );
    }
    let empty = runtime_text("", &settings, "ask", true, &prefs).unwrap();
    assert!(!ConfigSnapshot::parse(&empty)
        .unwrap()
        .root
        .fields
        .contains_key("mcp_servers"));
}

#[test]
fn permission_and_compat_overlays_alone_preserve_nested_values_and_literal_table_text() {
    let text = "ui = { permission_mode = 'default', yolo_mode = false }\ncompat = { claude = { skills = true } }\n[model.carrier]\nnote = '''\n[ui]\nyolo = 'literal'\n[compat.claude]\nmcps = true\n'''\n";
    let permission = ConfigSnapshot::parse(text)
        .unwrap()
        .permission_text("dont_ask")
        .unwrap();
    let next = ConfigSnapshot::parse(&permission)
        .unwrap()
        .compat_text()
        .unwrap();
    assert_eq!(scalar(&next, &["ui", "permission_mode"]), "dontAsk");
    assert_eq!(scalar(&next, &["compat", "claude", "mcps"]), "false");
    assert_eq!(scalar(&next, &["compat", "claude", "skills"]), "true");
    assert_eq!(
        scalar(&next, &["model", "carrier", "note"]),
        scalar(text, &["model", "carrier", "note"])
    );
}
