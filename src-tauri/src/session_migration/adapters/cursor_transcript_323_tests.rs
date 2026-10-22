//! Agent transcripts written by Cursor 3.23.12 (owner export, 2026-10-21):
//! `{role, message:{content}}` records, user text wrapped in `<timestamp>` and
//! `<user_query>`, id-less tool_use blocks and `{"type":"turn_ended"}` lines.
use super::*;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/session-migration/cursor-3.23")
        .join(name)
}

const AGENT: &str = "cdfdffc1-49a8-4b40-b6cb-2025940216eb.jsonl";
const ASK: &str = "204eedd5-b641-4465-b81d-7f1d42f8bf09.jsonl";

fn users(out: &AdapterOutput) -> Vec<(String, Option<String>)> {
    out.graph
        .events
        .iter()
        .filter_map(|event| match event {
            ImportedEvent::Message {
                role: MessageRole::User,
                blocks,
                base,
                ..
            } => Some((
                blocks
                    .iter()
                    .filter_map(ContentBlock::plain_text)
                    .collect::<Vec<_>>()
                    .join(""),
                base.timestamp.clone(),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn cursor_323_turn_ended_is_a_boundary_not_a_missing_role() {
    let out = normalize_transcript_file(&fixture(AGENT)).unwrap();
    assert!(
        !out.issues.iter().any(|issue| issue.code == "unknown-role"),
        "{:?}",
        out.issues
    );
    let boundaries = out
        .graph
        .events
        .iter()
        .filter(|event| {
            matches!(event, ImportedEvent::Metadata { source_type, .. } if source_type == "turn_ended:success")
        })
        .count();
    assert_eq!(boundaries, 5);
}

#[test]
fn cursor_323_user_query_is_the_message_and_its_timestamp_is_the_source_time() {
    let out = normalize_transcript_file(&fixture(AGENT)).unwrap();
    let users = users(&out);
    let texts: Vec<_> = users.iter().map(|(text, _)| text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "img-a.png 图里写的是什么？",
            "calc.py 解释一下这个文件",
            "在终端运行 python calc.py 2 3",
            "把 notes.md 第二行改成 line two: edited by cursor",
            "notes.md 有几行？",
        ]
    );
    assert_eq!(users[0].1.as_deref(), Some("2026-10-22T00:31:00+08:00"));
    assert_eq!(users[4].1.as_deref(), Some("2026-10-22T00:34:00+08:00"));
    assert_eq!(
        out.graph.title.as_deref(),
        Some("img-a.png 图里写的是什么？")
    );
}

#[test]
fn cursor_323_ask_transcript_keeps_its_tool_call_and_answer() {
    let out = normalize_transcript_file(&fixture(ASK)).unwrap();
    assert_eq!(users(&out)[0].0, "img-b.png 这张图是什么颜色？");
    assert!(out
        .graph
        .events
        .iter()
        .any(|event| matches!(event, ImportedEvent::ToolCall { name, .. } if name == "Read")));
    assert!(out.graph.events.iter().any(|event| matches!(
        event,
        ImportedEvent::Message { role: MessageRole::Assistant, blocks, .. }
            if blocks.iter().filter_map(ContentBlock::plain_text).any(|t| t.contains("IMG-B 海洋"))
    )));
}

#[test]
fn cursor_323_unrecognised_wrappers_and_times_are_kept_verbatim() {
    let dir = std::env::temp_dir().join(format!("cursor-323-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("other.jsonl");
    let records = [
        // Extra wrapper text must not be dropped.
        "{\"role\":\"user\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"<timestamp>Thursday, Oct 22, 2026, 12:31 AM (UTC+8)</timestamp>\\n<attached_files>a.md</attached_files>\\n<user_query>\\nq1\\n</user_query>\"}]}}",
        // A weekday that does not match the date is not a source time.
        "{\"role\":\"user\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"<timestamp>Monday, Oct 22, 2026, 12:31 AM (UTC+8)</timestamp>\\n<user_query>\\nq2\\n</user_query>\"}]}}",
        "{\"type\":\"turn_ended\",\"status\":\"aborted\"}",
    ];
    std::fs::write(&path, records.join("\n") + "\n").unwrap();
    let out = normalize_transcript_file(&path).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let users = users(&out);
    assert!(users[0].0.contains("<attached_files>a.md</attached_files>"));
    assert!(users[0].0.contains("q1"));
    assert_eq!(users[1].0, "q2");
    assert_eq!(users[1].1, None, "inconsistent weekday stays unknown");
    assert!(out.graph.events.iter().any(|event| matches!(
        event,
        ImportedEvent::Metadata { source_type, .. } if source_type == "turn_ended:aborted"
    )));
}
