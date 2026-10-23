//! ADR 0017 S1 round 2. In-flight revocation, key-window admission, approval
//! generation, and batch state. Fake driver only.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod fixture_scope_support;

use std::future::poll_fn;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use fixture_scope_support::{
    allowed_scope, assert_clean, element_index, permissive, routed, scene_driver, HoldOp, Spy,
    World, SCOPE_MESSAGE,
};
use gcu_core::action::ToolCall;
use gcu_core::driver::{
    Cancel, Driver, DriverAction, DriverError, KeyChord, KeyRoute, TreeOpts, UiEvent, WindowRef,
};
use gcu_core::model::{NativeHandle, RawNode};
use gcu_core::scope::ScopeLease;
use gcu_core::session::Session;
use gcu_host_api::testing::{
    FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals,
};
use gcu_host_api::{
    ActionSummary, AppInfo as HostApp, ApprovalDecision, ApprovalProvider, Cancel as HostCancel,
    ConfirmationLevel, ForegroundPolicy, HandoffReason, ImageRef, ModelReason, Risk, SessionCtx,
};
use gcu_protocol::result::{Delivery, Outcome};
use gcu_protocol::tools::*;
use gcu_protocol::ErrorCode;
use serde_json::{json, Value};

#[tokio::test]
async fn reads_revoked_mid_call_drop_the_payload() {
    for op in [
        HoldOp::ListApps,
        HoldOp::ListWindows,
        HoldOp::Snapshot,
        HoldOp::Capture,
        HoldOp::Geometry,
        HoldOp::Focus,
    ] {
        let world = World::open(permissive());
        let driver = world.direct();
        let win = world.spy.real_window("fixture.a", "w-a").await;
        let err = revoke_during(&world, op, async move {
            match op {
                HoldOp::ListApps => driver.list_apps().await.map(|apps| format!("{apps:?}")),
                HoldOp::ListWindows => driver
                    .list_windows(Some(&win.app))
                    .await
                    .map(|windows| format!("{windows:?}")),
                HoldOp::Snapshot => driver
                    .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
                    .await
                    .map(|tree| format!("{tree:?}")),
                HoldOp::Capture => driver
                    .capture(&win, &Default::default(), &Cancel::default())
                    .await
                    .map(|shot| format!("{shot:?}")),
                HoldOp::Geometry => driver
                    .window_geometry(&win)
                    .await
                    .map(|geometry| format!("{geometry:?}")),
                HoldOp::Focus => driver
                    .focused_element(&win.app)
                    .await
                    .map(|found| format!("{found:?}")),
                HoldOp::Perform | HoldOp::BeforeOpen | HoldOp::Admit | HoldOp::MarkSeen => {
                    unreachable!("perform has its own tests")
                }
            }
        })
        .await;
        assert!(matches!(err, DriverError::ScopeDenied), "{op:?} {err:?}");
        assert_clean(&format!("{op:?}"), &err.to_string());
    }

    let world = World::open(permissive());
    let driver = world.direct();
    world.spy.arm_hold(HoldOp::ListApps, false);
    let spy = world.spy.clone();
    let lease = world.lease.clone();
    let join = tokio::spawn(async move { driver.list_apps().await });
    spy.wait_held().await;
    lease.rebind(allowed_scope(2)).unwrap();
    spy.release_hold();
    let err = join.await.unwrap().unwrap_err();
    assert!(matches!(err, DriverError::ScopeDenied), "{err:?}");
    assert_clean("rebind list", &err.to_string());
}

