//! Exercise the actual native tool consumer, journal writer and pool transitions.
use super::runtime_transfer_tests::{action, finish, start_move, Move};
use super::*;
use crate::session_manager::tool_event::{ToolCallChunk, ToolEvent, ToolUpdate};
use serde_json::json;

struct Pause {
    reached: mpsc::SyncSender<()>,
    proceed: mpsc::Receiver<()>,
}

fn pauses() -> &'static StdMutex<HashMap<String, Pause>> {
    static PAUSES: OnceLock<StdMutex<HashMap<String, Pause>>> = OnceLock::new();
    PAUSES.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn pause(id: &str, stage: &str) {
    let pause = pauses().lock().unwrap().remove(&format!("{id}:{stage}"));
    if let Some(pause) = pause {
        pause.reached.send(()).unwrap();
        pause.proceed.recv_timeout(Duration::from_secs(4)).unwrap();
    }
}

pub(crate) fn pause_after_route(id: &str) {
    pause(id, "route");
}

pub(crate) fn pause_before_mutation(id: &str) {
    pause(id, "mutation");
}

fn call(status: &str) -> ToolEvent {
    ToolEvent::Call(ToolCallChunk {
        id: "synthetic-tool".into(),
        title: "读取中文文件".into(),
        kind: "read".into(),
        status: status.into(),
        raw: json!({
            "toolName": "read_file",
            "rawInput": { "target_file": "说明.md", "old_string": "之前", "new_string": "之后" },
            "content": [{ "type": "content", "content": { "type": "text", "text": "实际中文输出" } }],
        }),
    })
}

fn commit(update: Option<ToolUpdate>) -> Option<ToolUpdate> {
    let mut update = update?;
    SessionManager::commit_session_persists(None, std::mem::take(&mut update.persists));
    if let Some(job) = update.journal.take() {
        persist_completed_tool_journal_job(job);
    }
    Some(update)
}

pub(super) fn receipt(fixture: &Fixture, event: ToolEvent) -> Option<ToolUpdate> {
    commit(fixture.manager.prepare_tool_event(
        "target-process",
        fixture.target_meta.agent_session_id.as_deref(),
        event,
    ))
}

fn busy(session: &mut LiveSession) {
    session.fsm.begin_stream().unwrap();
    session.prompt_in_flight = true;
    session.active_turn_id = Some(uuid::Uuid::new_v4().to_string());
}

pub(super) fn prepare_busy(fixture: &Fixture) {
    busy(fixture.manager.inner.lock().as_mut().unwrap());
}

pub(super) struct Paused {
    proceed: mpsc::SyncSender<()>,
    worker: std::thread::JoinHandle<Option<ToolUpdate>>,
}

pub(super) fn start_receipt(fixture: &Fixture, event: ToolEvent, stage: &str) -> Paused {
    let (reached_tx, reached_rx) = mpsc::sync_channel(1);
    let (proceed_tx, proceed_rx) = mpsc::sync_channel(1);
    pauses().lock().unwrap().insert(
        format!("{}:{stage}", fixture.target_meta.id),
        Pause {
            reached: reached_tx,
            proceed: proceed_rx,
        },
    );
    let manager = fixture.manager.clone();
    let agent = fixture.target_meta.agent_session_id.clone();
    let worker = std::thread::spawn(move || {
        commit(manager.prepare_tool_event("target-process", agent.as_deref(), event))
    });
    reached_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    Paused {
        proceed: proceed_tx,
        worker,
    }
}

pub(super) fn finish_receipt(paused: Paused) -> Option<ToolUpdate> {
    paused.proceed.send(()).unwrap();
    paused.worker.join().unwrap()
}

fn assert_durable_target(fixture: &Fixture) {
    let rows = store::load_messages(&fixture.target_meta.id);
    assert_eq!(
        rows.len(),
        1,
        "tool result lost its actual durable journal row"
    );
    assert!(rows[0].content.contains("实际中文输出"));
    assert!(store::load_messages(&fixture.other_meta.id).is_empty());
}

