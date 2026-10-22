//! Actual approval command, synthetic duplex replies and real isolated stores.
use super::tool_owner_tests::prepare_busy;
use super::*;
use crate::session_manager::permission_resolution::{PermissionAnswer, PermissionResolution};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};

#[path = "permission_delivery_tests.rs"]
mod delivery;

#[path = "permission_ui_identity_tests.rs"]
mod ui_identity;

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

fn options() -> Value {
    json!([
        {"optionId":"allow-once", "kind":"allow_once", "name":"允许一次"},
        {"optionId":"allow-always-command", "kind":"allow_always", "name":"允许该命令"},
        {"optionId":"reject-once", "kind":"reject_once", "name":"拒绝"},
    ])
}

fn answer(fixture: &Fixture, decision: &str) -> PermissionAnswer {
    PermissionAnswer {
        rpc_id: 81,
        request_generation: fixture
            .manager
            .pending_permission(Some(fixture.target_meta.id.clone()))
            .map(|card| card.request_generation),
        decision: decision.into(),
        option_id: Some("always-allow".into()),
        scope: Some("shell:python".into()),
        session_id: Some(fixture.target_meta.id.clone()),
        client_options: Some(options()),
        client_tool_name: Some("shell".into()),
    }
}

fn gate(fixture: &Fixture, id: u64, label: &str) {
    fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| {
            let _ = session.fsm.await_permission();
            session.pending_permission_rpc_id = Some(id);
            session.pending_permission_options = Some(options());
            session.pending_permission_tool_name = Some("shell".into());
            session.pending_permission_ui = Some(UiPermissionRequest {
                request_generation: uuid::Uuid::new_v4(),
                rpc_id: id,
                session_id: session.app_session_id.clone(),
                tool_call_id: label.into(),
                tool_name: "shell".into(),
                title: format!("批准中文命令 {label}"),
                preview: "python 示例.py".into(),
                scope_key: "shell:python".into(),
                options: options(),
                response_error: None,
            });
        })
        .unwrap();
}

fn setup(fixture: &Fixture, background: bool) {
    prepare_busy(fixture);
    if background {
        fixture.move_focus(false);
    }
    gate(fixture, 81, "原请求");
}

fn bind_execution_context(fixture: &Fixture) {
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
    fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| {
            session.project_path = Some(project.path.clone());
            session.workspace_context = Some(context);
        })
        .unwrap();
}

fn resolve(fixture: &Fixture, answer: PermissionAnswer) -> Result<PermissionResolution, String> {
    fixture
        .runtime
        .block_on(fixture.manager.resolve_permission_core(answer))
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
                Ok(Err(error)) => panic!("synthetic peer read failed: {error}"),
            }
        }
        frames
    })
}

struct Paused {
    proceed: tokio::sync::oneshot::Sender<()>,
    abort: tokio::task::AbortHandle,
    worker: std::thread::JoinHandle<Result<PermissionResolution, String>>,
}

fn start(fixture: &Fixture, stage: &str) -> Paused {
    let (reached_tx, reached_rx) = mpsc::sync_channel(1);
    let (proceed_tx, proceed_rx) = tokio::sync::oneshot::channel();
    let (abort_tx, abort_rx) = mpsc::sync_channel(1);
    pauses().lock().unwrap().insert(
        format!("{}:{stage}", fixture.target_meta.id),
        Pause {
            reached: reached_tx,
            proceed: proceed_rx,
        },
    );
    let manager = fixture.manager.clone();
    let request = answer(fixture, "allow_session");
    let worker = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let task =
                    tokio::spawn(async move { manager.resolve_permission_core(request).await });
                abort_tx.send(task.abort_handle()).unwrap();
                task.await
                    .unwrap_or_else(|error| Err(format!("synthetic task: {error}")))
            })
    });
    let abort = abort_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    reached_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    Paused {
        proceed: proceed_tx,
        abort,
        worker,
    }
}

fn finish(paused: Paused) -> Result<PermissionResolution, String> {
    paused.proceed.send(()).unwrap();
    paused.worker.join().unwrap()
}

