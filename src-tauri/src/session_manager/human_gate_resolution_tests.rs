//! Actual production replies over synthetic duplex ACP and isolated stores.
use super::tool_owner_tests::prepare_busy;
use super::*;
use crate::session_manager::human_gate_event::HumanGateEvent;
use crate::session_manager::human_gate_resolution::{
    HumanGateAnswer, HumanGateKind, HumanGateResolution,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};

struct Pause {
    reached: mpsc::SyncSender<()>,
    proceed: tokio::sync::oneshot::Receiver<()>,
}
fn pauses() -> &'static StdMutex<HashMap<String, Pause>> {
    static PAUSES: OnceLock<StdMutex<HashMap<String, Pause>>> = OnceLock::new();
    PAUSES.get_or_init(|| StdMutex::new(HashMap::new()))
}
pub(crate) async fn pause(id: &str, stage: &str) {
    let pause = pauses().lock().unwrap().remove(&format!("{id}:{stage}"));
    if let Some(pause) = pause {
        pause.reached.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(4), pause.proceed)
            .await
            .unwrap()
            .unwrap();
    }
}
fn receive(fixture: &Fixture, kind: HumanGateKind, rpc: u64) {
    let event = match kind {
        HumanGateKind::Plan => HumanGateEvent::Plan {
            entries: json!([{"content":"保留工作区", "status":"pending"}]),
            body: Some("# 开发计划\n保留工作区".into()),
            rpc_id: Some(rpc),
            tool_call_id: Some("plan".into()),
        },
        HumanGateKind::AskUser => HumanGateEvent::AskUser {
            rpc_id: rpc,
            tool_call_id: Some("ask".into()),
            questions: serde_json::from_value(
                json!([{"id":"方案", "question":"采用哪种方案？", "multiSelect":false,
                "options":[{"id":"甲", "label":"方案甲"}]}]),
            )
            .unwrap(),
        },
    };
    fixture
        .manager
        .prepare_human_gate_event(
            "target-process",
            fixture.target_meta.agent_session_id.as_deref(),
            event,
        )
        .unwrap()
        .persist();
}
fn setup(fixture: &Fixture, kind: HumanGateKind, background: bool) {
    prepare_busy(fixture);
    if background {
        fixture.move_focus(false);
    }
    receive(fixture, kind, 70);
}
fn answer(fixture: &Fixture, kind: HumanGateKind) -> HumanGateAnswer {
    HumanGateAnswer {
        kind,
        rpc_id: Some(70),
        request_generation: fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                session.human_gates.generation(kind)
            })
            .flatten(),
        decision: if kind == HumanGateKind::Plan {
            "approved"
        } else {
            "accepted"
        }
        .into(),
        feedback: None,
        answers: Some(json!({"采用哪种方案？":"方案甲"})),
        session_id: Some(fixture.target_meta.id.clone()),
    }
}
fn resolve(fixture: &Fixture, answer: HumanGateAnswer) -> Result<HumanGateResolution, String> {
    fixture
        .runtime
        .block_on(fixture.manager.resolve_human_gate_core(answer))
}
fn frames(fixture: &mut Fixture) -> Vec<Value> {
    let mut reader = BufReader::new(&mut fixture._peers[0]);
    fixture.runtime.block_on(async {
        let mut frames = Vec::new();
        loop {
            let mut line = String::new();
            match tokio::time::timeout(Duration::from_millis(100), reader.read_line(&mut line))
                .await
            {
                Ok(Ok(0)) | Err(_) => break,
                Ok(Ok(_)) => frames.push(serde_json::from_str(&line).unwrap()),
                Ok(Err(error)) => panic!("synthetic peer: {error}"),
            }
        }
        frames
    })
}
struct Paused {
    proceed: tokio::sync::oneshot::Sender<()>,
    worker: std::thread::JoinHandle<Result<HumanGateResolution, String>>,
}
fn start(fixture: &Fixture, kind: HumanGateKind, stage: &str) -> Paused {
    let (reached_tx, reached_rx) = mpsc::sync_channel(1);
    let (proceed_tx, proceed_rx) = tokio::sync::oneshot::channel();
    pauses().lock().unwrap().insert(
        format!("{}:{stage}", fixture.target_meta.id),
        Pause {
            reached: reached_tx,
            proceed: proceed_rx,
        },
    );
    let manager = fixture.manager.clone();
    let request = answer(fixture, kind);
    let worker = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(manager.resolve_human_gate_core(request))
    });
    reached_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    Paused {
        proceed: proceed_tx,
        worker,
    }
}
fn finish(paused: Paused) -> Result<HumanGateResolution, String> {
    paused.proceed.send(()).unwrap();
    paused.worker.join().unwrap()
}
fn pending(fixture: &Fixture, kind: HumanGateKind) -> Option<u64> {
    fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| match kind {
            HumanGateKind::Plan => session.pending_plan_rpc_id,
            HumanGateKind::AskUser => session.pending_ask_user_rpc_id,
        })
        .unwrap()
}