#[tokio::test]
async fn perform_revoked_before_delivery_releases_and_drops_the_action() {
    let world = World::open(permissive());
    let driver = world.direct();
    let win = world.spy.real_window("fixture.a", "w-a").await;
    let action = press_keys(win);
    let before = world.spy.external().perform;
    world.spy.arm_hold(HoldOp::Perform, false);
    let spy = world.spy.clone();
    let lease = world.lease.clone();
    let join = tokio::spawn(async move { driver.perform(&action, &Cancel::default()).await });
    spy.wait_held().await;
    lease.revoke();
    wait_until(|| spy.releases() > 0).await;
    assert_eq!(
        spy.external().perform,
        before,
        "delivery counted before release"
    );
    spy.release_hold();
    let err = join.await.unwrap().unwrap_err();
    assert!(matches!(err, DriverError::ScopeDenied), "{err:?}");
    assert_eq!(spy.external().perform, before);
    assert_clean("cancel perform", &err.to_string());
}

#[tokio::test]
async fn perform_revoked_after_start_reports_unknown() {
    let world = World::open(permissive());
    let seen = observe(&world, "fixture.a", Some("w-a"), json!(false)).await;
    let index = element_index(&seen.text, "Go");
    world.spy.arm_hold(HoldOp::Perform, true);
    let spy = world.spy.clone();
    let lease = world.lease.clone();
    let side = tokio::spawn(async move {
        spy.wait_held().await;
        lease.revoke();
        wait_until(|| spy.releases() > 0).await;
        spy.release_hold();
    });
    let outcome = act(&world, "fixture.a", Some("w-a"), click_index(index)).await;
    side.await.unwrap();
    assert_eq!(outcome.action.outcome, Outcome::Unknown, "{outcome:?}");
    let err = outcome.error.expect("started delivery returned success");
    assert_eq!(err.code, ErrorCode::DriverError, "{err:?}");
    assert_ne!(err.message, SCOPE_MESSAGE);
    assert!(world.spy.releases() > 0);
    assert!(world.spy.external().perform > 0);
    assert_clean("unknown", &format!("{err}"));
}

#[tokio::test]
async fn stop_beats_revocation_during_perform() {
    let world = World::open(permissive());
    let seen = observe(&world, "fixture.a", Some("w-a"), json!(false)).await;
    let index = element_index(&seen.text, "Go");
    world.spy.arm_hold(HoldOp::Perform, true);
    let spy_side = world.spy.clone();
    let lease_side = world.lease.clone();
    let activity = world.activity.clone();
    let side = tokio::spawn(async move {
        spy_side.wait_held().await;
        lease_side.revoke();
        wait_until(|| spy_side.releases() > 0).await;
        activity.request_stop();
        spy_side.release_hold();
    });
    let outcome = act(&world, "fixture.a", Some("w-a"), click_index(index)).await;
    side.await.unwrap();
    let err = outcome.error.expect("stop returned success");
    assert_eq!(err.code, ErrorCode::StoppedByUser, "{err:?}");
    assert_eq!(err.message, "stopped by user");
    assert_ne!(err.message, SCOPE_MESSAGE);
    assert!(world.spy.releases() > 0, "release was skipped");
}

#[tokio::test]
async fn subscribe_ends_when_the_lease_changes_or_stop_latches() {
    let world = World::open(permissive());
    world.spy.arm_events();
    let driver = world.direct();
    let win = world.spy.real_window("fixture.a", "w-a").await;
    let mut stream = driver.subscribe(&win).expect("admitted window subscribes");
    world.spy.push_event(UiEvent::Changed);
    assert_eq!(next_event(&mut stream).await, Some(UiEvent::Changed));
    world.lease.revoke();
    world.spy.push_event(UiEvent::GeometryChanged);
    assert_eq!(next_event(&mut stream).await, None);

    let world = World::open(permissive());
    world.spy.arm_events();
    let driver = world.direct();
    let win = world.spy.real_window("fixture.a", "w-a").await;
    let mut stream = driver.subscribe(&win).expect("admitted window subscribes");
    world.spy.push_event(UiEvent::Changed);
    world.activity.request_stop();
    assert_eq!(next_event(&mut stream).await, None);
}

