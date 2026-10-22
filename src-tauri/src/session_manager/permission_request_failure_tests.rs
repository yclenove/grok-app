//! The actual native-delegated failure publication core, not a copied reducer.
use super::*;
use crate::session_manager::permission_request::PermissionRequestFailure;

fn failed(fixture: &mut Fixture) -> PermissionRequestFailure {
    drop(fixture._peers.remove(0));
    fixture
        .runtime
        .block_on(fixture.manager.prepare_permission_request(
            "target-process",
            fixture.target_meta.agent_session_id.as_deref(),
            event(),
        ))
        .err()
        .expect("synthetic writer must fail")
}

#[test]
fn failed_automatic_reply_publishes_the_actual_recoverable_card_in_both_pools() {
    for background in [false, true] {
        for policy in [
            PermissionPolicy::AlwaysApprove,
            PermissionPolicy::Deny,
            PermissionPolicy::DontAsk,
        ] {
            let mut fixture = Fixture::new();
            setup(&fixture, background, policy);
            let failure = failed(&mut fixture);
            let update = fixture
                .manager
                .prepare_permission_request_effect(Err(failure))
                .expect("native failure path hid the recoverable approval card");
            assert_eq!(
                update.snapshot.session_id.as_deref(),
                Some(fixture.target_meta.id.as_str())
            );
            assert_eq!(update.snapshot.state, SessionState::AwaitingPermission);
            assert!(update.emits.is_empty());
            assert!(update.persists.is_empty());
            let card = update.request.unwrap();
            assert_eq!(card.rpc_id, 92);
            assert_eq!(card.tool_call_id, "中文工具");
            let wire = serde_json::to_value(&card).unwrap();
            assert!(wire["responseError"]
                .as_str()
                .is_some_and(|message| !message.is_empty()));
            let restored = fixture
                .manager
                .pending_permission(Some(fixture.target_meta.id.clone()))
                .unwrap();
            assert_eq!(restored, card);
            if policy != PermissionPolicy::AlwaysApprove {
                assert!(card
                    .options
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|option| option["kind"] == "reject_once"));
            }
            fixture
                .manager
                .with_session_mut(&fixture.target_meta.id, |session| {
                    assert_eq!(session.pending_permission_rpc_id, Some(92));
                    assert!(!session.allow_cache.is_allowed("shell:python"));
                })
                .unwrap();
            assert!(fixture.manager.permission_responses.lock().is_empty());
        }
    }
}

#[test]
fn failed_automatic_reply_follows_its_owner_after_focus_moves() {
    let mut fixture = Fixture::new();
    setup(&fixture, false, PermissionPolicy::AlwaysApprove);
    let failure = failed(&mut fixture);
    fixture.move_focus(false);
    let update = fixture
        .manager
        .prepare_permission_request_effect(Err(failure))
        .expect("background recovery was lost after focus moved");
    assert_eq!(
        update.snapshot.session_id.as_deref(),
        Some(fixture.target_meta.id.as_str())
    );
    assert!(update.background);
    fixture.assert_other_untouched();
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
fn recovered_permission_failure_persists_the_real_pending_lease_without_success() {
    let mut fixture = Fixture::new();
    setup(&fixture, true, PermissionPolicy::Deny);
    let turn = fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| {
            session.active_turn_id.clone()
        })
        .unwrap();
    crate::turn_lease::begin_active(
        &fixture.target_meta.id,
        fixture.target_meta.agent_session_id.as_deref(),
        turn.as_deref(),
    );
    let failure = failed(&mut fixture);
    let update = fixture
        .manager
        .prepare_permission_request_effect(Err(failure))
        .expect("failure recovery was not available to the native lease writer");
    update.persist();
    let lease = crate::turn_lease::read_lease(&fixture.target_meta.id).unwrap();
    assert_eq!(lease.phase, "permission_prompt");
    assert!(lease.permission_pending);
    assert_eq!(lease.pending_tool.unwrap().tool_call_id, "中文工具");
    assert!(store::load_messages(&fixture.target_meta.id).is_empty());
    assert!(store::load_messages(&fixture.other_meta.id).is_empty());
}

