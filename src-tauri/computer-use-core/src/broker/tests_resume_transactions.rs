//! Resume must not hold Broker state while querying a surface. A new Pause or
//! Stop wins over a late idle result, and concurrent grants cannot enter.
use super::*;

fn pending_resume(
    broker: &Arc<ComputerUseBroker>,
    adapter: &GatedAdapter,
) -> (
    mpsc::Sender<()>,
    std::thread::JoinHandle<Result<(), BrokerError>>,
) {
    broker.pause("run").unwrap();
    let (entered, waiting) = mpsc::channel();
    let (release, released) = mpsc::channel();
    *adapter.next_idle.lock() = Some(CaptureGate {
        entered,
        release: released,
    });
    let broker = broker.clone();
    let resume = std::thread::spawn(move || broker.resume("run"));
    waiting.recv_timeout(Duration::from_secs(2)).unwrap();
    (release, resume)
}

#[test]
fn slow_resume_cannot_block_stop_fencing_or_publish_after_stop() {
    let (broker, adapter) = ready(10);
    let (release, resume) = pending_resume(&broker, &adapter);
    let (fenced, fence_result) = mpsc::channel();
    let stopping = broker.clone();
    let stop = std::thread::spawn(move || fenced.send(stopping.fence_stop("run")).unwrap());
    let immediate = fence_result.recv_timeout(Duration::from_millis(400));
    release.send(()).unwrap();
    let resumed = resume.join().unwrap();
    stop.join().unwrap();
    assert!(
        immediate.is_ok(),
        "Stop waited on the adapter while Broker state was locked"
    );
    let ticket = immediate.unwrap().unwrap().unwrap();
    assert!(resumed.is_err(), "late idle response resumed a stopped run");
    assert!(broker.is_paused("run").unwrap());
    broker.finish_stop_cleanup(&ticket).unwrap();
    assert_eq!(broker.stop_state("run").unwrap(), StopState::Stopped);
}

#[test]
fn new_pause_preempts_a_pending_resume() {
    let (broker, adapter) = ready(10);
    let (release, resume) = pending_resume(&broker, &adapter);
    let (paused, pause_result) = mpsc::channel();
    let pausing = broker.clone();
    let pause = std::thread::spawn(move || paused.send(pausing.pause("run")).unwrap());
    let immediate = pause_result.recv_timeout(Duration::from_millis(400));
    release.send(()).unwrap();
    let resumed = resume.join().unwrap();
    pause.join().unwrap();
    assert!(
        immediate.is_ok(),
        "Pause waited on the old resume's adapter query"
    );
    immediate.unwrap().unwrap();
    assert!(resumed.is_err(), "old resume overrode a later user Pause");
    assert!(broker.is_paused("run").unwrap());
    assert!(broker.observe("run").is_err());
    broker.resume("run").unwrap();
    assert!(!broker.is_paused("run").unwrap());
    assert!(!broker.has_authorized_target("run"));
}

#[test]
fn resume_reserves_admission_and_keeps_other_runs_responsive() {
    let (broker, adapter) = ready(10);
    let (release, resume) = pending_resume(&broker, &adapter);
    let (checked, check_result) = mpsc::channel();
    let inspecting = broker.clone();
    let target = adapter.fake.fixture_id();
    let check = std::thread::spawn(move || {
        let grant = inspecting.authorize_target("run", &target);
        let second_resume = inspecting.resume("run");
        let other_run = inspecting.open_run("other-session", "other-run");
        checked.send((grant, second_resume, other_run)).unwrap();
    });
    let immediate = check_result.recv_timeout(Duration::from_millis(400));
    release.send(()).unwrap();
    let resumed = resume.join().unwrap();
    check.join().unwrap();
    assert!(
        immediate.is_ok(),
        "slow surface query blocked unrelated Broker requests"
    );
    let (grant, second_resume, other_run) = immediate.unwrap();
    assert!(matches!(grant, Err(BrokerError::LeaseHeld { .. })));
    assert!(matches!(second_resume, Err(BrokerError::LeaseHeld { .. })));
    other_run.unwrap();
    resumed.unwrap();
    assert!(!broker.is_paused("run").unwrap());
}

#[test]
fn failed_resume_releases_admission_but_never_unpauses_or_reuses_snapshot() {
    let (broker, adapter) = ready(10);
    broker.observe("run").unwrap();
    broker.pause("run").unwrap();
    adapter.fake.set_alive(false);
    assert!(matches!(broker.resume("run"), Err(BrokerError::DeadTarget)));
    assert!(broker.is_paused("run").unwrap());
    assert!(broker.model_snapshot_id("run").is_none());
    adapter.fake.set_alive(true);
    broker.resume("run").unwrap();
    assert!(!broker.has_authorized_target("run"));
    broker
        .authorize_target("run", &adapter.fake.fixture_id())
        .unwrap();
    broker.observe("run").unwrap();
}

#[test]
fn resume_without_a_pause_is_rejected_without_discarding_authority() {
    let (broker, _) = ready(10);
    let observation = broker.observe("run").unwrap();
    assert!(broker.resume("run").is_err());
    assert!(broker.has_authorized_target("run"));
    assert_eq!(
        broker.model_snapshot_id("run"),
        Some(observation.snapshot_id)
    );
}
