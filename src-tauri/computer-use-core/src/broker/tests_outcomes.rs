//! S1.4: exclusive lease, stop vs stopped, unknown/applied, abort keeps in_flight.

use super::*;
use crate::broker::gates::{click_req, enabled_opts};
use crate::fake::FakeAdapter;
use crate::protocol::OutcomeKind;
use std::sync::Arc;
use std::time::Duration;

fn ready(
    timeout_ms: u64,
) -> (
    ComputerUseBroker,
    Arc<FakeAdapter>,
    String,
    u64,
    String,
    u64,
) {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(timeout_ms));
    broker.open_run("s", "run-1").unwrap();
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
fn abort_failure_keeps_in_flight_and_blocks_second_action() {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_hang(true);
    fake.set_abort_does_not_quiesce(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(40));
    broker.open_run("s", "run-1").unwrap();
    let tid = fake.fixture_id().to_string();
    let gen = broker.authorize_target("run-1", &tid).unwrap();
    let obs = broker.observe("run-1").unwrap();
    let first = click_req(
        "run-1",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "hang",
    );
    let broker = Arc::new(broker);
    let b2 = broker.clone();
    let th = std::thread::spawn(move || b2.act(first));
    let until = Instant::now() + Duration::from_secs(2);
    while fake.adapter_in_flight() == 0 && Instant::now() < until {
        std::thread::yield_now();
    }
    assert!(fake.adapter_in_flight() > 0);
    let second = broker.act(click_req(
        "run-1",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "second",
    ));
    assert_eq!(second.kind, OutcomeKind::Rejected);
    assert!(!second.executed);
    assert!(matches!(
        second.reason.as_deref(),
        Some(r) if r.contains("unknown") || r.contains("lease") || r.contains("paused") || r.contains("in flight") || r.contains("held")
    ));
    fake.finish_in_flight();
    let _ = th.join();
    assert_eq!(
        fake.executions()
            .iter()
            .filter(|id| *id == "second")
            .count(),
        0
    );
}

#[test]
fn verification_failure_is_applied_and_not_redone() {
    let (broker, fake, tid, gen, snap, geo) = ready(400);
    fake.set_verify_ok(false);
    let first = broker.act(click_req("run-1", &tid, gen, &snap, geo, "v1"));
    assert_eq!(first.kind, OutcomeKind::Applied);
    let again = broker.act(click_req("run-1", &tid, gen, &snap, geo, "v1"));
    assert_eq!(again.kind, OutcomeKind::Applied);
    assert_eq!(fake.executions(), ["v1"]);
}

#[test]
fn stop_requested_is_not_stopped_while_worker_runs() {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_hang(true);
    fake.set_abort_does_not_quiesce(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(80));
    broker.open_run("s", "run-1").unwrap();
    let tid = fake.fixture_id().to_string();
    let gen = broker.authorize_target("run-1", &tid).unwrap();
    let obs = broker.observe("run-1").unwrap();
    let req = click_req(
        "run-1",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "slow",
    );
    let broker = Arc::new(broker);
    let b2 = broker.clone();
    let th = std::thread::spawn(move || b2.act(req));
    let until = Instant::now() + Duration::from_secs(2);
    while fake.adapter_in_flight() == 0 && Instant::now() < until {
        std::thread::yield_now();
    }
    assert_eq!(
        broker.request_stop("run-1").unwrap(),
        StopState::StopRequested
    );
    assert_eq!(
        broker.stop_state("run-1").unwrap(),
        StopState::StopRequested
    );
    fake.finish_in_flight();
    assert_eq!(
        broker
            .wait_stopped("run-1", Duration::from_secs(1))
            .unwrap(),
        StopState::Stopped
    );
    let _ = th.join();
}

#[test]
fn exclusive_lease_rejects_second_run_without_replay() {
    let fake = Arc::new(FakeAdapter::new());
    let opts = enabled_opts(400);
    let path = opts.lease_path.clone();
    let a = ComputerUseBroker::new(fake.clone(), opts);
    let b = ComputerUseBroker::new(
        fake.clone(),
        BrokerOptions {
            lease_path: path,
            ..enabled_opts(400)
        },
    );
    a.open_run("s", "run-a").unwrap();
    b.open_run("s", "run-b").unwrap();
    let tid = fake.fixture_id().to_string();
    let ga = a.authorize_target("run-a", &tid).unwrap();
    let gb = b.authorize_target("run-b", &tid).unwrap();
    let oa = a.observe("run-a").unwrap();
    let ob = b.observe("run-b").unwrap();
    let mut type_a = click_req(
        "run-a",
        &tid,
        ga,
        &oa.snapshot_id,
        oa.geometry_revision,
        "aa",
    );
    type_a.action = crate::protocol::ActionKind::TypeText;
    type_a.parameters = serde_json::json!({"text": "hi"});
    assert_eq!(a.act(type_a).kind, OutcomeKind::Verified);
    let mut type_b = click_req(
        "run-b",
        &tid,
        gb,
        &ob.snapshot_id,
        ob.geometry_revision,
        "bb",
    );
    type_b.action = crate::protocol::ActionKind::TypeText;
    type_b.parameters = serde_json::json!({"text": "no"});
    let out = b.act(type_b);
    assert_eq!(out.kind, OutcomeKind::Rejected);
    assert!(!out.executed);
    assert!(out.reason.unwrap().contains("lease"));
}