#[test]
fn actual_current_plan_and_question_replies_keep_the_cli_wire_contract() {
    for kind in [HumanGateKind::Plan, HumanGateKind::AskUser] {
        let mut fixture = Fixture::new();
        setup(&fixture, kind, false);
        assert!(resolve(&fixture, answer(&fixture, kind)).is_ok());
        let wire = frames(&mut fixture);
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0]["id"], 70);
        assert_eq!(
            wire[0]["result"]["outcome"],
            if kind == HumanGateKind::Plan {
                "approved"
            } else {
                "accepted"
            }
        );
        assert_eq!(pending(&fixture, kind), None);
        assert!(fixture.target.is_alive());
        assert!(fixture.other.is_alive());
    }
}
#[test]
fn an_explicit_expired_human_rpc_cannot_override_the_current_native_request() {
    for kind in [HumanGateKind::Plan, HumanGateKind::AskUser] {
        for background in [false, true] {
            let mut fixture = Fixture::new();
            setup(&fixture, kind, background);
            let mut stale = answer(&fixture, kind);
            stale.rpc_id = Some(69);
            assert!(
                resolve(&fixture, stale).is_err(),
                "an explicit old RPC overrode the actual pending gate"
            );
            assert!(frames(&mut fixture).is_empty());
            assert_eq!(pending(&fixture, kind), Some(70));
        }
    }
}
#[test]
fn a_late_human_reply_cannot_clear_the_next_native_gate() {
    for kind in [HumanGateKind::Plan, HumanGateKind::AskUser] {
        for background in [false, true] {
            let mut fixture = Fixture::new();
            setup(&fixture, kind, background);
            let paused = start(&fixture, kind, "after_write");
            receive(&fixture, kind, 71);
            assert!(
                finish(paused).is_err(),
                "the original completion adopted a newer request"
            );
            assert_eq!(pending(&fixture, kind), Some(71));
            assert_eq!(frames(&mut fixture).len(), 1);
        }
    }
}
#[test]
fn a_background_human_reply_returns_its_target_snapshot_without_stealing_focus() {
    for kind in [HumanGateKind::Plan, HumanGateKind::AskUser] {
        let fixture = Fixture::new();
        setup(&fixture, kind, true);
        let result = resolve(&fixture, answer(&fixture, kind)).unwrap();
        assert_eq!(
            result.snapshot.session_id.as_deref(),
            Some(fixture.target_meta.id.as_str())
        );
        assert_eq!(
            fixture.manager.snapshot().session_id.as_deref(),
            Some(fixture.other_meta.id.as_str())
        );
    }
}

#[test]
fn restored_cards_and_live_events_share_an_identity_without_persisting_it() {
    for kind in [HumanGateKind::Plan, HumanGateKind::AskUser] {
        let fixture = Fixture::new();
        setup(&fixture, kind, true);
        let generation = answer(&fixture, kind).request_generation.unwrap();
        let restored = match kind {
            HumanGateKind::Plan => serde_json::to_value(
                fixture
                    .manager
                    .pending_plan(Some(fixture.target_meta.id.clone()))
                    .unwrap(),
            )
            .unwrap(),
            HumanGateKind::AskUser => serde_json::to_value(
                fixture
                    .manager
                    .pending_ask_user(Some(fixture.target_meta.id.clone()))
                    .unwrap(),
            )
            .unwrap(),
        };
        assert_eq!(restored["requestGeneration"], generation.to_string());
        assert_eq!(restored["rpcId"], 70);
        if kind == HumanGateKind::Plan {
            let disk = std::fs::read_to_string(
                crate::paths::session_dir(&fixture.target_meta.id).join("plan_chrome.json"),
            )
            .unwrap();
            assert!(!disk.contains(&generation.to_string()));
        }
    }
}

