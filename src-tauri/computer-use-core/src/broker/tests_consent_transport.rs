//! Synthetic transport/adapter evidence, not a human grant or native acceptance.
use super::*;
use crate::fake::FakeAdapter;
use crate::task_consent::BearerClass;
use serde_json::json;

fn endpoint() -> ComputerUseBroker {
    ComputerUseBroker::request_endpoint(BrokerOptions {
        feature_enabled: false,
        lease_path: std::env::temp_dir().join(format!("cu-request-{}.lease", uuid::Uuid::new_v4())),
        ..BrokerOptions::default()
    })
}

#[test]
fn request_only_endpoint_has_no_executor_or_execution_authority() {
    let broker = endpoint();
    let issued = broker.reserve_transport_connection("app").unwrap();
    assert!(!broker.capabilities().observe_screenshot);
    for surface in [
        SurfaceKind::Desktop,
        SurfaceKind::ManagedBrowser,
        SurfaceKind::ExistingTab,
        SurfaceKind::WebView,
    ] {
        assert!(broker.require_surface_executor(surface).is_err());
    }
    assert!(broker.submit_access(issued.token(), json!({"action":"request", "purpose":"test", "scopeIntent":"test", "surfaces":["desktop"], "capabilities":["observe"]})).is_err());
    broker
        .bind_transport_agent(issued.token(), "agent")
        .unwrap();
    let pending = broker.submit_access(issued.token(), json!({"action":"request", "purpose":"test", "scopeIntent":"test", "surfaces":["desktop"], "capabilities":["observe"]})).unwrap();
    assert_eq!(pending["state"], "pending");
    assert_eq!(broker.executable_grant_count("app"), 0);
    assert!(broker
        .begin_model_call(issued.token(), "computer_observe")
        .is_err());
    assert!(!broker.feature_enabled());
}

#[test]
fn late_handshake_bind_and_retirement_cannot_touch_successor() {
    let broker = endpoint();
    let first = broker.reserve_transport_connection("app").unwrap();
    let second = broker.reserve_transport_connection("app").unwrap();
    assert_ne!(first.token(), second.token());
    assert!(broker
        .bind_transport_agent(first.token(), "old-agent")
        .is_err());
    broker.retire_transport_connection(first.token());
    broker
        .bind_transport_agent(second.token(), "current-agent")
        .unwrap();
    let BearerClass::Live(current) = broker.classify_bearer(second.token()) else {
        panic!("successor retired")
    };
    assert_eq!(current.agent_session_id.as_deref(), Some("current-agent"));
    assert!(broker
        .bind_transport_agent(second.token(), "foreign-agent")
        .is_err());
    broker.retire_transport_connection(second.token());
    assert!(matches!(
        broker.classify_bearer(second.token()),
        BearerClass::Retired
    ));
}

#[test]
fn selection_registers_executor_on_same_request_authority() {
    let broker = endpoint();
    let issued = broker.reserve_transport_connection("app").unwrap();
    broker
        .bind_transport_agent(issued.token(), "agent")
        .unwrap();
    let adapter = Arc::new(FakeAdapter::new());
    broker
        .register_surface_adapter(SurfaceKind::Desktop, adapter.clone())
        .unwrap();
    assert!(broker
        .require_surface_executor(SurfaceKind::Desktop)
        .is_ok());
    assert!(matches!(
        broker.classify_bearer(issued.token()),
        BearerClass::Live(_)
    ));
    assert_eq!(adapter.observe_calls(), 0);
    assert!(adapter.executions().is_empty());
    assert_eq!(broker.executable_grant_count("app"), 0);
}

#[test]
fn host_tick_expires_pending_request_without_model_or_ui_polling() {
    let broker = endpoint();
    let issued = broker.reserve_transport_connection("app").unwrap();
    broker
        .bind_transport_agent(issued.token(), "agent")
        .unwrap();
    broker.submit_access(issued.token(), json!({"action":"request","purpose":"deadline","scopeIntent":"fixture","surfaces":["desktop"],"capabilities":["observe"]})).unwrap();
    broker
        .inner
        .lock()
        .consent
        .advance_clock_without_sweep(std::time::Duration::from_secs(11 * 60));
    assert!(broker.no_authority_reasons("app").is_empty());
    broker.sweep_task_consent();
    assert_eq!(broker.no_authority_reasons("app"), vec!["expired"]);
    assert_eq!(broker.executable_grant_count("app"), 0);
    assert_eq!(broker.capabilities().backend_id, "request-only");
}

