//! Actual shared Plan/AskUser consumer, synthetic duplex and persisted chrome.
use super::runtime_transfer_tests::{action, finish, start_move, Move};
use super::tool_owner_tests::prepare_busy;
use super::*;
use crate::session_manager::human_gate_event::{HumanGateEvent, HumanGatePayload, HumanGateUpdate};
use serde_json::json;

#[path = "human_gate_invalidation_tests.rs"]
mod invalidation;

struct Pause {
    reached: mpsc::SyncSender<()>,
    proceed: mpsc::Receiver<()>,
}

fn pauses() -> &'static StdMutex<HashMap<String, Pause>> {
    static PAUSES: OnceLock<StdMutex<HashMap<String, Pause>>> = OnceLock::new();
    PAUSES.get_or_init(|| StdMutex::new(HashMap::new()))
}

pub(crate) fn pause_after_route(id: &str) {
    let pause = pauses().lock().unwrap().remove(id);
    if let Some(pause) = pause {
        pause.reached.send(()).unwrap();
        pause.proceed.recv_timeout(Duration::from_secs(4)).unwrap();
    }
}

fn event(ask: bool) -> HumanGateEvent {
    let wire = if ask {
        crate::acp_client::AcpEvent::AskUserQuestion {
            rpc_id: 71,
            tool_call_id: Some("中文提问工具".into()),
            questions: serde_json::from_value(json!([{
                "id": "方案", "question": "采用哪个方案？", "multiSelect": false,
                "options": [{ "id": "甲", "label": "保留工作区", "description": "推荐" }],
            }]))
            .unwrap(),
            raw: json!({}),
        }
    } else {
        crate::acp_client::AcpEvent::Plan {
            entries: json!([{ "content": "保留原工作区", "status": "pending" }]),
            body: Some("# 中文开发计划\n保留原工作区与会话。".into()),
            rpc_id: Some(70),
            tool_call_id: Some("中文计划工具".into()),
        }
    };
    HumanGateEvent::from_acp(wire).unwrap()
}

fn commit(update: Option<HumanGateUpdate>) -> Option<HumanGateUpdate> {
    let update = update?;
    update.persist();
    Some(update)
}

fn receipt(fixture: &Fixture, ask: bool) -> Option<HumanGateUpdate> {
    commit(fixture.manager.prepare_human_gate_event(
        "target-process",
        fixture.target_meta.agent_session_id.as_deref(),
        event(ask),
    ))
}

struct Paused {
    proceed: mpsc::SyncSender<()>,
    worker: std::thread::JoinHandle<Option<HumanGateUpdate>>,
}

fn start_receipt(fixture: &Fixture, ask: bool) -> Paused {
    let (reached_tx, reached_rx) = mpsc::sync_channel(1);
    let (proceed_tx, proceed_rx) = mpsc::sync_channel(1);
    pauses().lock().unwrap().insert(
        fixture.target_meta.id.clone(),
        Pause {
            reached: reached_tx,
            proceed: proceed_rx,
        },
    );
    let manager = fixture.manager.clone();
    let agent = fixture.target_meta.agent_session_id.clone();
    let worker = std::thread::spawn(move || {
        commit(manager.prepare_human_gate_event("target-process", agent.as_deref(), event(ask)))
    });
    reached_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    Paused {
        proceed: proceed_tx,
        worker,
    }
}

fn finish_receipt(paused: Paused) -> Option<HumanGateUpdate> {
    paused.proceed.send(()).unwrap();
    paused.worker.join().unwrap()
}

