//! Actual native Stream mutation and durable journal consumer with synthetic ACP.
use super::runtime_transfer_tests::{action, finish, start_move, Move};
use super::*;
use crate::acp_client::StreamKind;
use crate::session_manager::stream_event::{StreamChunk, StreamUpdate};

struct Pause {
    reached: mpsc::SyncSender<()>,
    proceed: mpsc::Receiver<()>,
}

fn route_pauses() -> &'static StdMutex<HashMap<String, Pause>> {
    static PAUSES: OnceLock<StdMutex<HashMap<String, Pause>>> = OnceLock::new();
    PAUSES.get_or_init(|| StdMutex::new(HashMap::new()))
}

pub(crate) fn pause_after_route(id: &str) {
    let pause = route_pauses().lock().unwrap().remove(id);
    if let Some(pause) = pause {
        pause.reached.send(()).unwrap();
        pause.proceed.recv_timeout(Duration::from_secs(4)).unwrap();
    }
}

fn chunk(text: &str) -> StreamChunk {
    StreamChunk {
        kind: StreamKind::Assistant,
        text: text.into(),
        message_id: Some("synthetic-wire-message".into()),
        done: false,
    }
}

fn commit(update: Option<StreamUpdate>) -> Option<StreamUpdate> {
    let mut update = update?;
    if let Some(journal) = update.journal.take() {
        SessionManager::commit_stream_journal_flush(journal);
    }
    Some(update)
}

fn target_receipt(fixture: &Fixture, chunk: StreamChunk) -> Option<StreamUpdate> {
    commit(fixture.manager.prepare_stream_chunk(
        "target-process",
        fixture.target_meta.agent_session_id.as_deref(),
        chunk,
    ))
}

struct ReceiptPause {
    proceed: mpsc::SyncSender<()>,
    worker: std::thread::JoinHandle<Option<StreamUpdate>>,
}

fn start_receipt(fixture: &Fixture, chunk: StreamChunk) -> ReceiptPause {
    let (reached_tx, reached_rx) = mpsc::sync_channel(1);
    let (proceed_tx, proceed_rx) = mpsc::sync_channel(1);
    route_pauses().lock().unwrap().insert(
        fixture.target_meta.id.clone(),
        Pause {
            reached: reached_tx,
            proceed: proceed_rx,
        },
    );
    let manager = fixture.manager.clone();
    let agent = fixture.target_meta.agent_session_id.clone();
    let worker = std::thread::spawn(move || {
        commit(manager.prepare_stream_chunk("target-process", agent.as_deref(), chunk))
    });
    reached_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    ReceiptPause {
        proceed: proceed_tx,
        worker,
    }
}

fn finish_receipt(paused: ReceiptPause) -> Option<StreamUpdate> {
    paused.proceed.send(()).unwrap();
    paused.worker.join().unwrap()
}

fn make_busy(session: &mut LiveSession) {
    session.fsm.begin_stream().unwrap();
    session.prompt_in_flight = true;
    session.active_turn_id = Some(uuid::Uuid::new_v4().to_string());
}

fn prepare_busy(fixture: &Fixture) {
    make_busy(fixture.manager.inner.lock().as_mut().unwrap());
}

fn seed_other_background(fixture: &Fixture, busy: bool, process: &str) {
    let mut other = routing_tests::bare_live_session(&fixture.other_meta.id, process);
    other.meta = fixture.other_meta.clone();
    other.acp = Some(fixture.other.clone());
    if busy {
        make_busy(&mut other);
    }
    fixture
        .manager
        .background
        .lock()
        .insert(fixture.other_meta.id.clone(), other);
}

fn assert_target_answer(fixture: &Fixture) {
    let rows = store::load_messages(&fixture.target_meta.id);
    assert_eq!(rows.len(), 1, "answer did not reach its durable journal");
    assert_eq!(rows[0].content, "继续回答\n\n");
    assert!(store::load_messages(&fixture.other_meta.id).is_empty());
}

