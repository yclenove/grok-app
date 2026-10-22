use super::*;
use crate::paths::percent_encode_path_component;
use crate::session_migration::test_home::IsolatedAppHome;

#[test]
fn discovers_imports_and_links_cli_history_without_duplicates() {
    let home = IsolatedAppHome::acquire("cli-import");
    let agent_home = home.path.join("agent-home");
    let project = home.path.join("workspace-中文");
    let agent_id = "agent-import-1234";
    let dir = agent_home
        .join("sessions")
        .join(percent_encode_path_component(project.to_str().unwrap()))
        .join(agent_id);
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&project).unwrap();
    fs::write(
        dir.join("summary.json"),
        serde_json::to_vec(&serde_json::json!({
            "generated_title": "隔离 CLI 会话",
            "updated_at": "2026-09-19T00:00:00Z",
            "num_messages": 2,
            "info": { "cwd": project.to_str().unwrap() },
        }))
        .unwrap(),
    )
    .unwrap();
    let history = concat!(
        r#"{"type":"user","content":"<user_query>\n隔离导入问题\n</user_query>"}"#,
        "\n",
        r#"{"type":"assistant","content":"隔离导入回答"}"#,
        "\n",
    );
    fs::write(dir.join("chat_history.jsonl"), history).unwrap();
    let source_summary = fs::read(dir.join("summary.json")).unwrap();

    let discovered = list_cli_sessions("independent").unwrap();
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].agent_session_id, agent_id);
    assert_eq!(discovered[0].cwd.as_deref(), project.to_str());
    assert_eq!(discovered[0].num_messages, 2);
    assert!(!discovered[0].already_linked);

    let first = import_cli_session(agent_id, None, None, "independent").unwrap();
    assert_eq!(first.agent_session_id.as_deref(), Some(agent_id));
    assert_eq!(first.title, "隔离 CLI 会话");
    let messages = store::load_messages(&first.id);
    assert_eq!(
        messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(),
        vec!["user", "assistant"]
    );
    assert_eq!(messages[0].content, "隔离导入问题");
    assert_eq!(messages[1].content, "隔离导入回答");

    let linked = list_cli_sessions("independent").unwrap();
    assert_eq!(linked.len(), 1);
    assert!(linked[0].already_linked);
    assert_eq!(linked[0].app_session_id.as_deref(), Some(first.id.as_str()));

    let second = import_cli_session(agent_id, None, None, "independent").unwrap();
    assert_eq!(second.id, first.id);
    assert_eq!(store::load_messages(&second.id).len(), 2);
    assert_eq!(store::load_sessions_index().len(), 1);
    assert_eq!(
        fs::read_to_string(dir.join("chat_history.jsonl")).unwrap(),
        history
    );
    assert_eq!(fs::read(dir.join("summary.json")).unwrap(), source_summary);
}
