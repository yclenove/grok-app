//! Stop latch, serial session calls, and the window lock.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use async_trait::async_trait;
use gcu_core::driver::NATIVE_TIMEOUT;
use gcu_core::session::{
    InputRelease, Session, SessionError, WindowLocks, NATIVE_CALL_TIMEOUT, RELEASE_INPUT_BUDGET,
    STOP_LATCH_BUDGET,
};
use gcu_host_api::testing::FakeUserActivity;
use gcu_host_api::UserActivity;
use gcu_protocol::ErrorCode;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

mod router_support;

#[tokio::test]
async fn queued_call_next_turn_reconnect_and_engine_restart_stay_stopped_until_user_resume() {
    use gcu_core::action::{ActionRouter, ToolCall};
    use router_support::{click, Harness};
    let h = Harness::new(true, true);
    let entered = Arc::new(tokio::sync::Notify::new());
    let unblock = Arc::new(tokio::sync::Notify::new());
    let holder = tokio::spawn({
        let session = h.session.clone();
        let entered = entered.clone();
        let unblock = unblock.clone();
        async move {
            session
                .enqueue(async move {
                    entered.notify_one();
                    unblock.notified().await;
                })
                .await
                .unwrap();
        }
    });
    entered.notified().await;
    // Poll once while the queue is held, proving the call arrived before Stop.
    let mut queued = Box::pin(h.execute(click(3)));
    std::future::poll_fn(|cx| {
        use std::future::Future;
        assert!(queued.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    h.activity.request_stop();
    h.session.turn_ended().await;
    unblock.notify_one();
    holder.await.unwrap();
    assert_eq!(queued.await.error.unwrap().code, ErrorCode::StoppedByUser);
    h.session.begin_turn().unwrap();
    assert_eq!(
        h.execute(click(3)).await.error.unwrap().code,
        ErrorCode::StoppedByUser
    );
    h.session.session_closed().await;
    let reconnected = Session::open("reconnected", h.activity.clone(), h.router.input_release());
    assert_eq!(
        h.router
            .execute(
                &reconnected,
                ToolCall::new("reconnect", "fake.form", click(3))
            )
            .await
            .error
            .unwrap()
            .code,
        ErrorCode::StoppedByUser
    );
    let restarted = ActionRouter::new(
        h.driver.clone(),
        h.approvals.clone(),
        h.policy.clone(),
        h.events.clone(),
        WindowLocks::new(),
    );
    let next = Session::open(
        "engine-restart",
        h.activity.clone(),
        restarted.input_release(),
    );
    assert_eq!(
        restarted
            .execute(&next, ToolCall::new("restart", "fake.form", click(3)))
            .await
            .error
            .unwrap()
            .code,
        ErrorCode::StoppedByUser
    );
    assert_eq!(h.driver.perform_calls(), 0);
    h.activity.clear_stop_by_user();
    next.begin_turn().unwrap();
    restarted
        .observe(
            &next,
            serde_json::from_value(serde_json::json!({"app":"fake.form"})).unwrap(),
        )
        .await
        .unwrap();
    let allowed = restarted
        .execute(&next, ToolCall::new("user-resumed", "fake.form", click(3)))
        .await;
    assert!(allowed.error.is_none());
    assert_eq!(h.driver.perform_calls(), 1);
}

struct CountingRelease {
    calls: AtomicUsize,
}

impl CountingRelease {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl InputRelease for CountingRelease {
    async fn release_all_input(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

struct SlowRelease {
    started: AtomicBool,
}

#[async_trait]
impl InputRelease for SlowRelease {
    async fn release_all_input(&self) {
        self.started.store(true, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

struct NopRelease;

#[async_trait]
impl InputRelease for NopRelease {
    async fn release_all_input(&self) {}
}

#[test]
fn native_call_timeout_matches_the_driver_ceiling() {
    assert_eq!(NATIVE_CALL_TIMEOUT, NATIVE_TIMEOUT);
    assert!(STOP_LATCH_BUDGET <= Duration::from_millis(100));
    assert!(RELEASE_INPUT_BUDGET <= Duration::from_millis(500));
    assert_eq!(NATIVE_CALL_TIMEOUT, Duration::from_secs(5));
}

#[tokio::test]
async fn stop_latch_survives_turn_reconnect_and_restart_until_user_clears() {
    let activity = Arc::new(FakeUserActivity::new());
    activity.request_stop();
    let release = Arc::new(CountingRelease::new());
    let first = Session::open("first", activity.clone(), release.clone());
    assert!(first.started_stopped());
    assert!(first.is_stopped());
    first.begin_turn().unwrap();
    first.turn_ended().await;
    assert!(first.is_stopped());
    assert!(activity.latched_stop());
    let second = Session::open("reconnect", activity.clone(), release.clone());
    assert!(second.started_stopped());
    assert!(second.is_stopped());
    activity.clear_stop_by_user();
    assert!(!first.is_stopped());
    assert!(first.started_stopped());
    let third = Session::open("restart", activity.clone(), release.clone());
    assert!(!third.started_stopped());
    assert!(!third.is_stopped());
    assert!(release.calls() >= 1);
}

#[tokio::test]
async fn stop_issues_release_within_budget_and_does_not_clear() {
    let activity = Arc::new(FakeUserActivity::new());
    let release = Arc::new(CountingRelease::new());
    let session = Session::open("s", activity.clone(), release.clone());
    let idle = session.handle_stop().await;
    assert!(idle.latch <= STOP_LATCH_BUDGET);
    assert_eq!(release.calls(), 0);
    activity.request_stop();
    let started = std::time::Instant::now();
    let timings = session.handle_stop().await;
    assert!(timings.latch <= STOP_LATCH_BUDGET);
    assert!(timings.release_issued <= RELEASE_INPUT_BUDGET);
    assert!(started.elapsed() <= RELEASE_INPUT_BUDGET);
    assert_eq!(release.calls(), 1);
    assert!(session.is_stopped());
    assert!(activity.latched_stop());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn native_release_over_5s_times_out_and_latch_stays() {
    let activity = Arc::new(FakeUserActivity::new());
    activity.request_stop();
    let release = Arc::new(SlowRelease {
        started: AtomicBool::new(false),
    });
    let session = Session::open("s", activity.clone(), release.clone());
    let timings = session.handle_stop().await;
    assert!(release.started.load(Ordering::SeqCst));
    assert!(timings.release_issued <= RELEASE_INPUT_BUDGET);
    assert!(session.is_stopped());
    assert!(activity.latched_stop());
}

#[tokio::test(flavor = "current_thread")]
async fn stop_does_not_wait_behind_the_session_queue() {
    let activity = Arc::new(FakeUserActivity::new());
    let release = Arc::new(CountingRelease::new());
    let session = Arc::new(Session::open("s", activity.clone(), release.clone()));
    let entered = Arc::new(tokio::sync::Notify::new());
    let gate = Arc::new(tokio::sync::Notify::new());
    let worker_session = session.clone();
    let worker_entered = entered.clone();
    let worker_gate = gate.clone();
    let worker = tokio::spawn(async move {
        worker_session
            .enqueue(async move {
                worker_entered.notify_one();
                worker_gate.notified().await;
            })
            .await
            .unwrap();
    });
    entered.notified().await;
    activity.request_stop();
    let timings = session.handle_stop().await;
    assert!(timings.release_issued <= RELEASE_INPUT_BUDGET);
    assert_eq!(release.calls(), 1);
    gate.notify_one();
    worker.await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn session_calls_run_one_at_a_time() {
    let session = Arc::new(Session::open(
        "s",
        Arc::new(FakeUserActivity::new()),
        Arc::new(NopRelease),
    ));
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));
    let first_session = session.clone();
    let first_order = order.clone();
    let first = tokio::spawn(async move {
        first_session
            .enqueue(async move {
                first_order.lock().unwrap().push(1);
                tokio::task::yield_now().await;
                first_order.lock().unwrap().push(2);
            })
            .await
            .unwrap();
    });
    tokio::task::yield_now().await;
    let second_session = session.clone();
    let second_order = order.clone();
    let second = tokio::spawn(async move {
        second_session
            .enqueue(async move {
                second_order.lock().unwrap().push(3);
            })
            .await
            .unwrap();
    });
    first.await.unwrap();
    second.await.unwrap();
    assert_eq!(*order.lock().unwrap(), vec![1, 2, 3]);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn window_lock_wait_over_5s_returns_busy() {
    let locks = WindowLocks::new();
    let _held = locks.acquire("window-1").await.unwrap();
    let waiting = locks.clone();
    let waiter = tokio::spawn(async move { waiting.acquire("window-1").await });
    let error = waiter.await.unwrap().unwrap_err();
    assert_eq!(error, SessionError::Busy);
    assert_eq!(error.code(), ErrorCode::Busy);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn two_sessions_waiting_on_one_window_return_busy() {
    let locks = WindowLocks::new();
    let first = Arc::new(Session::open(
        "a",
        Arc::new(FakeUserActivity::new()),
        Arc::new(NopRelease),
    ));
    let second = Arc::new(Session::open(
        "b",
        Arc::new(FakeUserActivity::new()),
        Arc::new(NopRelease),
    ));
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let holder = first.clone();
    let holder_locks = locks.clone();
    let holder_entered = entered.clone();
    let holder_release = release.clone();
    let held = tokio::spawn(async move {
        holder
            .enqueue_window(&holder_locks, "shared", async move {
                holder_entered.notify_one();
                holder_release.notified().await;
                1
            })
            .await
    });
    entered.notified().await;
    let waiter_locks = locks.clone();
    let waited = tokio::spawn(async move {
        second
            .enqueue_window(&waiter_locks, "shared", async { 2 })
            .await
    });
    let error = waited.await.unwrap().unwrap_err();
    assert_eq!(error, SessionError::Busy);
    release.notify_one();
    assert_eq!(held.await.unwrap().unwrap(), 1);
}
