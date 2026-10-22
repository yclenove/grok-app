//! ADR 0017 S1 acceptance. Fake driver only; real OS identity and the private
//! socket stay not_run on later cards.
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

mod fixture_scope_support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use fixture_scope_support::{
    allowed_scope, assert_clean, catalog_names, element_index, forbidden_actions, permissive,
    process, routed, scene_driver, scope, scope_only_b, script_source_mentions_scope, Spy, World,
    SCOPE_MESSAGE,
};
use gcu_core::action::ToolCall;
use gcu_core::browser::{BrowserHub, DomChannel, DomEffect, DomFault, DomOperation};
use gcu_core::driver::{Cancel, Driver, DriverError, TreeOpts};
use gcu_core::model::RawNode;
use gcu_core::scope::{
    BootIdentity, ExecutableProof, FixtureScope, OsStartTime, ScopeError, ScopeLease,
    WindowBinding, SCOPE_DENIED_MESSAGE,
};
use gcu_core::session::Session;
use gcu_host_api::testing::{
    FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals,
};
use gcu_host_api::{
    ActionSummary, AppInfo, ApprovalDecision, ApprovalProvider, Cancel as HostCancel,
    ConfirmationLevel, ForegroundPolicy, HandoffReason, ImageRef, ModelReason, Risk, SessionCtx,
};
use gcu_protocol::result::{Effect, Outcome};
use gcu_protocol::tools::*;
use gcu_protocol::{ErrorCode, ToolError};
use serde_json::{json, Value};

#[test]
fn scope_denial_message_matches_driver_display() {
    assert_eq!(DriverError::ScopeDenied.to_string(), SCOPE_DENIED_MESSAGE);
    assert_eq!(SCOPE_MESSAGE, SCOPE_DENIED_MESSAGE);
    assert_eq!(DriverError::ScopeDenied.error_code(), ErrorCode::AppDenied);
    assert!(!DriverError::ScopeDenied.maybe_executed());
}

#[test]
fn incomplete_scope_records_are_rejected() {
    assert_eq!(OsStartTime::new(0), Err(ScopeError::Incomplete));
    assert_eq!(BootIdentity::new(""), Err(ScopeError::Incomplete));
    let start = OsStartTime::new(1).unwrap();
    let boot = BootIdentity::new("boot-1").unwrap();
    assert_eq!(
        gcu_core::scope::ProcessInstance::new(0, start, boot.clone()),
        Err(ScopeError::Incomplete)
    );
    let owner = process(1101, 10);
    assert!(WindowBinding::new("", 1, owner.clone()).is_err());
    assert!(WindowBinding::new("w", 0, owner.clone()).is_err());
    assert!(ExecutableProof::new("").is_err());
    assert!(FixtureScope::try_new("", 1, "e", "d", "c", vec![]).is_err());
    assert!(FixtureScope::try_new("r", 0, "e", "d", "c", vec![]).is_err());
    let lease = ScopeLease::install(allowed_scope(1));
    assert_eq!(
        lease.rebind(allowed_scope(1)),
        Err(ScopeError::StaleGeneration)
    );
    assert_eq!(
        lease.rebind(scope(2, "other-run", vec![])),
        Err(ScopeError::RunMismatch)
    );
    lease.revoke();
    assert!(lease.current().is_none());
    assert_eq!(lease.rebind(allowed_scope(3)), Err(ScopeError::Revoked));
}

#[test]
fn catalog_profiles_have_no_scope_setter() {
    let source = include_str!("../../gcu-mcp/src/catalog/profiles.rs");
    assert!(!source.contains("set_fixture_scope"));
    assert!(!source.contains("set_scope"));
    let names = catalog_names();
    assert_eq!(names.len(), 16, "{names:?}");
    for name in &names {
        assert!(
            matches!(
                name.as_str(),
                "list_apps"
                    | "list_windows"
                    | "launch_app"
                    | "get_app_state"
                    | "click"
                    | "perform_secondary_action"
                    | "set_value"
                    | "select_text"
                    | "type_text"
                    | "press_key"
                    | "scroll"
                    | "drag"
                    | "wait_for"
                    | "computer_batch"
                    | "computer_script"
                    | "capabilities"
            ),
            "unmapped catalog tool {name}"
        );
    }
    assert!(script_source_mentions_scope());
}