#[test]
fn drained_denial_validates_full_owner_request_and_never_grants_execution() {
    let broker = endpoint();
    let issued = broker.reserve_transport_connection("app").unwrap();
    broker
        .bind_transport_agent(issued.token(), "agent")
        .unwrap();
    let request = broker.submit_access(issued.token(), json!({"action":"request","purpose":"notice fixture","scopeIntent":"fixture","surfaces":["desktop"],"capabilities":["observe"]})).unwrap();
    broker
        .deny_access(
            issued.token(),
            request["requestId"].as_str().unwrap(),
            request["revision"].as_u64().unwrap(),
        )
        .unwrap();
    let BearerClass::Live(owner) = broker.classify_bearer(issued.token()) else {
        panic!("live fixture owner")
    };
    let notices = broker.take_no_authority("app", owner.owner_nonce);
    assert_eq!(notices.len(), 1);
    let notice = &notices[0];
    broker.validate_no_authority(notice).unwrap();
    assert!(broker
        .take_no_authority("app", owner.owner_nonce)
        .is_empty());
    assert_eq!(broker.executable_grant_count("app"), 0);
    assert!(!broker.feature_enabled());
    let mut alternatives = Vec::new();
    let mut wrong = notice.clone();
    wrong.app_session_id = "other".into();
    alternatives.push(wrong);
    let mut wrong = notice.clone();
    wrong.agent_session_id = "old-agent".into();
    alternatives.push(wrong);
    let mut wrong = notice.clone();
    wrong.owner_nonce += 1;
    alternatives.push(wrong);
    let mut wrong = notice.clone();
    wrong.incarnation += 1;
    alternatives.push(wrong);
    let mut wrong = notice.clone();
    wrong.connection_generation += 1;
    alternatives.push(wrong);
    let mut wrong = notice.clone();
    wrong.request_id = "other".into();
    alternatives.push(wrong);
    let mut wrong = notice.clone();
    wrong.task_id = "other".into();
    alternatives.push(wrong);
    let mut wrong = notice.clone();
    wrong.revision += 1;
    alternatives.push(wrong);
    let mut wrong = notice.clone();
    wrong.feature_epoch += 1;
    alternatives.push(wrong);
    let mut wrong = notice.clone();
    wrong.dispatch_id = "other".into();
    alternatives.push(wrong);
    let mut wrong = notice.clone();
    wrong.reason = "committed";
    alternatives.push(wrong);
    for wrong in alternatives {
        assert!(broker.validate_no_authority(&wrong).is_err());
    }
    broker.begin_new_task("app").unwrap();
    assert!(broker.validate_no_authority(notice).is_err());
}

#[test]
fn old_notice_is_not_deliverable_after_chat_stop_or_transport_replacement() {
    for replace in [false, true] {
        let broker = endpoint();
        let issued = broker.reserve_transport_connection("app").unwrap();
        broker
            .bind_transport_agent(issued.token(), "agent")
            .unwrap();
        let request = broker.submit_access(issued.token(), json!({"action":"request","purpose":"notice fixture","scopeIntent":"fixture","surfaces":["desktop"],"capabilities":["observe"]})).unwrap();
        broker
            .cancel_access(
                issued.token(),
                request["requestId"].as_str().unwrap(),
                request["revision"].as_u64().unwrap(),
            )
            .unwrap();
        let BearerClass::Live(owner) = broker.classify_bearer(issued.token()) else {
            panic!("live fixture owner")
        };
        let notice = broker
            .take_no_authority("app", owner.owner_nonce)
            .pop()
            .unwrap();
        broker.validate_no_authority(&notice).unwrap();
        if replace {
            let replacement = broker.reserve_transport_connection("app").unwrap();
            broker
                .bind_transport_agent(replacement.token(), "replacement-agent")
                .unwrap();
        } else {
            broker.fence_task_consent("app");
        }
        assert!(broker.validate_no_authority(&notice).is_err());
        assert_eq!(broker.executable_grant_count("app"), 0);
    }
}
