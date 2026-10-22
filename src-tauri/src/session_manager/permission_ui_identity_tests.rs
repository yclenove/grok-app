//! UI replies must refer to the request shown, even after native RPC reuse.
use super::*;
use crate::acp_client::AcpEvent;
use crate::permission::PermissionPolicy;
use crate::session_manager::permission_request::PermissionRequestEvent;

fn receive(fixture: &Fixture) -> UiPermissionRequest {
    receive_tool(fixture, "shell", "python 示例.py")
}

fn receive_tool(fixture: &Fixture, tool: &str, title: &str) -> UiPermissionRequest {
    let event = PermissionRequestEvent::from_acp(AcpEvent::PermissionRequest {
        rpc_id: 81,
        tool_call_id: "相同工具调用".into(),
        tool_name: tool.into(),
        title: title.into(),
        options: options(),
        raw: json!({"rawInput":{"command":"python 示例.py"}}),
    })
    .unwrap();
    fixture
        .runtime
        .block_on(fixture.manager.prepare_permission_request(
            "target-process",
            fixture.target_meta.agent_session_id.as_deref(),
            event,
        ))
        .unwrap()
        .unwrap()
        .request
        .unwrap()
}

#[test]
fn missing_or_wrong_ui_identity_cannot_reserve_write_or_cache_a_modern_gate() {
    for background in [false, true] {
        for generation in [None, Some(uuid::Uuid::new_v4())] {
            let mut fixture = Fixture::new();
            let card = begin(&fixture, background);
            let mut request = reply(&card, "allow_session");
            request.request_generation = generation;
            assert!(resolve(&fixture, request).is_err());
            assert_eq!(
                fixture
                    .manager
                    .pending_permission(Some(card.session_id.clone())),
                Some(card.clone())
            );
            assert!(fixture.manager.permission_responses.lock().is_empty());
            assert!(frames(&mut fixture).is_empty());
            fixture
                .manager
                .with_session_mut(&card.session_id, |session| {
                    assert!(!session.allow_cache.is_allowed(&card.scope_key));
                    assert_eq!(session.fsm.state(), SessionState::AwaitingPermission);
                })
                .unwrap();
            assert!(resolve(&fixture, reply(&card, "allow_session")).is_ok());
            assert_eq!(frames(&mut fixture).len(), 1);
        }
    }
}

#[test]
fn a_modern_ui_reply_cannot_fall_through_when_its_host_card_is_missing() {
    let mut fixture = Fixture::new();
    let card = begin(&fixture, false);
    fixture
        .manager
        .with_session_mut(&card.session_id, |session| {
            session.pending_permission_ui = None;
        })
        .unwrap();
    assert!(resolve(&fixture, reply(&card, "allow_once")).is_err());
    assert!(frames(&mut fixture).is_empty());
    assert!(fixture.manager.permission_responses.lock().is_empty());
}

#[test]
fn a_delivered_old_reply_audits_its_captured_card_without_consuming_the_new_context() {
    for background in [false, true] {
        let mut fixture = Fixture::new();
        let old = begin(&fixture, background);
        let paused = start(&fixture, "before_audit");
        let current = receive_tool(&fixture, "write_file", "新文件.txt");
        assert!(finish(paused).is_err());
        let ledger = crate::audit_ledger::list_recent(Some(10));
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].tool_name, old.tool_name);
        assert_eq!(ledger[0].summary.as_deref(), Some(old.title.as_str()));
        assert_eq!(ledger[0].permission.as_deref(), Some("allow_session"));
        assert_eq!(
            fixture
                .manager
                .pending_permission(Some(current.session_id.clone())),
            Some(current.clone())
        );
        assert!(resolve(&fixture, reply(&current, "deny")).is_ok());
        let ledger = crate::audit_ledger::list_recent(Some(10));
        assert_eq!(ledger.len(), 2);
        assert_eq!(ledger[0].tool_name, current.tool_name);
        assert_eq!(ledger[0].summary.as_deref(), Some(current.title.as_str()));
        assert_eq!(ledger[1].tool_name, old.tool_name);
        assert_eq!(frames(&mut fixture).len(), 2);
    }
}

fn begin(fixture: &Fixture, background: bool) -> UiPermissionRequest {
    prepare_busy(fixture);
    if background {
        fixture.move_focus(false);
    }
    fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| {
            session.policy = PermissionPolicy::Ask;
        })
        .unwrap();
    receive(fixture)
}

fn reply(card: &UiPermissionRequest, decision: &str) -> PermissionAnswer {
    PermissionAnswer {
        rpc_id: card.rpc_id,
        request_generation: Some(card.request_generation),
        decision: decision.into(),
        option_id: None,
        scope: Some(card.scope_key.clone()),
        session_id: Some(card.session_id.clone()),
        client_options: Some(card.options.clone()),
        client_tool_name: Some(card.tool_name.clone()),
    }
}

#[test]
fn native_permission_card_serializes_the_identity_used_by_the_ui_and_restore() {
    let fixture = Fixture::new();
    let card = begin(&fixture, false);
    let restored = fixture
        .manager
        .pending_permission(Some(fixture.target_meta.id.clone()))
        .unwrap();
    let serialized = serde_json::to_value(&card).unwrap();
    assert_eq!(
        serialized["requestGeneration"],
        json!(card.request_generation),
        "the actual permission event hides its identity from the UI"
    );
    assert_eq!(restored, card);
    assert_eq!(serde_json::to_value(restored).unwrap(), serialized);
}

#[test]
fn an_old_ui_callback_cannot_answer_a_new_native_request_with_identical_rpc_and_content() {
    for background in [false, true] {
        for decision in ["deny", "allow_once", "allow_session"] {
            let mut fixture = Fixture::new();
            let old = begin(&fixture, background);
            let stale = reply(&old, decision);
            assert!(resolve(&fixture, reply(&old, "deny")).is_ok());
            let current = receive(&fixture);
            assert_ne!(current.request_generation, old.request_generation);
            let mut old_ui = serde_json::to_value(&old).unwrap();
            let mut current_ui = serde_json::to_value(&current).unwrap();
            old_ui.as_object_mut().unwrap().remove("requestGeneration");
            current_ui
                .as_object_mut()
                .unwrap()
                .remove("requestGeneration");
            assert_eq!(old_ui, current_ui, "descriptive fields must be identical");
            let result = resolve(&fixture, stale);
            assert!(
                result.is_err(),
                "an old UI callback answered the new request"
            );
            assert_eq!(
                fixture
                    .manager
                    .pending_permission(Some(current.session_id.clone())),
                Some(current.clone())
            );
            fixture
                .manager
                .with_session_mut(&current.session_id, |session| {
                    assert!(!session.allow_cache.is_allowed(&current.scope_key));
                    assert_eq!(session.fsm.state(), SessionState::AwaitingPermission);
                })
                .unwrap();
            assert!(fixture.manager.permission_responses.lock().is_empty());
            assert_eq!(
                frames(&mut fixture).len(),
                1,
                "only the original denial was sent"
            );
            assert!(resolve(&fixture, reply(&current, "allow_once")).is_ok());
            let wire = frames(&mut fixture);
            assert_eq!(wire.len(), 1);
            assert_eq!(wire[0]["id"], 81);
            assert_eq!(wire[0]["result"]["outcome"]["optionId"], "allow-once");
            assert!(fixture.target.is_alive());
            assert!(fixture.other.is_alive());
        }
    }
}