#[test]
fn same_rpc_and_content_cannot_revive_an_old_human_card() {
    for kind in [HumanGateKind::Plan, HumanGateKind::AskUser] {
        for background in [false, true] {
            let mut fixture = Fixture::new();
            setup(&fixture, kind, background);
            let stale = answer(&fixture, kind);
            receive(&fixture, kind, 70);
            assert_ne!(
                stale.request_generation,
                answer(&fixture, kind).request_generation
            );
            assert!(resolve(&fixture, stale).is_err());
            assert!(frames(&mut fixture).is_empty());
            assert!(resolve(&fixture, answer(&fixture, kind)).is_ok());
            assert_eq!(frames(&mut fixture).len(), 1);
        }
    }
}

#[test]
fn missing_wrong_or_displaced_modern_identity_never_writes() {
    for kind in [HumanGateKind::Plan, HumanGateKind::AskUser] {
        for mutation in 0..4 {
            let mut fixture = Fixture::new();
            setup(&fixture, kind, false);
            let mut request = answer(&fixture, kind);
            match mutation {
                0 => request.request_generation = None,
                1 => request.request_generation = Some(uuid::Uuid::new_v4()),
                2 => request.rpc_id = None,
                _ => {
                    fixture
                        .manager
                        .with_session_mut(&fixture.target_meta.id, |session| {
                            session.human_gates = Default::default()
                        })
                        .unwrap();
                }
            }
            assert!(resolve(&fixture, request).is_err());
            assert!(frames(&mut fixture).is_empty());
            assert_eq!(pending(&fixture, kind), Some(70));
        }
    }
}

#[test]
fn an_actual_legacy_gate_with_no_generated_card_remains_answerable() {
    for kind in [HumanGateKind::Plan, HumanGateKind::AskUser] {
        let mut fixture = Fixture::new();
        prepare_busy(&fixture);
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| match kind {
                HumanGateKind::Plan => session.pending_plan_rpc_id = Some(70),
                HumanGateKind::AskUser => session.pending_ask_user_rpc_id = Some(70),
            })
            .unwrap();
        let mut request = answer(&fixture, kind);
        request.rpc_id = None;
        assert!(resolve(&fixture, request).is_ok());
        assert_eq!(frames(&mut fixture).len(), 1);
    }
}

#[test]
fn delayed_replies_recheck_process_turn_and_workspace_before_any_wire_byte() {
    for kind in [HumanGateKind::Plan, HumanGateKind::AskUser] {
        for mutation in 0..5 {
            let mut fixture = Fixture::new();
            setup(&fixture, kind, true);
            let paused = start(&fixture, kind, "before_write");
            fixture
                .manager
                .with_session_mut(&fixture.target_meta.id, |session| match mutation {
                    0 => session.acp = Some(fixture.other.clone()),
                    1 => session.turn_open_epoch = session.turn_open_epoch.wrapping_add(1),
                    2 => session.active_turn_id = Some("new-turn".into()),
                    3 => session.project_path = Some("new-workspace".into()),
                    _ => session.meta.agent_session_id = Some("new-agent".into()),
                })
                .unwrap();
            assert!(finish(paused).is_err());
            assert!(frames(&mut fixture).is_empty());
            assert_eq!(pending(&fixture, kind), Some(70));
            assert!(fixture.other.is_alive());
        }
    }
}

#[test]
fn a_duplicate_reply_is_rejected_while_the_original_owns_the_gate() {
    for kind in [HumanGateKind::Plan, HumanGateKind::AskUser] {
        let mut fixture = Fixture::new();
        setup(&fixture, kind, false);
        let paused = start(&fixture, kind, "before_write");
        assert!(resolve(&fixture, answer(&fixture, kind)).is_err());
        assert!(finish(paused).is_ok());
        assert_eq!(frames(&mut fixture).len(), 1);
        assert!(fixture.manager.human_responses.lock().is_empty());
    }
}