fn assert_pending(fixture: &Fixture, update: &HumanGateUpdate, ask: bool, background: bool) {
    assert_eq!(update.session_id, fixture.target_meta.id);
    assert_eq!(update.background.is_some(), background);
    if let Some(snapshot) = &update.background {
        assert_eq!(
            snapshot.session_id.as_deref(),
            Some(fixture.target_meta.id.as_str())
        );
    }
    fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| {
            if ask {
                assert_eq!(session.pending_ask_user_rpc_id, Some(71));
                let HumanGatePayload::AskUser(payload) = &update.payload else {
                    panic!("wrong gate payload");
                };
                let stored = session.pending_ask_user_ui.as_ref().unwrap();
                assert_eq!(
                    serde_json::to_value(stored).unwrap(),
                    serde_json::to_value(payload).unwrap()
                );
                assert_eq!(payload.questions[0].question, "采用哪个方案？");
                assert_eq!(payload.session_id, fixture.target_meta.id);
            } else {
                assert_eq!(session.pending_plan_rpc_id, Some(70));
            }
        })
        .unwrap();
    if !ask {
        let chrome = crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id).unwrap();
        assert_eq!(chrome.rpc_id, Some(70));
        assert!(chrome.body.contains("保留原工作区与会话"));
        assert_eq!(chrome.tool_call_id.as_deref(), Some("中文计划工具"));
    }
    assert!(crate::plan_chrome::load_plan_chrome(&fixture.other_meta.id).is_none());
    fixture
        .manager
        .with_session_mut(&fixture.other_meta.id, |session| {
            assert!(session.pending_plan_rpc_id.is_none());
            assert!(session.pending_ask_user_rpc_id.is_none());
            assert!(session.pending_ask_user_ui.is_none());
        });
    assert!(store::load_messages(&fixture.target_meta.id).is_empty());
    assert!(store::load_messages(&fixture.other_meta.id).is_empty());
}

fn assert_no_pending(fixture: &Fixture) {
    fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| {
            assert!(session.pending_plan_rpc_id.is_none());
            assert!(session.pending_ask_user_rpc_id.is_none());
            assert!(session.pending_ask_user_ui.is_none());
        })
        .unwrap();
    assert!(crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id).is_none());
    assert!(store::load_messages(&fixture.target_meta.id).is_empty());
}

#[test]
fn idle_restored_plan_and_ask_reverse_rpcs_remain_valid_in_both_pools() {
    for background in [false, true] {
        for ask in [false, true] {
            let fixture = Fixture::new();
            if background {
                fixture.move_focus(false);
            }
            let update = receipt(&fixture, ask).unwrap();
            assert_pending(&fixture, &update, ask, background);
        }
    }
}

#[test]
fn idle_plan_notifications_drop_without_a_gate_and_preserve_existing_chrome() {
    for background in [false, true] {
        let fixture = Fixture::new();
        if background {
            fixture.move_focus(false);
        }
        let mut replay = event(false);
        if let HumanGateEvent::Plan { rpc_id, .. } = &mut replay {
            *rpc_id = None;
        }
        assert!(fixture
            .manager
            .prepare_human_gate_event(
                "target-process",
                fixture.target_meta.agent_session_id.as_deref(),
                replay,
            )
            .is_none());
        assert_no_pending(&fixture);
        let update = receipt(&fixture, false).unwrap();
        assert_pending(&fixture, &update, false, background);
        let mut progress = event(false);
        if let HumanGateEvent::Plan { rpc_id, body, .. } = &mut progress {
            *rpc_id = None;
            *body = Some("已有确认中的计划更新".into());
        }
        assert!(commit(fixture.manager.prepare_human_gate_event(
            "target-process",
            fixture.target_meta.agent_session_id.as_deref(),
            progress,
        ))
        .is_some());
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                assert_eq!(session.pending_plan_rpc_id, Some(70));
            })
            .unwrap();
        assert_eq!(
            crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id)
                .unwrap()
                .body,
            "已有确认中的计划更新"
        );
    }
}

#[test]
fn active_plan_notifications_remain_valid_in_both_pools() {
    for background in [false, true] {
        let fixture = Fixture::new();
        prepare_busy(&fixture);
        if background {
            fixture.move_focus(false);
        }
        let mut progress = event(false);
        if let HumanGateEvent::Plan { rpc_id, .. } = &mut progress {
            *rpc_id = None;
        }
        let update = commit(fixture.manager.prepare_human_gate_event(
            "target-process",
            fixture.target_meta.agent_session_id.as_deref(),
            progress,
        ))
        .unwrap();
        assert_eq!(update.session_id, fixture.target_meta.id);
        assert_eq!(update.background.is_some(), background);
        assert!(
            crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id)
                .unwrap()
                .body
                .contains("中文开发计划")
        );
    }
}

