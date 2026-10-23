//! ADR 0017 S1 round 4. Publication is checked after the last synchronous step.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod fixture_scope_support;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Waker;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fixture_scope_support::{permissive, scope_only_b, World, SCOPE_MESSAGE};
use gcu_core::action::ToolCall;
use gcu_core::session::Session;
use gcu_core::settle::{Clock, SettleConfig, SettleEngine};
use gcu_host_api::testing::{MemoryPolicyStore, RecordingEventSink};
use gcu_host_api::ApprovalProvider;
use gcu_protocol::tools::{BatchStep, Condition, GetAppStateParams, WaitForArgs};
use gcu_protocol::ErrorCode;

#[tokio::test]
async fn publication_after_policy_load_drops_the_sample_on_revoke_or_rebind() {
    for remove in [false, true] {
        let world = World::open(permissive());
        let before = world.session.delivered_snapshot_count();
        let gate = world.router.publication_gate();
        gate.arm();
        let release = std::thread::spawn({
            let gate = Arc::clone(&gate);
            let lease = world.lease.clone();
            move || invalidate_when_parked(&gate, &lease, remove)
        });
        let result = world
            .router
            .observe(
                &world.session,
                state_params("fixture.a", Some("w-a"), serde_json::json!(true)),
            )
            .await;
        release.join().unwrap();
        let err = result.unwrap_err();
        assert_eq!(err.message, SCOPE_MESSAGE, "{err:?}");
        assert!(err.state.is_none(), "{err:?}");
        assert!(!format!("{err:?}").contains("Go"), "{err:?}");
        assert!(!format!("{err:?}").contains("Allowed A"), "{err:?}");
        assert_eq!(world.session.delivered_snapshot_count(), before);
        if remove {
            let again = world
                .router
                .observe(
                    &world.session,
                    state_params("fixture.b", Some("w-b"), serde_json::json!(false)),
                )
                .await
                .expect("window B stays in scope");
            assert_eq!(again.state.snapshot_id, "s_1");
            assert!(again.image.is_none());
        }
    }
}

#[tokio::test]
async fn publication_stop_during_the_gate_wins_over_scope_denial() {
    let world = World::open(permissive());
    let before = world.session.delivered_snapshot_count();
    let gate = world.router.publication_gate();
    gate.arm();
    let release = std::thread::spawn({
        let gate = Arc::clone(&gate);
        let lease = world.lease.clone();
        let activity = Arc::clone(&world.activity);
        move || {
            wait_until_parked(&gate);
            activity.request_stop();
            lease.revoke();
            gate.release();
        }
    });
    let err = world
        .router
        .observe(
            &world.session,
            state_params("fixture.a", Some("w-a"), serde_json::json!(true)),
        )
        .await
        .unwrap_err();
    release.join().unwrap();
    assert_eq!(err.code, ErrorCode::StoppedByUser, "{err:?}");
    assert_eq!(err.message, "stopped by user");
    assert!(err.state.is_none());
    assert_eq!(world.session.delivered_snapshot_count(), before);
}

#[tokio::test]
async fn wait_publication_after_the_sample_drops_on_revoke_or_rebind() {
    for remove in [false, true] {
        let world = World::open(permissive());
        let clock = Arc::new(ParkClock::new());
        let router = parked_router(&world, Arc::clone(&clock));
        let session = Session::open(
            "park-publish",
            world.activity.clone(),
            router.input_release(),
        );
        session.begin_turn().unwrap();
        let before = session.delivered_snapshot_count();
        let gate = router.publication_gate();
        let release = std::thread::spawn({
            let gate = Arc::clone(&gate);
            let lease = world.lease.clone();
            let clock = Arc::clone(&clock);
            move || {
                let _unblock = Unblock {
                    clock: Some(&clock),
                    gate: &gate,
                };
                let deadline = Instant::now() + Duration::from_secs(5);
                while clock.parks() < 2 {
                    if Instant::now() >= deadline {
                        panic!("wait did not park after the sample");
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                gate.arm();
                clock.finish();
                wait_until_parked(&gate);
                if remove {
                    lease.rebind(scope_only_b(2)).unwrap();
                } else {
                    lease.revoke();
                }
                gate.release();
            }
        });
        let mut call = ToolCall::new(
            "wait-publish",
            "fixture.a",
            BatchStep::WaitFor(WaitForArgs {
                until: Condition::TextPresent("missing-token".into()),
                timeout_ms: 200,
            }),
        );
        call.window = Some("w-a".into());
        call.settle_budget_ms = Some(0);
        let outcome = router.execute(&session, call).await;
        release.join().unwrap();
        let err = outcome
            .error
            .as_ref()
            .expect("timed-out sample was published");
        assert_eq!(err.message, SCOPE_MESSAGE, "{err:?}");
        assert!(outcome.state.is_none(), "{outcome:?}");
        assert!(err.state.is_none());
        let rendered = format!("{outcome:?} {err:?}");
        assert!(!rendered.contains("Go"), "{rendered}");
        assert!(!rendered.contains("Allowed A"), "{rendered}");
        assert_eq!(session.delivered_snapshot_count(), before);
        if remove {
            let again = router
                .observe(
                    &session,
                    state_params("fixture.b", Some("w-b"), serde_json::json!(false)),
                )
                .await
                .expect("window B stays in scope");
            assert_eq!(again.state.snapshot_id, "s_1");
        }
    }
}

struct Unblock<'a> {
    clock: Option<&'a ParkClock>,
    gate: &'a gcu_core::action::PublishGate,
}

impl Drop for Unblock<'_> {
    fn drop(&mut self) {
        if let Some(clock) = self.clock.take() {
            clock.finish();
        }
        self.gate.release();
    }
}

fn invalidate_when_parked(
    gate: &gcu_core::action::PublishGate,
    lease: &gcu_core::scope::ScopeLease,
    remove: bool,
) {
    wait_until_parked(gate);
    if remove {
        lease.rebind(scope_only_b(2)).unwrap();
    } else {
        lease.revoke();
    }
    gate.release();
}

fn wait_until_parked(gate: &gcu_core::action::PublishGate) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !gate.is_parked() {
        if Instant::now() >= deadline {
            gate.release();
            panic!("publication did not park after policy load");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn state_params(app: &str, window: Option<&str>, shot: serde_json::Value) -> GetAppStateParams {
    let mut value = serde_json::json!({"app": app, "include_screenshot": shot});
    if let Some(window) = window {
        value["window"] = serde_json::json!(window);
    }
    serde_json::from_value(value).unwrap()
}

fn parked_router(world: &World, clock: Arc<ParkClock>) -> gcu_core::action::ActionRouter {
    gcu_core::action::ActionRouter::with_fixture_scope(
        world.spy.clone(),
        world.lease.clone(),
        world.activity.clone(),
        world.approvals.clone() as Arc<dyn ApprovalProvider>,
        Arc::new(MemoryPolicyStore::new(permissive())),
        Arc::new(RecordingEventSink::new()),
        gcu_core::session::WindowLocks::new(),
    )
    .with_settle_engine(SettleEngine::new(SettleConfig::default(), clock))
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