fn assert_gate(fixture: &Fixture, id: u64, label: &str) {
    fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| {
            assert_eq!(session.pending_permission_rpc_id, Some(id));
            assert_eq!(
                session.pending_permission_ui.as_ref().unwrap().tool_call_id,
                label
            );
            assert_eq!(
                session.pending_permission_options.as_ref(),
                Some(&options())
            );
            assert_eq!(
                session.pending_permission_tool_name.as_deref(),
                Some("shell")
            );
            assert_eq!(session.fsm.state(), SessionState::AwaitingPermission);
            assert!(!session.allow_cache.is_allowed("shell:python"));
        })
        .unwrap();
}

fn assert_resolved(fixture: &Fixture, allowed: bool) {
    assert!(fixture.manager.permission_responses.lock().is_empty());
    fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| {
            assert!(session.pending_permission_rpc_id.is_none());
            assert!(session.pending_permission_ui.is_none());
            assert!(session.pending_permission_options.is_none());
            assert!(session.pending_permission_tool_name.is_none());
            assert_eq!(session.allow_cache.is_allowed("shell:python"), allowed);
            assert_eq!(session.fsm.state(), SessionState::Streaming);
        })
        .unwrap();
    assert!(store::load_messages(&fixture.target_meta.id).is_empty());
    assert!(store::load_messages(&fixture.other_meta.id).is_empty());
}

#[test]
fn actual_approval_preserves_wire_coercion_cache_and_target_snapshot_in_both_pools() {
    for background in [false, true] {
        for decision in [
            "allow_session",
            "allow_for_session",
            "allow_once",
            "deny",
            "cancel",
        ] {
            let mut fixture = Fixture::new();
            setup(&fixture, background);
            let result = resolve(&fixture, answer(&fixture, decision)).unwrap();
            assert_eq!(
                result.snapshot.session_id.as_deref(),
                Some(fixture.target_meta.id.as_str())
            );
            assert!(result.emits.is_empty());
            assert!(result.persists.is_empty());
            assert!(result.empty_run.is_none());
            let wire = frames(&mut fixture);
            assert_eq!(wire.len(), 1);
            assert_eq!(wire[0]["id"], 81);
            if decision == "cancel" {
                assert_eq!(wire[0]["result"]["outcome"]["outcome"], "cancelled");
            } else {
                let expected = match decision {
                    "allow_once" => "allow-once",
                    "deny" => "reject-once",
                    _ => "allow-always-command",
                };
                assert_eq!(wire[0]["result"]["outcome"]["optionId"], expected);
            }
            assert_resolved(
                &fixture,
                matches!(decision, "allow_session" | "allow_for_session"),
            );
        }
    }
}

#[test]
fn explicit_background_approval_does_not_resolve_foreign_focus_with_the_same_rpc_id() {
    let mut fixture = Fixture::new();
    setup(&fixture, true);
    let before = fixture.manager.inner.lock().as_ref().unwrap().last_activity;
    fixture
        .manager
        .inner
        .lock()
        .as_mut()
        .unwrap()
        .pending_permission_rpc_id = Some(81);
    let result = resolve(&fixture, answer(&fixture, "allow_session")).unwrap();
    assert_resolved(&fixture, true);
    {
        let foreign = fixture.manager.inner.lock();
        let foreign = foreign.as_ref().unwrap();
        assert_eq!(foreign.pending_permission_rpc_id, Some(81));
        assert!(!foreign.allow_cache.is_allowed("shell:python"));
        assert_eq!(foreign.last_activity, before);
    }
    assert_eq!(
        result.snapshot.session_id.as_deref(),
        Some(fixture.target_meta.id.as_str())
    );
    assert_eq!(frames(&mut fixture).len(), 1);
}

#[test]
fn missing_host_options_and_tool_name_keep_the_existing_ui_fallback() {
    for background in [false, true] {
        let mut fixture = Fixture::new();
        setup(&fixture, background);
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                session.pending_permission_options = None;
                session.pending_permission_tool_name = None;
                session.pending_permission_ui = None;
            })
            .unwrap();
        assert!(resolve(&fixture, answer(&fixture, "allow_session")).is_ok());
        let wire = frames(&mut fixture);
        assert_eq!(wire.len(), 1);
        assert_eq!(
            wire[0]["result"]["outcome"]["optionId"],
            "allow-always-command"
        );
        assert_resolved(&fixture, true);
    }
}

