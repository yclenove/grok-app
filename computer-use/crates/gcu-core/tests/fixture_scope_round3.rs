//! ADR 0017 S1 round 3. One attempt keeps the authority it armed.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod fixture_scope_support;

use std::future::poll_fn;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Waker;
use std::time::Duration;

use async_trait::async_trait;
use fixture_scope_support::{
    allowed_scope, element_index, permissive, scope_only_b, HoldOp, World, SCOPE_MESSAGE,
};
use gcu_core::action::ToolCall;
use gcu_core::driver::{Driver, Stream, UiEvent};
use gcu_core::scope::ScopeLease;
use gcu_core::session::{Session, WindowLocks};
use gcu_core::settle::{Clock, SettleConfig, SettleEngine};
use gcu_host_api::testing::{MemoryPolicyStore, RecordingEventSink};
use gcu_host_api::ApprovalProvider;
use gcu_protocol::result::Outcome;
use gcu_protocol::tools::*;
use gcu_protocol::ErrorCode;

#[tokio::test]
async fn fence_revoke_or_rebind_refuses_before_delivery() {
    for phase in [HoldOp::Admit, HoldOp::MarkSeen] {
        for revoke in [true, false] {
            let world = World::open(permissive());
            let seen = observe(&world, "fixture.a", Some("w-a"), serde_json::json!(false)).await;
            let index = element_index(&seen.text, "Go");
            let before = world.spy.external().perform;
            world.spy.arm_hold(phase, false);
            let spy = world.spy.clone();
            let lease = world.lease.clone();
            let side = tokio::spawn(async move {
                spy.wait_held().await;
                if revoke {
                    lease.revoke();
                } else {
                    lease.rebind(allowed_scope(2)).unwrap();
                }
                spy.release_hold();
            });
            let outcome = act(&world, "fixture.a", Some("w-a"), click_index(index)).await;
            side.await.unwrap();
            let err = outcome.error.expect("fence delivery succeeded");
            assert_eq!(err.message, SCOPE_MESSAGE, "{phase:?} revoke={revoke}");
            assert_eq!(outcome.action.outcome, Outcome::Refused);
            assert_eq!(
                world.spy.external().perform,
                before,
                "delivered through {phase:?}"
            );
            assert!(world.spy.releases() > 0, "release skipped for {phase:?}");
            assert!(err.state.is_none());
        }
    }
}