#[tokio::test]
async fn catalog_calls_on_external_targets_do_not_touch_content() {
    let world = World::open(permissive());
    let before = world.spy.external();
    for name in catalog_names() {
        for target in [Target::External, Target::Impostor, Target::Forged] {
            exercise(&world, &name, target).await;
        }
    }
    assert_eq!(
        world.spy.external(),
        before,
        "external content, capture, delivery, or launch"
    );
    assert_eq!(world.spy.windows_none(), 0);
    assert_eq!(world.spy.resolve_calls(), 0);
}

#[tokio::test]
async fn discovery_hides_external_canaries_and_lists_only_admitted_windows() {
    let world = World::open(permissive());
    let apps = world.router.list_apps(&world.session).await.unwrap();
    let dumped = serde_json::to_string(&apps).unwrap();
    assert_clean("list_apps", &dumped);
    let ids: Vec<_> = apps.iter().map(|app| app.id.as_str()).collect();
    assert_eq!(ids, ["fixture.a", "fixture.b"]);
    let windows = world
        .router
        .list_windows(&world.session, None)
        .await
        .unwrap();
    let dumped = serde_json::to_string(&windows).unwrap();
    assert_clean("list_windows", &dumped);
    let ids: Vec<_> = windows.iter().map(|win| win.id.as_str()).collect();
    assert_eq!(ids, ["w-a", "w-a2", "w-b"]);
    assert_eq!(world.spy.windows_none(), 0, "desktop list was forwarded");
    let state = observe(&world, "Fixture A", None, json!(false)).await;
    assert_eq!(state.state.app.id, "fixture.a");
    assert_clean("named resolve", &state.text);
}

#[tokio::test]
async fn missing_external_and_impostor_errors_match() {
    let world = World::open(permissive());
    let missing = denied_windows(&world, "missing-app").await;
    let external = denied_windows(&world, "CANARY-EXTERNAL-C").await;
    let impostor = denied_windows(&world, "fixture.d").await;
    assert_eq!(missing, external);
    assert_eq!(external, impostor);
    assert!(missing.state.is_none());
    assert_eq!(missing.message, SCOPE_MESSAGE);
    for shot in [json!(false), json!(true), json!("auto")] {
        let err = observe_err(&world, "CANARY-EXTERNAL-C", None, shot).await;
        assert_eq!(err, missing);
    }
}

#[tokio::test]
async fn allowed_windows_observe_and_click_and_launch_stays_closed() {
    let world = World::open(permissive());
    let before = world.spy.external();
    // The first observation of a window is the full tree. A later read of the
    // same window is a diff and does not repeat every label.
    let seen = observe(&world, "fixture.a", Some("w-a"), json!(true)).await;
    assert_clean("w-a", &seen.text);
    assert!(seen.text.contains("Go"), "w-a text {}", seen.text);
    assert!(
        seen.state.screenshot.is_some(),
        "allowed capture was dropped"
    );
    assert!(world.spy.external().capture > before.capture);
    let index = element_index(&seen.text, "Go");
    for (app, window) in [("fixture.a", "w-a2"), ("fixture.b", "w-b")] {
        let state = observe(&world, app, Some(window), json!(false)).await;
        assert_clean(window, &state.text);
        assert!(state.text.contains("Go"), "{window} text {}", state.text);
    }
    let outcome = act(&world, "fixture.a", Some("w-a"), click_index(index)).await;
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert_eq!(
        outcome.action.outcome,
        Outcome::Done,
        "{:?}",
        outcome.action
    );
    assert_eq!(world.spy.external().perform, before.perform + 1);
    for app in ["fixture.a", "CANARY-EXTERNAL-C"] {
        let launched = world
            .router
            .launch_app(
                &world.session,
                "launch",
                LaunchAppParams {
                    app: app.into(),
                    reason: None,
                },
            )
            .await;
        let err = launched.error.unwrap();
        assert_eq!(err.message, SCOPE_MESSAGE, "{app}");
        assert!(err.state.is_none(), "{app}");
        assert_clean(app, &err.to_string());
    }
    assert_eq!(world.spy.external().launch, 0);
    let blocked = observe_err(&world, "fixture.a", Some("w-a3"), json!(false)).await;
    assert_eq!(blocked.message, SCOPE_MESSAGE);
    assert_eq!(world.spy.resolve_calls(), 0);
}