#[test]
fn human_gates_survive_the_actual_busy_live_background_transfer_window() {
    for ask in [false, true] {
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
            commit(manager.prepare_human_gate_event("target-process", agent.as_deref(), event(ask)))
        });
        let update = finish(moving, finished, worker);
        assert!(update.is_some(), "migrating human gate was discarded");
        assert_pending(&fixture, &update.unwrap(), ask, true);
    }
}

#[test]
fn human_gates_follow_a_routed_owner_into_background_without_touching_foreign_focus() {
    for ask in [false, true] {
        let fixture = Fixture::new();
        prepare_busy(&fixture);
        let paused = start_receipt(&fixture, ask);
        fixture.move_focus(false);
        let update = finish_receipt(paused).unwrap();
        assert_pending(&fixture, &update, ask, true);
    }
}

#[test]
fn human_gates_follow_a_routed_background_owner_back_to_live() {
    for ask in [false, true] {
        let fixture = Fixture::new();
        prepare_busy(&fixture);
        fixture.move_focus(false);
        let paused = start_receipt(&fixture, ask);
        let parked = fixture.manager.try_park_live();
        let promoted = fixture
            .manager
            .promote_background_to_live(&fixture.target_meta.id);
        let update = finish_receipt(paused);
        parked.unwrap();
        assert!(promoted);
        assert!(update.is_some(), "gate was lost after background promotion");
        assert_pending(&fixture, &update.unwrap(), ask, false);
    }
}

#[test]
fn routed_human_gates_cannot_borrow_a_replacement_acp_with_the_same_aliases() {
    for background in [false, true] {
        for ask in [false, true] {
            let fixture = Fixture::new();
            prepare_busy(&fixture);
            if background {
                fixture.move_focus(false);
            }
            let entered = fixture.runtime.enter();
            let (replacement, _peer) =
                test_transport::client_for(false, SessionMcpPolicy::default(), None);
            drop(entered);
            let paused = start_receipt(&fixture, ask);
            fixture
                .manager
                .with_session_mut(&fixture.target_meta.id, |session| {
                    session.acp = Some(replacement.clone());
                })
                .unwrap();
            let update = finish_receipt(paused);
            assert!(
                update.is_none(),
                "old human gate attached to replacement ACP"
            );
            assert_no_pending(&fixture);
            assert!(replacement.is_alive());
        }
    }
}

#[test]
fn routed_human_gates_cannot_attach_to_a_new_epoch_or_active_turn() {
    for background in [false, true] {
        for ask in [false, true] {
            for epoch in [false, true] {
                let fixture = Fixture::new();
                prepare_busy(&fixture);
                if background {
                    fixture.move_focus(false);
                }
                let paused = start_receipt(&fixture, ask);
                fixture
                    .manager
                    .with_session_mut(&fixture.target_meta.id, |session| {
                        if epoch {
                            session.turn_open_epoch = session.turn_open_epoch.wrapping_add(1);
                        } else {
                            session.active_turn_id = Some(uuid::Uuid::new_v4().to_string());
                        }
                    })
                    .unwrap();
                let update = finish_receipt(paused);
                assert!(update.is_none(), "old human gate attached to a new turn");
                assert_no_pending(&fixture);
            }
        }
    }
}

#[test]
fn routed_human_gates_recheck_process_and_agent_at_the_final_mutation() {
    for background in [false, true] {
        for ask in [false, true] {
            for process in [false, true] {
                let fixture = Fixture::new();
                if background {
                    fixture.move_focus(false);
                }
                let paused = start_receipt(&fixture, ask);
                fixture
                    .manager
                    .with_session_mut(&fixture.target_meta.id, |session| {
                        if process {
                            session.process_id = "replacement-process".into();
                        } else {
                            session.meta.agent_session_id = Some("replacement-agent".into());
                        }
                    })
                    .unwrap();
                let update = finish_receipt(paused);
                assert!(update.is_none(), "gate attached after wire owner changed");
                assert_no_pending(&fixture);
            }
        }
    }
}

