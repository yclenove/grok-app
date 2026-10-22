//! Actual accepted native jobs, Host writer and journal store interleavings.
use super::tool_owner_tests::prepare_busy;
use super::*;
use crate::session_manager::tool_event::{ToolCallChunk, ToolEvent};
use serde_json::json;

struct Pause {
    reached: mpsc::SyncSender<()>,
    proceed: mpsc::Receiver<()>,
}

fn pauses() -> &'static StdMutex<HashMap<String, Pause>> {
    static PAUSES: OnceLock<StdMutex<HashMap<String, Pause>>> = OnceLock::new();
    PAUSES.get_or_init(|| StdMutex::new(HashMap::new()))
}

pub(crate) fn pause_before_commit(session: &str, tool: &str) {
    let pause = pauses()
        .lock()
        .unwrap()
        .remove(&format!("{session}:{tool}"));
    if let Some(pause) = pause {
        pause.reached.send(()).unwrap();
        pause.proceed.recv_timeout(Duration::from_secs(4)).unwrap();
    }
}

fn fixture(background: bool) -> Fixture {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    if background {
        fixture.manager.try_park_live().unwrap();
    }
    fixture
}

fn job(fixture: &Fixture, status: &str, output: &str) -> PendingCompletedToolJournal {
    fixture
        .manager
        .prepare_tool_event(
            "target-process",
            fixture.target_meta.agent_session_id.as_deref(),
            ToolEvent::Call(ToolCallChunk {
                id: "journal-call".into(),
                title: "读取说明".into(),
                kind: "read".into(),
                status: status.into(),
                raw: json!({
                    "toolName": "read_file", "rawInput": {"target_file": "说明.md"},
                    "content": [{"type": "content", "content": {"type": "text", "text": output}}],
                }),
            }),
        )
        .unwrap()
        .journal
        .unwrap()
}

enum Write {
    Native(PendingCompletedToolJournal),
    Host { session: String, detail: String },
}

impl Write {
    fn run(self) {
        match self {
            Self::Native(job) => persist_completed_tool_journal_job(job),
            Self::Host { session, detail } => journal_host_tool_step(
                &session,
                "journal-call",
                "completed",
                "vision",
                "识别图像",
                &detail,
            ),
        }
    }
}

struct Paused {
    proceed: mpsc::SyncSender<()>,
    worker: std::thread::JoinHandle<()>,
}

fn start_write(fixture: &Fixture, write: Write) -> Paused {
    let (reached_tx, reached_rx) = mpsc::sync_channel(1);
    let (proceed_tx, proceed_rx) = mpsc::sync_channel(1);
    pauses().lock().unwrap().insert(
        format!("{}:journal-call", fixture.target_meta.id),
        Pause {
            reached: reached_tx,
            proceed: proceed_rx,
        },
    );
    let worker = std::thread::spawn(move || write.run());
    reached_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    Paused {
        proceed: proceed_tx,
        worker,
    }
}

fn finish_write(paused: Paused) {
    paused.proceed.send(()).unwrap();
    paused.worker.join().unwrap();
}

fn append_assistant(fixture: &Fixture) {
    store::append_message(
        &fixture.target_meta.id,
        store::ChatMessageStored {
            id: "concurrent-assistant".into(),
            role: "assistant".into(),
            content: "同时落盘的中文回答".into(),
            thought: None,
            created_at: chrono::Utc::now(),
            is_error: false,
            attachments: None,
            marker: None,
        },
    )
    .unwrap();
}

fn assert_current(fixture: &Fixture, output: &str) {
    let rows = store::load_messages(&fixture.target_meta.id);
    assert_eq!(
        rows.len(),
        2,
        "tool upsert discarded or duplicated another journal row"
    );
    let tools = rows
        .iter()
        .filter(|row| row.id == "tool-journal-call")
        .collect::<Vec<_>>();
    assert_eq!(tools.len(), 1);
    assert!(
        tools[0].content.contains(output),
        "a delayed writer replaced the richer committed tool result"
    );
    assert_eq!(
        rows.iter()
            .find(|row| row.id == "concurrent-assistant")
            .unwrap()
            .content,
        "同时落盘的中文回答"
    );
    assert!(store::load_messages(&fixture.other_meta.id).is_empty());
}

#[test]
fn actual_completed_tool_rmw_compares_with_latest_committed_row() {
    for background in [false, true] {
        let fixture = fixture(background);
        Write::Native(job(&fixture, "completed", "初始结果")).run();
        let delayed = job(&fixture, "completed", &"稍多的结果".repeat(8));
        let strongest = "完整的中文结果".repeat(60);
        let latest = job(&fixture, "completed", &strongest);
        let paused = start_write(&fixture, Write::Native(delayed));
        Write::Native(latest).run();
        append_assistant(&fixture);
        finish_write(paused);
        assert_current(&fixture, &strongest);
    }
}

#[test]
fn actual_first_tool_insert_does_not_overwrite_a_richer_concurrent_insert() {
    for background in [false, true] {
        let fixture = fixture(background);
        let delayed = job(&fixture, "completed", "短结果");
        let strongest = "第一次落盘的完整结果".repeat(60);
        let latest = job(&fixture, "completed", &strongest);
        let paused = start_write(&fixture, Write::Native(delayed));
        Write::Native(latest).run();
        append_assistant(&fixture);
        finish_write(paused);
        assert_current(&fixture, &strongest);
    }
}

#[test]
fn actual_host_and_native_tool_updates_share_the_current_row_transaction() {
    for background in [false, true] {
        let fixture = fixture(background);
        Write::Host {
            session: fixture.target_meta.id.clone(),
            detail: "初始图像分析".into(),
        }
        .run();
        let strongest = "完整原生工具输出".repeat(100);
        let latest = job(&fixture, "completed", &strongest);
        let paused = start_write(
            &fixture,
            Write::Host {
                session: fixture.target_meta.id.clone(),
                detail: "较短的图像分析".repeat(8),
            },
        );
        Write::Native(latest).run();
        append_assistant(&fixture);
        finish_write(paused);
        assert_current(&fixture, &strongest);
    }
}