#[tokio::test]
async fn implicit_window_paths_do_not_restore_an_unregistered_key_window() {
    let world = World::open(permissive());
    let app = world.spy.real_window("fixture.a", "w-a").await.app;
    let raised = world.spy.key_window(&app).await.unwrap();
    assert_eq!(raised.id, "w-a3");
    assert_eq!(raised.title, "CANARY-WINDOW-A3");
    assert_eq!(world.spy.restores(), 1);

    let world = World::open(permissive());
    let app = world.spy.real_window("fixture.a", "w-a").await.app;
    let info = world.direct().key_window(&app).await.unwrap();
    assert_eq!(info.id, "w-a");
    assert_ne!(info.title, "CANARY-WINDOW-A3");
    assert_eq!(world.spy.restores(), 0, "scoped key_window restored");

    let state = observe(&world, "fixture.a", None, json!(false)).await;
    assert!(state.text.contains("Go"), "{}", state.text);
    assert_clean("implicit state", &state.text);
    assert_eq!(world.spy.restores(), 0, "get_app_state restored");

    let index = element_index(&state.text, "Go");
    let outcome = act(&world, "fixture.a", None, click_index(index)).await;
    assert!(outcome.error.is_none(), "{outcome:?}");
    assert_eq!(world.spy.restores(), 0, "input restored");

    let waited = act(
        &world,
        "fixture.a",
        None,
        BatchStep::WaitFor(WaitForArgs {
            until: Condition::TextPresent("Go".into()),
            timeout_ms: 50,
        }),
    )
    .await;
    assert!(waited.error.is_none(), "{waited:?}");
    assert_eq!(world.spy.restores(), 0, "wait restored");
}

#[tokio::test]
async fn rebind_during_confirmation_does_not_deliver() {
    let foreground = confirmation_world(ForegroundPolicy::Ask, ConfirmationLevel::Off, false);
    foreground
        .approvals
        .push_decision(ApprovalDecision::Session);
    observe(&foreground, "fixture.a", Some("w-a"), json!(false)).await;
    let outcome = act(
        &foreground,
        "fixture.a",
        Some("w-a"),
        BatchStep::PressKey(PressKeyArgs { key: "a".into() }),
    )
    .await;
    let err = outcome.error.expect("foreground confirm delivered");
    assert_eq!(err.message, SCOPE_MESSAGE, "{err:?}");
    assert!(err.state.is_none() && outcome.state.is_none());
    assert_eq!(outcome.action.delivery, Delivery::None);
    assert_eq!(foreground.spy.external().perform, 0);
    assert_eq!(foreground.lease.current().unwrap().generation(), 2);

    let sensitive = confirmation_world(ForegroundPolicy::Always, ConfirmationLevel::Strict, true);
    sensitive.approvals.push_decision(ApprovalDecision::Session);
    let seen = observe(&sensitive, "fixture.a", Some("w-a"), json!(false)).await;
    let index = element_index(&seen.text, "Go");
    let outcome = act(&sensitive, "fixture.a", Some("w-a"), click_index(index)).await;
    let err = outcome.error.expect("sensitive confirm delivered");
    assert_eq!(err.message, SCOPE_MESSAGE, "{err:?}");
    assert!(err.state.is_none() && outcome.state.is_none());
    assert_eq!(sensitive.spy.external().perform, 0);
}