#[tokio::test]
async fn batch_mid_refusal_does_not_run_later_steps() {
    let world = World::open(permissive());
    let seen = observe(&world, "fixture.a", Some("w-a"), json!(false)).await;
    let index = element_index(&seen.text, "Go");
    world.spy.arm_flip();
    let report = world
        .router
        .run_batch(
            &world.session,
            "batch",
            ComputerBatchParams {
                app: "fixture.a".into(),
                window: Some("w-a".into()),
                steps: vec![
                    click_index(index),
                    click_index(index),
                    BatchStep::TypeText(TypeTextArgs {
                        text: "later".into(),
                        element_index: None,
                    }),
                ],
                stop_on_error: false,
                final_state: false,
            },
        )
        .await;
    assert_eq!(report.failed_step, Some(1), "{report:?}");
    assert_eq!(report.steps.len(), 3, "{report:?}");
    assert_eq!(report.steps[2].effect, Effect::NotChecked, "{report:?}");
    let err = report.error.unwrap();
    assert_eq!(err.message, SCOPE_MESSAGE);
    assert!(err.state.is_none());
    assert_eq!(
        world.spy.external().perform,
        1,
        "later step delivered input"
    );
    assert_eq!(
        world.spy.lists_after_flip(),
        1,
        "a step after the refusal resolved again"
    );
    assert_eq!(world.spy.external().launch, 0);
}

#[tokio::test]
async fn identity_races_refuse_before_content() {
    let world = World::open(permissive());
    let driver = world.direct();
    let win = world.spy.real_window("fixture.a", "w-a").await;
    let before = world.spy.external();
    world.spy.set_incarnation("w-a", 7);
    let err = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert!(matches!(err, DriverError::ScopeDenied));
    world.spy.shift_start("fixture.a", 50);
    let err = observe_err(&world, "fixture.a", Some("w-a"), json!(false)).await;
    assert_eq!(err.message, SCOPE_MESSAGE);
    assert_eq!(world.spy.external(), before);

    let world = World::open(permissive());
    let driver = world.direct();
    let win = world.spy.real_window("fixture.a", "w-a").await;
    let tree = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let handle = find_named(tree.root.as_ref().unwrap(), "Go").unwrap();
    let before = world.spy.external();
    world.lease.rebind(allowed_scope(2)).unwrap();
    let err = driver
        .perform(
            &gcu_core::driver::DriverAction::Press { el: handle },
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, DriverError::ScopeDenied));
    assert_eq!(world.spy.external().perform, before.perform);
    world.lease.rebind(scope_only_b(3)).unwrap();
    let err = driver
        .capture(
            &win,
            &gcu_core::driver::CaptureOpts::default(),
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, DriverError::ScopeDenied));
    assert_eq!(world.spy.external().capture, before.capture);

    let world = World::open(permissive());
    let driver = world.direct();
    let win = world.spy.real_window("fixture.a", "w-a").await;
    let tree = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let handle = find_named(tree.root.as_ref().unwrap(), "Go").unwrap();
    let before = world.spy.external();
    world.spy.retarget_handles("w-a", "w-c");
    let err = driver
        .perform(
            &gcu_core::driver::DriverAction::Press { el: handle },
            &Cancel::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, DriverError::ScopeDenied));
    assert_eq!(world.spy.external(), before);
}

