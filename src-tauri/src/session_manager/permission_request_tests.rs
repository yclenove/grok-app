//! Real permission request consumer, bounded races and synthetic ACP wire.
use super::tool_owner_tests::prepare_busy;
use super::*;
use crate::permission::PermissionPolicy;
use crate::session_manager::permission_request::{PermissionRequestEvent, PermissionRequestUpdate};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};

#[path = "permission_write_cancellation_tests.rs"]
mod cancellation;
#[path = "permission_request_failure_tests.rs"]
mod failure;

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

fn event() -> PermissionRequestEvent {
    PermissionRequestEvent::from_acp(crate::acp_client::AcpEvent::PermissionRequest {
        rpc_id: 92,
        tool_call_id: "中文工具".into(),
        tool_name: "shell".into(),
        title: "python 示例.py".into(),
        options: json!([
            {"optionId":"allow-once", "kind":"allow_once"},
            {"optionId":"reject-once", "kind":"reject_once"},
        ]),
        raw: json!({"rawInput":{"command":"python 示例.py"}}),
    })
    .unwrap()
}

fn setup(fixture: &Fixture, background: bool, policy: PermissionPolicy) {
    prepare_busy(fixture);
    fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| {
            session.policy = policy;
        })
        .unwrap();
    if background {
        fixture.move_focus(false);
    }
}

fn commit(
    result: Result<
        Option<PermissionRequestUpdate>,
        crate::session_manager::permission_request::PermissionRequestFailure,
    >,
) -> Result<Option<PermissionRequestUpdate>, String> {
    let mut update = result.map_err(|error| error.to_string())?;
    if let Some(update) = update.as_mut() {
        update.persist();
        SessionManager::commit_session_persists(None, std::mem::take(&mut update.persists));
    }
    Ok(update)
}

fn receive(fixture: &Fixture) -> Result<Option<PermissionRequestUpdate>, String> {
    commit(
        fixture
            .runtime
            .block_on(fixture.manager.prepare_permission_request(
                "target-process",
                fixture.target_meta.agent_session_id.as_deref(),
                event(),
            )),
    )
}

fn frames(fixture: &mut Fixture) -> Vec<Value> {
    frames_from(fixture, 0)
}

fn frames_from(fixture: &mut Fixture, peer: usize) -> Vec<Value> {
    let mut reader = BufReader::new(&mut fixture._peers[peer]);
    fixture.runtime.block_on(async {
        let mut result = Vec::new();
        loop {
            let mut line = String::new();
            match tokio::time::timeout(Duration::from_millis(80), reader.read_line(&mut line)).await
            {
                Ok(Ok(0)) | Err(_) => break,
                Ok(Ok(_)) => result.push(serde_json::from_str(&line).unwrap()),
                Ok(Err(error)) => panic!("synthetic peer: {error}"),
            }
        }
        result
    })
}

struct Paused {
    proceed: tokio::sync::oneshot::Sender<()>,
    worker: std::thread::JoinHandle<Result<Option<PermissionRequestUpdate>, String>>,
}

fn start(fixture: &Fixture, stage: &str) -> Paused {
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
    let agent = fixture.target_meta.agent_session_id.clone();
    let worker = std::thread::spawn(move || {
        commit(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(manager.prepare_permission_request(
                    "target-process",
                    agent.as_deref(),
                    event(),
                )),
        )
    });
    reached_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    Paused {
        proceed: proceed_tx,
        worker,
    }
}

fn finish(paused: Paused) -> Result<Option<PermissionRequestUpdate>, String> {
    paused.proceed.send(()).unwrap();
    paused.worker.join().unwrap()
}

fn newer_gate(fixture: &Fixture) {
    fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| {
            let _ = session.fsm.await_permission();
            session.pending_permission_rpc_id = Some(93);
            session.pending_permission_tool_name = Some("新工具".into());
            session.pending_permission_options = Some(json!([]));
            session.pending_permission_ui = None;
        })
        .unwrap();
}