#[tokio::test]
async fn rebind_during_observation_approval_does_not_publish() {
    let activity = Arc::new(FakeUserActivity::new());
    let mut spy = Spy::new(scene_driver(), activity.clone());
    spy.set_targeted(true, true);
    let lease = ScopeLease::install(allowed_scope(1));
    let inner = Arc::new(ScriptedApprovals::new());
    for _ in 0..4 {
        inner.push_decision(ApprovalDecision::Session);
    }
    let router = routed(
        spy.clone(),
        lease.clone(),
        activity.clone(),
        Arc::new(RebindOnAsk {
            lease: lease.clone(),
            inner,
            armed: AtomicBool::new(true),
        }),
        Arc::new(MemoryPolicyStore::new(permissive())),
        Arc::new(RecordingEventSink::new()),
    );
    let session = Session::open("scope-rebind-observe", activity, router.input_release());
    session.begin_turn().unwrap();
    let err = router
        .observe(
            &session,
            state_params("fixture.a", Some("w-a"), json!(false)),
        )
        .await
        .unwrap_err();
    assert_eq!(err.message, SCOPE_MESSAGE);
    assert!(err.state.is_none());
    assert_eq!(lease.current().unwrap().generation(), 2);
    assert_eq!(spy.external().perform, 0);
    assert_clean("observe rebind", &err.to_string());
    let again = router
        .observe(
            &session,
            state_params("fixture.a", Some("w-a"), json!(false)),
        )
        .await
        .expect("the new generation is a live scope");
    assert!(again.text.contains("Go"), "{}", again.text);
    assert_clean("observe after rebind", &again.text);
    assert_eq!(spy.external().perform, 0);
}

#[tokio::test]
async fn scope_hard_stop_drops_retained_batch_state() {
    let world = World::open(permissive());
    let seen = observe(&world, "fixture.a", Some("w-a"), json!(false)).await;
    let index = element_index(&seen.text, "Go");
    world.spy.arm_flip();
    let report = world
        .router
        .run_batch(
            &world.session,
            "scope-batch",
            ComputerBatchParams {
                app: "fixture.a".into(),
                window: Some("w-a".into()),
                steps: vec![click_index(index), click_index(index)],
                stop_on_error: false,
                final_state: true,
            },
        )
        .await;
    let err = report
        .error
        .clone()
        .expect("scope batch admitted the second step");
    assert_eq!(err.message, SCOPE_MESSAGE);
    assert!(err.state.is_none(), "{err:?}");
    assert!(report.state.is_none(), "{report:?}");
    assert_clean("scope batch", &format!("{err} {report:?}"));

    let world = World::open(permissive());
    let seen = observe(&world, "fixture.a", Some("w-a"), json!(false)).await;
    let index = element_index(&seen.text, "Go");
    let report = world
        .router
        .run_batch(
            &world.session,
            "other-batch",
            ComputerBatchParams {
                app: "fixture.a".into(),
                window: Some("w-a".into()),
                steps: vec![
                    click_index(index),
                    BatchStep::WaitFor(WaitForArgs {
                        until: Condition::TextPresent("missing-token".into()),
                        timeout_ms: 1,
                    }),
                ],
                stop_on_error: false,
                final_state: true,
            },
        )
        .await;
    let err = report.error.expect("timeout kept going");
    assert_ne!(err.message, SCOPE_MESSAGE);
    assert!(report.state.is_some(), "non-scope hard stop dropped state");
}

#[tokio::test]
async fn scope_denial_inside_perform_is_refused() {
    let world = World::open(permissive());
    let seen = observe(&world, "fixture.a", Some("w-a"), json!(false)).await;
    let index = element_index(&seen.text, "Go");
    let before = world.spy.external().perform;
    world.spy.arm_deny_perform();
    let outcome = act(&world, "fixture.a", Some("w-a"), click_index(index)).await;
    assert_eq!(outcome.action.outcome, Outcome::Refused, "{outcome:?}");
    assert_eq!(outcome.action.delivery, Delivery::None);
    let err = outcome.error.expect("refused perform had no error");
    assert_eq!(err.message, SCOPE_MESSAGE);
    assert_eq!(world.spy.external().perform, before);
    assert_clean("refused", &format!("{err}"));
}