#[test]
fn actual_stream_receipt_survives_the_busy_live_background_transfer_window() {
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
        commit(manager.prepare_stream_chunk(
            "target-process",
            agent.as_deref(),
            chunk("继续回答\n\n"),
        ))
    });
    let update = finish(moving, finished, worker);
    assert!(
        update.is_some(),
        "a migrating answer lost its Stream receipt"
    );
    assert_target_answer(&fixture);
}

#[test]
fn a_routed_live_stream_follows_its_owner_into_background() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    let paused = start_receipt(&fixture, chunk("继续回答\n\n"));
    let moved = fixture.manager.try_park_live();
    let update = finish_receipt(paused);
    moved.unwrap();
    assert!(update.is_some(), "the route became stale after demotion");
    assert_target_answer(&fixture);
}

#[test]
fn a_routed_live_stream_never_writes_into_the_new_foreign_focus() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    seed_other_background(&fixture, true, "other-process");
    let paused = start_receipt(&fixture, chunk("继续回答\n\n"));
    let moved = fixture.manager.try_park_live();
    let promoted = fixture
        .manager
        .promote_background_to_live(&fixture.other_meta.id);
    let update = finish_receipt(paused);
    moved.unwrap();
    assert!(promoted);
    assert!(update.is_some());
    assert_target_answer(&fixture);
    assert!(fixture
        .manager
        .inner
        .lock()
        .as_ref()
        .unwrap()
        .stream_buf
        .is_empty());
    assert!(fixture.other.is_alive());
}

#[test]
fn a_routed_background_stream_follows_its_owner_into_live() {
    let fixture = Fixture::new();
    fixture.move_focus(true);
    fixture.manager.try_park_live().unwrap();
    let paused = start_receipt(&fixture, chunk("继续回答\n\n"));
    let promoted = fixture
        .manager
        .promote_background_to_live(&fixture.target_meta.id);
    let update = finish_receipt(paused);
    assert!(promoted);
    assert!(update.is_some(), "the route became stale after promotion");
    assert_target_answer(&fixture);
}

#[test]
fn a_routed_stream_cannot_borrow_a_replacement_acp_with_the_same_aliases() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    let entered = fixture.runtime.enter();
    let (replacement, _peer) = test_transport::client_for(false, SessionMcpPolicy::default(), None);
    drop(entered);
    let paused = start_receipt(&fixture, chunk("继续回答\n\n"));
    fixture.manager.inner.lock().as_mut().unwrap().acp = Some(replacement.clone());
    let update = finish_receipt(paused);
    assert!(
        update.is_none(),
        "old receipt mutated the replacement client"
    );
    assert!(fixture
        .manager
        .inner
        .lock()
        .as_ref()
        .unwrap()
        .stream_buf
        .is_empty());
    assert!(store::load_messages(&fixture.target_meta.id).is_empty());
    assert!(replacement.is_alive());
}

#[test]
fn a_routed_stream_cannot_append_to_a_new_turn_epoch() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    let paused = start_receipt(&fixture, chunk("继续回答\n\n"));
    {
        let mut live = fixture.manager.inner.lock();
        let live = live.as_mut().unwrap();
        live.turn_open_epoch = live.turn_open_epoch.wrapping_add(1);
        live.active_turn_id = Some(uuid::Uuid::new_v4().to_string());
    }
    let update = finish_receipt(paused);
    assert!(update.is_none(), "old receipt appended to a new turn");
    assert!(store::load_messages(&fixture.target_meta.id).is_empty());
}

#[test]
fn background_done_ticks_do_not_finish_a_prompt_still_in_flight() {
    let fixture = Fixture::new();
    fixture.move_focus(true);
    let update = target_receipt(
        &fixture,
        StreamChunk {
            done: true,
            ..chunk("")
        },
    )
    .unwrap();
    assert!(!update.emits.is_empty());
    assert!(
        update.emits.iter().all(|event| !event.done),
        "background IPC ended the active answer"
    );
    assert!(update.emits.iter().all(|event| !event.host_finalized));
    assert!(
        fixture
            .manager
            .background
            .lock()
            .get(&fixture.target_meta.id)
            .unwrap()
            .prompt_in_flight
    );
}