#[test]
fn native_permission_manual_request_keeps_chinese_card_and_wire_options_in_both_pools() {
    for (background, policy) in [
        (false, PermissionPolicy::Ask),
        (true, PermissionPolicy::Ask),
        (false, PermissionPolicy::AllowOnce),
        (true, PermissionPolicy::AllowOnce),
    ] {
        let mut fixture = Fixture::new();
        setup(&fixture, background, policy);
        let update = receive(&fixture).unwrap().unwrap();
        assert_eq!(update.snapshot.state, SessionState::AwaitingPermission);
        let card = update.request.unwrap();
        assert_eq!(card.session_id, fixture.target_meta.id);
        assert_eq!(card.tool_call_id, "中文工具");
        assert!(card.preview.contains("示例.py"));
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                assert_eq!(session.pending_permission_ui.as_ref(), Some(&card));
                assert_eq!(session.pending_permission_rpc_id, Some(92));
            })
            .unwrap();
        assert!(frames(&mut fixture).is_empty());
    }
}

#[test]
fn native_permission_auto_policies_keep_existing_allow_and_reject_wire_ids() {
    for background in [false, true] {
        for (policy, expected) in [
            (PermissionPolicy::AlwaysApprove, "allow-once"),
            (PermissionPolicy::Deny, "reject-once"),
            (PermissionPolicy::DontAsk, "reject-once"),
        ] {
            let mut fixture = Fixture::new();
            setup(&fixture, background, policy);
            let update = receive(&fixture).unwrap().unwrap();
            assert!(update.request.is_none());
            assert_eq!(update.snapshot.state, SessionState::Streaming);
            let replies = frames(&mut fixture);
            assert_eq!(replies.len(), 1);
            assert_eq!(replies[0]["id"], 92);
            assert_eq!(replies[0]["result"]["outcome"]["optionId"], expected);
        }
    }
}

#[test]
fn native_permission_session_cache_retains_the_existing_policy_decision() {
    let mut fixture = Fixture::new();
    setup(&fixture, true, PermissionPolicy::Ask);
    fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| {
            session.allow_cache.allow("shell:python".into())
        })
        .unwrap();
    let update = receive(&fixture).unwrap().unwrap();
    assert!(update.request.is_none());
    assert_eq!(frames(&mut fixture).len(), 1);
}

#[test]
fn native_permission_replay_answers_the_original_client_without_a_gate_in_both_pools() {
    for background in [false, true] {
        let mut fixture = Fixture::new();
        if background {
            fixture.move_focus(false);
        }
        let activity = fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |s| s.last_activity)
            .unwrap();
        let update = receive(&fixture).unwrap();
        assert!(
            update.is_none(),
            "historical replay surfaced a new approval gate"
        );
        assert_eq!(frames(&mut fixture).len(), 1);
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |s| {
                assert!(s.pending_permission_rpc_id.is_none());
                assert_eq!(s.last_activity, activity);
            })
            .unwrap();
    }
}

#[test]
fn native_permission_route_follows_manual_owner_when_focus_moves() {
    let mut fixture = Fixture::new();
    setup(&fixture, false, PermissionPolicy::Ask);
    let paused = start(&fixture, "after_route");
    fixture.move_focus(false);
    let result = finish(paused).unwrap().unwrap();
    assert_eq!(
        result.snapshot.session_id.as_deref(),
        Some(fixture.target_meta.id.as_str())
    );
    assert!(fixture
        .manager
        .inner
        .lock()
        .as_ref()
        .unwrap()
        .pending_permission_rpc_id
        .is_none());
    fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |s| {
            assert_eq!(s.pending_permission_rpc_id, Some(92))
        })
        .unwrap();
    assert!(frames(&mut fixture).is_empty());
}

