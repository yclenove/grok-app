//! `computer_batch` on a fake desktop and a deterministic settle clock.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use async_trait::async_trait;
use gcu_core::{
    action::ActionRouter,
    batch::{BatchReport, BATCH_DEADLINE, MAX_STEPS},
    driver::{DRIVER_DEADLINE, NATIVE_TIMEOUT},
    session::{Session, WindowLocks},
    settle::{Clock, SettleConfig, SettleEngine},
};
use gcu_driver_fake::{faults::Fault, FakeDriver};
use gcu_host_api::{
    testing::{FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals},
    ApprovalDecision, ConfirmationLevel, ForegroundPolicy, PolicyConfig,
};
use gcu_protocol::{
    result::{Effect, Outcome, VerifyState},
    tools::*,
    ErrorCode,
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{sync::Notify, task::JoinHandle};

struct Rig {
    driver: Arc<FakeDriver>,
    activity: Arc<FakeUserActivity>,
    approvals: Arc<ScriptedApprovals>,
    router: Arc<ActionRouter>,
    session: Arc<Session>,
}

impl Rig {
    fn new(clock: Arc<dyn Clock>, level: ConfirmationLevel) -> Self {
        let driver = Arc::new(FakeDriver::from_scene("form").unwrap());
        let activity = Arc::new(FakeUserActivity::new());
        let approvals = Arc::new(ScriptedApprovals::new());
        approvals.push_decision(ApprovalDecision::Session);
        approvals.push_decision(ApprovalDecision::Session);
        let policy = Arc::new(MemoryPolicyStore::new(PolicyConfig {
            sensitive_action_confirmation: level,
            allow_foreground: ForegroundPolicy::Always,
            ..PolicyConfig::default()
        }));
        let router = Arc::new(
            ActionRouter::new(
                driver.clone(),
                approvals.clone(),
                policy,
                Arc::new(RecordingEventSink::new()),
                WindowLocks::new(),
            )
            .with_settle_engine(SettleEngine::new(SettleConfig::default(), clock)),
        );
        let session = Arc::new(Session::open("s", activity.clone(), router.input_release()));
        session.begin_turn().unwrap();
        Self {
            driver,
            activity,
            approvals,
            router,
            session,
        }
    }

    async fn observe(&self) {
        self.router
            .observe(
                &self.session,
                serde_json::from_value(serde_json::json!({"app":"fake.form"})).unwrap(),
            )
            .await
            .unwrap();
    }

    fn spawn(&self, params: ComputerBatchParams) -> JoinHandle<BatchReport> {
        let router = Arc::clone(&self.router);
        let session = Arc::clone(&self.session);
        tokio::spawn(async move { router.run_batch(&session, "b", params).await })
    }
}

fn batch(steps: Vec<BatchStep>, stop_on_error: bool) -> ComputerBatchParams {
    ComputerBatchParams {
        app: "fake.form".into(),
        window: None,
        steps,
        stop_on_error,
        final_state: true,
    }
}

fn typed(text: &str) -> BatchStep {
    typed_at(None, text)
}

fn typed_at(element_index: Option<u32>, text: &str) -> BatchStep {
    BatchStep::TypeText(TypeTextArgs {
        text: text.into(),
        element_index,
    })
}

fn expect_text(text: &str) -> BatchStep {
    BatchStep::Expect(Condition::TextPresent(text.into()))
}

fn wait_text(text: &str, timeout_ms: u32) -> BatchStep {
    BatchStep::WaitFor(WaitForArgs {
        until: Condition::TextPresent(text.into()),
        timeout_ms,
    })
}

fn click(index: u32) -> BatchStep {
    BatchStep::Click(ClickArgs {
        element_index: Some(index),
        x: None,
        y: None,
        screenshot_id: None,
        click_count: 1,
        mouse_button: MouseButton::Left,
    })
}

fn skipped(step: &gcu_protocol::result::BatchStepResult) {
    assert_eq!(step.outcome, Outcome::Refused);
    assert_eq!(step.effect, Effect::NotChecked);
    assert_eq!(step.ms, 0);
}

fn code(report: &BatchReport) -> Option<ErrorCode> {
    report.error.as_ref().map(|error| error.code)
}

async fn field_text(driver: &FakeDriver) -> String {
    driver
        .oracle()
        .await
        .unwrap()
        .text
        .get("text")
        .cloned()
        .unwrap_or_default()
}

async fn submits(driver: &FakeDriver) -> u64 {
    driver
        .oracle()
        .await
        .unwrap()
        .presses
        .get("submit")
        .copied()
        .unwrap_or(0)
}

async fn soon(work: impl std::future::Future<Output = BatchReport>) -> BatchReport {
    tokio::time::timeout(Duration::from_secs(8), work)
        .await
        .expect("batch hung")
}

async fn driven(clock: &ManualClock, task: JoinHandle<BatchReport>) -> BatchReport {
    // Screenshot capture uses the blocking pool, and settle waits on this clock.
    // A tight yield loop finishes before either wakeup is observed.
    let finished = tokio::time::timeout(Duration::from_secs(20), async {
        for _ in 0..400 {
            if task.is_finished() {
                return task.await.unwrap();
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
            if task.is_finished() {
                return task.await.unwrap();
            }
            // 50ms stays inside the 150ms settle poll interval.
            clock.advance(50);
        }
        panic!("batch did not finish at {}ms", clock.now().as_millis());
    })
    .await;
    finished.expect("batch hung")
}

async fn wait_until_paused(clock: &ManualClock, session: &Session, task: &JoinHandle<BatchReport>) {
    for i in 0..200 {
        if !session.action_ledger().entries().is_empty() && !task.is_finished() {
            for _ in 0..16 {
                tokio::task::yield_now().await;
            }
            if !task.is_finished() {
                return;
            }
        }
        if i % 8 == 7 {
            clock.advance(10);
        }
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
    }
    panic!("batch did not reach the waiting step");
}

#[derive(Default)]
struct ManualClock {
    millis: AtomicU64,
    changed: Notify,
}

impl ManualClock {
    fn advance(&self, ms: u64) {
        self.millis.fetch_add(ms, Ordering::SeqCst);
        self.changed.notify_waiters();
    }
}

#[async_trait]
impl Clock for ManualClock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.millis.load(Ordering::SeqCst))
    }

    async fn wait_until(&self, deadline: Duration) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.now() >= deadline {
                return;
            }
            changed.await;
        }
    }
}