#[test]
fn foreign_unbound_and_parked_human_gate_events_do_not_create_pending_state() {
    for ask in [false, true] {
        let fixture = Fixture::new();
        assert!(fixture
            .manager
            .prepare_human_gate_event(
                "other-process",
                fixture.target_meta.agent_session_id.as_deref(),
                event(ask)
            )
            .is_none());
        assert!(fixture
            .manager
            .prepare_human_gate_event("target-process", Some("foreign-agent"), event(ask))
            .is_none());
        fixture
            .manager
            .inner
            .lock()
            .as_mut()
            .unwrap()
            .meta
            .agent_session_id = None;
        assert!(receipt(&fixture, ask).is_none());
        assert_no_pending(&fixture);
        fixture.manager.inner.lock().as_mut().unwrap().meta = fixture.target_meta.clone();
        fixture.move_focus(false);
        fixture.park_target();
        assert!(receipt(&fixture, ask).is_none());
        assert!(crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id).is_none());
    }
}

#[test]
fn ambiguous_unstamped_busy_human_gates_are_dropped() {
    for ask in [false, true] {
        let fixture = Fixture::new();
        prepare_busy(&fixture);
        fixture.move_focus(false);
        let mut other = fixture.manager.inner.lock().take().unwrap();
        other.process_id = "target-process".into();
        other.prompt_in_flight = true;
        fixture
            .manager
            .background
            .lock()
            .insert(fixture.other_meta.id.clone(), other);
        assert!(fixture
            .manager
            .prepare_human_gate_event("target-process", None, event(ask))
            .is_none());
        assert_no_pending(&fixture);
    }
}

#[test]
fn a_routed_idle_human_gate_does_not_rescue_a_parked_owner() {
    for ask in [false, true] {
        let fixture = Fixture::new();
        let paused = start_receipt(&fixture, ask);
        let parked = fixture.manager.try_park_live();
        let update = finish_receipt(paused);
        parked.unwrap();
        assert!(update.is_none());
        assert!(fixture
            .manager
            .parked
            .lock()
            .contains_key(&fixture.target_meta.id));
        assert!(crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id).is_none());
    }
}

fn prepare_plan_body(fixture: &Fixture, body: &str) -> HumanGateUpdate {
    let mut next = event(false);
    if let HumanGateEvent::Plan { body: text, .. } = &mut next {
        *text = Some(body.into());
    }
    fixture
        .manager
        .prepare_human_gate_event(
            "target-process",
            fixture.target_meta.agent_session_id.as_deref(),
            next,
        )
        .unwrap()
}

fn resolve_plan(
    fixture: &Fixture,
    decision: &str,
) -> crate::session_manager::human_gate_resolution::HumanGateResolution {
    use crate::session_manager::human_gate_resolution::{HumanGateAnswer, HumanGateKind};
    let generation = fixture
        .manager
        .pending_plan(Some(fixture.target_meta.id.clone()))
        .unwrap()
        .request_generation;
    let resolved = fixture
        .runtime
        .block_on(fixture.manager.resolve_human_gate_core(HumanGateAnswer {
            kind: HumanGateKind::Plan,
            rpc_id: Some(70),
            request_generation: generation,
            decision: decision.into(),
            feedback: None,
            answers: None,
            session_id: Some(fixture.target_meta.id.clone()),
        }))
        .unwrap();
    assert_eq!(
        resolved.snapshot.session_id.as_deref(),
        Some(fixture.target_meta.id.as_str())
    );
    resolved
}

fn resolve_plan_and_persist(fixture: &Fixture, decision: &str) {
    // The command and this test use the same captured post-core writer.
    resolve_plan(fixture, decision).persist_plan_chrome();
}

#[test]
fn delayed_plan_persistence_cannot_overwrite_the_newer_prepared_generation() {
    for background in [false, true] {
        let fixture = Fixture::new();
        if background {
            fixture.move_focus(false);
        }
        let old = prepare_plan_body(&fixture, "过时的原计划");
        let current = prepare_plan_body(&fixture, "当前的新计划");
        current.persist();
        old.persist();
        let chrome = crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id).unwrap();
        assert_eq!(
            chrome.body, "当前的新计划",
            "late old persistence overwrote the new gate"
        );
        assert_eq!(chrome.rpc_id, Some(70));
        assert!(crate::plan_chrome::load_plan_chrome(&fixture.other_meta.id).is_none());
    }
}

