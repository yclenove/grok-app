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