#[test]
fn actual_tool_call_survives_the_busy_pool_transfer_window() {
    let fixture = Fixture::new();
    fixture.prepare_transfer(Move::BusyLiveBackground);
    let moving = start_move(
        &fixture.manager,
        &fixture.target_meta.id,
        Move::BusyLiveBackground,
    );
    let manager = fixture.manager.clone();
    let agent = fixture.target_meta.agent_session_id.clone();
    let (finished, worker) = action(move || {
        commit(manager.prepare_tool_event("target-process", agent.as_deref(), call("completed")))
    });
    let update = finish(moving, finished, worker);
    assert!(update.is_some(), "migrating tool result was discarded");
    assert_durable_target(&fixture);
}

#[test]
fn actual_tool_release_survives_the_busy_pool_transfer_window() {
    let fixture = Fixture::new();
    fixture.prepare_transfer(Move::BusyLiveBackground);
    fixture
        .manager
        .inner
        .lock()
        .as_mut()
        .unwrap()
        .open_tool_ids
        .insert("synthetic-tool".into());
    let moving = start_move(
        &fixture.manager,
        &fixture.target_meta.id,
        Move::BusyLiveBackground,
    );
    let manager = fixture.manager.clone();
    let agent = fixture.target_meta.agent_session_id.clone();
    let (finished, worker) = action(move || {
        commit(manager.prepare_tool_event(
            "target-process",
            agent.as_deref(),
            ToolEvent::Released("synthetic-tool".into()),
        ))
    });
    let update = finish(moving, finished, worker);
    assert!(update.is_some(), "migrating tool kept a ghost open id");
    assert!(fixture
        .manager
        .background
        .lock()
        .get(&fixture.target_meta.id)
        .unwrap()
        .open_tool_ids
        .is_empty());
}

#[test]
fn a_routed_live_tool_follows_its_owner_into_background() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    let paused = start_receipt(&fixture, call("completed"), "route");
    let moved = fixture.manager.try_park_live();
    let update = finish_receipt(paused);
    moved.unwrap();
    assert!(update.is_some(), "demotion lost a routed tool result");
    assert_durable_target(&fixture);
}

#[test]
fn a_routed_tool_never_writes_into_the_new_foreign_focus() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    let mut other = routing_tests::bare_live_session(&fixture.other_meta.id, "other-process");
    other.meta = fixture.other_meta.clone();
    other.acp = Some(fixture.other.clone());
    busy(&mut other);
    fixture
        .manager
        .background
        .lock()
        .insert(fixture.other_meta.id.clone(), other);
    let paused = start_receipt(&fixture, call("completed"), "route");
    let moved = fixture.manager.try_park_live();
    let promoted = fixture
        .manager
        .promote_background_to_live(&fixture.other_meta.id);
    let update = finish_receipt(paused);
    moved.unwrap();
    assert!(promoted);
    assert_eq!(
        update.unwrap().snapshot.session_id.as_deref(),
        Some(fixture.target_meta.id.as_str())
    );
    assert_durable_target(&fixture);
    assert_eq!(
        fixture
            .manager
            .inner
            .lock()
            .as_ref()
            .unwrap()
            .tools_this_turn,
        0
    );
}

#[test]
fn a_routed_background_tool_follows_its_owner_into_live() {
    let fixture = Fixture::new();
    fixture.move_focus(true);
    fixture.manager.try_park_live().unwrap();
    let paused = start_receipt(&fixture, call("completed"), "route");
    let promoted = fixture
        .manager
        .promote_background_to_live(&fixture.target_meta.id);
    let update = finish_receipt(paused);
    assert!(promoted);
    assert!(update.is_some(), "promotion lost a routed tool result");
    assert_durable_target(&fixture);
}

fn replace_acp(fixture: &Fixture) -> Arc<AcpClient> {
    let entered = fixture.runtime.enter();
    let (replacement, _peer) = test_transport::client_for(false, SessionMcpPolicy::default(), None);
    drop(entered);
    fixture.manager.inner.lock().as_mut().unwrap().acp = Some(replacement.clone());
    replacement
}