#[test]
fn a_same_rpc_new_generation_reopens_after_actual_abandonment() {
    for background in [false, true] {
        let fixture = Fixture::new();
        if background {
            fixture.move_focus(false);
        }
        prepare_plan_body(&fixture, "已放弃的计划").persist();
        resolve_plan_and_persist(&fixture, "abandoned");
        assert!(
            crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id)
                .unwrap()
                .user_closed
        );
        prepare_plan_body(&fixture, "复用编号的新计划").persist();
        let chrome = crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id).unwrap();
        assert!(
            !chrome.user_closed,
            "a fresh reverse request was mistaken for the abandoned gate"
        );
        assert!(chrome.visible);
        assert_eq!(chrome.body, "复用编号的新计划");
        assert_eq!(chrome.rpc_id, Some(70));
    }
}

#[test]
fn delayed_prepared_plan_cannot_resurrect_an_actually_approved_gate() {
    for background in [false, true] {
        let fixture = Fixture::new();
        if background {
            fixture.move_focus(false);
        }
        let pending = prepare_plan_body(&fixture, "已审批的计划");
        pending.persist();
        resolve_plan_and_persist(&fixture, "approved");
        pending.persist();
        let chrome = crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id).unwrap();
        assert!(
            chrome.rpc_id.is_none(),
            "late prepared event restored a completed reverse RPC"
        );
        assert!(!chrome.visible);
        assert!(!chrome.awaiting_agent_approval);
    }
}

#[test]
fn an_actual_decision_delayed_until_after_a_new_gate_cannot_hide_that_gate() {
    for background in [false, true] {
        let fixture = Fixture::new();
        if background {
            fixture.move_focus(false);
        }
        prepare_plan_body(&fixture, "审批前的原计划").persist();
        let old = resolve_plan(&fixture, "approved");
        prepare_plan_body(&fixture, "审批后到达的新计划").persist();
        old.persist_plan_chrome();
        let chrome = crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id).unwrap();
        assert_eq!(chrome.body, "审批后到达的新计划");
        assert_eq!(chrome.rpc_id, Some(70));
        assert!(chrome.visible);
        assert!(chrome.awaiting_agent_approval);
    }
}

#[test]
fn decision_keeps_the_runtime_plan_when_the_event_has_not_reached_disk_yet() {
    let fixture = Fixture::new();
    let original = prepare_plan_body(&fixture, "尚未写盘但已审批的中文计划");
    resolve_plan_and_persist(&fixture, "approved");
    original.persist();
    let chrome = crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id).unwrap();
    assert_eq!(chrome.body, "尚未写盘但已审批的中文计划");
    assert!(chrome.rpc_id.is_none());
    assert!(!chrome.visible);
    let raw = std::fs::read_to_string(
        crate::paths::session_dir(&fixture.target_meta.id).join("plan_chrome.json"),
    )
    .unwrap();
    assert!(!raw.contains("requestGeneration"));
    assert!(!raw.contains("revision"));
}

#[test]
fn preparing_a_plan_never_waits_for_an_existing_store_transaction() {
    let fixture = Fixture::new();
    prepare_plan_body(&fixture, "第一份计划").persist();
    let lock = crate::store_lock::lock_exclusive(
        &crate::paths::session_dir(&fixture.target_meta.id).join("plan_chrome.json"),
    )
    .unwrap();
    let manager = fixture.manager.clone();
    let agent = fixture.target_meta.agent_session_id.clone();
    let (finished, worker) = action(move || {
        manager.prepare_human_gate_event("target-process", agent.as_deref(), event(false))
    });
    let finished_without_disk = finished.recv_timeout(Duration::from_secs(1));
    drop(lock);
    let result = worker.join().unwrap();
    assert!(
        finished_without_disk.is_ok(),
        "runtime preparation waited for a disk transaction"
    );
    assert!(result.is_some());
}