struct JumpClock {
    jumped: AtomicBool,
}

impl JumpClock {
    fn new() -> Self {
        Self {
            jumped: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl Clock for JumpClock {
    fn now(&self) -> Duration {
        if self.jumped.load(Ordering::SeqCst) {
            Duration::from_secs(61)
        } else {
            Duration::ZERO
        }
    }

    async fn wait_until(&self, deadline: Duration) {
        if self.now() < deadline {
            self.jumped.store(true, Ordering::SeqCst);
        }
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn batch_all_steps_succeed_and_expect_is_satisfied() {
    let clock = Arc::new(ManualClock::default());
    let rig = Rig::new(clock.clone(), ConfirmationLevel::Off);
    let task = rig.spawn(batch(vec![typed("Ada"), expect_text("Ada")], true));
    let report = driven(&clock, task).await;
    assert_eq!(code(&report), None);
    assert_eq!(report.failed_step, None);
    assert_eq!(report.steps[0].outcome, Outcome::Done);
    assert_eq!(report.steps[0].tool, "type_text");
    assert_eq!(report.steps[1].tool, "expect");
    assert_eq!(report.steps[1].outcome, Outcome::Done);
    let verify = report.last.unwrap().verify.unwrap();
    assert_eq!(verify.state, VerifyState::Satisfied);
    assert_eq!(verify.expect, "text_present:Ada");
    // The expect delivery is a diff against the type step, so the value lives
    // in the oracle and the verification, not necessarily in the diff text.
    assert!(report.state.is_some());
    assert_eq!(field_text(&rig.driver).await, "Ada");
    assert_eq!(rig.driver.perform_calls(), 1);
    assert_eq!(rig.session.action_ledger().entries().len(), 2);
}

#[tokio::test]
async fn batch_stale_index_after_refresh_stops_before_later_steps() {
    let clock = Arc::new(ManualClock::default());
    let rig = Rig::new(clock.clone(), ConfirmationLevel::Off);
    rig.observe().await;
    rig.driver
        .inject(Fault::ElementDeleted {
            window: "form-window".into(),
            identifier: "submit".into(),
        })
        .await
        .unwrap();
    let task = rig.spawn(batch(vec![typed("A"), click(3), typed("B")], true));
    let report = driven(&clock, task).await;
    assert_eq!(code(&report), Some(ErrorCode::StaleElement));
    assert_eq!(report.failed_step, Some(1));
    assert_eq!(report.steps[0].outcome, Outcome::Done);
    assert_eq!(report.steps[1].tool, "click");
    skipped(&report.steps[2]);
    assert_eq!(field_text(&rig.driver).await, "A");
    assert_eq!(submits(&rig.driver).await, 0);
    assert_eq!(rig.driver.perform_calls(), 1);
    assert_eq!(rig.session.action_ledger().entries().len(), 2);
}

#[tokio::test]
async fn batch_denied_sensitive_confirmation_stops_without_later_delivery() {
    let clock = Arc::new(ManualClock::default());
    let rig = Rig::new(clock.clone(), ConfirmationLevel::Standard);
    rig.observe().await;
    rig.approvals.push_confirm(false);
    // Untargeted typing is an unknown target and would confirm first. The name
    // field is a known control; Submit is the sensitive step.
    let task = rig.spawn(batch(
        vec![typed_at(Some(1), "A"), click(3), typed("B")],
        true,
    ));
    let report = driven(&clock, task).await;
    assert_eq!(code(&report), Some(ErrorCode::ApprovalDenied));
    assert_eq!(report.failed_step, Some(1));
    assert_eq!(report.steps[0].outcome, Outcome::Done);
    skipped(&report.steps[2]);
    assert!(!rig.approvals.confirm_requests().is_empty());
    assert_eq!(field_text(&rig.driver).await, "A");
    assert_eq!(submits(&rig.driver).await, 0);
    assert_eq!(rig.driver.perform_calls(), 1);
}

#[tokio::test]
async fn batch_wait_timeout_stops_before_later_steps() {
    let clock = Arc::new(ManualClock::default());
    let rig = Rig::new(clock.clone(), ConfirmationLevel::Off);
    let task = rig.spawn(batch(
        vec![wait_text("never-appears", 200), typed("Z")],
        true,
    ));
    let report = driven(&clock, task).await;
    assert_eq!(code(&report), Some(ErrorCode::Timeout));
    assert_eq!(report.failed_step, Some(0));
    assert_eq!(report.steps[0].outcome, Outcome::Failed);
    assert_eq!(report.steps[0].tool, "wait_for");
    assert!(report.steps[0].ms > 0);
    skipped(&report.steps[1]);
    assert_eq!(rig.driver.perform_calls(), 0);
    assert_eq!(field_text(&rig.driver).await, "");
}

#[tokio::test]
async fn batch_total_deadline_stops_before_later_steps_without_raising_native_ceiling() {
    assert_eq!(NATIVE_TIMEOUT, Duration::from_secs(5));
    assert_eq!(DRIVER_DEADLINE, Duration::from_millis(4_750));
    assert_eq!(BATCH_DEADLINE, Duration::from_secs(60));
    assert_eq!(MAX_STEPS, 32);
    let rig = Rig::new(Arc::new(JumpClock::new()), ConfirmationLevel::Off);
    let report = soon(rig.router.run_batch(
        &rig.session,
        "b",
        batch(vec![typed("A"), typed("B")], true),
    ))
    .await;
    assert_eq!(code(&report), Some(ErrorCode::Timeout));
    let message = report.error.unwrap().message;
    assert!(message.contains("60s"), "{message}");
    assert!(message.contains("step 1"), "{message}");
    assert!(!message.to_ascii_lowercase().contains("raise"));
    assert_eq!(report.failed_step, Some(1));
    assert_eq!(report.steps[0].outcome, Outcome::Done);
    assert_eq!(report.steps[0].tool, "type_text");
    skipped(&report.steps[1]);
    assert_eq!(field_text(&rig.driver).await, "A");
    assert_eq!(rig.driver.perform_calls(), 1);
    assert_eq!(NATIVE_TIMEOUT, Duration::from_secs(5));
    assert_eq!(DRIVER_DEADLINE, Duration::from_millis(4_750));
}

#[tokio::test]
async fn batch_stop_before_start_executes_nothing() {
    let rig = Rig::new(Arc::new(ManualClock::default()), ConfirmationLevel::Off);
    rig.activity.request_stop();
    let report = soon(rig.router.run_batch(
        &rig.session,
        "b",
        batch(vec![click(3), typed("Z")], true),
    ))
    .await;
    assert_eq!(code(&report), Some(ErrorCode::StoppedByUser));
    assert_eq!(report.failed_step, Some(0));
    assert_eq!(report.steps[0].outcome, Outcome::Refused);
    skipped(&report.steps[1]);
    assert_eq!(rig.driver.perform_calls(), 0);
    assert!(rig.driver.release_calls() > 0);
    let ledger = rig.session.action_ledger();
    assert_eq!(ledger.entries().len(), 1);
    assert_eq!(ledger.entries()[0].call_id, "b:0");
}

#[tokio::test]
async fn batch_stop_during_wait_aborts_remaining_steps() {
    let clock = Arc::new(ManualClock::default());
    let rig = Rig::new(clock.clone(), ConfirmationLevel::Off);
    let task = rig.spawn(batch(
        vec![
            expect_text("Submit"),
            wait_text("never-appears", 30_000),
            typed("Z"),
        ],
        true,
    ));
    wait_until_paused(&clock, &rig.session, &task).await;
    rig.activity.request_stop();
    let report = driven(&clock, task).await;
    assert_eq!(report.steps[0].outcome, Outcome::Done);
    assert_eq!(report.steps[0].tool, "expect");
    assert_eq!(code(&report), Some(ErrorCode::StoppedByUser));
    assert_eq!(report.failed_step, Some(1));
    skipped(&report.steps[2]);
    assert_eq!(rig.driver.perform_calls(), 0);
    assert_eq!(field_text(&rig.driver).await, "");
}

#[tokio::test]
async fn batch_session_close_before_start_executes_nothing() {
    let rig = Rig::new(Arc::new(ManualClock::default()), ConfirmationLevel::Off);
    rig.session.session_closed().await;
    let report = soon(rig.router.run_batch(
        &rig.session,
        "b",
        batch(vec![typed("A"), typed("B")], true),
    ))
    .await;
    assert_eq!(code(&report), Some(ErrorCode::ApprovalDenied));
    assert!(report
        .steps
        .iter()
        .all(|step| step.outcome == Outcome::Refused && step.ms == 0));
    assert_eq!(rig.driver.perform_calls(), 0);
    assert_eq!(field_text(&rig.driver).await, "");
}

#[tokio::test]
async fn batch_session_close_during_wait_aborts_remaining_steps() {
    let clock = Arc::new(ManualClock::default());
    let rig = Rig::new(clock.clone(), ConfirmationLevel::Off);
    let task = rig.spawn(batch(
        vec![
            expect_text("Submit"),
            wait_text("never-appears", 30_000),
            typed("Z"),
        ],
        true,
    ));
    wait_until_paused(&clock, &rig.session, &task).await;
    rig.session.session_closed().await;
    let report = driven(&clock, task).await;
    assert_eq!(report.steps[0].outcome, Outcome::Done);
    assert_eq!(code(&report), Some(ErrorCode::ApprovalDenied));
    assert_eq!(report.failed_step, Some(1));
    skipped(&report.steps[2]);
    assert_eq!(rig.driver.perform_calls(), 0);
}

#[tokio::test]
async fn batch_invalid_preflight_executes_nothing() {
    let rig = Rig::new(Arc::new(ManualClock::default()), ConfirmationLevel::Off);
    let before = field_text(&rig.driver).await;
    let cases = [
        batch(
            vec![
                typed("Ada"),
                BatchStep::PressKey(PressKeyArgs {
                    key: "not a key".into(),
                }),
            ],
            true,
        ),
        batch(
            vec![BatchStep::Click(ClickArgs {
                element_index: None,
                x: Some(1.0),
                y: Some(1.0),
                screenshot_id: Some(String::new()),
                click_count: 1,
                mouse_button: MouseButton::Left,
            })],
            true,
        ),
        ComputerBatchParams {
            app: "  ".into(),
            window: None,
            steps: vec![typed("A")],
            stop_on_error: true,
            final_state: true,
        },
        batch(
            vec![BatchStep::Expect(Condition::Element(ElementCondition {
                role: "not_a_role".into(),
                name: None,
            }))],
            true,
        ),
    ];
    for params in cases {
        let report = soon(rig.router.run_batch(&rig.session, "b", params)).await;
        assert!(code(&report).is_some());
        assert!(report
            .steps
            .iter()
            .all(|step| step.outcome == Outcome::Refused && step.ms == 0));
        assert!(report.failure.is_none());
    }
    let invalid_key = soon(rig.router.run_batch(
        &rig.session,
        "b",
        batch(
            vec![
                typed("Ada"),
                BatchStep::PressKey(PressKeyArgs {
                    key: "not a key".into(),
                }),
            ],
            true,
        ),
    ))
    .await;
    assert_eq!(code(&invalid_key), Some(ErrorCode::InvalidKey));
    assert_eq!(invalid_key.failed_step, Some(1));
    assert!(invalid_key.error.unwrap().message.contains("key"));
    assert_eq!(rig.driver.perform_calls(), 0);
    assert_eq!(field_text(&rig.driver).await, before);
    assert!(rig.session.action_ledger().entries().is_empty());
}

#[tokio::test]
async fn batch_unknown_outcome_is_not_replayed_and_stops() {
    let clock = Arc::new(ManualClock::default());
    let rig = Rig::new(clock.clone(), ConfirmationLevel::Off);
    rig.observe().await;
    rig.driver
        .inject_action(Fault::Timeout {
            maybe_executed: true,
        })
        .await
        .unwrap();
    let task = rig.spawn(batch(vec![click(3), click(3)], false));
    let report = driven(&clock, task).await;
    assert_eq!(report.steps[0].outcome, Outcome::Unknown);
    skipped(&report.steps[1]);
    assert_eq!(report.failed_step, Some(0));
    assert_eq!(rig.driver.perform_calls(), 1);
    assert_eq!(submits(&rig.driver).await, 1);
    assert!(rig.session.action_ledger().has_unknown());
    assert_eq!(rig.session.action_ledger().entries().len(), 1);
}

#[tokio::test]
async fn batch_nested_step_and_over_limit_are_rejected() {
    assert!(serde_json::from_value::<BatchStep>(
        serde_json::json!({"computer_batch": {"app": "fake.form", "steps": []}})
    )
    .is_err());
    assert!(
        serde_json::from_value::<BatchStep>(serde_json::json!({"launch_app": {"app": "x"}}))
            .is_err()
    );
    let rig = Rig::new(Arc::new(ManualClock::default()), ConfirmationLevel::Off);
    let too_many = soon(
        rig.router
            .run_batch(&rig.session, "b", batch(vec![typed("a"); 33], true)),
    )
    .await;
    assert_eq!(code(&too_many), Some(ErrorCode::CapabilityUnavailable));
    assert!(too_many.error.unwrap().message.contains("32"));
    assert_eq!(too_many.steps.len(), 33);
    assert!(too_many.steps.iter().all(|step| step.ms == 0));
    let empty = soon(rig.router.run_batch(&rig.session, "b", batch(vec![], true))).await;
    assert_eq!(code(&empty), Some(ErrorCode::CapabilityUnavailable));
    assert!(empty.steps.is_empty());
    assert_eq!(rig.driver.perform_calls(), 0);
    assert!(rig.session.action_ledger().entries().is_empty());
}

#[tokio::test]
async fn batch_expect_unsatisfied_stops_and_delivers_nothing() {
    let rig = Rig::new(Arc::new(ManualClock::default()), ConfirmationLevel::Off);
    let report = soon(rig.router.run_batch(
        &rig.session,
        "b",
        batch(vec![expect_text("missing-xyz"), typed("B")], true),
    ))
    .await;
    assert_eq!(code(&report), None);
    assert_eq!(report.failed_step, Some(0));
    assert_eq!(report.steps[0].outcome, Outcome::Failed);
    assert_eq!(report.steps[0].tool, "expect");
    skipped(&report.steps[1]);
    let failure = report.failure.expect("expect step");
    let verify = failure.verify.expect("verification");
    assert_eq!(verify.state, VerifyState::Unsatisfied);
    assert_eq!(verify.expect, "text_present:missing-xyz");
    assert!(failure.notes.iter().any(|note| note.contains("not met")));
    assert_eq!(rig.driver.perform_calls(), 0);
    assert_eq!(field_text(&rig.driver).await, "");
    assert_eq!(rig.session.action_ledger().entries().len(), 1);
}