#[tokio::test]
async fn forged_and_cross_target_actions_do_not_deliver() {
    let world = World::open(permissive());
    let driver = world.direct();
    let external = world.spy.real_window("fixture.c", "w-c").await;
    let impostor = world.spy.real_window("fixture.d", "w-d").await;
    let (_, raw) = world.spy.raw_handle("fixture.c", "w-c", "go").await;
    let forged = gcu_core::model::NativeHandle::new("forged");
    let before = world.spy.external();
    for win in [&external, &impostor] {
        for action in forbidden_actions(win, raw.clone()) {
            let err = driver
                .perform(&action, &Cancel::default())
                .await
                .unwrap_err();
            assert!(matches!(err, DriverError::ScopeDenied), "{err:?}");
        }
        for action in forbidden_actions(win, forged.clone()) {
            let err = driver
                .perform(&action, &Cancel::default())
                .await
                .unwrap_err();
            assert!(matches!(err, DriverError::ScopeDenied), "{err:?}");
        }
    }
    let allowed = world.spy.real_window("fixture.a", "w-a").await;
    let mut forged_win = allowed.clone();
    forged_win.id = "forged-window".into();
    for action in forbidden_actions(&forged_win, forged) {
        let err = driver
            .perform(&action, &Cancel::default())
            .await
            .unwrap_err();
        assert!(matches!(err, DriverError::ScopeDenied), "{err:?}");
    }
    assert_eq!(world.spy.external(), before);
    assert!(driver.subscribe(&external).is_none());
    assert_eq!(world.spy.external().subscribe, before.subscribe);
    let focused = driver.focused_element(&external.app).await.unwrap_err();
    assert!(matches!(focused, DriverError::ScopeDenied));
    assert_eq!(world.spy.external().focus, before.focus);
}

#[tokio::test]
async fn delivery_that_changes_identity_reports_uncertainty() {
    let world = World::open(permissive());
    let seen = observe(&world, "fixture.a", Some("w-a"), json!(false)).await;
    let index = element_index(&seen.text, "Go");
    world.spy.arm_change_during_perform();
    let outcome = act(&world, "fixture.a", Some("w-a"), click_index(index)).await;
    assert_eq!(
        outcome.action.outcome,
        Outcome::Unknown,
        "{:?}",
        outcome.action
    );
    assert_eq!(world.spy.external().perform, 1);
    let err = outcome.error.unwrap();
    assert_eq!(err.code, ErrorCode::DriverError);
    assert_ne!(err.message, SCOPE_MESSAGE);
    assert_clean("identity change", &format!("{err:?} {:?}", outcome.action));

    let world = World::open(permissive());
    let seen = observe(&world, "fixture.a", Some("w-a"), json!(false)).await;
    let index = element_index(&seen.text, "Go");
    world.spy.arm_change_during_perform();
    world.spy.arm_stop_during_perform();
    let outcome = act(&world, "fixture.a", Some("w-a"), click_index(index)).await;
    assert_eq!(
        outcome.action.outcome,
        Outcome::Unknown,
        "{:?}",
        outcome.action
    );
    assert_eq!(world.spy.external().perform, 1);
    let err = outcome.error.unwrap();
    assert_eq!(err.code, ErrorCode::StoppedByUser, "{err:?}");
    assert_ne!(err.message, SCOPE_MESSAGE);
}

#[tokio::test]
async fn stop_latch_wins_and_release_still_runs() {
    let world = World::open(permissive());
    let before = world.spy.metadata();
    world.activity.request_stop();
    let err = world.router.list_apps(&world.session).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::StoppedByUser);
    assert_eq!(err.message, "stopped by user");
    assert_ne!(err.message, SCOPE_MESSAGE);
    assert_eq!(world.spy.metadata(), before);
    let driver = world.direct();
    let released = world.spy.releases();
    driver.release_all_input().await;
    assert_eq!(world.spy.releases(), released + 1);
    let err = driver.list_apps().await.unwrap_err();
    assert!(matches!(
        err,
        DriverError::Cancelled {
            maybe_executed: false
        }
    ));
}