#[test]
fn actual_richer_terminal_update_keeps_error_flag_in_sync_with_status() {
    for background in [false, true] {
        let fixture = fixture(background);
        Write::Native(job(&fixture, "completed", "初始结果")).run();
        Write::Native(job(&fixture, "failed", &"失败原因".repeat(60))).run();
        let failed = store::load_messages(&fixture.target_meta.id);
        assert!(failed[0].content.starts_with("tool_step|failed|"));
        assert!(
            failed[0].is_error,
            "durable failed tool retained a success flag"
        );
        Write::Native(job(&fixture, "completed", &"补齐后的成功结果".repeat(100))).run();
        let completed = store::load_messages(&fixture.target_meta.id);
        assert!(completed[0].content.starts_with("tool_step|completed|"));
        assert!(!completed[0].is_error);
    }
}

#[test]
fn actual_tool_enrichment_survives_a_later_stale_full_journal_save() {
    for background in [false, true] {
        let fixture = fixture(background);
        Write::Native(job(&fixture, "completed", "初始结果")).run();
        let stale = store::load_messages(&fixture.target_meta.id);
        let strongest = "补齐后的中文结果".repeat(60);
        Write::Native(job(&fixture, "completed", &strongest)).run();
        append_assistant(&fixture);
        store::save_messages(&fixture.target_meta.id, &stale).unwrap();
        assert_current(&fixture, &strongest);
    }
}

#[test]
fn actual_tool_revision_advances_even_when_stored_clock_is_ahead() {
    let fixture = fixture(false);
    Write::Native(job(&fixture, "completed", "初始结果")).run();
    let mut before = store::load_messages(&fixture.target_meta.id).remove(0);
    before.created_at = chrono::Utc::now() + chrono::Duration::seconds(180);
    store::append_message(&fixture.target_meta.id, before.clone()).unwrap();
    Write::Native(job(&fixture, "completed", &"完整结果".repeat(60))).run();
    let after = store::load_messages(&fixture.target_meta.id).remove(0);
    assert!(
        after.created_at > before.created_at,
        "accepted tool update did not advance the actual row revision"
    );
}

#[test]
fn actual_noop_tool_upsert_preserves_bytes_and_row_revision() {
    let fixture = fixture(false);
    let pending = job(&fixture, "completed", "完整中文结果");
    Write::Native(pending.clone()).run();
    let path = crate::paths::session_dir(&fixture.target_meta.id).join("messages.json");
    let before = std::fs::read(&path).unwrap();
    Write::Native(pending).run();
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn actual_tool_update_preserves_other_row_metadata_and_order() {
    let fixture = fixture(false);
    Write::Native(job(&fixture, "completed", "初始结果")).run();
    let mut before = store::load_messages(&fixture.target_meta.id).remove(0);
    before.thought = Some("保留的说明".into());
    before.attachments = Some(vec![attachment_from_path(
        fixture.home.path.join("合成图片.png").to_str().unwrap(),
    )]);
    store::append_message(&fixture.target_meta.id, before.clone()).unwrap();
    append_assistant(&fixture);
    Write::Native(job(&fixture, "completed", &"补齐后的结果".repeat(60))).run();
    let after = store::load_messages(&fixture.target_meta.id);
    assert_eq!(after[0].id, before.id);
    assert_eq!(after[1].id, "concurrent-assistant");
    assert_eq!(after[0].thought, before.thought);
    assert_eq!(
        serde_json::to_value(&after[0].attachments).unwrap(),
        serde_json::to_value(&before.attachments).unwrap()
    );
    assert_eq!(after[0].marker.as_deref(), Some("tool_step"));
}

#[test]
fn actual_tool_revision_overflow_keeps_the_committed_journal_unchanged() {
    let fixture = fixture(false);
    Write::Native(job(&fixture, "completed", "初始结果")).run();
    let mut before = store::load_messages(&fixture.target_meta.id).remove(0);
    before.created_at = chrono::DateTime::<chrono::Utc>::MAX_UTC;
    store::append_message(&fixture.target_meta.id, before).unwrap();
    let path = crate::paths::session_dir(&fixture.target_meta.id).join("messages.json");
    let bytes = std::fs::read(&path).unwrap();
    Write::Native(job(&fixture, "failed", &"更长的错误内容".repeat(60))).run();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let current = store::load_messages(&fixture.target_meta.id);
    assert!(!current[0].is_error);
    assert_eq!(
        current[0].created_at,
        chrono::DateTime::<chrono::Utc>::MAX_UTC
    );
}

#[test]
fn actual_tool_write_failure_keeps_tool_and_concurrent_assistant_rows() {
    let fixture = fixture(false);
    Write::Native(job(&fixture, "completed", "初始结果")).run();
    append_assistant(&fixture);
    let directory = crate::paths::session_dir(&fixture.target_meta.id);
    let path = directory.join("messages.json");
    let bytes = std::fs::read(&path).unwrap();
    // A directory at the atomic temporary-file location fails writes on every
    // platform; no OS permission or real user's file is modified for this test.
    std::fs::create_dir(directory.join("messages.json.tmp")).unwrap();
    Write::Native(job(&fixture, "failed", &"无法写入的新结果".repeat(60))).run();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let current = store::load_messages(&fixture.target_meta.id);
    assert_eq!(current.len(), 2);
    assert!(!current[0].is_error);
    assert_eq!(current[1].content, "同时落盘的中文回答");
}