#[test]
fn post_write_workspace_failure_does_not_turn_delivery_into_an_unsent_reply() {
    for kind in [HumanGateKind::Plan, HumanGateKind::AskUser] {
        let mut fixture = Fixture::new();
        setup(&fixture, kind, false);
        let original_path = fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                session.project_path.clone()
            })
            .unwrap();
        let paused = start(&fixture, kind, "after_write");
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                session.project_path = Some("changed".into())
            })
            .unwrap();
        assert!(finish(paused).is_err());
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                session.project_path = original_path
            })
            .unwrap();
        assert!(resolve(&fixture, answer(&fixture, kind)).is_err());
        assert_eq!(frames(&mut fixture).len(), 1);
        assert_eq!(pending(&fixture, kind), Some(70));
    }
}

#[test]
fn stop_or_steer_does_not_cancel_a_response_already_delivered() {
    for kind in [HumanGateKind::Plan, HumanGateKind::AskUser] {
        let mut fixture = Fixture::new();
        setup(&fixture, kind, true);
        let paused = start(&fixture, kind, "after_write");
        let cancellation = fixture
            .manager
            .with_session_mut(
                &fixture.target_meta.id,
                SessionManager::take_human_gates_for_cancel,
            )
            .unwrap();
        let cancellation = if kind == HumanGateKind::Plan {
            cancellation.1
        } else {
            cancellation.0
        }
        .unwrap();
        fixture
            .runtime
            .block_on(cancellation.respond(&fixture.target))
            .unwrap();
        assert!(finish(paused).is_err());
        assert_eq!(frames(&mut fixture).len(), 1);
        assert_eq!(pending(&fixture, kind), None);
        assert!(fixture.manager.human_responses.lock().is_empty());
    }
}

#[test]
fn progress_updates_keep_the_live_plan_gate_identity_and_restore_body() {
    let fixture = Fixture::new();
    setup(&fixture, HumanGateKind::Plan, false);
    let first = fixture
        .manager
        .pending_plan(Some(fixture.target_meta.id.clone()))
        .unwrap();
    fixture
        .manager
        .prepare_human_gate_event(
            "target-process",
            fixture.target_meta.agent_session_id.as_deref(),
            HumanGateEvent::Plan {
                entries: json!([]),
                body: Some("最新计划".into()),
                rpc_id: None,
                tool_call_id: Some("plan".into()),
            },
        )
        .unwrap()
        .persist();
    let restored = fixture
        .manager
        .pending_plan(Some(fixture.target_meta.id.clone()))
        .unwrap();
    assert_eq!(restored.request_generation, first.request_generation);
    assert_eq!(restored.rpc_id, Some(70));
    assert_eq!(restored.body.as_deref(), Some("最新计划"));
}

#[test]
fn stop_waits_for_an_unwritten_answer_and_then_sends_one_cancel() {
    for kind in [HumanGateKind::Plan, HumanGateKind::AskUser] {
        let mut fixture = Fixture::new();
        setup(&fixture, kind, false);
        let paused = start(&fixture, kind, "before_write");
        let gates = fixture
            .manager
            .with_session_mut(
                &fixture.target_meta.id,
                SessionManager::take_human_gates_for_cancel,
            )
            .unwrap();
        let gate = if kind == HumanGateKind::Plan {
            gates.1
        } else {
            gates.0
        }
        .unwrap();
        fixture.runtime.block_on(async {
            let client = fixture.target.clone();
            let cancel = tokio::spawn(async move { gate.respond(&client).await });
            tokio::task::yield_now().await;
            assert!(!cancel.is_finished());
            paused.proceed.send(()).unwrap();
            assert!(
                tokio::task::spawn_blocking(move || paused.worker.join().unwrap())
                    .await
                    .unwrap()
                    .is_err()
            );
            tokio::time::timeout(Duration::from_secs(3), cancel)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        });
        let wire = frames(&mut fixture);
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0]["result"]["outcome"], "cancelled");
    }
}

