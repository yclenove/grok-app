//! ADR 0017 S1 round 6. Indexing finishes before the snapshot is installed.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod fixture_scope_support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fixture_scope_support::{permissive, scope_only_b, World, SCOPE_MESSAGE};
use gcu_core::action::{PublishGate, ToolCall};
use gcu_driver_fake::faults::Fault;
use gcu_protocol::tools::{
    BatchStep, ClickArgs, Condition, GetAppStateParams, MouseButton, WaitForArgs,
};
use gcu_protocol::ErrorCode;

#[tokio::test]
async fn index_finished_revoke_before_commit_returns_no_snapshot() {
    for remove in [false, true] {
        let world = World::open(permissive());
        let before = world.session.delivered_snapshot_count();
        let gate = world.session.index_gate();
        gate.arm();
        let release = std::thread::spawn({
            let gate = Arc::clone(&gate);
            let lease = world.lease.clone();
            move || {
                let _unblock = Unblock(&gate);
                wait_until_parked(&gate);
                if remove {
                    lease.rebind(scope_only_b(2)).unwrap();
                } else {
                    lease.revoke();
                }
                gate.release();
            }
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
        let rendered = format!("{err:?}");
        assert!(!rendered.contains("Go"), "{rendered}");
        assert!(!rendered.contains("Allowed A"), "{rendered}");
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
            assert!(!again.text.contains("Allowed A"), "{}", again.text);
            assert!(again.text.contains("Allowed B"), "{}", again.text);
            assert_ne!(again.state.snapshot_id, "s_1");
        }
    }
}

#[tokio::test]
async fn wait_index_finished_revoke_returns_no_snapshot_for_ready_or_timeout() {
    for matching in [true, false] {
        let world = World::open(permissive());
        let before = world.session.delivered_snapshot_count();
        let gate = world.session.index_gate();
        gate.arm();
        let release = std::thread::spawn({
            let gate = Arc::clone(&gate);
            let lease = world.lease.clone();
            move || {
                let _unblock = Unblock(&gate);
                wait_until_parked(&gate);
                lease.revoke();
            }
        });
        let mut call = ToolCall::new(
            "wait-commit",
            "fixture.a",
            BatchStep::WaitFor(WaitForArgs {
                until: Condition::TextPresent(if matching { "Go" } else { "missing-token" }.into()),
                timeout_ms: 200,
            }),
        );
        call.window = Some("w-a".into());
        let outcome = world.router.execute(&world.session, call).await;
        release.join().unwrap();
        let error = outcome.error.as_ref().expect("sample must be refused");
        assert_eq!(error.message, SCOPE_MESSAGE, "{outcome:?}");
        assert!(error.state.is_none());
        assert!(outcome.state.is_none());
        assert!(outcome.windows.is_empty());
        assert_eq!(world.session.delivered_snapshot_count(), before);
    }
}

#[tokio::test]
async fn revoke_during_snapshot_commit_waits_and_keeps_the_sample() {
    let world = World::open(permissive());
    let before = world.session.delivered_snapshot_count();
    let gate = world.lease.commit_during_gate();
    gate.arm();
    let done = Arc::new(AtomicBool::new(false));
    let release = std::thread::spawn({
        let gate = Arc::clone(&gate);
        let lease = world.lease.clone();
        let done = Arc::clone(&done);
        move || {
            let unblock = Unblock(&gate);
            wait_until_parked(&gate);
            let revoked = Arc::clone(&done);
            let revoker = std::thread::spawn(move || {
                lease.revoke();
                revoked.store(true, Ordering::SeqCst);
            });
            std::thread::sleep(Duration::from_millis(200));
            assert!(
                !done.load(Ordering::SeqCst),
                "revoke interleaved the snapshot commit"
            );
            drop(unblock);
            revoker.join().unwrap();
        }
    });
    let state = world
        .router
        .observe(
            &world.session,
            state_params("fixture.a", Some("w-a"), serde_json::json!(false)),
        )
        .await
        .expect("registration finished before revoke");
    release.join().unwrap();
    assert!(state.text.contains("Allowed A"), "{}", state.text);
    assert_eq!(world.session.delivered_snapshot_count(), before + 1);
}

#[tokio::test]
async fn revoke_after_commit_does_not_roll_back_the_snapshot() {
    let world = World::open(permissive());
    let before = world.session.delivered_snapshot_count();
    let gate = world.lease.commit_after_gate();
    gate.arm();
    let release = std::thread::spawn({
        let gate = Arc::clone(&gate);
        let lease = world.lease.clone();
        move || {
            let _unblock = Unblock(&gate);
            wait_until_parked(&gate);
            lease.revoke();
            gate.release();
        }
    });
    let state = world
        .router
        .observe(
            &world.session,
            state_params("fixture.a", Some("w-a"), serde_json::json!(false)),
        )
        .await
        .expect("a committed snapshot stays published");
    release.join().unwrap();
    assert!(state.text.contains("Allowed A"), "{}", state.text);
    assert_eq!(state.state.snapshot_id, "s_1");
    assert_eq!(world.session.delivered_snapshot_count(), before + 1);
}

#[tokio::test]
async fn window_gone_revoke_while_snapshot_gate_waits_drops_metadata() {
    let world = World::open(permissive());
    let session = Arc::new(world.session);
    world
        .router
        .observe(
            &session,
            state_params("fixture.a", Some("w-a"), serde_json::json!(true)),
        )
        .await
        .expect("A is readable before the fault");
    world
        .spy
        .inject(Fault::WindowGone {
            window: "w-a".into(),
        })
        .await
        .unwrap();
    let gate = session.snapshot_gate();
    gate.arm();
    let release = std::thread::spawn({
        let gate = Arc::clone(&gate);
        let lease = world.lease.clone();
        let session = Arc::clone(&session);
        move || {
            let _unblock = Unblock(&gate);
            wait_until_parked(&gate);
            // Hold the actual snapshot mutex on another thread. Let the
            // fallback leave its gate, then revoke before it can acquire it.
            let locked = session.delivery_gate();
            locked.arm();
            let unlock = Unblock(&locked);
            let holder = std::thread::spawn(move || {
                let _delivery =
                    session.prepare_screenshot_delivery("fixture.a", "w-a", "s_1", 200, 120);
            });
            wait_until_parked(&locked);
            gate.release();
            let deadline = Instant::now() + Duration::from_secs(8);
            while gate.is_parked() {
                assert!(Instant::now() < deadline, "fallback did not leave its gate");
                std::thread::sleep(Duration::from_millis(1));
            }
            lease.revoke();
            drop(unlock);
            holder.join().unwrap();
        }
    });
    let mut call = ToolCall::new("click-gone", "fixture.a", click_point());
    call.window = Some("w-a".into());
    call.settle_budget_ms = Some(0);
    let outcome = world.router.execute(&session, call).await;
    release.join().unwrap();
    let err = outcome.error.as_ref().expect("window gone");
    assert_eq!(err.code, ErrorCode::WindowGone, "{err:?}");
    assert!(outcome.state.is_none(), "{outcome:?}");
    assert!(outcome.windows.is_empty(), "{outcome:?}");
    let rendered = format!("{outcome:?} {err:?}");
    assert!(!rendered.contains("Allowed A"), "{rendered}");
    assert!(!rendered.contains("Fixture A"), "{rendered}");
}

struct Unblock<'a>(&'a PublishGate);

impl Drop for Unblock<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}

fn wait_until_parked(gate: &PublishGate) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !gate.is_parked() {
        if Instant::now() >= deadline {
            gate.release();
            panic!("commit boundary did not park");
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

fn click_point() -> BatchStep {
    BatchStep::Click(ClickArgs {
        element_index: None,
        x: Some(10.0),
        y: Some(10.0),
        screenshot_id: None,
        click_count: 1,
        mouse_button: MouseButton::Left,
    })
}