#[test]
fn prepared_plan_publication_serializes_effects_without_blocking_runtime_reservation() {
    let fixture = Fixture::new();
    let old = prepare_plan_body(&fixture, "原发布计划");
    let order = Arc::new(StdMutex::new(Vec::new()));
    let old_order = order.clone();
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let old_worker = std::thread::spawn(move || {
        old.publish(|| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(4)).unwrap();
            old_order.lock().unwrap().push("old");
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    // Only reservation runs in the runtime guard; the newer write/publish
    // waits outside it for the older effect, using the production publisher.
    let manager = fixture.manager.clone();
    let agent = fixture.target_meta.agent_session_id.clone();
    let (reserved_tx, reserved_rx) = mpsc::sync_channel(1);
    let new_order = order.clone();
    let new_worker = std::thread::spawn(move || {
        let update = manager
            .prepare_human_gate_event("target-process", agent.as_deref(), event(false))
            .unwrap();
        reserved_tx.send(()).unwrap();
        update.publish(|| new_order.lock().unwrap().push("new"));
    });
    let reserved = reserved_rx.recv_timeout(Duration::from_secs(1));
    release_tx.send(()).unwrap();
    old_worker.join().unwrap();
    new_worker.join().unwrap();
    assert!(
        reserved.is_ok(),
        "runtime reservation waited for effect publication"
    );
    assert_eq!(*order.lock().unwrap(), vec!["old", "new"]);
    assert_eq!(
        crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id)
            .unwrap()
            .rpc_id,
        Some(70)
    );
}

fn progress_plan(fixture: &Fixture, body: &str) -> HumanGateUpdate {
    fixture
        .manager
        .prepare_human_gate_event(
            "target-process",
            fixture.target_meta.agent_session_id.as_deref(),
            HumanGateEvent::Plan {
                entries: json!([]),
                body: Some(body.into()),
                rpc_id: None,
                tool_call_id: Some("同一个计划工具".into()),
            },
        )
        .unwrap()
}

fn dismiss_chrome(id: &str, revision: Option<String>) -> Result<(), String> {
    crate::commands::session_plan_chrome_set(
        id.into(),
        crate::plan_chrome::PlanChromeStored {
            user_closed: true,
            ..Default::default()
        },
        revision,
    )
}

#[test]
fn no_runtime_restored_chrome_can_close_once_without_persisting_the_ui_revision() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    progress_plan(&fixture, "重启后仍能关闭的中文草稿").persist();
    fixture.manager.inner.lock().take();
    let view = crate::commands::session_plan_chrome_get(fixture.target_meta.id.clone())
        .unwrap()
        .unwrap();
    let token = view
        .chrome_revision
        .expect("idle UI read must retain its clock");
    dismiss_chrome(&fixture.target_meta.id, Some(token.clone())).unwrap();
    assert!(dismiss_chrome(&fixture.target_meta.id, Some(token)).is_err());
    let chrome = crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id).unwrap();
    assert!(chrome.user_closed && !chrome.visible && chrome.body.is_empty());
    let raw = std::fs::read_to_string(
        crate::paths::session_dir(&fixture.target_meta.id).join("plan_chrome.json"),
    )
    .unwrap();
    assert!(!raw.contains("chromeRevision"));
    assert!(!raw.contains("requestGeneration"));
}

#[test]
fn closing_current_no_rpc_progress_invalidates_its_delayed_prepared_event() {
    for background in [false, true] {
        let fixture = Fixture::new();
        prepare_busy(&fixture);
        if background {
            fixture.move_focus(false);
        }
        let pending = progress_plan(&fixture, "正在展示的中文计划");
        let HumanGatePayload::Plan(plan) = &pending.payload else {
            panic!("wrong payload");
        };
        // A displayed event is sufficient even if its optional disk write has
        // not completed; closing must not manufacture a reverse-RPC answer.
        dismiss_chrome(&fixture.target_meta.id, plan.chrome_revision.clone()).unwrap();
        pending.persist();
        let chrome = crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id).unwrap();
        assert!(chrome.user_closed && !chrome.visible && chrome.rpc_id.is_none());
        progress_plan(&fixture, "已关闭周期的残余进度").persist();
        assert!(
            crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id)
                .unwrap()
                .user_closed,
            "a progress update resurrected the cycle closed before its first disk write"
        );
        assert!(fixture
            .manager
            .pending_plan(Some(fixture.target_meta.id.clone()))
            .is_none());
        assert!(fixture.target.is_alive());
    }
}

