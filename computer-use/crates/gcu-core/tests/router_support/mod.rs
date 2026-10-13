#![allow(dead_code)]
use gcu_core::{
    action::{ActionRouter, ToolCall, ToolOutcome},
    session::{Session, WindowLocks},
};
use gcu_driver_fake::FakeDriver;
use gcu_host_api::{
    testing::{FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals},
    ApprovalDecision, ConfirmationLevel, ForegroundPolicy, PolicyConfig,
};
use gcu_protocol::tools::*;
use std::sync::Arc;

pub struct Harness {
    pub driver: Arc<FakeDriver>,
    pub activity: Arc<FakeUserActivity>,
    pub approvals: Arc<ScriptedApprovals>,
    pub policy: Arc<MemoryPolicyStore>,
    pub events: Arc<RecordingEventSink>,
    pub router: Arc<ActionRouter>,
    pub session: Arc<Session>,
}
impl Harness {
    pub fn new(pointer: bool, keys: bool) -> Self {
        Self::driver(
            FakeDriver::from_scene("form")
                .unwrap()
                .with_targeted_input(pointer, keys),
        )
    }
    pub fn driver(driver: FakeDriver) -> Self {
        let driver = Arc::new(driver);
        let activity = Arc::new(FakeUserActivity::new());
        let approvals = Arc::new(ScriptedApprovals::new());
        approvals.push_decision(ApprovalDecision::Session);
        let policy = Arc::new(MemoryPolicyStore::new(PolicyConfig {
            sensitive_action_confirmation: ConfirmationLevel::Off,
            allow_foreground: ForegroundPolicy::Always,
            ..PolicyConfig::default()
        }));
        let events = Arc::new(RecordingEventSink::new());
        let router = Arc::new(ActionRouter::new(
            driver.clone(),
            approvals.clone(),
            policy.clone(),
            events.clone(),
            WindowLocks::new(),
        ));
        let session = Arc::new(Session::open("s", activity.clone(), router.input_release()));
        session.begin_turn().unwrap();
        Self {
            driver,
            activity,
            approvals,
            policy,
            events,
            router,
            session,
        }
    }
    pub async fn observe(&self) -> gcu_core::action::ActionState {
        self.router
            .observe(
                &self.session,
                serde_json::from_value(serde_json::json!({"app":"fake.form"})).unwrap(),
            )
            .await
            .unwrap()
    }
    pub async fn execute(&self, step: BatchStep) -> ToolOutcome {
        self.router
            .execute(&self.session, ToolCall::new("call", "fake.form", step))
            .await
    }
}
pub fn click(index: u32) -> BatchStep {
    BatchStep::Click(ClickArgs {
        element_index: Some(index),
        x: None,
        y: None,
        screenshot_id: None,
        click_count: 1,
        mouse_button: MouseButton::Left,
    })
}
pub fn point(x: f64, y: f64) -> BatchStep {
    BatchStep::Click(ClickArgs {
        element_index: None,
        x: Some(x),
        y: Some(y),
        screenshot_id: None,
        click_count: 1,
        mouse_button: MouseButton::Left,
    })
}
pub fn key(key: &str) -> BatchStep {
    BatchStep::PressKey(PressKeyArgs { key: key.into() })
}
pub fn text(text: &str) -> BatchStep {
    BatchStep::TypeText(TypeTextArgs {
        text: text.into(),
        element_index: None,
    })
}
pub fn set(index: u32, value: &str) -> BatchStep {
    BatchStep::SetValue(SetValueArgs {
        element_index: index,
        value: value.into(),
    })
}