#[test]
fn expired_wrong_and_dead_approval_requests_never_write_a_reply() {
    for background in [false, true] {
        for kind in ["missing", "wrong", "dead", "unknown_target"] {
            let mut fixture = Fixture::new();
            setup(&fixture, background);
            let mut request = answer(&fixture, "allow_session");
            match kind {
                "missing" => {
                    fixture
                        .manager
                        .with_session_mut(&fixture.target_meta.id, |s| {
                            s.pending_permission_rpc_id = None
                        });
                }
                "wrong" => request.rpc_id = 82,
                "dead" => fixture.runtime.block_on(fixture.target.kill()),
                _ => request.session_id = Some(fixture.other_meta.id.clone()),
            }
            assert!(resolve(&fixture, request).is_err());
            assert!(frames(&mut fixture).is_empty());
            if kind != "missing" {
                assert_gate(&fixture, 81, "原请求");
            }
        }
    }
}

#[test]
fn failed_actual_transport_write_preserves_pending_ui_and_allow_cache() {
    for background in [false, true] {
        let mut fixture = Fixture::new();
        setup(&fixture, background);
        let peer = fixture._peers.remove(0);
        drop(peer);
        assert!(resolve(&fixture, answer(&fixture, "allow_session")).is_err());
        assert_gate(&fixture, 81, "原请求");
        assert!(fixture.manager.permission_responses.lock().is_empty());
    }
}

#[test]
fn approval_before_write_rejects_changed_acp_and_sends_nothing_to_either_client() {
    for background in [false, true] {
        let mut fixture = Fixture::new();
        setup(&fixture, background);
        let entered = fixture.runtime.enter();
        let (replacement, mut replacement_peer) =
            test_transport::client_for(false, SessionMcpPolicy::default(), None);
        drop(entered);
        let paused = start(&fixture, "before_write");
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |s| {
                s.acp = Some(replacement.clone())
            })
            .unwrap();
        let result = finish(paused);
        assert!(
            result.is_err(),
            "old approval accepted after ACP replacement"
        );
        assert_gate(&fixture, 81, "原请求");
        assert!(frames(&mut fixture).is_empty());
        let mut line = String::new();
        assert!(fixture
            .runtime
            .block_on(async {
                tokio::time::timeout(
                    Duration::from_millis(100),
                    BufReader::new(&mut replacement_peer).read_line(&mut line),
                )
                .await
            })
            .is_err());
        assert!(replacement.is_alive());
    }
}

#[test]
fn approval_after_write_does_not_clear_a_replacement_acp_gate() {
    for background in [false, true] {
        let mut fixture = Fixture::new();
        setup(&fixture, background);
        let entered = fixture.runtime.enter();
        let (replacement, _peer) =
            test_transport::client_for(false, SessionMcpPolicy::default(), None);
        drop(entered);
        let paused = start(&fixture, "after_write");
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |s| {
                s.acp = Some(replacement.clone())
            })
            .unwrap();
        let result = finish(paused);
        assert!(
            result.is_err(),
            "old successful write finalized the replacement ACP"
        );
        assert_gate(&fixture, 81, "原请求");
        assert_eq!(frames(&mut fixture).len(), 1);
        assert!(replacement.is_alive());
    }
}

#[test]
fn approval_before_write_rejects_a_new_gate_even_when_the_rpc_id_is_reused() {
    for background in [false, true] {
        for id in [81, 82] {
            let mut fixture = Fixture::new();
            setup(&fixture, background);
            let paused = start(&fixture, "before_write");
            gate(&fixture, id, "新请求");
            let result = finish(paused);
            assert!(result.is_err(), "old approval wrote for a replaced gate");
            assert_gate(&fixture, id, "新请求");
            assert!(frames(&mut fixture).is_empty());
        }
    }
}

#[test]
fn approval_after_write_preserves_new_gate_state_and_does_not_cache_old_authorization() {
    for background in [false, true] {
        for id in [81, 82] {
            let mut fixture = Fixture::new();
            setup(&fixture, background);
            let paused = start(&fixture, "after_write");
            gate(&fixture, id, "新请求");
            let result = finish(paused);
            assert!(
                result.is_err(),
                "old approval marked a newer gate as resolved"
            );
            assert_gate(&fixture, id, "新请求");
            assert_eq!(frames(&mut fixture).len(), 1);
        }
    }
}