#[tokio::test]
async fn quiet_stream_ends_on_revoke_rebind_or_stop_without_an_event() {
    for kind in [End::Revoke, End::Rebind, End::Stop] {
        let world = World::open(permissive());
        world.spy.arm_events();
        let driver = world.direct();
        let win = world.spy.real_window("fixture.a", "w-a").await;
        let mut stream = driver.subscribe(&win).expect("admitted subscribe");
        let spy = world.spy.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let item = poll_fn(|cx| Stream::poll_next(Pin::new(&mut stream), cx)).await;
            let _ = tx.send(item);
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !spy.events_parked() {
            assert!(std::time::Instant::now() < deadline, "stream did not park");
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert_eq!(spy.unsubscribed(), 0);
        match kind {
            End::Revoke => world.lease.revoke(),
            End::Rebind => world.lease.rebind(allowed_scope(2)).unwrap(),
            End::Stop => world.activity.request_stop(),
        }
        let item = tokio::time::timeout(Duration::from_secs(2), rx)
            .await
            .expect("quiet stream hung")
            .unwrap();
        assert!(item.is_none(), "{kind:?} forwarded {item:?}");
        assert_eq!(
            spy.unsubscribed(),
            1,
            "{kind:?} kept the inner subscription"
        );
    }
}

#[derive(Clone, Copy, Debug)]
enum End {
    Revoke,
    Rebind,
    Stop,
}

#[tokio::test]
async fn changed_owner_or_incarnation_is_not_forwarded_without_rebind() {
    let world = World::open(permissive());
    world.spy.arm_events();
    let driver = world.direct();
    let win = world.spy.real_window("fixture.a", "w-a").await;
    let mut stream = driver.subscribe(&win).unwrap();
    world.spy.set_incarnation("w-a", 9);
    world.spy.push_event(UiEvent::Changed);
    let item = poll_fn(|cx| Stream::poll_next(Pin::new(&mut stream), cx)).await;
    assert!(item.is_none(), "old incarnation forwarded {item:?}");
    assert_eq!(world.spy.unsubscribed(), 1);

    let world = World::open(permissive());
    world.spy.arm_events();
    let driver = world.direct();
    let win = world.spy.real_window("fixture.a", "w-a").await;
    let mut stream = driver.subscribe(&win).unwrap();
    world.spy.shift_start("fixture.a", 99);
    world.spy.push_event(UiEvent::Changed);
    let item = poll_fn(|cx| Stream::poll_next(Pin::new(&mut stream), cx)).await;
    assert!(item.is_none(), "old owner forwarded {item:?}");
    assert_eq!(world.lease.current().unwrap().generation(), 1);
}

#[tokio::test]
async fn press_keys_rebind_after_recheck_before_open_is_not_delivered() {
    let world = World::open(permissive());
    observe(&world, "fixture.a", Some("w-a"), serde_json::json!(false)).await;
    let before = world.spy.external().perform;
    world.spy.arm_hold(HoldOp::BeforeOpen, false);
    let spy = world.spy.clone();
    let lease = world.lease.clone();
    let side = tokio::spawn(async move {
        spy.wait_held().await;
        lease.rebind(allowed_scope(2)).unwrap();
        spy.release_hold();
    });
    let outcome = act(
        &world,
        "fixture.a",
        Some("w-a"),
        BatchStep::PressKey(PressKeyArgs { key: "a".into() }),
    )
    .await;
    side.await.unwrap();
    let err = outcome.error.expect("press keys used the new generation");
    assert_eq!(err.message, SCOPE_MESSAGE);
    assert_eq!(outcome.action.outcome, Outcome::Refused);
    assert_eq!(world.spy.external().perform, before);
    assert_eq!(world.lease.current().unwrap().generation(), 2);
}

#[tokio::test]
async fn observe_rebind_before_the_second_read_is_denied() {
    let world = World::open(permissive());
    world.spy.arm_hold_after(HoldOp::Snapshot, 1, false);
    let spy = world.spy.clone();
    let lease = world.lease.clone();
    let side = tokio::spawn(async move {
        spy.wait_held().await;
        lease.rebind(allowed_scope(2)).unwrap();
        spy.release_hold();
    });
    let err = world
        .router
        .observe(
            &world.session,
            state_params("fixture.a", Some("w-a"), serde_json::json!(false)),
        )
        .await
        .unwrap_err();
    side.await.unwrap();
    assert_eq!(err.message, SCOPE_MESSAGE);
    assert!(err.state.is_none());
    assert_eq!(world.spy.external().perform, 0);
}

#[tokio::test]
async fn read_keeps_the_epoch_captured_before_its_awaits() {
    let world = World::open(permissive());
    world.spy.arm_hold(HoldOp::Geometry, false);
    let spy = world.spy.clone();
    let lease = world.lease.clone();
    let side = tokio::spawn(async move {
        spy.wait_held().await;
        lease.rebind(allowed_scope(2)).unwrap();
        spy.release_hold();
    });
    let err = world
        .router
        .observe(
            &world.session,
            state_params("fixture.a", Some("w-a"), serde_json::json!(false)),
        )
        .await
        .unwrap_err();
    side.await.unwrap();
    assert_eq!(err.message, SCOPE_MESSAGE);
    assert!(err.state.is_none());
    assert!(!format!("{err:?}").contains("Go"));
}

#[tokio::test]
async fn batch_clears_output_when_the_attempt_epoch_dies_after_delivery_starts() {
    for revoke in [true, false] {
        let world = World::open(permissive());
        let seen = observe(&world, "fixture.a", Some("w-a"), serde_json::json!(false)).await;
        let index = element_index(&seen.text, "Go");
        let before = world.spy.external().perform;
        world.spy.arm_hold_after(HoldOp::Perform, 1, true);
        let spy = world.spy.clone();
        let lease = world.lease.clone();
        let side = tokio::spawn(async move {
            spy.wait_held().await;
            assert!(spy.started() >= 1);
            assert!(spy.held_input() >= 1);
            assert_eq!(spy.releases(), 0);
            assert_eq!(spy.external().perform, before + 1);
            if revoke {
                lease.revoke();
            } else {
                lease.rebind(allowed_scope(2)).unwrap();
            }
            spy.release_hold();
        });
        let report = world
            .router
            .run_batch(
                &world.session,
                "round3-batch",
                ComputerBatchParams {
                    app: "fixture.a".into(),
                    window: Some("w-a".into()),
                    steps: vec![click_index(index), click_index(index), click_index(index)],
                    stop_on_error: false,
                    final_state: true,
                },
            )
            .await;
        side.await.unwrap();
        let err = report
            .error
            .as_ref()
            .expect("batch treated the started step as clean");
        assert_eq!(err.code, ErrorCode::DriverError, "{err:?}");
        assert_ne!(err.message, SCOPE_MESSAGE);
        assert!(err.state.is_none());
        assert!(report.state.is_none());
        assert_eq!(report.steps.len(), 3);
        assert_eq!(report.steps[0].outcome, Outcome::Done);
        assert_eq!(report.steps[1].outcome, Outcome::Unknown);
        assert_eq!(report.steps[2].outcome, Outcome::Refused);
        assert_eq!(report.steps[2].ms, 0);
        assert_eq!(report.failed_step, Some(1));
        assert_eq!(world.spy.external().perform, before + 2);
        assert_eq!(world.spy.held_input(), 0);
        assert!(world.spy.releases() > 0);
        let retained = format!("{:?} {:?}", report.state, err.state);
        assert!(!retained.contains("Go"), "{retained}");
        assert!(!retained.contains("Allowed A"), "{retained}");
    }
}

#[tokio::test]
async fn held_wait_sample_is_not_published_after_revoke_or_rebind() {
    for remove in [false, true] {
        let world = World::open(permissive());
        let clock = Arc::new(ParkClock::new());
        let router = parked_router(&world, clock.clone());
        let session = Session::open("park-wait", world.activity.clone(), router.input_release());
        session.begin_turn().unwrap();
        world.spy.arm_hold_after(HoldOp::Snapshot, 1, false);
        let spy = world.spy.clone();
        let lease = world.lease.clone();
        let clock_side = clock.clone();
        let side = tokio::spawn(async move {
            spy.wait_held().await;
            assert_eq!(clock_side.parks(), 1);
            spy.release_hold();
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while clock_side.parks() < 2 {
                assert!(
                    std::time::Instant::now() < deadline,
                    "wait did not park after the sample"
                );
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            if remove {
                lease.rebind(scope_only_b(2)).unwrap();
            } else {
                lease.revoke();
            }
            clock_side.finish();
        });
        let mut call = ToolCall::new(
            "wait",
            "fixture.a",
            BatchStep::WaitFor(WaitForArgs {
                until: Condition::TextPresent("missing-token".into()),
                timeout_ms: 200,
            }),
        );
        call.window = Some("w-a".into());
        call.settle_budget_ms = Some(0);
        let outcome = router.execute(&session, call).await;
        side.await.unwrap();
        let err = outcome.error.as_ref().expect("held sample was published");
        assert_eq!(err.message, SCOPE_MESSAGE);
        assert!(outcome.state.is_none());
        assert!(err.state.is_none());
        let rendered = format!("{outcome:?} {err:?}");
        assert!(!rendered.contains("Go"), "{rendered}");
        assert!(!rendered.contains("Allowed A"), "{rendered}");
        if remove {
            let again = router
                .observe(
                    &session,
                    state_params("fixture.b", Some("w-b"), serde_json::json!(false)),
                )
                .await
                .expect("window B is still in scope");
            assert_eq!(again.state.snapshot_id, "s_1");
        }
    }
}

#[tokio::test]
async fn stop_wins_when_a_wait_snapshot_fails_after_the_latch() {
    let world = World::open(permissive());
    world.spy.arm_snapshot_fault_after(1, world.lease.clone());
    let mut call = ToolCall::new(
        "wait-stop",
        "fixture.a",
        BatchStep::WaitFor(WaitForArgs {
            until: Condition::TextPresent("missing-token".into()),
            timeout_ms: 200,
        }),
    );
    call.window = Some("w-a".into());
    call.settle_budget_ms = Some(0);
    let outcome = world.router.execute(&world.session, call).await;
    let err = outcome.error.expect("faulted wait succeeded");
    assert_eq!(err.code, ErrorCode::StoppedByUser, "{err:?}");
    assert_eq!(err.message, "stopped by user");
    assert!(outcome.state.is_none());
}

#[tokio::test]
async fn coordinate_click_checks_the_selected_screenshot_epoch() {
    let world = World::open(permissive());
    observe(&world, "fixture.a", Some("w-a"), serde_json::json!(true)).await;
    world.lease.rebind(allowed_scope(2)).unwrap();
    observe(&world, "fixture.a", Some("w-a"), serde_json::json!(false)).await;
    let before = world.spy.external().perform;
    let outcome = act(
        &world,
        "fixture.a",
        Some("w-a"),
        BatchStep::Click(ClickArgs {
            element_index: None,
            x: Some(10.0),
            y: Some(10.0),
            screenshot_id: None,
            click_count: 1,
            mouse_button: MouseButton::Left,
        }),
    )
    .await;
    let err = outcome.error.expect("old screenshot was accepted");
    assert_eq!(err.message, SCOPE_MESSAGE, "{err:?}");
    assert_eq!(outcome.action.outcome, Outcome::Refused);
    assert_eq!(world.spy.external().perform, before);
    assert!(err.state.is_none());
}

#[tokio::test]
async fn rebind_asks_for_a_new_session_decision() {
    let world = World::open(permissive());
    let before = world.approvals.app_requests().len();
    observe(&world, "fixture.a", Some("w-a"), serde_json::json!(false)).await;
    assert_eq!(world.approvals.app_requests().len(), before + 1);
    world.lease.rebind(allowed_scope(2)).unwrap();
    observe(&world, "fixture.a", Some("w-a"), serde_json::json!(false)).await;
    assert_eq!(world.approvals.app_requests().len(), before + 2);
}

#[tokio::test]
async fn confirm_leaves_a_rejected_generation_unseen() {
    let lease = ScopeLease::install(allowed_scope(1));
    let mut authority = lease.arm_attempt().unwrap();
    lease.rebind(allowed_scope(2)).unwrap();
    assert!(!authority.confirm(&lease));
    tokio::time::timeout(Duration::from_millis(50), authority.changed())
        .await
        .expect("confirm marked the new generation seen")
        .unwrap();
}

#[test]
fn publish_with_no_receiver_is_the_next_attempt() {
    let lease = ScopeLease::install(allowed_scope(1));
    lease.revoke();
    assert!(lease.arm_attempt().is_none());
    let lease = ScopeLease::install(allowed_scope(1));
    lease.rebind(allowed_scope(2)).unwrap();
    let authority = lease.arm_attempt().unwrap();
    assert_eq!(authority.scope().generation(), 2);
}

struct ParkClock {
    now: Mutex<Duration>,
    parks: AtomicUsize,
    go: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl ParkClock {
    fn new() -> Self {
        Self {
            now: Mutex::new(Duration::ZERO),
            parks: AtomicUsize::new(0),
            go: AtomicBool::new(false),
            waker: Mutex::new(None),
        }
    }

    fn parks(&self) -> usize {
        self.parks.load(Ordering::SeqCst)
    }

    fn finish(&self) {
        *self.now.lock().unwrap() = Duration::from_millis(500);
        self.go.store(true, Ordering::SeqCst);
        if let Some(waker) = self.waker.lock().unwrap().take() {
            waker.wake();
        }
    }
}

#[async_trait]
impl Clock for ParkClock {
    fn now(&self) -> Duration {
        *self.now.lock().unwrap()
    }

    async fn wait_until(&self, deadline: Duration) {
        self.parks.fetch_add(1, Ordering::SeqCst);
        std::future::poll_fn(|cx| {
            if self.go.load(Ordering::SeqCst) {
                let mut now = self.now.lock().unwrap();
                if *now < deadline {
                    *now = deadline;
                }
                self.go.store(false, Ordering::SeqCst);
                std::task::Poll::Ready(())
            } else {
                *self.waker.lock().unwrap() = Some(cx.waker().clone());
                std::task::Poll::Pending
            }
        })
        .await;
    }
}

fn parked_router(world: &World, clock: Arc<ParkClock>) -> gcu_core::action::ActionRouter {
    gcu_core::action::ActionRouter::with_fixture_scope(
        world.spy.clone(),
        world.lease.clone(),
        world.activity.clone(),
        world.approvals.clone() as Arc<dyn ApprovalProvider>,
        Arc::new(MemoryPolicyStore::new(permissive())),
        Arc::new(RecordingEventSink::new()),
        WindowLocks::new(),
    )
    .with_settle_engine(SettleEngine::new(SettleConfig::default(), clock))
}

async fn observe(
    world: &World,
    app: &str,
    window: Option<&str>,
    shot: serde_json::Value,
) -> gcu_core::action::ActionState {
    world
        .router
        .observe(&world.session, state_params(app, window, shot))
        .await
        .unwrap()
}

fn state_params(app: &str, window: Option<&str>, shot: serde_json::Value) -> GetAppStateParams {
    let mut value = serde_json::json!({"app": app, "include_screenshot": shot});
    if let Some(window) = window {
        value["window"] = serde_json::json!(window);
    }
    serde_json::from_value(value).unwrap()
}

async fn act(
    world: &World,
    app: &str,
    window: Option<&str>,
    step: BatchStep,
) -> gcu_core::action::ToolOutcome {
    let mut call = ToolCall::new("act", app, step);
    call.window = window.map(str::to_owned);
    call.settle_budget_ms = Some(0);
    world.router.execute(&world.session, call).await
}

fn click_index(index: u32) -> BatchStep {
    BatchStep::Click(ClickArgs {
        element_index: Some(index),
        x: None,
        y: None,
        screenshot_id: None,
        click_count: 1,
        mouse_button: MouseButton::Left,
    })
}
