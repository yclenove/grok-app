//! Delivered replies must stay single-use even when finalization fails.
use super::*;
use crate::acp_client::AcpEvent;
use crate::permission::PermissionPolicy;
use crate::session_manager::permission_request::PermissionRequestEvent;

fn remove_card(fixture: &Fixture) {
    fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| {
            session.pending_permission_options = None;
            session.pending_permission_tool_name = None;
            session.pending_permission_ui = None;
        })
        .unwrap();
}

fn assert_pending(fixture: &Fixture, card: bool) {
    fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| {
            assert_eq!(session.pending_permission_rpc_id, Some(81));
            assert_eq!(session.fsm.state(), SessionState::AwaitingPermission);
            assert!(!session.allow_cache.is_allowed("shell:python"));
            assert_eq!(session.pending_permission_ui.is_some(), card);
        })
        .unwrap();
    assert!(fixture.manager.permission_responses.lock().is_empty());
    assert!(fixture.target.is_alive());
    assert!(fixture.other.is_alive());
    assert_eq!(
        serde_json::to_value(workspace_context::session(&fixture.other_meta.id).unwrap()).unwrap(),
        serde_json::to_value(&fixture.other_meta).unwrap()
    );
}

#[test]
fn delivered_reply_cannot_be_retried_after_a_transient_stored_binding_failure() {
    for background in [false, true] {
        for card in [false, true] {
            let mut fixture = Fixture::new();
            setup(&fixture, background);
            if !card {
                remove_card(&fixture);
            }
            let paused = start(&fixture, "after_write");
            let mut changed = fixture.target_meta.clone();
            changed.project_id = fixture.other_meta.project_id.clone();
            store::update_session_meta(&changed).unwrap();
            let first = finish(paused);
            store::update_session_meta(&fixture.target_meta).unwrap();
            let retry = resolve(&fixture, answer(&fixture, "allow_session"));
            let wire = frames(&mut fixture);
            assert!(first.is_err());
            assert!(
                retry.is_err(),
                "a delivered reply was sent again after the store recovered"
            );
            assert_eq!(wire.len(), 1);
            assert_eq!(wire[0]["id"], 81);
            assert_pending(&fixture, card);
        }
    }
}

#[test]
fn cancellation_at_a_post_delivery_checkpoint_cannot_reopen_the_same_rpc() {
    // This checkpoint is test-only. Production has no async pause after a
    // successful write; this asserts the delivery invariant, not a live race.
    for background in [false, true] {
        for card in [false, true] {
            let mut fixture = Fixture::new();
            setup(&fixture, background);
            if !card {
                remove_card(&fixture);
            }
            let paused = start(&fixture, "after_write");
            paused.abort.abort();
            let first = paused.worker.join().unwrap();
            drop(paused.proceed);
            let retry = resolve(&fixture, answer(&fixture, "deny"));
            let wire = frames(&mut fixture);
            assert!(first.is_err());
            assert!(
                retry.is_err(),
                "cancelled bookkeeping allowed a duplicate response"
            );
            assert_eq!(wire.len(), 1);
            assert_pending(&fixture, card);
        }
    }
}

#[test]
fn delivered_reply_is_bound_to_its_gate_even_when_turn_ownership_changes() {
    for background in [false, true] {
        let mut fixture = Fixture::new();
        setup(&fixture, background);
        let paused = start(&fixture, "after_write");
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                session.turn_open_epoch = session.turn_open_epoch.wrapping_add(1);
                session.active_turn_id = Some(uuid::Uuid::new_v4().to_string());
            })
            .unwrap();
        let first = finish(paused);
        let retry = resolve(&fixture, answer(&fixture, "deny"));
        let wire = frames(&mut fixture);
        assert!(first.is_err());
        assert!(
            retry.is_err(),
            "a new turn resent the original gate's reply"
        );
        assert_eq!(wire.len(), 1);
        assert_pending(&fixture, true);
    }
}

#[test]
fn a_new_native_permission_can_replace_a_delivered_gate_and_reuse_its_rpc() {
    for background in [false, true] {
        let mut fixture = Fixture::new();
        setup(&fixture, background);
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                session.policy = PermissionPolicy::Ask;
            })
            .unwrap();
        let paused = start(&fixture, "after_write");
        let incoming = PermissionRequestEvent::from_acp(AcpEvent::PermissionRequest {
            rpc_id: 81,
            tool_call_id: "新请求".into(),
            tool_name: "shell".into(),
            title: "批准新的中文命令".into(),
            options: options(),
            raw: json!({"rawInput":{"command":"python 新命令.py"}}),
        })
        .unwrap();
        let next = fixture
            .runtime
            .block_on(fixture.manager.prepare_permission_request(
                "target-process",
                fixture.target_meta.agent_session_id.as_deref(),
                incoming,
            ));
        let first = finish(paused);
        let next = next
            .ok()
            .flatten()
            .expect("the delivered gate blocked a real subsequent request");
        let card = next.request.unwrap();
        assert!(
            first.is_err(),
            "old completion cleared the newly installed request"
        );
        assert_eq!(card.tool_call_id, "新请求");
        assert_pending(&fixture, true);
        let result = resolve(
            &fixture,
            PermissionAnswer {
                rpc_id: card.rpc_id,
                request_generation: Some(card.request_generation),
                decision: "deny".into(),
                option_id: None,
                scope: None,
                session_id: Some(card.session_id),
                client_options: Some(card.options),
                client_tool_name: Some(card.tool_name),
            },
        );
        assert!(result.is_ok());
        let wire = frames(&mut fixture);
        assert_eq!(wire.len(), 2);
        assert_eq!(wire[0]["id"], 81);
        assert_eq!(wire[1]["id"], 81);
        assert_eq!(wire[1]["result"]["outcome"]["optionId"], "reject-once");
        assert_resolved(&fixture, false);
    }
}