#[test]
fn a_routed_tool_cannot_borrow_a_replacement_acp_with_the_same_aliases() {
    for stage in ["route", "mutation"] {
        let fixture = Fixture::new();
        prepare_busy(&fixture);
        let paused = start_receipt(&fixture, call("in_progress"), stage);
        let replacement = replace_acp(&fixture);
        let update = finish_receipt(paused);
        assert!(
            update.is_none(),
            "old tool receipt mutated replacement at {stage}"
        );
        assert!(replacement.is_alive());
        assert!(fixture
            .manager
            .inner
            .lock()
            .as_ref()
            .unwrap()
            .open_tool_ids
            .is_empty());
        assert!(fixture
            .manager
            .tool_identities
            .lock()
            .unwrap()
            .get(&fixture.target_meta.id)
            .is_none());
    }
}

#[test]
fn a_routed_tool_cannot_mutate_a_new_turn_epoch() {
    for stage in ["route", "mutation"] {
        let fixture = Fixture::new();
        prepare_busy(&fixture);
        let paused = start_receipt(&fixture, call("completed"), stage);
        {
            let mut guard = fixture.manager.inner.lock();
            let session = guard.as_mut().unwrap();
            session.turn_open_epoch = session.turn_open_epoch.wrapping_add(1);
            session.active_turn_id = Some(uuid::Uuid::new_v4().to_string());
        }
        let update = finish_receipt(paused);
        assert!(
            update.is_none(),
            "old tool appended to a new turn at {stage}"
        );
        assert!(store::load_messages(&fixture.target_meta.id).is_empty());
        assert_eq!(
            fixture
                .manager
                .inner
                .lock()
                .as_ref()
                .unwrap()
                .tools_this_turn,
            0
        );
    }
}

#[test]
fn old_tool_release_cannot_clear_a_new_turn_open_set() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    let paused = start_receipt(
        &fixture,
        ToolEvent::Released("synthetic-tool".into()),
        "mutation",
    );
    {
        let mut guard = fixture.manager.inner.lock();
        let session = guard.as_mut().unwrap();
        session.active_turn_id = Some(uuid::Uuid::new_v4().to_string());
        session.turn_open_epoch += 1;
        session.open_tool_ids.insert("synthetic-tool".into());
    }
    let update = finish_receipt(paused);
    assert!(update.is_none(), "old release cleared a reused tool id");
    assert!(fixture
        .manager
        .inner
        .lock()
        .as_ref()
        .unwrap()
        .open_tool_ids
        .contains("synthetic-tool"));
}

#[test]
fn background_tool_ipc_keeps_real_output_and_change_snippets() {
    let fixture = Fixture::new();
    fixture.move_focus(true);
    let update = receipt(&fixture, call("completed")).unwrap();
    let payload = update.tool.unwrap();
    assert_eq!(payload["output"], "实际中文输出");
    assert_eq!(payload["before"], "之前");
    assert_eq!(payload["after"], "之后");
}

#[test]
fn background_tool_journal_keeps_real_output_for_reload() {
    let fixture = Fixture::new();
    fixture.move_focus(true);
    receipt(&fixture, call("completed")).unwrap();
    assert_durable_target(&fixture);
    assert!(store::load_messages(&fixture.target_meta.id)[0]
        .content
        .contains(TOOL_OUTPUT_SENTINEL));
}

#[test]
fn background_generated_images_and_videos_keep_their_attachment_effects() {
    for extension in ["png", "mp4"] {
        let fixture = Fixture::new();
        fixture.move_focus(true);
        let path = format!("https://synthetic.invalid/outputs/生成结果.{extension}");
        let update = receipt(
            &fixture,
            ToolEvent::Call(ToolCallChunk {
                id: "synthetic-media".into(),
                title: "生成媒体".into(),
                kind: "other".into(),
                status: "completed".into(),
                raw: json!({ "toolName": "image_edit", "rawOutput": { "path": path } }),
            }),
        )
        .unwrap();
        let generated = update
            .generated
            .expect("background generation lost its media attachment");
        assert_eq!(generated["sessionId"], fixture.target_meta.id);
        assert_eq!(generated["path"], path);
        assert_eq!(
            generated["kind"],
            if extension == "mp4" { "video" } else { "image" }
        );
        let guard = fixture.manager.background.lock();
        assert_eq!(
            guard
                .get(&fixture.target_meta.id)
                .unwrap()
                .stream_attachments[0]
                .path,
            path
        );
    }
}

