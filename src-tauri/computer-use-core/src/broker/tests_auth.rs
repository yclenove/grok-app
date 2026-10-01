//! S1.3: authorization tickets; model cannot self-grant.

use super::*;
use crate::adapter::skip_desktop_target;
use crate::broker::gates::{click_req, enabled_opts};
use crate::error::BrokerError;
use crate::fake::FakeAdapter;
use crate::ipc::SessionBinding;
use crate::protocol::OutcomeKind;
use std::sync::Arc;

#[test]
fn write_before_authorize_is_zero_execution() {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("chat", "run-1").unwrap();
    let tid = fake.fixture_id();
    let out = broker.act(click_req("run-1", &tid, 1, "s", 1, "early"));
    assert_eq!(out.kind, OutcomeKind::Rejected);
    assert!(!out.executed);
    assert!(fake.executions().is_empty());
    assert!(broker.observe("run-1").is_err());
}

#[test]
fn host_window_is_not_listable_or_authorizable() {
    assert!(skip_desktop_target("Grok Dev", "grok-app"));
    assert!(skip_desktop_target("Grok", "grok-app"));
    assert!(!skip_desktop_target("GrokCuFixture", "fixture"));
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("chat", "run-1").unwrap();
    let listed = broker.discover_targets().unwrap();
    assert!(listed.iter().all(|t| t.app_name != "grok-app"));
    assert!(listed.iter().all(|t| t.target_id != fake.host_id()));
    match broker.authorize_target("run-1", &fake.host_id()) {
        Err(BrokerError::TargetUnauthorized) | Err(BrokerError::DeadTarget) => {}
        other => panic!("host authorize must fail, got {other:?}"),
    }
    assert!(fake.executions().is_empty());
}

#[test]
fn model_cannot_authorize_resume_or_reconnect() {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("chat", "run-1").unwrap();
    let binding = SessionBinding {
        session_id: "chat".into(),
        run_id: "run-1".into(),
    };
    for name in [
        "computer_authorize",
        "computer_resume",
        "computer_reconnect",
        "computer_clear_profile",
        "computer_open_profile",
        "computer_evaluate",
        "browser_run_code_unsafe",
        "computer_upload",
        "authorize",
        "resume",
        "reconnect",
    ] {
        let result = crate::tools::dispatch(&broker, &binding, name, serde_json::json!({}));
        assert_eq!(result["isError"], true, "{name}");
    }
    assert!(fake.executions().is_empty());
}

#[test]
fn dead_target_cannot_be_authorized() {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("chat", "run-1").unwrap();
    fake.set_alive(false);
    assert_eq!(
        broker.authorize_target("run-1", &fake.fixture_id()),
        Err(BrokerError::DeadTarget)
    );
}

#[test]
fn feature_off_stops_run_so_late_authorize_fails() {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("chat", "run-1").unwrap();
    broker.set_feature_enabled(false);
    assert!(broker
        .authorize_target("run-1", &fake.fixture_id())
        .is_err());
    assert!(fake.executions().is_empty());
}
