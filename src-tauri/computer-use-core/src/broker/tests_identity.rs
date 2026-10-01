//! S1.2: Host-bound identity; stale IDs and duplicate actionId never re-execute.

use super::*;
use crate::broker::gates::{click_req, enabled_opts};
use crate::error::BrokerError;
use crate::fake::FakeAdapter;
use crate::protocol::OutcomeKind;
use std::sync::Arc;
use std::time::Duration;

fn setup() -> (
    ComputerUseBroker,
    Arc<FakeAdapter>,
    String,
    u64,
    String,
    u64,
) {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("sess", "run-1").unwrap();
    let tid = fake.fixture_id().to_string();
    let gen = broker.authorize_target("run-1", &tid).unwrap();
    let obs = broker.observe("run-1").unwrap();
    (
        broker,
        fake,
        tid,
        gen,
        obs.snapshot_id,
        obs.geometry_revision,
    )
}

#[test]
fn geometry_mismatch_zero_execution() {
    let (broker, fake, tid, gen, snap, geo) = setup();
    let out = broker.act(click_req("run-1", &tid, gen, &snap, geo + 9, "geo"));
    assert_eq!(out.kind, OutcomeKind::Rejected);
    assert!(!out.executed);
    assert!(fake.executions().is_empty());
}

#[test]
fn cross_session_cannot_reuse_run_id() {
    let (broker, fake, tid, gen, snap, geo) = setup();
    assert_eq!(
        broker.open_run("other-session", "run-1").unwrap_err(),
        BrokerError::IdentityMismatch("appSessionId")
    );
    assert!(broker.require_owner("other-session", "run-1").is_err());
    let out = broker.act(click_req("run-1", &tid, gen, &snap, geo, "cross"));
    assert_eq!(out.kind, OutcomeKind::Verified);
    assert_eq!(fake.executions(), ["cross"]);
}

#[test]
fn resume_drops_authorization_snapshot_and_lease() {
    let (broker, fake, tid, gen, snap, geo) = setup();
    broker.pause("run-1").unwrap();
    broker.resume("run-1").unwrap();
    assert_eq!(
        broker.observe("run-1").unwrap_err(),
        BrokerError::TargetUnauthorized
    );
    let out = broker.act(click_req("run-1", &tid, gen, &snap, geo, "after-resume"));
    assert_eq!(out.kind, OutcomeKind::Rejected);
    assert!(!out.executed);
    assert!(fake.executions().is_empty());
}

#[test]
fn fork_does_not_inherit_authorization_or_snapshot() {
    let (broker, fake, tid, gen, snap, geo) = setup();
    let first = broker.act(click_req("run-1", &tid, gen, &snap, geo, "orig"));
    assert_eq!(first.kind, OutcomeKind::Verified);
    broker.fork_run("run-1", "run-2").unwrap();
    assert_eq!(
        broker.observe("run-2").unwrap_err(),
        BrokerError::TargetUnauthorized
    );
    let replay = broker.act(click_req("run-2", &tid, gen, &snap, geo, "orig"));
    assert_eq!(replay.kind, OutcomeKind::Rejected);
    assert!(!replay.executed);
    assert_eq!(fake.executions(), ["orig"]);
}

#[test]
fn context_change_drops_authorization_and_old_actions() {
    let (broker, fake, tid, gen, snap, geo) = setup();
    broker.invalidate_for_context_change("run-1").unwrap();
    let err = broker.observe("run-1").unwrap_err();
    assert!(
        err == BrokerError::TargetUnauthorized
            || err == BrokerError::Schema("run is paused".into()),
        "{err}"
    );
    let out = broker.act(click_req("run-1", &tid, gen, &snap, geo, "after-compact"));
    assert_eq!(out.kind, OutcomeKind::Rejected);
    assert!(!out.executed);
    assert!(fake.executions().is_empty());
}

#[test]
fn concurrent_duplicate_action_id_executes_once() {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_delay(Duration::from_millis(80));
    let broker = Arc::new(ComputerUseBroker::new(fake.clone(), enabled_opts(400)));
    broker.open_run("sess", "run-1").unwrap();
    let tid = fake.fixture_id().to_string();
    let gen = broker.authorize_target("run-1", &tid).unwrap();
    let obs = broker.observe("run-1").unwrap();
    let req = click_req(
        "run-1",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "once",
    );
    let a = broker.clone();
    let b = broker.clone();
    let ra = req.clone();
    let rb = req;
    let ta = std::thread::spawn(move || a.act(ra));
    let tb = std::thread::spawn(move || b.act(rb));
    let oa = ta.join().unwrap();
    let ob = tb.join().unwrap();
    assert!(matches!(
        oa.kind,
        OutcomeKind::Verified | OutcomeKind::Unknown | OutcomeKind::Rejected
    ));
    assert!(matches!(
        ob.kind,
        OutcomeKind::Verified | OutcomeKind::Unknown | OutcomeKind::Rejected
    ));
    assert_eq!(fake.executions(), ["once"]);
}