#[test]
fn background_done_ticks_do_not_finish_unresolved_tools_and_gates() {
    for gate in ["tool", "plan", "ask_user", "permission"] {
        let fixture = Fixture::new();
        fixture.move_focus(true);
        {
            let mut background = fixture.manager.background.lock();
            let live = background.get_mut(&fixture.target_meta.id).unwrap();
            live.prompt_in_flight = false;
            live.deferred_prompt_complete = Some("end_turn".into());
            match gate {
                "tool" => {
                    live.open_tool_ids.insert("仍在运行的工具".into());
                }
                "plan" => live.pending_plan_rpc_id = Some(17),
                "ask_user" => live.pending_ask_user_rpc_id = Some(18),
                _ => {
                    live.fsm.await_permission().unwrap();
                }
            }
        }
        let update = target_receipt(
            &fixture,
            StreamChunk {
                done: true,
                ..chunk("")
            },
        )
        .unwrap();
        assert!(!update.emits.is_empty());
        assert!(
            update.emits.iter().all(|event| !event.done),
            "background IPC hid unresolved {gate}"
        );
    }
}

#[test]
fn background_done_ticks_finish_when_the_rpc_completed_and_no_work_remains() {
    let fixture = Fixture::new();
    fixture.move_focus(true);
    {
        let mut background = fixture.manager.background.lock();
        let live = background.get_mut(&fixture.target_meta.id).unwrap();
        live.prompt_in_flight = false;
        live.deferred_prompt_complete = Some("end_turn".into());
    }
    let update = target_receipt(
        &fixture,
        StreamChunk {
            done: true,
            ..chunk("最终答案\n\n")
        },
    )
    .unwrap();
    assert!(
        update.emits.iter().any(|event| event.done),
        "a completed answer lost its terminal IPC"
    );
    assert!(
        update
            .emits
            .iter()
            .filter(|event| event.done)
            .all(|event| !event.host_finalized),
        "a raw done tick claimed the Host had already retired the turn"
    );
    assert_eq!(
        store::load_messages(&fixture.target_meta.id)[0].content,
        "最终答案\n\n"
    );
}

#[test]
fn idle_and_parked_replay_never_creates_a_new_answer() {
    let fixture = Fixture::new();
    assert!(target_receipt(&fixture, chunk("历史回放\n\n")).is_none());
    fixture.manager.try_park_live().unwrap();
    assert!(target_receipt(&fixture, chunk("历史回放\n\n")).is_none());
    assert!(store::load_messages(&fixture.target_meta.id).is_empty());
    assert!(fixture
        .manager
        .parked
        .lock()
        .contains_key(&fixture.target_meta.id));
}

#[test]
fn ambiguous_unstamped_shared_process_stream_is_not_assigned_to_focus() {
    let fixture = Fixture::new();
    fixture.move_focus(true);
    fixture.manager.try_park_live().unwrap();
    seed_other_background(&fixture, true, "target-process");
    let update =
        fixture
            .manager
            .prepare_stream_chunk("target-process", None, chunk("不明来源\n\n"));
    assert!(update.is_none());
    assert!(store::load_messages(&fixture.target_meta.id).is_empty());
    assert!(store::load_messages(&fixture.other_meta.id).is_empty());
}

#[test]
fn actual_shared_stream_consumer_preserves_chinese_thought_phase_boundaries() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    let first = target_receipt(
        &fixture,
        StreamChunk {
            kind: StreamKind::Thought,
            ..chunk("分析\n\n")
        },
    )
    .unwrap();
    assert_eq!(first.emits[0].thought_phase, "open");
    target_receipt(&fixture, chunk("继续回答\n\n")).unwrap();
    let resumed = target_receipt(
        &fixture,
        StreamChunk {
            kind: StreamKind::Thought,
            ..chunk("再分析\n\n")
        },
    )
    .unwrap();
    assert!(resumed
        .emits
        .iter()
        .any(|event| event.thought_phase == "new"));
    let rows = store::load_messages(&fixture.target_meta.id);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].content, "继续回答\n\n");
    assert_eq!(
        rows[0].thought.as_deref(),
        Some("分析\n\n\n\n⟪phase⟫\n\n再分析\n\n")
    );
    assert!(fixture.manager.runtime_transfer.try_lock().is_some());
    assert!(fixture.manager.inner.try_lock().is_some());
    assert!(fixture.manager.background.try_lock().is_some());
}