#[test]
fn old_no_rpc_close_cannot_borrow_a_new_same_tool_plan_or_another_chat() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    progress_plan(&fixture, "原草稿").persist();
    let token = crate::commands::session_plan_chrome_get(fixture.target_meta.id.clone())
        .unwrap()
        .unwrap()
        .chrome_revision;
    progress_plan(&fixture, "同工具的新草稿").persist();
    let before =
        std::fs::read(crate::paths::session_dir(&fixture.target_meta.id).join("plan_chrome.json"))
            .unwrap();
    assert!(dismiss_chrome(&fixture.target_meta.id, token.clone()).is_err());
    assert!(dismiss_chrome(&fixture.target_meta.id, None).is_err());
    assert!(dismiss_chrome(&fixture.other_meta.id, token).is_err());
    assert_eq!(
        std::fs::read(crate::paths::session_dir(&fixture.target_meta.id).join("plan_chrome.json"))
            .unwrap(),
        before
    );
    assert!(crate::plan_chrome::load_plan_chrome(&fixture.other_meta.id).is_none());
}

#[test]
fn a_live_rpc_cannot_be_closed_through_the_read_only_chrome_setter() {
    let fixture = Fixture::new();
    prepare_plan_body(&fixture, "必须真正审批的计划").persist();
    let view = crate::commands::session_plan_chrome_get(fixture.target_meta.id.clone())
        .unwrap()
        .unwrap();
    assert!(dismiss_chrome(&fixture.target_meta.id, view.chrome_revision).is_err());
    assert!(fixture
        .manager
        .pending_plan(Some(fixture.target_meta.id.clone()))
        .is_some());
    assert_eq!(
        crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id)
            .unwrap()
            .rpc_id,
        Some(70)
    );
}

#[test]
fn a_revised_plan_can_close_with_its_actual_decision_receipt() {
    let fixture = Fixture::new();
    prepare_plan_body(&fixture, "需要修改的计划").persist();
    let reply = resolve_plan(&fixture, "cancelled");
    reply.persist_plan_chrome();
    dismiss_chrome(&fixture.target_meta.id, reply.snapshot.plan_chrome_revision).unwrap();
    assert!(
        crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id)
            .unwrap()
            .user_closed
    );
}

#[test]
fn no_rpc_close_rejects_an_identical_session_and_chrome_in_another_store() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    progress_plan(&fixture, "同编号同内容的原存储").persist();
    let token = crate::commands::session_plan_chrome_get(fixture.target_meta.id.clone())
        .unwrap()
        .unwrap()
        .chrome_revision;
    let original_root = crate::paths::app_data_root();
    let original =
        std::fs::read(crate::paths::session_dir(&fixture.target_meta.id).join("plan_chrome.json"))
            .unwrap();
    let other_root = fixture.home.path.join("copied-store");
    let other_path = other_root
        .join("sessions")
        .join(&fixture.target_meta.id)
        .join("plan_chrome.json");
    std::fs::create_dir_all(other_path.parent().unwrap()).unwrap();
    std::fs::write(&other_path, &original).unwrap();
    std::env::set_var("GROK_APP_HOME", &other_root);
    let result = dismiss_chrome(&fixture.target_meta.id, token);
    std::env::set_var("GROK_APP_HOME", &original_root);
    assert!(result.is_err());
    assert_eq!(std::fs::read(&other_path).unwrap(), original);
}

