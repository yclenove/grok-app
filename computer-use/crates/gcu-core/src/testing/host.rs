//! Synthetic Host for driver contracts, enabled only by core's `testing` feature.
use crate::{
    action::ActionRouter,
    driver::Driver,
    policy::{
        ActionIntent, GrantTable, PolicyCtx, PolicyDecision, PolicyEngine, PolicyWindow, Tool,
    },
    session::{Session, WindowLocks},
    tree::ProcessedTree,
};
use gcu_host_api::{
    testing::{FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals},
    AppIdentity, AppInfo, ApprovalDecision, ConfirmationLevel, ForegroundPolicy, Platform,
    PolicyConfig, SignatureState, UserActivity,
};
use std::sync::Arc;

/// A fresh idle activity source and Stop latch for driver-level scope tests,
/// so drivers can build a `ScopedDriver` without a Host dependency.
/// Memory only. No blocking, timeout or cancellation.
pub fn fake_activity() -> Arc<dyn gcu_host_api::UserActivity> {
    Arc::new(FakeUserActivity::new())
}

/// Assemble an in-memory, explicitly permissive Host for a disposable driver.
/// No OS IO, blocking, timeout or cancellation occurs during assembly. One
/// session app approval is scripted; production Hosts never use this fixture.
pub fn router(driver: Arc<dyn Driver>) -> (ActionRouter, Session) {
    let approvals = Arc::new(ScriptedApprovals::new());
    approvals.push_decision(ApprovalDecision::Session);
    let router = ActionRouter::new(
        driver,
        approvals,
        Arc::new(MemoryPolicyStore::new(PolicyConfig {
            sensitive_action_confirmation: ConfirmationLevel::Off,
            allow_foreground: ForegroundPolicy::Always,
            ..Default::default()
        })),
        Arc::new(RecordingEventSink::new()),
        WindowLocks::new(),
    );
    let session = Session::open(
        "driver-contract",
        Arc::new(FakeUserActivity::new()),
        router.input_release(),
    );
    // A newly assembled session has no existing turn or concurrent owner.
    assert!(session.begin_turn().is_ok());
    (router, session)
}

/// Host Stop latch shared by a scoped router and the test that trips it.
///
/// [`Self::activity`] is the object the session and `ScopedDriver` hold.
/// [`Self::request_stop`] sets that same latch. Memory only. No blocking,
/// timeout, or cancellation. Safe to call from another thread.
#[derive(Clone, Debug)]
pub struct StopControl {
    activity: Arc<FakeUserActivity>,
}

impl Default for StopControl {
    fn default() -> Self {
        Self::new()
    }
}

impl StopControl {
    /// Idle latch.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn new() -> Self {
        Self {
            activity: Arc::new(FakeUserActivity::new()),
        }
    }

    /// Activity for [`Session::open`] and the scoped wrapper.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn activity(&self) -> Arc<dyn UserActivity> {
        self.activity.clone()
    }

    /// Latch Host Stop. Idempotent while the latch is already set.
    ///
    /// Memory only. No blocking, timeout, or cancellation. Thread safe.
    pub fn request_stop(&self) {
        self.activity.request_stop();
    }
}

/// Assemble a permissive Host whose router holds a [`crate::scope::ScopedDriver`].
///
/// The same activity object is the session stop latch and the wrapper's stop
/// probe. Memory only. No blocking, timeout, or cancellation. One session app
/// approval is scripted. Production Hosts never use this fixture.
#[cfg(feature = "testing")]
pub fn scoped_router<D>(driver: D, lease: crate::scope::ScopeLease) -> (ActionRouter, Session)
where
    D: Driver + crate::scope::InstanceEvidence + 'static,
{
    scoped_router_with(driver, lease, Arc::new(FakeUserActivity::new()))
}

/// [`scoped_router`] with a caller-supplied activity.
///
/// Pass [`StopControl::activity`] so an outside thread can latch the same
/// Stop the session and wrapper observe. Memory only. No blocking, timeout,
/// or cancellation. One session app approval is scripted.
#[cfg(feature = "testing")]
pub fn scoped_router_with<D>(
    driver: D,
    lease: crate::scope::ScopeLease,
    activity: Arc<dyn UserActivity>,
) -> (ActionRouter, Session)
where
    D: Driver + crate::scope::InstanceEvidence + 'static,
{
    let approvals = Arc::new(ScriptedApprovals::new());
    approvals.push_decision(ApprovalDecision::Session);
    let router = ActionRouter::with_fixture_scope(
        driver,
        lease,
        activity.clone(),
        approvals,
        Arc::new(MemoryPolicyStore::new(PolicyConfig {
            sensitive_action_confirmation: ConfirmationLevel::Off,
            allow_foreground: ForegroundPolicy::Always,
            ..Default::default()
        })),
        Arc::new(RecordingEventSink::new()),
        WindowLocks::new(),
    );
    let session = Session::open("driver-contract", activity, router.input_release());
    assert!(session.begin_turn().is_ok());
    (router, session)
}

/// Evaluate every input route against a real driver's processed execution hints.
/// The synthetic editor is unsigned and has no grants. Each pair contains the
/// exact-target and unresolved-target decisions; callers assert refusal/handoff.
/// Pure memory work, with no blocking, timeout, cancellation or native delivery.
pub fn execution_decisions(
    tree: &ProcessedTree,
    index: u32,
) -> Vec<(Tool, PolicyDecision, PolicyDecision)> {
    let activity = FakeUserActivity::new();
    let config = PolicyConfig::default();
    let grants = GrantTable::default();
    let window = PolicyWindow {
        id: "contract".into(),
        title: "Workspace".into(),
        url: None,
    };
    let app = AppInfo {
        name: "Editor".into(),
        pid: Some(7),
        icon: None,
        identity: AppIdentity {
            platform: Platform::Windows,
            key: "c:\\editor.exe".into(),
            image_path: "c:\\editor.exe".into(),
            signature: SignatureState::Unsigned,
            sha256: Some([1; 32]),
            team_id: None,
            publisher: None,
            proof: gcu_host_api::GrantProof::default(),
        },
    };
    let ctx = PolicyCtx {
        session_id: "contract",
        app: &app,
        window: Some(&window),
        tree: Some(tree),
        activity: &activity,
        config: &config,
        grants: &grants,
        policy_version: 1,
        closed: false,
        browser: None,
        scope_epoch: None,
    };
    [
        Tool::Click,
        Tool::Secondary,
        Tool::Menu,
        Tool::SetValue,
        Tool::TypeText,
        Tool::PressKey,
        Tool::Scroll,
        Tool::Drag,
        Tool::Batch,
        Tool::Script,
    ]
    .into_iter()
    .map(|tool| {
        let mut intent = ActionIntent::new(tool);
        intent.element_index = Some(index);
        if tool == Tool::PressKey {
            intent.key = Some("Return".into());
        }
        let targeted = PolicyEngine::new().evaluate(&ctx, &intent);
        intent.element_index = None;
        (tool, targeted, PolicyEngine::new().evaluate(&ctx, &intent))
    })
    .collect()
}