#[tokio::test]
async fn revoked_and_empty_scopes_do_not_publish_the_desktop() {
    let world = World::open(permissive());
    let before = world.spy.metadata();
    world.lease.revoke();
    let err = world.router.list_apps(&world.session).await.unwrap_err();
    assert_eq!(err.message, SCOPE_MESSAGE);
    assert!(err.state.is_none());
    assert_eq!(
        world.spy.metadata(),
        before,
        "revoked scope still listed apps"
    );

    let world = World::open(permissive());
    world.lease.rebind(scope(2, "run-s1", vec![])).unwrap();
    let apps = world.router.list_apps(&world.session).await.unwrap();
    assert!(apps.is_empty(), "{apps:?}");
    assert_clean("empty", &serde_json::to_string(&apps).unwrap());
    assert_eq!(world.spy.external().content, 0);
}

#[tokio::test]
async fn doctor_and_capabilities_drop_free_form_canaries() {
    let world = World::open(permissive());
    let driver = world.direct();
    let report = driver.doctor().await;
    assert_eq!(
        report.issues,
        [
            "permission missing: accessibility".to_owned(),
            "permission missing: screen_recording".to_owned(),
        ]
    );
    assert!(driver.capabilities().private_apis.is_empty());
    assert!(world
        .router
        .bound_driver()
        .capabilities()
        .private_apis
        .is_empty());
    let dumped = format!(
        "{report:?} {}",
        serde_json::to_string(driver.capabilities()).unwrap()
    );
    assert_clean("doctor", &dumped);
    world.activity.request_stop();
    let stopped = driver.doctor().await;
    assert!(!stopped.ready);
    assert!(stopped.issues.is_empty());
    assert_clean("stopped doctor", &format!("{stopped:?}"));
}

#[tokio::test]
async fn existing_policy_refusals_still_block_delivery() {
    let mut denied = permissive();
    denied.extra_denied_apps = vec!["fixture.a".into()];
    let world = World::open(denied);
    let err = observe_err(&world, "fixture.a", Some("w-a"), json!(false)).await;
    assert_eq!(err.message, "app denied");
    assert_ne!(err.message, SCOPE_MESSAGE);
    assert_eq!(world.spy.external().perform, 0);

    let mut strict = permissive();
    strict.sensitive_action_confirmation = ConfirmationLevel::Strict;
    let world = World::open(strict);
    world.approvals.push_confirm(false);
    let seen = observe(&world, "fixture.a", Some("w-a"), json!(false)).await;
    let index = element_index(&seen.text, "Go");
    let outcome = act(&world, "fixture.a", Some("w-a"), click_index(index)).await;
    let err = outcome.error.unwrap();
    assert_eq!(err.code, ErrorCode::ApprovalDenied, "{err:?}");
    assert_eq!(world.spy.external().perform, 0);

    let mut never = permissive();
    never.allow_foreground = ForegroundPolicy::Never;
    let world = World::open_targeted(never, false);
    observe(&world, "fixture.a", Some("w-a"), json!(true)).await;
    let outcome = act(&world, "fixture.a", Some("w-a"), click_point(20.0, 40.0)).await;
    let err = outcome.error.unwrap();
    assert_eq!(err.code, ErrorCode::CapabilityUnavailable, "{err:?}");
    assert_eq!(world.spy.external().perform, 0);

    let world = World::open_targeted(permissive(), false);
    world
        .activity
        .set_last_physical_input(std::time::Instant::now());
    observe(&world, "fixture.a", Some("w-a"), json!(true)).await;
    let outcome = act(&world, "fixture.a", Some("w-a"), click_point(20.0, 40.0)).await;
    let err = outcome.error.unwrap();
    assert_eq!(err.code, ErrorCode::UserActive, "{err:?}");
    assert_eq!(world.spy.external().perform, 0);
}