#[test]
fn reading_agent_plan_snapshot_preserves_a_current_live_chrome_byte_for_byte() {
    let fixture = Fixture::new();
    prepare_plan_body(&fixture, "当前可审批的计划").persist();
    let mut settings = store::load_settings();
    settings.session_data_mode = "independent".into();
    store::save_settings(&settings).unwrap();
    let project = store::load_projects_snapshot()
        .unwrap()
        .into_iter()
        .find(|project| Some(&project.id) == fixture.target_meta.project_id.as_ref())
        .unwrap();
    let dir = crate::paths::agent_home_dir()
        .join("sessions")
        .join(crate::paths::percent_encode_path_component(&project.path))
        .join(fixture.target_meta.agent_session_id.as_ref().unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("plan.md"), "代理磁盘上的另一个草稿").unwrap();
    std::fs::write(
        dir.join("plan_mode.json"),
        r#"{"awaiting_plan_approval":true,"state":"awaiting"}"#,
    )
    .unwrap();
    let path = crate::paths::session_dir(&fixture.target_meta.id).join("plan_chrome.json");
    let before = std::fs::read(&path).unwrap();
    let snapshot =
        crate::commands::session_agent_plan_snapshot(fixture.target_meta.id.clone()).unwrap();
    assert!(snapshot.found && snapshot.awaiting_plan_approval);
    assert_eq!(
        snapshot.plan_body.as_deref(),
        Some("代理磁盘上的另一个草稿")
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(fixture
        .manager
        .pending_plan(Some(fixture.target_meta.id.clone()))
        .is_some());
}

#[test]
fn a_failed_conditional_disk_write_can_retry_the_same_displayed_revision() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    progress_plan(&fixture, "写盘失败后可重试的计划").persist();
    let token = crate::commands::session_plan_chrome_get(fixture.target_meta.id.clone())
        .unwrap()
        .unwrap()
        .chrome_revision;
    let path = crate::paths::session_dir(&fixture.target_meta.id).join("plan_chrome.json");
    let backup = path.with_extension("fixture-backup");
    std::fs::rename(&path, &backup).unwrap();
    std::fs::create_dir(&path).unwrap();
    let failed = dismiss_chrome(&fixture.target_meta.id, token.clone());
    std::fs::remove_dir(&path).unwrap();
    std::fs::rename(&backup, &path).unwrap();
    assert!(failed.is_err());
    assert!(
        !crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id)
            .unwrap()
            .user_closed
    );
    dismiss_chrome(&fixture.target_meta.id, token).unwrap();
    assert!(
        crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id)
            .unwrap()
            .user_closed
    );
}

#[test]
fn a_human_reply_does_not_borrow_identical_metadata_from_a_copied_store() {
    use crate::session_manager::human_gate_resolution::{HumanGateAnswer, HumanGateKind};
    use tokio::io::AsyncReadExt;
    let mut fixture = Fixture::new();
    prepare_plan_body(&fixture, "原存储的真实审批").persist();
    let generation = fixture
        .manager
        .pending_plan(Some(fixture.target_meta.id.clone()))
        .unwrap()
        .request_generation;
    let original_root = crate::paths::app_data_root();
    let other_root = fixture.home.path.join("copied-reply-store");
    std::fs::create_dir_all(&other_root).unwrap();
    std::fs::copy(
        crate::paths::projects_file(),
        other_root.join("projects.json"),
    )
    .unwrap();
    std::env::set_var("GROK_APP_HOME", &other_root);
    store::update_session_meta(&fixture.target_meta).unwrap();
    let result = fixture
        .runtime
        .block_on(fixture.manager.resolve_human_gate_core(HumanGateAnswer {
            kind: HumanGateKind::Plan,
            rpc_id: Some(70),
            request_generation: generation,
            decision: "approved".into(),
            feedback: None,
            answers: None,
            session_id: Some(fixture.target_meta.id.clone()),
        }));
    std::env::set_var("GROK_APP_HOME", &original_root);
    assert!(result.is_err());
    let mut byte = [0];
    assert!(
        fixture
            .runtime
            .block_on(async {
                tokio::time::timeout(Duration::from_millis(30), fixture._peers[0].read(&mut byte))
                    .await
            })
            .is_err(),
        "a foreign store's reply reached ACP"
    );
    assert!(fixture
        .manager
        .pending_plan(Some(fixture.target_meta.id.clone()))
        .is_some());
}