#[tokio::test]
async fn type_text_rejects_a_focus_handle_from_another_admitted_window() {
    let world = World::open(permissive());
    let driver = world.direct();
    let first = world.spy.real_window("fixture.a", "w-a").await;
    let second = world.spy.real_window("fixture.a", "w-a2").await;
    let left = driver
        .snapshot_tree(&first, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let right = driver
        .snapshot_tree(&second, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let focus_left = find_named(left.root.as_ref().unwrap(), "Go").unwrap();
    let focus_right = find_named(right.root.as_ref().unwrap(), "Go").unwrap();
    let before = world.spy.external().perform;
    let err = driver
        .perform(
            &type_text(first.clone(), Some(focus_right)),
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, DriverError::ScopeDenied), "{err:?}");
    assert_eq!(world.spy.external().perform, before);
    let reached = driver
        .perform(&type_text(first, Some(focus_left)), &Cancel::default())
        .await
        .unwrap_err();
    assert!(
        matches!(
            reached,
            DriverError::NotSettable {
                maybe_executed: false
            }
        ),
        "{reached:?}"
    );
    assert_eq!(world.spy.external().perform, before + 1);
}

#[tokio::test]
async fn capability_version_and_permission_tokens_are_allowlisted() {
    let activity = Arc::new(FakeUserActivity::new());
    let mut spy = Spy::new(scene_driver(), activity.clone());
    spy.set_version("CANARY-VERSION");
    let lease = ScopeLease::install(allowed_scope(1));
    let driver = gcu_core::scope::ScopedDriver::new(spy.clone(), lease.clone(), activity.clone());
    assert_eq!(driver.capabilities().version, "unlisted");
    assert_clean(
        "version",
        &serde_json::to_string(driver.capabilities()).unwrap(),
    );
    spy.set_version("fake-memory-v1 (no OS input)");
    let known = gcu_core::scope::ScopedDriver::new(spy.clone(), lease, activity);
    assert_eq!(known.capabilities().version, "fake-memory-v1 (no OS input)");

    let world = World::open(permissive());
    let driver = world.direct();
    let win = world.spy.real_window("fixture.a", "w-a").await;
    world.spy.arm_permission("zz");
    let err = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert!(matches!(err, DriverError::ScopeDenied), "{err:?}");
    assert!(!err.to_string().contains("zz"));
    world.spy.arm_permission("accessibility");
    let err = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    match err {
        DriverError::PermissionMissing { permission } => {
            assert_eq!(permission, "accessibility");
        }
        other => panic!("{other:?}"),
    }
}

async fn revoke_during<T: std::fmt::Debug + Send + 'static>(
    world: &World,
    op: HoldOp,
    call: impl std::future::Future<Output = Result<T, DriverError>> + Send + 'static,
) -> DriverError {
    world.spy.arm_hold(op, false);
    let spy = world.spy.clone();
    let lease = world.lease.clone();
    let join = tokio::spawn(call);
    spy.wait_held().await;
    lease.revoke();
    spy.release_hold();
    join.await.unwrap().unwrap_err()
}