#[test]
fn plain_terminal_urls_do_not_become_generated_attachments_in_either_pool() {
    for background in [false, true] {
        let fixture = Fixture::new();
        if background {
            fixture.move_focus(true);
        } else {
            prepare_busy(&fixture);
        }
        let update = receipt(&fixture, ToolEvent::Call(ToolCallChunk {
            id: "synthetic-terminal".into(), title: "执行 curl".into(), kind: "execute".into(),
            status: "completed".into(), raw: json!({
                "toolName": "run_terminal_command", "content": ["https://synthetic.invalid/unrelated.png"],
            }),
        })).unwrap();
        assert!(update.generated.is_none());
    }
}

#[test]
fn tool_release_finishes_deferred_answer_after_the_rpc_and_tool_settle() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    {
        let mut guard = fixture.manager.inner.lock();
        let session = guard.as_mut().unwrap();
        session.prompt_in_flight = false;
        session.deferred_prompt_complete = Some("end_turn".into());
        session.open_tool_ids.insert("synthetic-tool".into());
        session.stream_buf = "工具结束后的完整答案".into();
        session.streaming_message_id = Some("synthetic-assistant".into());
    }
    let update = receipt(&fixture, ToolEvent::Released("synthetic-tool".into())).unwrap();
    assert!(update.finished);
    assert_eq!(update.snapshot.state, SessionState::Ready);
    assert_eq!(
        store::load_messages(&fixture.target_meta.id)[0].content,
        "工具结束后的完整答案"
    );
    assert!(fixture.manager.runtime_transfer.try_lock().is_some());
    assert!(fixture.manager.inner.try_lock().is_some());
    assert!(fixture.manager.background.try_lock().is_some());
}

#[test]
fn old_tool_completion_cannot_park_a_replacement_background_process() {
    let fixture = Fixture::new();
    fixture.move_focus(true);
    {
        let mut guard = fixture.manager.background.lock();
        let session = guard.get_mut(&fixture.target_meta.id).unwrap();
        session.prompt_in_flight = false;
        session.deferred_prompt_complete = Some("end_turn".into());
        session.active_turn_id = Some(uuid::Uuid::new_v4().to_string());
        session.open_tool_ids.insert("synthetic-tool".into());
    }
    let completion = receipt(&fixture, ToolEvent::Released("synthetic-tool".into()))
        .unwrap()
        .completion
        .unwrap();
    let entered = fixture.runtime.enter();
    let (replacement, _peer) = test_transport::client_for(false, SessionMcpPolicy::default(), None);
    drop(entered);
    fixture
        .manager
        .background
        .lock()
        .get_mut(&fixture.target_meta.id)
        .unwrap()
        .acp = Some(replacement.clone());
    let snapshot = fixture.manager.park_tool_completion(completion);
    assert!(
        snapshot.is_none(),
        "old completion emitted Ready for a replacement"
    );
    assert!(
        fixture
            .manager
            .background
            .lock()
            .contains_key(&fixture.target_meta.id),
        "old completion parked the replacement"
    );
    assert!(!fixture
        .manager
        .parked
        .lock()
        .contains_key(&fixture.target_meta.id));
    assert!(replacement.is_alive());
}

#[test]
fn sparse_terminal_tool_keeps_start_identity_and_single_line_input() {
    for background in [false, true] {
        let fixture = Fixture::new();
        if background {
            fixture.move_focus(true);
        } else {
            prepare_busy(&fixture);
        }
        receipt(&fixture, call("in_progress")).unwrap();
        let terminal = receipt(
            &fixture,
            ToolEvent::Call(ToolCallChunk {
                id: "synthetic-tool".into(),
                title: String::new(),
                kind: String::new(),
                status: "completed".into(),
                raw: json!({ "content": ["稀疏终态输出"] }),
            }),
        )
        .unwrap();
        assert_eq!(terminal.tool.unwrap()["input"], "说明.md");
        let rows = store::load_messages(&fixture.target_meta.id);
        assert!(rows[0]
            .content
            .starts_with("tool_step|completed|read_file|"));
        assert_eq!(rows[0].content.lines().nth(1), Some("input:说明.md"));
    }
}