#[test]
fn late_permission_failure_never_publishes_for_a_new_gate_or_execution_owner() {
    for background in [false, true] {
        for field in [
            "gate", "card", "client", "epoch", "turn", "policy", "cwd", "store",
        ] {
            let mut fixture = Fixture::new();
            setup(&fixture, background, PermissionPolicy::AlwaysApprove);
            let failure = failed(&mut fixture);
            let entered = fixture.runtime.enter();
            let (replacement, _peer) =
                test_transport::client_for(false, SessionMcpPolicy::default(), None);
            drop(entered);
            fixture
                .manager
                .with_session_mut(&fixture.target_meta.id, |session| match field {
                    "gate" => session.pending_permission_rpc_id = Some(93),
                    "card" => {
                        session.pending_permission_ui.as_mut().unwrap().title =
                            "同 RPC 的新卡片".into()
                    }
                    "client" => session.acp = Some(replacement),
                    "epoch" => session.turn_open_epoch += 1,
                    "turn" => session.active_turn_id = Some("新回合".into()),
                    "policy" => session.policy = PermissionPolicy::Deny,
                    "cwd" => session.project_path = Some("新目录".into()),
                    "store" => {}
                    _ => unreachable!(),
                })
                .unwrap();
            if field == "store" {
                let mut row = fixture.target_meta.clone();
                row.project_id = fixture.other_meta.project_id.clone();
                store::update_session_meta(&row).unwrap();
            }
            let update = fixture
                .manager
                .prepare_permission_request_effect(Err(failure));
            assert!(
                update.is_none(),
                "old failure published against changed {field}"
            );
            fixture
                .manager
                .with_session_mut(&fixture.target_meta.id, |session| {
                    assert_eq!(
                        session.pending_permission_rpc_id,
                        Some(if field == "gate" { 93 } else { 92 })
                    );
                })
                .unwrap();
        }
    }
}

#[test]
fn an_already_written_reply_does_not_become_a_retryable_failure_card() {
    let mut fixture = Fixture::new();
    setup(&fixture, false, PermissionPolicy::AlwaysApprove);
    let (reached_tx, reached_rx) = mpsc::sync_channel(1);
    let (proceed_tx, proceed_rx) = tokio::sync::oneshot::channel();
    pauses().lock().unwrap().insert(
        format!("{}:after_write", fixture.target_meta.id),
        Pause {
            reached: reached_tx,
            proceed: proceed_rx,
        },
    );
    let manager = fixture.manager.clone();
    let agent = fixture.target_meta.agent_session_id.clone();
    let worker = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(manager.prepare_permission_request(
                "target-process",
                agent.as_deref(),
                event(),
            ))
    });
    reached_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    let replies = frames(&mut fixture);
    fixture.runtime.block_on(fixture.target.kill());
    proceed_tx.send(()).unwrap();
    let result = worker.join().unwrap();
    assert_eq!(replies.len(), 1);
    assert!(result.is_err());
    assert!(
        fixture
            .manager
            .prepare_permission_request_effect(result)
            .is_none(),
        "written reply offered a duplicate retry"
    );
}

#[test]
fn ordinary_manual_cards_keep_their_existing_serialized_payload() {
    let fixture = Fixture::new();
    setup(&fixture, false, PermissionPolicy::Ask);
    let result = fixture
        .runtime
        .block_on(fixture.manager.prepare_permission_request(
            "target-process",
            fixture.target_meta.agent_session_id.as_deref(),
            event(),
        ));
    let update = fixture
        .manager
        .prepare_permission_request_effect(result)
        .unwrap();
    let wire = serde_json::to_value(update.request.unwrap()).unwrap();
    assert_eq!(wire["rpcId"], 92);
    assert!(wire.get("responseError").is_none());
}

#[test]
fn partial_permission_write_retires_transport_without_offering_a_duplicate_reply() {
    use tokio::io::AsyncReadExt;
    for background in [false, true] {
        let mut fixture = Fixture::new();
        setup(&fixture, background, PermissionPolicy::AlwaysApprove);
        let written = fixture
            .runtime
            .block_on(test_transport::install_writer_fault(
                &fixture.target,
                Some(17),
                false,
                7,
            ));
        let result = fixture
            .runtime
            .block_on(fixture.manager.prepare_permission_request(
                "target-process",
                fixture.target_meta.agent_session_id.as_deref(),
                event(),
            ));
        assert!(result.is_err());
        let update = fixture.manager.prepare_permission_request_effect(result);
        let mut prefix = [0; 17];
        fixture.runtime.block_on(async {
            tokio::time::timeout(
                Duration::from_secs(2),
                fixture._peers[0].read_exact(&mut prefix),
            )
            .await
            .unwrap()
            .unwrap();
        });
        assert_eq!(&prefix, b"{\"id\":92,\"jsonrpc");
        assert_eq!(written.load(std::sync::atomic::Ordering::SeqCst), 17);
        assert!(update.is_none(), "partially sent reply was offered again");
        assert!(
            !fixture.target.is_alive(),
            "partial frame transport remained reusable"
        );
        assert!(fixture
            .runtime
            .block_on(
                fixture
                    .target
                    .respond_permission(92, crate::acp_client::PermissionOutcome::Cancelled,)
            )
            .is_err());
        assert_eq!(written.load(std::sync::atomic::Ordering::SeqCst), 17);
        if background {
            fixture.assert_other_untouched();
        } else {
            assert!(fixture.other.is_alive());
            assert!(fixture
                .manager
                .inner
                .lock()
                .as_ref()
                .unwrap()
                .acp
                .as_ref()
                .is_some_and(|client| Arc::ptr_eq(client, &fixture.target)));
            assert_eq!(
                serde_json::to_value(workspace_context::session(&fixture.other_meta.id).unwrap())
                    .unwrap(),
                serde_json::to_value(&fixture.other_meta).unwrap()
            );
        }
    }
}

