//! ADR 0017 S1 round 5. Commit boundaries keep the originating epoch.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod fixture_scope_support;

use std::sync::Arc;
use std::time::Duration;

use fixture_scope_support::{permissive, scope_only_b, World, SCOPE_MESSAGE};
use gcu_core::action::{PublishGate, ToolCall};
use gcu_driver_fake::faults::Fault;
use gcu_protocol::tools::{
    BatchStep, ClickArgs, Condition, GetAppStateParams, MouseButton, WaitForArgs,
};
use gcu_protocol::ErrorCode;

#[tokio::test]
async fn publication_after_record_discards_snapshot_on_revoke_or_rebind() {
    for remove in [false, true] {
        let world = World::open(permissive());
        let before = world.session.delivered_snapshot_count();
        let gate = world.router.commit_gate();
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
async fn window_gone_rebind_during_refresh_drops_old_metadata() {
    for kind in [GonePath::Geometry, GonePath::Read, GonePath::Wait] {
        let world = World::open(permissive());
        match kind {
            GonePath::Geometry => {
                world
                    .spy
                    .inject(Fault::WindowGone {
                        window: "w-a".into(),
                    })
                    .await
                    .unwrap();
                world
                    .router
                    .observe(
                        &world.session,
                        state_params("fixture.b", Some("w-b"), serde_json::json!(false)),
                    )
                    .await
                    .expect("consuming the fault on B leaves B readable");
            }
            GonePath::Read => {
                world
                    .router
                    .observe(
                        &world.session,
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
            }
            GonePath::Wait => {
                // InstantClock can exhaust the wait horizon before another task
                // or thread runs. Inject inline in the first subscribed sample,
                // after authorization and before the wait can time out.
                world.spy.arm_read_fault_after_subscribe(Fault::WindowGone {
                    window: "w-a".into(),
                });
            }
        }
        let gate = world.router.refresh_gate();
        gate.arm();
        let subscribed = world.spy.external().subscribe;
        let release = std::thread::spawn({
            let gate = Arc::clone(&gate);
            let lease = world.lease.clone();
            move || {
                let _unblock = Unblock(&gate);
                wait_until_parked(&gate);
                lease.rebind(scope_only_b(2)).unwrap();
                gate.release();
            }
        });
        let outcome = if kind == GonePath::Wait {
            let mut call = ToolCall::new(
                "wait-gone",
                "fixture.a",
                BatchStep::WaitFor(WaitForArgs {
                    until: Condition::TextPresent("missing-token".into()),
                    timeout_ms: 8_000,
                }),
            );
            call.window = Some("w-a".into());
            call.settle_budget_ms = Some(0);
            world.router.execute(&world.session, call).await
        } else {
            let mut call = ToolCall::new("click-gone", "fixture.a", click_point());
            call.window = Some("w-a".into());
            call.settle_budget_ms = Some(0);
            world.router.execute(&world.session, call).await
        };
        release.join().unwrap();
        if kind == GonePath::Wait {
            assert_eq!(world.spy.external().subscribe, subscribed + 1);
        }
        let err = outcome.error.as_ref().expect("window gone");
        assert_eq!(err.code, ErrorCode::WindowGone, "{err:?}");
        assert!(outcome.state.is_none(), "{outcome:?}");
        assert!(outcome.windows.is_empty(), "{outcome:?}");
        let rendered = format!("{outcome:?} {err:?}");
        assert!(!rendered.contains("Allowed A"), "{rendered}");
        assert!(!rendered.contains("Fixture A"), "{rendered}");
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GonePath {
    Geometry,
    Read,
    Wait,
}

struct Unblock<'a>(&'a PublishGate);

impl Drop for Unblock<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}

fn wait_until_parked(gate: &PublishGate) {
    assert!(
        gate.wait_until_parked(Duration::from_secs(8)),
        "commit boundary did not park"
    );
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