#[test]
fn idle_and_parked_tool_replay_never_changes_journal_or_identity_cache() {
    let fixture = Fixture::new();
    assert!(receipt(&fixture, call("completed")).is_none());
    fixture.manager.try_park_live().unwrap();
    assert!(receipt(&fixture, call("completed")).is_none());
    assert!(store::load_messages(&fixture.target_meta.id).is_empty());
    assert!(fixture.manager.tool_identities.lock().unwrap().is_empty());
}

#[test]
fn ambiguous_unstamped_shared_process_tool_is_not_assigned_to_focus() {
    let fixture = Fixture::new();
    fixture.move_focus(true);
    let mut other = routing_tests::bare_live_session(&fixture.other_meta.id, "target-process");
    other.meta = fixture.other_meta.clone();
    other.acp = Some(fixture.other.clone());
    busy(&mut other);
    fixture
        .manager
        .background
        .lock()
        .insert(fixture.other_meta.id.clone(), other);
    assert!(fixture
        .manager
        .prepare_tool_event("target-process", None, call("completed"))
        .is_none());
    assert!(store::load_messages(&fixture.other_meta.id).is_empty());
    assert!(store::load_messages(&fixture.target_meta.id).is_empty());
}

#[test]
fn rejected_old_local_media_tool_never_publishes_a_path_grant() {
    let fixture = Fixture::new();
    let _scope = crate::path_scope::TestScopeRestore::acquire();
    prepare_busy(&fixture);
    let file = fixture.home.path.join("旧工具生成的图片.png");
    std::fs::write(&file, b"synthetic-image").unwrap();
    assert!(!crate::path_scope::explicitly_granted_for_test(&file));
    let paused = start_receipt(
        &fixture,
        ToolEvent::Call(ToolCallChunk {
            id: "synthetic-local-media".into(),
            title: "生成图片".into(),
            kind: "other".into(),
            status: "completed".into(),
            raw: json!({ "toolName": "image_edit", "rawOutput": { "path": file } }),
        }),
        "mutation",
    );
    let replacement = replace_acp(&fixture);
    let update = finish_receipt(paused);
    assert!(
        !crate::path_scope::explicitly_granted_for_test(&file),
        "old media preparation left an explicit path grant"
    );
    assert!(update.is_none());
    assert!(replacement.is_alive());
}

fn deferred_background(fixture: &Fixture) {
    fixture.move_focus(true);
    let mut guard = fixture.manager.background.lock();
    let session = guard.get_mut(&fixture.target_meta.id).unwrap();
    session.prompt_in_flight = false;
    session.deferred_prompt_complete = Some("end_turn".into());
    session.active_turn_id = Some(uuid::Uuid::new_v4().to_string());
    session.open_tool_ids.insert("synthetic-tool".into());
}

#[test]
fn completed_background_tool_flushes_the_answer_and_parks_its_actual_owner() {
    let fixture = Fixture::new();
    deferred_background(&fixture);
    let stream = fixture
        .manager
        .prepare_stream_chunk(
            "target-process",
            fixture.target_meta.agent_session_id.as_deref(),
            crate::session_manager::stream_event::StreamChunk {
                kind: crate::acp_client::StreamKind::Assistant,
                text: "工具完成后的答案".into(),
                message_id: Some("synthetic-assistant".into()),
                done: false,
            },
        )
        .unwrap();
    if let Some(journal) = stream.journal {
        SessionManager::commit_stream_journal_flush(journal);
    }
    let mut update = receipt(&fixture, call("completed")).unwrap();
    assert!(update.finished);
    assert!(update
        .emits
        .iter()
        .any(|p| p.done && p.host_finalized && p.text == "工具完成后的答案"));
    let ready = fixture
        .manager
        .park_tool_completion(update.completion.take().unwrap())
        .unwrap();
    assert_eq!(ready.state, SessionState::Ready);
    assert_eq!(
        ready.session_id.as_deref(),
        Some(fixture.target_meta.id.as_str())
    );
    assert!(fixture
        .manager
        .parked
        .lock()
        .contains_key(&fixture.target_meta.id));
    let rows = store::load_messages(&fixture.target_meta.id);
    assert!(rows
        .iter()
        .any(|r| r.role == "assistant" && r.content == "工具完成后的答案"));
    assert!(rows
        .iter()
        .any(|r| r.role == "tool" && r.content.contains("实际中文输出")));
    assert!(store::load_messages(&fixture.other_meta.id).is_empty());
}