#[test]
fn approvals_recheck_epoch_and_active_turn_before_and_after_the_actual_write() {
    for background in [false, true] {
        for stage in ["before_write", "after_write"] {
            for epoch in [false, true] {
                let mut fixture = Fixture::new();
                setup(&fixture, background);
                let paused = start(&fixture, stage);
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
                let result = finish(paused);
                assert!(result.is_err(), "approval crossed a turn generation");
                assert_gate(&fixture, 81, "原请求");
                assert_eq!(
                    frames(&mut fixture).len(),
                    usize::from(stage == "after_write")
                );
            }
        }
    }
}

#[test]
fn approvals_recheck_workspace_binding_before_and_after_the_actual_write() {
    for stage in ["before_write", "after_write"] {
        for runtime in [false, true] {
            let mut fixture = Fixture::new();
            setup(&fixture, false);
            let paused = start(&fixture, stage);
            if runtime {
                fixture
                    .manager
                    .with_session_mut(&fixture.target_meta.id, |session| {
                        session.project_path = Some("synthetic-new-cwd".into())
                    })
                    .unwrap();
            } else {
                let mut row = fixture.target_meta.clone();
                row.project_id = fixture.other_meta.project_id.clone();
                store::update_session_meta(&row).unwrap();
            }
            let result = finish(paused);
            assert!(result.is_err(), "approval crossed a workspace binding");
            assert_gate(&fixture, 81, "原请求");
            assert_eq!(
                frames(&mut fixture).len(),
                usize::from(stage == "after_write")
            );
        }
    }
}

#[test]
fn deleted_stored_owner_cannot_receive_an_approval_or_be_revived() {
    let mut fixture = Fixture::new();
    setup(&fixture, false);
    let paused = start(&fixture, "before_write");
    store::delete_session(&fixture.target_meta.id).unwrap();
    let result = finish(paused);
    assert!(
        result.is_err(),
        "deleted stored owner received a permission reply"
    );
    assert_gate(&fixture, 81, "原请求");
    assert!(frames(&mut fixture).is_empty());
    assert!(!store::load_sessions_snapshot()
        .unwrap()
        .iter()
        .any(|row| row.id == fixture.target_meta.id));
}

#[test]
fn mismatched_ui_scope_cannot_grant_an_unrelated_command() {
    let mut fixture = Fixture::new();
    setup(&fixture, false);
    let mut request = answer(&fixture, "allow_session");
    request.scope = Some("shell:unrelated-command".into());
    assert!(resolve(&fixture, request).is_err());
    assert_gate(&fixture, 81, "原请求");
    assert!(frames(&mut fixture).is_empty());
    assert!(!fixture
        .manager
        .inner
        .lock()
        .as_ref()
        .unwrap()
        .allow_cache
        .is_allowed("shell:unrelated-command"));
}

#[test]
fn parallel_approvals_send_only_one_actual_reply() {
    for background in [false, true] {
        let mut fixture = Fixture::new();
        setup(&fixture, background);
        let paused = start(&fixture, "before_write");
        let duplicate = resolve(&fixture, answer(&fixture, "allow_session"));
        let first = finish(paused);
        assert!(first.is_ok());
        assert!(
            duplicate.is_err(),
            "concurrent approval wrote a duplicate reply"
        );
        assert_eq!(frames(&mut fixture).len(), 1);
        assert_resolved(&fixture, true);
    }
}

#[test]
fn cancelled_approval_future_does_not_strand_the_pending_gate_or_next_attempt() {
    let mut fixture = Fixture::new();
    setup(&fixture, false);
    let paused = start(&fixture, "before_write");
    paused.abort.abort();
    let result = paused.worker.join().unwrap();
    drop(paused.proceed);
    assert!(result.is_err());
    assert_gate(&fixture, 81, "原请求");
    assert!(fixture.manager.permission_responses.lock().is_empty());
    assert!(frames(&mut fixture).is_empty());
    assert!(resolve(&fixture, answer(&fixture, "allow_session")).is_ok());
    assert_eq!(frames(&mut fixture).len(), 1);
    assert_resolved(&fixture, true);
}