#[test]
fn native_permission_route_rejects_replacement_client_before_manual_mutation() {
    for background in [false, true] {
        let fixture = Fixture::new();
        setup(&fixture, background, PermissionPolicy::Ask);
        let paused = start(&fixture, "after_route");
        let entered = fixture.runtime.enter();
        let (replacement, _peer) =
            test_transport::client_for(false, SessionMcpPolicy::default(), None);
        drop(entered);
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |s| s.acp = Some(replacement))
            .unwrap();
        let result = finish(paused);
        assert!(
            result.is_err() || result.unwrap().is_none(),
            "old client request changed the replacement runtime"
        );
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |s| {
                assert!(s.pending_permission_rpc_id.is_none())
            })
            .unwrap();
    }
}

#[test]
fn native_permission_auto_write_checks_epoch_and_turn_changes_before_reply() {
    for background in [false, true] {
        for field in ["epoch", "turn"] {
            let mut fixture = Fixture::new();
            setup(&fixture, background, PermissionPolicy::AlwaysApprove);
            let paused = start(&fixture, "before_write");
            fixture
                .manager
                .with_session_mut(&fixture.target_meta.id, |s| {
                    if field == "epoch" {
                        s.turn_open_epoch += 1;
                    } else {
                        s.active_turn_id = Some("new-turn".into());
                    }
                })
                .unwrap();
            let result = finish(paused);
            assert!(
                result.is_err() || result.unwrap().is_none(),
                "automatic reply accepted a replaced turn"
            );
            assert!(frames(&mut fixture).is_empty());
        }
    }
}

#[test]
fn native_permission_auto_write_failure_preserves_a_recoverable_pending_gate() {
    for background in [false, true] {
        for policy in [PermissionPolicy::AlwaysApprove, PermissionPolicy::Deny] {
            let mut fixture = Fixture::new();
            setup(&fixture, background, policy);
            drop(fixture._peers.remove(0));
            let result = receive(&fixture);
            assert!(
                result.is_err(),
                "transport failure was reported as automatic approval success"
            );
            fixture
                .manager
                .with_session_mut(&fixture.target_meta.id, |s| {
                    assert_eq!(s.fsm.state(), SessionState::AwaitingPermission);
                    assert_eq!(s.pending_permission_rpc_id, Some(92));
                    assert!(s.pending_permission_ui.is_some());
                })
                .unwrap();
            assert!(fixture.manager.permission_responses.lock().is_empty());
        }
    }
}

#[test]
fn native_permission_auto_reply_does_not_continue_a_newer_gate_after_write() {
    for background in [false, true] {
        let mut fixture = Fixture::new();
        setup(&fixture, background, PermissionPolicy::AlwaysApprove);
        let paused = start(&fixture, "after_write");
        newer_gate(&fixture);
        let result = finish(paused);
        assert!(
            result.is_err(),
            "old automatic reply finalized a newer gate"
        );
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |s| {
                assert_eq!(s.fsm.state(), SessionState::AwaitingPermission);
                assert_eq!(s.pending_permission_rpc_id, Some(93));
                assert_eq!(s.pending_permission_tool_name.as_deref(), Some("新工具"));
            })
            .unwrap();
        assert_eq!(frames(&mut fixture).len(), 1);
    }
}

#[test]
fn native_permission_auto_reply_follows_busy_demotion_after_write() {
    let mut fixture = Fixture::new();
    setup(&fixture, false, PermissionPolicy::AlwaysApprove);
    let paused = start(&fixture, "after_write");
    let moved = fixture.manager.try_park_live();
    let mut foreign = routing_tests::bare_live_session(&fixture.other_meta.id, "other-process");
    foreign.meta = fixture.other_meta.clone();
    foreign.acp = Some(fixture.other.clone());
    *fixture.manager.inner.lock() = Some(foreign);
    let result = finish(paused).unwrap().unwrap();
    moved.unwrap();
    assert_eq!(
        result.snapshot.session_id.as_deref(),
        Some(fixture.target_meta.id.as_str())
    );
    assert_eq!(result.snapshot.state, SessionState::Streaming);
    assert_eq!(frames(&mut fixture).len(), 1);
    assert!(fixture
        .manager
        .inner
        .lock()
        .as_ref()
        .unwrap()
        .pending_permission_rpc_id
        .is_none());
}