#[test]
fn permission_flush_failure_never_retries_a_complete_frame() {
    let mut fixture = Fixture::new();
    setup(&fixture, false, PermissionPolicy::AlwaysApprove);
    let written = fixture
        .runtime
        .block_on(test_transport::install_writer_fault(
            &fixture.target,
            None,
            true,
            7,
        ));
    let result = fixture
        .runtime
        .block_on(fixture.manager.prepare_permission_request(
            "target-process",
            fixture.target_meta.agent_session_id.as_deref(),
            event(),
        ));
    assert!(result.is_err());
    let update = fixture.manager.prepare_permission_request_effect(result);
    let replies = frames(&mut fixture);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["id"], 92);
    assert_eq!(replies[0]["result"]["outcome"]["optionId"], "allow-once");
    let original_bytes = written.load(std::sync::atomic::Ordering::SeqCst);
    assert!(original_bytes > 17);
    assert!(
        update.is_none(),
        "already sent frame became a recoverable failure card"
    );
    assert!(
        !fixture.target.is_alive(),
        "failed flush left the transport usable"
    );
    assert!(fixture
        .runtime
        .block_on(
            fixture
                .target
                .respond_permission(92, crate::acp_client::PermissionOutcome::Cancelled,)
        )
        .is_err());
    assert_eq!(
        written.load(std::sync::atomic::Ordering::SeqCst),
        original_bytes
    );
}

#[test]
fn permission_write_failure_before_any_bytes_keeps_the_original_error_card() {
    let fixture = Fixture::new();
    setup(&fixture, false, PermissionPolicy::Deny);
    let written = fixture
        .runtime
        .block_on(test_transport::install_writer_fault(
            &fixture.target,
            Some(0),
            false,
            7,
        ));
    let result = fixture
        .runtime
        .block_on(fixture.manager.prepare_permission_request(
            "target-process",
            fixture.target_meta.agent_session_id.as_deref(),
            event(),
        ));
    assert!(result.is_err());
    let update = fixture
        .manager
        .prepare_permission_request_effect(result)
        .unwrap();
    let card = update.request.unwrap();
    assert_eq!(card.rpc_id, 92);
    assert!(card
        .response_error
        .unwrap()
        .contains("synthetic permission writer failed"));
    assert_eq!(written.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(fixture.target.is_alive());
    assert!(card
        .options
        .as_array()
        .unwrap()
        .iter()
        .all(|option| option["kind"] == "reject_once"));
}

#[test]
fn successful_permission_short_writes_produce_one_complete_duplex_frame() {
    let mut fixture = Fixture::new();
    setup(&fixture, true, PermissionPolicy::AlwaysApprove);
    let written = fixture
        .runtime
        .block_on(test_transport::install_writer_fault(
            &fixture.target,
            None,
            false,
            1,
        ));
    let result = fixture
        .runtime
        .block_on(fixture.manager.prepare_permission_request(
            "target-process",
            fixture.target_meta.agent_session_id.as_deref(),
            event(),
        ));
    let update = fixture
        .manager
        .prepare_permission_request_effect(result)
        .unwrap();
    assert!(update.request.is_none());
    let replies = frames(&mut fixture);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["id"], 92);
    assert_eq!(
        written.load(std::sync::atomic::Ordering::SeqCst),
        serde_json::to_vec(&replies[0]).unwrap().len() + 1
    );
    assert!(fixture.target.is_alive());
    assert!(fixture
        .manager
        .pending_permission(Some(fixture.target_meta.id.clone()))
        .is_none());
    fixture.assert_other_untouched();
}