#[test]
fn cancelling_a_partial_human_frame_retires_only_its_original_transport() {
    use crate::acp_client::test_transport::WriterPauseAt;
    use tokio::io::AsyncReadExt;
    for kind in [HumanGateKind::Plan, HumanGateKind::AskUser] {
        let mut fixture = Fixture::new();
        setup(&fixture, kind, true);
        let request = answer(&fixture, kind);
        let manager = fixture.manager.clone();
        let mut peer = fixture._peers.remove(0);
        fixture.runtime.block_on(async {
            let control =
                test_transport::install_writer_pause(&fixture.target, WriterPauseAt::Bytes(17))
                    .await;
            let reply = tokio::spawn(async move { manager.resolve_human_gate_core(request).await });
            tokio::time::timeout(Duration::from_secs(2), control.wait())
                .await
                .unwrap();
            reply.abort();
            assert!(reply.await.err().is_some_and(|error| error.is_cancelled()));
            control.release();
            assert!(!fixture.target.is_alive());
            assert!(fixture.other.is_alive());
            assert!(fixture.manager.human_responses.lock().is_empty());
            assert!(fixture
                .manager
                .resolve_human_gate_core(answer(&fixture, kind))
                .await
                .is_err());
            fixture.target.kill().await;
            let mut bytes = vec![0; 17];
            tokio::time::timeout(Duration::from_secs(2), peer.read_exact(&mut bytes))
                .await
                .unwrap()
                .unwrap();
            assert!(!bytes.contains(&b'\n'));
        });
        assert_eq!(pending(&fixture, kind), Some(70));
    }
}

#[test]
fn a_reply_waiting_behind_a_frame_rechecks_the_gate_after_the_writer_unlocks() {
    use crate::acp_client::{test_transport::WriterPauseAt, PermissionOutcome};
    for kind in [HumanGateKind::Plan, HumanGateKind::AskUser] {
        let mut fixture = Fixture::new();
        setup(&fixture, kind, false);
        let old = answer(&fixture, kind);
        fixture.runtime.block_on(async {
            let control =
                test_transport::install_writer_pause(&fixture.target, WriterPauseAt::Bytes(0))
                    .await;
            let client = fixture.target.clone();
            let blocking = tokio::spawn(async move {
                client
                    .respond_permission(99, PermissionOutcome::Cancelled)
                    .await
            });
            tokio::time::timeout(Duration::from_secs(2), control.wait())
                .await
                .unwrap();
            let manager = fixture.manager.clone();
            let reply = tokio::spawn(async move { manager.resolve_human_gate_core(old).await });
            tokio::task::yield_now().await;
            tokio::task::yield_now().await;
            receive(&fixture, kind, 70);
            control.release();
            tokio::time::timeout(Duration::from_secs(3), blocking)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(tokio::time::timeout(Duration::from_secs(3), reply)
                .await
                .unwrap()
                .unwrap()
                .is_err());
        });
        let wire = frames(&mut fixture);
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0]["id"], 99);
        assert_eq!(pending(&fixture, kind), Some(70));
        assert!(resolve(&fixture, answer(&fixture, kind)).is_ok());
    }
}

#[test]
fn human_write_failures_preserve_the_gate_without_reusing_a_partial_frame() {
    for kind in [HumanGateKind::Plan, HumanGateKind::AskUser] {
        for fail_after in [0, 17] {
            let fixture = Fixture::new();
            setup(&fixture, kind, false);
            let written = fixture
                .runtime
                .block_on(test_transport::install_writer_fault(
                    &fixture.target,
                    Some(fail_after),
                    false,
                    3,
                ));
            assert!(resolve(&fixture, answer(&fixture, kind)).is_err());
            assert_eq!(
                written.load(std::sync::atomic::Ordering::SeqCst),
                fail_after
            );
            assert_eq!(pending(&fixture, kind), Some(70));
            assert!(fixture.manager.human_responses.lock().is_empty());
            if fail_after == 17 {
                assert!(!fixture.target.is_alive());
                assert!(resolve(&fixture, answer(&fixture, kind)).is_err());
                assert_eq!(written.load(std::sync::atomic::Ordering::SeqCst), 17);
            }
            assert!(fixture.other.is_alive());
        }
    }
}