#[test]
fn native_permission_auto_reply_finishes_and_persists_deferred_chinese_answers_in_both_pools() {
    for background in [false, true] {
        let fixture = Fixture::new();
        setup(&fixture, background, PermissionPolicy::AlwaysApprove);
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |s| {
                s.prompt_in_flight = false;
                s.deferred_prompt_complete = Some("end_turn".into());
                s.streaming_message_id = Some("auto-deferred".into());
                s.stream_buf = "自动回复后完整保留中文回答".into();
            })
            .unwrap();
        let result = receive(&fixture).unwrap().unwrap();
        assert_eq!(result.snapshot.state, SessionState::Ready);
        assert!(store::load_messages(&fixture.target_meta.id)
            .iter()
            .any(|m| m.id == "auto-deferred" && m.content == "自动回复后完整保留中文回答"));
        assert!(store::load_messages(&fixture.other_meta.id).is_empty());
    }
}

#[test]
fn native_permission_parallel_auto_requests_share_the_actual_reply_reservation() {
    let mut fixture = Fixture::new();
    setup(&fixture, false, PermissionPolicy::AlwaysApprove);
    let paused = start(&fixture, "before_write");
    let duplicate = receive(&fixture);
    let first = finish(paused);
    assert!(first.is_ok());
    assert!(
        duplicate.is_err(),
        "duplicate automatic request wrote twice"
    );
    assert_eq!(frames(&mut fixture).len(), 1);
}

#[test]
fn native_permission_foreign_unbound_and_parked_requests_remain_rejected() {
    let fixture = Fixture::new();
    setup(&fixture, false, PermissionPolicy::Ask);
    for (process, agent) in [
        ("foreign", fixture.target_meta.agent_session_id.as_deref()),
        ("target-process", Some("foreign")),
        ("", None),
    ] {
        let result = fixture
            .runtime
            .block_on(
                fixture
                    .manager
                    .prepare_permission_request(process, agent, event()),
            );
        assert!(result.unwrap().is_none());
    }
    fixture.move_focus(false);
    fixture.park_target();
    assert!(receive(&fixture).unwrap().is_none());
}

fn manual_answer(
    fixture: &Fixture,
    decision: &str,
) -> super::permission_resolution::PermissionAnswer {
    super::permission_resolution::PermissionAnswer {
        rpc_id: 92,
        request_generation: fixture
            .manager
            .pending_permission(Some(fixture.target_meta.id.clone()))
            .map(|card| card.request_generation),
        decision: decision.into(),
        option_id: Some("allow-once".into()),
        scope: Some("shell:python".into()),
        session_id: Some(fixture.target_meta.id.clone()),
        client_options: Some(json!([{"optionId":"allow-once","kind":"allow_once"}])),
        client_tool_name: Some("shell".into()),
    }
}

#[test]
fn native_permission_auto_capture_rejects_a_replacement_client_with_the_same_rpc() {
    for background in [false, true] {
        for policy in [PermissionPolicy::AlwaysApprove, PermissionPolicy::Deny] {
            let mut fixture = Fixture::new();
            setup(&fixture, background, policy);
            let paused = start(&fixture, "before_answer_capture");
            let entered = fixture.runtime.enter();
            let (replacement, peer) =
                test_transport::client_for(false, SessionMcpPolicy::default(), None);
            drop(entered);
            fixture._peers.push(peer);
            fixture
                .manager
                .with_session_mut(&fixture.target_meta.id, |session| {
                    session.acp = Some(replacement);
                    session.pending_permission_ui.as_mut().unwrap().title = "新连接的请求".into();
                })
                .unwrap();
            let result = finish(paused);
            let original = frames(&mut fixture);
            let replacement = frames_from(&mut fixture, 2);
            assert!(
                result.is_err(),
                "old automatic request adopted a replacement ACP"
            );
            assert!(original.is_empty());
            assert!(
                replacement.is_empty(),
                "old approval was sent to the new ACP"
            );
            fixture
                .manager
                .with_session_mut(&fixture.target_meta.id, |session| {
                    assert_eq!(session.pending_permission_rpc_id, Some(92));
                    assert_eq!(
                        session.pending_permission_ui.as_ref().unwrap().title,
                        "新连接的请求"
                    );
                    assert_eq!(session.fsm.state(), SessionState::AwaitingPermission);
                })
                .unwrap();
            assert!(fixture.manager.permission_responses.lock().is_empty());
        }
    }
}