fn age_pending(fixture: &Fixture) {
    fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| {
            session.pending_stream_emit.as_mut().unwrap().first_at =
                std::time::Instant::now() - Duration::from_millis(100);
        });
}

#[test]
fn actual_stream_timer_take_survives_the_busy_pool_transfer_window() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    let schedule = target_receipt(&fixture, chunk("尚未发出的中文片段"))
        .unwrap()
        .schedule
        .unwrap();
    age_pending(&fixture);
    let moving = start_move(
        &fixture.manager,
        &fixture.target_meta.id,
        Move::BusyLiveBackground,
    );
    let manager = fixture.manager.clone();
    let (finished, worker) = action(move || manager.prepare_stream_flush(&schedule));
    let payload = finish(moving, finished, worker);
    assert!(
        payload.is_some(),
        "the native flush timer lost a migrating buffer"
    );
    let payload = payload.unwrap();
    assert_eq!(payload.session_id, fixture.target_meta.id);
    assert_eq!(payload.text, "尚未发出的中文片段");
}

fn replace_buffered_owner(fixture: &Fixture, replace_acp: bool) {
    let entered = fixture.runtime.enter();
    let replacement = if replace_acp {
        let (client, peer) = test_transport::client_for(false, SessionMcpPolicy::default(), None);
        (client, Some(peer))
    } else {
        (fixture.target.clone(), None)
    };
    drop(entered);
    let mut live = routing_tests::bare_live_session(&fixture.target_meta.id, "target-process");
    live.meta = fixture.target_meta.clone();
    live.acp = Some(replacement.0);
    make_busy(&mut live);
    live.turn_open_epoch = 1;
    *fixture.manager.inner.lock() = Some(live);
    target_receipt(fixture, chunk("新的缓冲回答")).unwrap();
    age_pending(fixture);
    drop(replacement.1);
}

#[test]
fn an_old_native_timer_cannot_take_a_replacement_acp_buffer() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    let old = target_receipt(&fixture, chunk("旧片段"))
        .unwrap()
        .schedule
        .unwrap();
    replace_buffered_owner(&fixture, true);
    let payload = fixture.manager.prepare_stream_flush(&old);
    assert!(
        payload.is_none(),
        "old timer stole a replacement client's payload"
    );
    assert_eq!(
        fixture
            .manager
            .inner
            .lock()
            .as_ref()
            .unwrap()
            .pending_stream_emit
            .as_ref()
            .unwrap()
            .text,
        "新的缓冲回答"
    );
}

#[test]
fn an_old_native_timer_cannot_take_a_new_turn_with_a_reused_flush_counter() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    let old = target_receipt(&fixture, chunk("旧片段"))
        .unwrap()
        .schedule
        .unwrap();
    replace_buffered_owner(&fixture, false);
    let payload = fixture.manager.prepare_stream_flush(&old);
    assert!(payload.is_none(), "old timer stole a new turn's payload");
    assert!(fixture
        .manager
        .inner
        .lock()
        .as_ref()
        .unwrap()
        .pending_stream_emit
        .is_some());
}

#[test]
fn current_native_timer_merges_chunks_and_stale_counter_leaves_them_intact() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    let first = target_receipt(&fixture, chunk("中文甲"))
        .unwrap()
        .schedule
        .unwrap();
    let latest = target_receipt(&fixture, chunk("中文乙"))
        .unwrap()
        .schedule
        .unwrap();
    age_pending(&fixture);
    assert!(fixture.manager.prepare_stream_flush(&first).is_none());
    let payload = fixture.manager.prepare_stream_flush(&latest).unwrap();
    assert_eq!(payload.text, "中文甲中文乙");
    assert_eq!(payload.session_id, fixture.target_meta.id);
    assert!(fixture.manager.runtime_transfer.try_lock().is_some());
    assert!(fixture.manager.inner.try_lock().is_some());
}
