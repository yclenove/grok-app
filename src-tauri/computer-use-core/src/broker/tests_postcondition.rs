//! Named S3.5 tests: postcondition, unknown observe, traces without secrets.
use super::gates::{click_req, enabled_opts, type_req};
use super::*;
use crate::fake::FakeAdapter;
use crate::protocol::OutcomeKind;
use std::sync::Arc;
use std::time::Duration;

fn ready() -> (
    ComputerUseBroker,
    Arc<FakeAdapter>,
    String,
    u64,
    String,
    u64,
) {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("owner", "run").unwrap();
    let target = fake.fixture_id();
    let gen = broker.authorize_target("run", &target).unwrap();
    let obs = broker.observe("run").unwrap();
    (
        broker,
        fake,
        target,
        gen,
        obs.snapshot_id,
        obs.geometry_revision,
    )
}

#[test]
fn unverifiable_write_returns_applied_not_verified() {
    let (broker, fake, target, gen, snap, geo) = ready();
    fake.set_verifiable(false);
    fake.set_verify_ok(true);
    let out = broker.act(click_req("run", &target, gen, &snap, geo, "no-verify"));
    assert_eq!(out.kind, OutcomeKind::Applied, "{:?}", out.reason);
    assert!(out.executed);
    assert_eq!(fake.executions(), ["no-verify"]);
}

#[test]
fn verifiable_write_can_return_verified() {
    let (broker, fake, target, gen, snap, geo) = ready();
    fake.set_verifiable(true);
    fake.set_verify_ok(true);
    let out = broker.act(click_req("run", &target, gen, &snap, geo, "verified"));
    assert_eq!(out.kind, OutcomeKind::Verified, "{:?}", out.reason);
    assert!(out.executed);
}

#[test]
fn timeout_observe_parses_result_and_does_not_replay() {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_hang(true);
    fake.set_abort_does_not_quiesce(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(40));
    broker.open_run("owner", "run").unwrap();
    let target = fake.fixture_id();
    let gen = broker.authorize_target("run", &target).unwrap();
    let obs = broker.observe("run").unwrap();
    let first = broker.act(click_req(
        "run",
        &target,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "slow",
    ));
    assert_eq!(first.kind, OutcomeKind::Unknown);
    assert_eq!(fake.executions(), ["slow"]);
    assert!(matches!(
        broker.observe("run"),
        Err(BrokerError::LeaseHeld { .. })
    ));
    fake.finish_in_flight();
    let until = Instant::now() + Duration::from_secs(1);
    while broker.inner.lock().runs["run"]
        .in_flight
        .load(Ordering::SeqCst)
        && Instant::now() < until
    {
        std::thread::yield_now();
    }
    let after = broker.observe("run").unwrap();
    assert_ne!(after.snapshot_id, obs.snapshot_id);
    assert_eq!(fake.executions(), ["slow"]);
    let replay = broker.act(click_req(
        "run",
        &target,
        gen,
        &after.snapshot_id,
        after.geometry_revision,
        "slow",
    ));
    assert_eq!(replay.kind, OutcomeKind::Unknown);
    assert_eq!(fake.executions(), ["slow"]);
}

#[test]
fn traces_record_prepare_dispatch_apply_verify_without_secrets() {
    let (broker, fake, target, gen, snap, geo) = ready();
    let secret = "SECRET-TOKEN-NEVER-TRACE";
    let out = broker.act(type_req("run", &target, gen, &snap, geo, "typed", secret));
    assert_ne!(out.kind, OutcomeKind::Rejected, "{:?}", out.reason);
    let traces = broker.traces(Some("run"), true);
    let kinds: Vec<&str> = traces.iter().map(|t| t.kind.as_str()).collect();
    for stage in ["prepare", "dispatch", "apply", "verify"] {
        assert!(kinds.contains(&stage), "missing {stage} in {kinds:?}");
    }
    for event in &traces {
        assert!(
            !event.detail.contains(secret),
            "secret leaked in {} {}",
            event.kind,
            event.detail
        );
    }
    assert_eq!(fake.executions(), ["typed"]);
}

#[test]
fn cancel_trace_is_emitted_when_stop_revokes_dispatch() {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_hang(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(2_000));
    broker.open_run("owner", "run").unwrap();
    let target = fake.fixture_id();
    let gen = broker.authorize_target("run", &target).unwrap();
    let obs = broker.observe("run").unwrap();
    let req = click_req(
        "run",
        &target,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "hanging",
    );
    let broker = Arc::new(broker);
    let worker = broker.clone();
    let th = std::thread::spawn(move || worker.act(req));
    let until = Instant::now() + Duration::from_secs(2);
    while fake.adapter_in_flight() == 0 && Instant::now() < until {
        std::thread::yield_now();
    }
    broker.request_stop("run").unwrap();
    fake.finish_in_flight();
    let out = th.join().unwrap();
    let traces = broker.traces(Some("run"), true);
    assert!(
        traces
            .iter()
            .any(|t| t.kind == "cancel" || t.kind == "stop_requested"),
        "{traces:?}"
    );
    assert_ne!(out.kind, OutcomeKind::Verified);
}