#[test]
fn native_permission_auto_capture_rejects_changed_turn_and_execution_context() {
    for background in [false, true] {
        for field in [
            "epoch", "turn", "process", "agent", "binding", "cwd", "context", "policy",
        ] {
            let mut fixture = Fixture::new();
            setup(&fixture, background, PermissionPolicy::AlwaysApprove);
            let paused = start(&fixture, "before_answer_capture");
            let project = store::load_projects_snapshot()
                .unwrap()
                .into_iter()
                .find(|project| Some(&project.id) == fixture.target_meta.project_id.as_ref())
                .unwrap();
            let context = workspace_context::RuntimeWorkspaceContext::capture(
                &fixture.target_meta,
                &project.path,
                &store::load_settings(),
                false,
            )
            .unwrap();
            let changed_meta = fixture
                .manager
                .with_session_mut(&fixture.target_meta.id, |session| {
                    match field {
                        "epoch" => session.turn_open_epoch += 1,
                        "turn" => session.active_turn_id = Some("替换后的回合".into()),
                        "process" => session.process_id = "replacement-process".into(),
                        "agent" => session.meta.agent_session_id = Some("replacement-agent".into()),
                        "binding" => {
                            session.meta.project_id = fixture.other_meta.project_id.clone()
                        }
                        "cwd" => session.project_path = Some(project.path.clone()),
                        "context" => session.workspace_context = Some(context),
                        "policy" => session.policy = PermissionPolicy::Ask,
                        _ => unreachable!(),
                    }
                    session.meta.clone()
                })
                .unwrap();
            // The replacement is a valid stored binding. This must test the
            // original request owner, not merely a store/runtime disagreement.
            store::update_session_meta(&changed_meta).unwrap();
            let result = finish(paused);
            assert!(result.is_err(), "automatic capture adopted changed {field}");
            assert!(
                frames(&mut fixture).is_empty(),
                "changed {field} received a reply"
            );
            fixture
                .manager
                .with_session_mut(&fixture.target_meta.id, |session| {
                    assert_eq!(session.pending_permission_rpc_id, Some(92));
                    assert_eq!(session.fsm.state(), SessionState::AwaitingPermission);
                })
                .unwrap();
        }
    }
}

#[test]
fn native_permission_auto_capture_preserves_a_new_card_reusing_the_same_rpc() {
    for background in [false, true] {
        let mut fixture = Fixture::new();
        setup(&fixture, background, PermissionPolicy::AlwaysApprove);
        let paused = start(&fixture, "before_answer_capture");
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                let card = session.pending_permission_ui.as_mut().unwrap();
                card.tool_call_id = "替换工具".into();
                card.title = "相同 RPC 的新请求".into();
            })
            .unwrap();
        let result = finish(paused);
        assert!(
            result.is_err(),
            "automatic capture answered a reused RPC's new card"
        );
        assert!(frames(&mut fixture).is_empty());
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                assert_eq!(session.pending_permission_rpc_id, Some(92));
                assert_eq!(
                    session.pending_permission_ui.as_ref().unwrap().tool_call_id,
                    "替换工具"
                );
                assert_eq!(session.fsm.state(), SessionState::AwaitingPermission);
            })
            .unwrap();
    }
}