#[test]
fn actual_tool_completion_remains_valid_after_the_same_owner_is_already_parked() {
    let fixture = Fixture::new();
    deferred_background(&fixture);
    let completion = receipt(&fixture, ToolEvent::Released("synthetic-tool".into()))
        .unwrap()
        .completion
        .unwrap();
    fixture
        .manager
        .promote_background_ready_to_parked(&fixture.target_meta.id);
    let ready = fixture.manager.park_tool_completion(completion).unwrap();
    assert_eq!(ready.state, SessionState::Ready);
    assert_eq!(
        ready.session_id.as_deref(),
        Some(fixture.target_meta.id.as_str())
    );
    assert!(fixture
        .manager
        .parked
        .lock()
        .get(&fixture.target_meta.id)
        .unwrap()
        .acp
        .is_alive());
}

#[test]
fn old_tool_completion_cannot_emit_ready_for_a_new_background_turn() {
    let fixture = Fixture::new();
    deferred_background(&fixture);
    let completion = receipt(&fixture, call("completed"))
        .unwrap()
        .completion
        .unwrap();
    {
        let mut guard = fixture.manager.background.lock();
        let session = guard.get_mut(&fixture.target_meta.id).unwrap();
        busy(session);
        session.turn_open_epoch += 1;
        session.open_tool_ids.insert("新的工具".into());
    }
    assert!(fixture.manager.park_tool_completion(completion).is_none());
    let guard = fixture.manager.background.lock();
    assert_eq!(
        guard.get(&fixture.target_meta.id).unwrap().fsm.state(),
        SessionState::Streaming
    );
    assert!(guard
        .get(&fixture.target_meta.id)
        .unwrap()
        .open_tool_ids
        .contains("新的工具"));
}

#[test]
fn completed_tool_does_not_finish_unresolved_rpc_or_human_gates() {
    for gate in ["rpc", "plan", "ask_user", "permission", "other_tool"] {
        let fixture = Fixture::new();
        deferred_background(&fixture);
        {
            let mut guard = fixture.manager.background.lock();
            let session = guard.get_mut(&fixture.target_meta.id).unwrap();
            match gate {
                "rpc" => session.prompt_in_flight = true,
                "plan" => session.pending_plan_rpc_id = Some(11),
                "ask_user" => session.pending_ask_user_rpc_id = Some(12),
                "permission" => {
                    session.fsm.await_permission().unwrap();
                }
                _ => {
                    session.open_tool_ids.insert("其他仍在运行的工具".into());
                }
            }
        }
        let update = receipt(&fixture, call("completed")).unwrap();
        assert!(!update.finished, "tool completion hid unresolved {gate}");
        assert!(update.completion.is_none());
        assert!(update.emits.iter().all(|p| !p.done));
        assert!(fixture
            .manager
            .background
            .lock()
            .contains_key(&fixture.target_meta.id));
    }
}

#[test]
fn accepted_local_tool_media_grants_only_at_the_actual_native_effect_step() {
    let fixture = Fixture::new();
    let _scope = crate::path_scope::TestScopeRestore::acquire();
    prepare_busy(&fixture);
    let file = fixture.home.path.join("有效的工具结果.png");
    std::fs::write(&file, b"synthetic-image").unwrap();
    let update = receipt(
        &fixture,
        ToolEvent::Call(ToolCallChunk {
            id: "synthetic-accepted-media".into(),
            title: "生成图片".into(),
            kind: "other".into(),
            status: "completed".into(),
            raw: json!({ "toolName": "image_edit", "rawOutput": { "path": file } }),
        }),
    )
    .unwrap();
    let payload = update.generated.unwrap();
    assert!(!crate::path_scope::explicitly_granted_for_test(&file));
    assert!(fixture.manager.runtime_transfer.try_lock().is_some());
    assert!(fixture.manager.inner.try_lock().is_some());
    assert!(fixture.manager.background.try_lock().is_some());
    SessionManager::grant_tool_media(&payload);
    assert!(crate::path_scope::explicitly_granted_for_test(&file));
    assert_eq!(payload["sessionId"], fixture.target_meta.id);
}