#[test]
fn approval_follows_actual_busy_demotion_and_returns_the_original_target_snapshot() {
    let mut fixture = Fixture::new();
    setup(&fixture, false);
    let paused = start(&fixture, "before_write");
    let moved = fixture.manager.try_park_live();
    let mut foreign = routing_tests::bare_live_session(&fixture.other_meta.id, "other-process");
    foreign.meta = fixture.other_meta.clone();
    foreign.acp = Some(fixture.other.clone());
    *fixture.manager.inner.lock() = Some(foreign);
    let result = finish(paused);
    moved.unwrap();
    let result = result.unwrap();
    assert_eq!(
        result.snapshot.session_id.as_deref(),
        Some(fixture.target_meta.id.as_str())
    );
    assert_resolved(&fixture, true);
    assert_eq!(frames(&mut fixture).len(), 1);
    assert!(fixture
        .manager
        .background
        .lock()
        .contains_key(&fixture.target_meta.id));
    assert!(fixture
        .manager
        .inner
        .lock()
        .as_ref()
        .unwrap()
        .pending_permission_rpc_id
        .is_none());
    assert!(fixture.other.is_alive());
}

#[test]
fn legitimate_approvals_validate_the_actual_captured_project_execution_context() {
    for background in [false, true] {
        let mut fixture = Fixture::new();
        setup(&fixture, background);
        bind_execution_context(&fixture);
        let result = resolve(&fixture, answer(&fixture, "allow_session")).unwrap();
        assert_eq!(
            result.snapshot.session_id.as_deref(),
            Some(fixture.target_meta.id.as_str())
        );
        assert_resolved(&fixture, true);
        assert_eq!(frames(&mut fixture).len(), 1);
    }
}

#[test]
fn captured_execution_context_rejects_changed_project_settings_before_and_after_write() {
    for background in [false, true] {
        for stage in ["before_write", "after_write"] {
            let mut fixture = Fixture::new();
            setup(&fixture, background);
            bind_execution_context(&fixture);
            let paused = start(&fixture, stage);
            store::set_project_sandbox_profile(
                fixture.target_meta.project_id.as_deref().unwrap(),
                Some("strict".into()),
            )
            .unwrap();
            let result = finish(paused);
            assert!(
                result.is_err(),
                "approval ignored the captured project execution context"
            );
            assert_gate(&fixture, 81, "原请求");
            assert!(fixture.manager.permission_responses.lock().is_empty());
            assert_eq!(
                frames(&mut fixture).len(),
                usize::from(stage == "after_write")
            );
        }
    }
}

#[test]
fn valid_approval_finishes_and_persists_the_actual_deferred_answer_in_both_pools() {
    for background in [false, true] {
        let mut fixture = Fixture::new();
        setup(&fixture, background);
        bind_execution_context(&fixture);
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                session.prompt_in_flight = false;
                session.deferred_prompt_complete = Some("end_turn".into());
                session.streaming_message_id = Some("synthetic-approved-assistant".into());
                session.stream_buf = "审批解除后保留的完整中文回答".into();
            })
            .unwrap();
        let mut result = resolve(&fixture, answer(&fixture, "allow_session")).unwrap();
        assert_eq!(
            result.snapshot.session_id.as_deref(),
            Some(fixture.target_meta.id.as_str())
        );
        assert_eq!(result.snapshot.state, SessionState::Ready);
        assert!(result.empty_run.is_none());
        assert!(!result.persists.is_empty());
        SessionManager::commit_session_persists(None, std::mem::take(&mut result.persists));
        let rows = store::load_messages(&fixture.target_meta.id);
        assert!(rows
            .iter()
            .any(|row| row.id == "synthetic-approved-assistant"
                && row.content == "审批解除后保留的完整中文回答"));
        assert!(store::load_messages(&fixture.other_meta.id).is_empty());
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                assert!(session.pending_permission_rpc_id.is_none());
                assert!(session.active_turn_id.is_none());
                assert!(session.deferred_prompt_complete.is_none());
                assert!(session.allow_cache.is_allowed("shell:python"));
            })
            .unwrap();
        assert!(fixture.manager.permission_responses.lock().is_empty());
        assert_eq!(frames(&mut fixture).len(), 1);
    }
}