#[tokio::test]
async fn revoke_during_approval_does_not_deliver() {
    let activity = Arc::new(FakeUserActivity::new());
    let mut spy = Spy::new(scene_driver(), activity.clone());
    spy.set_targeted(true, true);
    let lease = ScopeLease::install(allowed_scope(1));
    let inner = Arc::new(ScriptedApprovals::new());
    inner.push_decision(ApprovalDecision::Session);
    let approvals = Arc::new(RevokeOnAsk {
        lease: lease.clone(),
        inner,
    });
    let router = routed(
        spy.clone(),
        lease.clone(),
        activity.clone(),
        approvals,
        Arc::new(MemoryPolicyStore::new(permissive())),
        Arc::new(RecordingEventSink::new()),
    );
    let session = Session::open("scope-revoke", activity, router.input_release());
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
    assert!(lease.current().is_none());
    let content = spy.external().content;
    assert!(
        content > 0,
        "authorization revoked before the first in-scope read"
    );
    assert_eq!(spy.external().perform, 0);
    assert_eq!(spy.external().capture, 0);
    assert_eq!(spy.external().launch, 0);
    assert_eq!(spy.resolve_calls(), 0);
    let again = router
        .observe(
            &session,
            state_params("fixture.a", Some("w-a"), json!(false)),
        )
        .await
        .unwrap_err();
    assert_eq!(again.message, SCOPE_MESSAGE);
    assert!(again.state.is_none());
    assert_eq!(spy.external().content, content, "revoked scope read again");
    assert_eq!(spy.external().perform, 0);
    let mut call = ToolCall::new("act", "fixture.a", click_index(1));
    call.window = Some("w-a".into());
    call.settle_budget_ms = Some(0);
    let outcome = router.execute(&session, call).await;
    let click = outcome.error.expect("revoked scope delivered a click");
    assert_eq!(click.message, SCOPE_MESSAGE);
    assert!(click.state.is_none() && outcome.state.is_none());
    assert_eq!(spy.external().perform, 0);
    assert_clean("revoke", &format!("{err} {again} {click}"));
}

#[tokio::test]
async fn browser_hub_is_ignored_when_the_router_is_scoped() {
    let activity = Arc::new(FakeUserActivity::new());
    let mut spy = Spy::new(scene_driver(), activity.clone());
    spy.set_targeted(true, true);
    let approvals = Arc::new(ScriptedApprovals::new());
    for _ in 0..64 {
        approvals.push_decision(ApprovalDecision::Session);
    }
    let dom = Arc::new(FakeDom::new());
    let hub = Arc::new(BrowserHub::new(dom.clone() as Arc<dyn DomChannel>));
    let router = routed(
        spy.clone(),
        ScopeLease::install(allowed_scope(1)),
        activity.clone(),
        approvals,
        Arc::new(MemoryPolicyStore::new(permissive())),
        Arc::new(RecordingEventSink::new()),
    )
    .with_browser(hub);
    let session = Session::open("scope-browser", activity, router.input_release());
    session.begin_turn().unwrap();
    let seen = router
        .observe(
            &session,
            state_params("fixture.a", Some("w-a"), json!(false)),
        )
        .await
        .unwrap();
    let index = element_index(&seen.text, "Go");
    let mut call = ToolCall::new("act", "fixture.a", click_index(index));
    call.window = Some("w-a".into());
    call.settle_budget_ms = Some(0);
    let outcome = router.execute(&session, call).await;
    assert!(outcome.error.is_none(), "{outcome:?}");
    assert_eq!(
        dom.performs(),
        0,
        "scoped router delivered through the DOM hub"
    );
    assert!(
        spy.external().perform > 0,
        "allowed click never reached the driver"
    );
    assert_clean("browser", &format!("{seen:?} {outcome:?}"));
}

struct FakeDom {
    performs: AtomicUsize,
}