async fn wait_until(mut ready: impl FnMut() -> bool) {
    for _ in 0..1_000 {
        if ready() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("condition was not met");
}

async fn next_event(stream: &mut gcu_core::driver::BoxStream<'static, UiEvent>) -> Option<UiEvent> {
    poll_fn(|cx| gcu_core::driver::Stream::poll_next(stream.as_mut(), cx)).await
}

fn press_keys(win: WindowRef) -> DriverAction {
    DriverAction::PressKeys {
        win,
        chord: KeyChord::parse("a").unwrap(),
        route: KeyRoute::TargetedOnly,
    }
}

fn type_text(win: WindowRef, focus: Option<NativeHandle>) -> DriverAction {
    DriverAction::TypeText {
        win,
        focus,
        text: "paste".into(),
        route: KeyRoute::TargetedOnly,
    }
}

fn confirmation_world(
    foreground: ForegroundPolicy,
    sensitive: ConfirmationLevel,
    targeted: bool,
) -> World {
    let activity = Arc::new(FakeUserActivity::new());
    let mut spy = Spy::new(scene_driver(), activity.clone());
    spy.set_targeted(targeted, targeted);
    let lease = ScopeLease::install(allowed_scope(1));
    let inner = Arc::new(ScriptedApprovals::new());
    for _ in 0..8 {
        inner.push_decision(ApprovalDecision::Session);
    }
    let mut config = permissive();
    config.allow_foreground = foreground;
    config.sensitive_action_confirmation = sensitive;
    let router = routed(
        spy.clone(),
        lease.clone(),
        activity.clone(),
        Arc::new(RebindOnConfirm {
            lease: lease.clone(),
            inner: inner.clone(),
        }),
        Arc::new(MemoryPolicyStore::new(config)),
        Arc::new(RecordingEventSink::new()),
    );
    let session = Session::open("scope-confirm", activity.clone(), router.input_release());
    session.begin_turn().unwrap();
    World {
        spy,
        lease,
        activity,
        approvals: inner,
        router,
        session,
    }
}

async fn observe(
    world: &World,
    app: &str,
    window: Option<&str>,
    shot: Value,
) -> gcu_core::action::ActionState {
    world
        .router
        .observe(&world.session, state_params(app, window, shot))
        .await
        .unwrap()
}

fn state_params(app: &str, window: Option<&str>, shot: Value) -> GetAppStateParams {
    let mut value = json!({"app": app, "include_screenshot": shot});
    if let Some(window) = window {
        value["window"] = json!(window);
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

fn find_named(node: &RawNode, name: &str) -> Option<NativeHandle> {
    if node.name.as_deref() == Some(name) {
        return Some(node.handle.clone());
    }
    node.children
        .iter()
        .find_map(|child| find_named(child, name))
}

struct RebindOnConfirm {
    lease: ScopeLease,
    inner: Arc<ScriptedApprovals>,
}

#[async_trait]
impl ApprovalProvider for RebindOnConfirm {
    async fn request_app(
        &self,
        session: &SessionCtx,
        app: &HostApp,
        reason: Option<&ModelReason>,
        risk: Risk,
        cancel: &HostCancel,
    ) -> ApprovalDecision {
        self.inner
            .request_app(session, app, reason, risk, cancel)
            .await
    }
    async fn confirm_action(
        &self,
        session: &SessionCtx,
        app: &HostApp,
        summary: &ActionSummary,
        crop: Option<&ImageRef>,
        cancel: &HostCancel,
    ) -> bool {
        self.lease.rebind(allowed_scope(2)).unwrap();
        let _ = (session, app, summary, crop, cancel);
        true
    }
    async fn handoff(
        &self,
        session: &SessionCtx,
        app: &HostApp,
        reason: HandoffReason,
        cancel: &HostCancel,
    ) {
        self.inner.handoff(session, app, reason, cancel).await;
    }
}

struct RebindOnAsk {
    lease: ScopeLease,
    inner: Arc<ScriptedApprovals>,
    armed: AtomicBool,
}

#[async_trait]
impl ApprovalProvider for RebindOnAsk {
    async fn request_app(
        &self,
        session: &SessionCtx,
        app: &HostApp,
        reason: Option<&ModelReason>,
        risk: Risk,
        cancel: &HostCancel,
    ) -> ApprovalDecision {
        if self.armed.swap(false, Ordering::SeqCst) {
            self.lease.rebind(allowed_scope(2)).unwrap();
        }
        self.inner
            .request_app(session, app, reason, risk, cancel)
            .await
    }
    async fn confirm_action(
        &self,
        session: &SessionCtx,
        app: &HostApp,
        summary: &ActionSummary,
        crop: Option<&ImageRef>,
        cancel: &HostCancel,
    ) -> bool {
        self.inner
            .confirm_action(session, app, summary, crop, cancel)
            .await
    }
    async fn handoff(
        &self,
        session: &SessionCtx,
        app: &HostApp,
        reason: HandoffReason,
        cancel: &HostCancel,
    ) {
        self.inner.handoff(session, app, reason, cancel).await;
    }
}