#[test]
fn native_permission_auto_and_manual_answers_share_the_actual_reply_reservation() {
    for background in [false, true] {
        for manual_first in [false, true] {
            let mut fixture = Fixture::new();
            setup(&fixture, background, PermissionPolicy::AlwaysApprove);
            let paused = start(
                &fixture,
                if manual_first {
                    "before_answer_capture"
                } else {
                    "before_write"
                },
            );
            let manual = fixture.runtime.block_on(
                fixture
                    .manager
                    .resolve_permission_core(manual_answer(&fixture, "allow_once")),
            );
            let automatic = finish(paused);
            assert_eq!(manual.is_ok(), manual_first);
            assert_eq!(automatic.is_ok(), !manual_first);
            assert_eq!(
                frames(&mut fixture).len(),
                1,
                "automatic/manual reply was duplicated"
            );
            assert!(fixture.manager.permission_responses.lock().is_empty());
        }
    }
}

#[test]
fn native_permission_failed_rejection_cannot_be_recovered_as_allow_by_client_fallback() {
    for background in [false, true] {
        for policy in [PermissionPolicy::Deny, PermissionPolicy::DontAsk] {
            let mut fixture = Fixture::new();
            setup(&fixture, background, policy);
            drop(fixture._peers.remove(0));
            assert!(receive(&fixture).is_err());
            for decision in [
                "allow_once",
                "allow_session",
                "approve",
                "Allow_Once",
                "unknown",
            ] {
                let result = fixture.runtime.block_on(
                    fixture
                        .manager
                        .resolve_permission_core(manual_answer(&fixture, decision)),
                );
                assert!(result.is_err(), "reject-only recovery accepted {decision}");
                fixture
                    .manager
                    .with_session_mut(&fixture.target_meta.id, |session| {
                        assert_eq!(session.pending_permission_rpc_id, Some(92));
                        assert_eq!(session.fsm.state(), SessionState::AwaitingPermission);
                        assert!(!session.allow_cache.is_allowed("shell:python"));
                        assert!(session
                            .pending_permission_ui
                            .as_ref()
                            .unwrap()
                            .options
                            .as_array()
                            .unwrap()
                            .iter()
                            .all(|option| option["kind"] == "reject_once"));
                    })
                    .unwrap();
            }
            assert!(fixture.manager.permission_responses.lock().is_empty());
        }
    }
}

#[test]
fn native_permission_rejection_card_rejects_unknown_decisions_on_a_live_transport() {
    for background in [false, true] {
        for policy in [PermissionPolicy::Deny, PermissionPolicy::DontAsk] {
            let mut fixture = Fixture::new();
            setup(&fixture, background, policy);
            // The actual incoming consumer has installed its deny-only card,
            // but has not reserved or written a response. A live writer means
            // transport failure or another reservation cannot hide an allow.
            let paused = start(&fixture, "before_answer_capture");
            let results: Vec<_> = [
                "allow_once",
                "allow_session",
                "approve",
                "Allow_Once",
                "unknown",
            ]
            .into_iter()
            .map(|decision| {
                let result = fixture.runtime.block_on(
                    fixture
                        .manager
                        .resolve_permission_core(manual_answer(&fixture, decision)),
                );
                (decision, result)
            })
            .collect();
            let automatic = finish(paused);
            for (decision, result) in results {
                let error = result
                    .err()
                    .unwrap_or_else(|| panic!("reject-only card accepted {decision}"));
                assert!(
                    error.contains("only allows rejection"),
                    "{decision} failed for an unrelated reason: {error}"
                );
            }
            assert!(automatic.unwrap().unwrap().request.is_none());
            let replies = frames(&mut fixture);
            assert_eq!(replies.len(), 1);
            assert_eq!(replies[0]["result"]["outcome"]["optionId"], "reject-once");
            fixture
                .manager
                .with_session_mut(&fixture.target_meta.id, |session| {
                    assert!(session.pending_permission_rpc_id.is_none());
                    assert!(!session.allow_cache.is_allowed("shell:python"));
                })
                .unwrap();
            assert!(fixture.manager.permission_responses.lock().is_empty());
        }
    }
}