impl FakeDom {
    fn new() -> Self {
        Self {
            performs: AtomicUsize::new(0),
        }
    }
    fn performs(&self) -> usize {
        self.performs.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl DomChannel for FakeDom {
    fn extension_id(&self) -> &str {
        "ext-scope"
    }
    async fn observe(&self, _share_id: &str) -> Result<Value, DomFault> {
        Ok(json!({}))
    }
    async fn perform(&self, _op: &DomOperation) -> Result<DomEffect, DomFault> {
        self.performs.fetch_add(1, Ordering::SeqCst);
        Ok(DomEffect::Done)
    }
}

#[derive(Clone, Copy)]
enum Target {
    External,
    Impostor,
    Forged,
}

impl Target {
    fn app(self) -> &'static str {
        match self {
            Self::External => "CANARY-EXTERNAL-C",
            Self::Impostor => "fixture.d",
            Self::Forged => "fixture.a",
        }
    }
    fn window(self) -> Option<String> {
        match self {
            Self::Forged => Some("forged-window".into()),
            _ => None,
        }
    }
}

async fn exercise(world: &World, tool: &str, target: Target) {
    let app = target.app();
    let window = target.window();
    let label = format!("{tool}/{app}");
    match tool {
        "list_apps" => {
            let apps = world.router.list_apps(&world.session).await.unwrap();
            assert_clean(&label, &serde_json::to_string(&apps).unwrap());
        }
        "list_windows" => {
            if matches!(target, Target::Forged) {
                let windows = world
                    .router
                    .list_windows(&world.session, Some(app))
                    .await
                    .unwrap();
                assert_clean(&label, &serde_json::to_string(&windows).unwrap());
            } else {
                let err = denied_windows(world, app).await;
                assert_eq!(err.message, SCOPE_MESSAGE, "{label}");
                assert_clean(&label, &err.to_string());
            }
        }
        "launch_app" => {
            let err = launch(world, app).await;
            assert_eq!(err.message, SCOPE_MESSAGE, "{label}");
            assert!(err.state.is_none(), "{label}");
            if matches!(target, Target::External) {
                let err = launch(world, "fixture.a").await;
                assert_eq!(err.message, SCOPE_MESSAGE);
                assert!(err.state.is_none());
            }
        }
        "get_app_state" => {
            for shot in [json!(false), json!(true), json!("auto")] {
                let err = observe_err(world, app, window.as_deref(), shot).await;
                assert_eq!(err.message, SCOPE_MESSAGE, "{label}");
                assert!(err.state.is_none(), "{label}");
                assert_clean(&label, &err.to_string());
            }
        }
        "capabilities" => {
            let driver = world.router.bound_driver();
            let caps = driver.capabilities();
            assert!(caps.private_apis.is_empty());
            assert_clean(&label, &serde_json::to_string(caps).unwrap());
        }
        "computer_script" => assert!(script_source_mentions_scope()),
        "computer_batch" => {
            let report = world
                .router
                .run_batch(
                    &world.session,
                    "deny-batch",
                    ComputerBatchParams {
                        app: app.into(),
                        window: window.clone(),
                        steps: vec![
                            click_index(1),
                            BatchStep::TypeText(TypeTextArgs {
                                text: "nope".into(),
                                element_index: None,
                            }),
                        ],
                        stop_on_error: false,
                        final_state: true,
                    },
                )
                .await;
            let err = report
                .error
                .clone()
                .expect("batch admitted an external target");
            assert_eq!(err.message, SCOPE_MESSAGE, "{label}");
            assert!(err.state.is_none(), "{label}");
            assert_eq!(report.failed_step, Some(0), "{label}");
            assert_clean(&label, &format!("{err} {report:?}"));
        }
        "wait_for" => {
            let mut call = ToolCall::new(
                "wait",
                app,
                BatchStep::WaitFor(WaitForArgs {
                    until: Condition::TextPresent("CANARY-EXTERNAL-C".into()),
                    timeout_ms: 50,
                }),
            );
            call.window = window;
            let outcome = world.router.wait_for(&world.session, call).await;
            let err = outcome.error.unwrap();
            assert_eq!(err.message, SCOPE_MESSAGE, "{label}");
            assert!(err.state.is_none() && outcome.state.is_none(), "{label}");
            assert_clean(&label, &format!("{err} {:?}", outcome.action));
        }
        other => {
            let step = denial_step(other);
            let outcome = act(world, app, window.as_deref(), step).await;
            let err = outcome.error.unwrap();
            assert_eq!(err.message, SCOPE_MESSAGE, "{label} {err:?}");
            assert!(err.state.is_none() && outcome.state.is_none(), "{label}");
            assert!(outcome.windows.is_empty(), "{label}");
            assert_clean(&label, &format!("{err} {:?}", outcome.action));
        }
    }
}

fn denial_step(tool: &str) -> BatchStep {
    match tool {
        "click" => click_index(1),
        "perform_secondary_action" => {
            BatchStep::PerformSecondaryAction(PerformSecondaryActionArgs {
                element_index: 1,
                action: "press".into(),
            })
        }
        "set_value" => BatchStep::SetValue(SetValueArgs {
            element_index: 1,
            value: "x".into(),
        }),
        "select_text" => BatchStep::SelectText(SelectTextArgs {
            element_index: 1,
            text: Some("x".into()),
            start: None,
            length: None,
        }),
        "type_text" => BatchStep::TypeText(TypeTextArgs {
            text: "paste".into(),
            element_index: None,
        }),
        "press_key" => BatchStep::PressKey(PressKeyArgs { key: "a".into() }),
        "scroll" => BatchStep::Scroll(ScrollArgs {
            element_index: Some(1),
            x: None,
            y: None,
            direction: Direction::Down,
            pages: 1.0,
        }),
        "drag" => BatchStep::Drag(DragArgs {
            from_x: 1.0,
            from_y: 1.0,
            to_x: 2.0,
            to_y: 2.0,
            screenshot_id: None,
        }),
        other => panic!("unmapped catalog tool {other}"),
    }
}

async fn denied_windows(world: &World, app: &str) -> ToolError {
    *world
        .router
        .list_windows(&world.session, Some(app))
        .await
        .unwrap_err()
}

async fn launch(world: &World, app: &str) -> ToolError {
    world
        .router
        .launch_app(
            &world.session,
            "launch",
            LaunchAppParams {
                app: app.into(),
                reason: None,
            },
        )
        .await
        .error
        .unwrap()
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

async fn observe_err(world: &World, app: &str, window: Option<&str>, shot: Value) -> ToolError {
    *world
        .router
        .observe(&world.session, state_params(app, window, shot))
        .await
        .unwrap_err()
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

fn click_point(x: f64, y: f64) -> BatchStep {
    BatchStep::Click(ClickArgs {
        element_index: None,
        x: Some(x),
        y: Some(y),
        screenshot_id: None,
        click_count: 1,
        mouse_button: MouseButton::Left,
    })
}

fn find_named(node: &RawNode, name: &str) -> Option<gcu_core::model::NativeHandle> {
    if node.name.as_deref() == Some(name) {
        return Some(node.handle.clone());
    }
    node.children
        .iter()
        .find_map(|child| find_named(child, name))
}

struct RevokeOnAsk {
    lease: ScopeLease,
    inner: Arc<ScriptedApprovals>,
}

#[async_trait]
impl ApprovalProvider for RevokeOnAsk {
    async fn request_app(
        &self,
        session: &SessionCtx,
        app: &AppInfo,
        reason: Option<&ModelReason>,
        risk: Risk,
        cancel: &HostCancel,
    ) -> ApprovalDecision {
        self.lease.revoke();
        self.inner
            .request_app(session, app, reason, risk, cancel)
            .await
    }
    async fn confirm_action(
        &self,
        session: &SessionCtx,
        app: &AppInfo,
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
        app: &AppInfo,
        reason: HandoffReason,
        cancel: &HostCancel,
    ) {
        self.inner.handoff(session, app, reason, cancel).await;
    }
}
