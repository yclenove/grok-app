//! S5 uses the production Host approvals and S3 fake evidence, never a desktop.
use super::fake::FakeEvidence;
use crate::host::{approval::Approvals, policy_store::HostPolicy};
use gcu_core::{
    action::{ActionRouter, ToolCall},
    driver::{AppQuery, Driver, WindowRef},
    scope::{
        ExecutableProof, FixtureScope, FixtureTarget, InstanceEvidence, RegisteredWindow,
        ScopeLease, SCOPE_DENIED_MESSAGE,
    },
    session::{Session, WindowLocks},
};
use gcu_driver_fake::FakeDriver;
use gcu_host_api::{
    testing::{FakeUserActivity, RecordingEventSink},
    ConfirmationLevel, ForegroundPolicy, PolicyConfig, PolicyStore, UserActivity,
};
use gcu_protocol::{tools::*, ErrorCode};
use serde_json::json;
use std::sync::Arc;

const SCENE: &str = include_str!("../../../gcu-mcp/tests/fixture_scope/desktop.ron");
const MODES: [&str; 3] = ["auto-allow-turn", "auto-allow-session", "auto-allow-always"];

async fn scope(evidence: &FakeEvidence, generation: u64) -> FixtureScope {
    let mut targets = Vec::new();
    for (id, windows) in [
        ("fixture.a", &["w-a", "w-a2"][..]),
        ("fixture.b", &["w-b"][..]),
    ] {
        let app = evidence.resolve_app(&AppQuery::from(id)).await.unwrap();
        let owner = evidence.app_instance(&app).unwrap();
        let registered = windows
            .iter()
            .map(|id| {
                RegisteredWindow::new(
                    evidence
                        .window_binding(&WindowRef {
                            app: app.clone(),
                            id: (*id).into(),
                        })
                        .unwrap(),
                )
            })
            .collect();
        targets.push(
            FixtureTarget::new(
                owner,
                id,
                ExecutableProof::new(
                    app.info
                        .identity
                        .as_ref()
                        .unwrap()
                        .executable
                        .as_ref()
                        .unwrap(),
                )
                .unwrap(),
                registered,
            )
            .unwrap(),
        );
    }
    FixtureScope::try_new("s5", generation, "engine", "desktop", "control", targets).unwrap()
}

struct Fixture {
    driver: FakeDriver,
    evidence: FakeEvidence,
    router: ActionRouter,
    session: Session,
    lease: ScopeLease,
    activity: Arc<FakeUserActivity>,
    policy: Arc<HostPolicy>,
    home: tempfile::TempDir,
}
impl Fixture {
    async fn open(mode: &str, config: PolicyConfig, targeted: bool, scene: &str) -> Self {
        let home = tempfile::tempdir().unwrap();
        let policy = Arc::new(HostPolicy::open(config, home.path().join("approvals.json")));
        let driver = FakeDriver::from_ron(scene)
            .unwrap()
            .with_targeted_input(targeted, targeted);
        let evidence = FakeEvidence::new(driver.clone()).await.unwrap();
        let lease = ScopeLease::install(scope(&evidence, 1).await);
        let activity = Arc::new(FakeUserActivity::new());
        let router = ActionRouter::with_fixture_scope(
            evidence.clone(),
            lease.clone(),
            activity.clone(),
            Arc::new(Approvals::new(Some(mode.into()), policy.clone())),
            policy.clone(),
            Arc::new(RecordingEventSink::new()),
            WindowLocks::new(),
        );
        let session = Session::open("s5", activity.clone(), router.input_release());
        session.begin_turn().unwrap();
        Self {
            driver,
            evidence,
            router,
            session,
            lease,
            activity,
            policy,
            home,
        }
    }
    async fn observe(
        &self,
        app: &str,
        window: &str,
    ) -> Result<gcu_core::action::ActionState, Box<gcu_protocol::ToolError>> {
        self.router
            .observe(
                &self.session,
                serde_json::from_value(json!({
                    "app":app,"window":window,"include_screenshot":false
                }))
                .unwrap(),
            )
            .await
    }
    async fn act(&self, window: &str, action: BatchStep) -> gcu_core::action::ToolOutcome {
        let mut call = ToolCall::new("s5", "fixture.a", action);
        call.window = Some(window.into());
        call.settle_budget_ms = Some(0);
        self.router.execute(&self.session, call).await
    }
}
fn config() -> PolicyConfig {
    PolicyConfig {
        sensitive_action_confirmation: ConfirmationLevel::Off,
        allow_foreground: ForegroundPolicy::Always,
        ..PolicyConfig::default()
    }
}
fn click(index: u32) -> BatchStep {
    serde_json::from_value(json!({"click":{"element_index":index}})).unwrap()
}
fn go(text: &str) -> u32 {
    text.lines()
        .find(|line| line.contains("Go"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

#[tokio::test]
async fn auto_app_grants_leave_both_sensitive_and_foreground_confirmations_denied() {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        for mode in MODES {
            for sensitive in [true, false] {
                let mut policy = config();
                if sensitive {
                    policy.sensitive_action_confirmation = ConfirmationLevel::Strict;
                } else {
                    policy.allow_foreground = ForegroundPolicy::Ask;
                }
                let f = Fixture::open(mode, policy, sensitive, SCENE).await;
                let seen = f.observe("fixture.a", "w-a").await.unwrap();
                let step = if sensitive {
                    click(go(&seen.text))
                } else {
                    BatchStep::PressKey(PressKeyArgs {
                        key: "ctrl+a".into(),
                    })
                };
                let out = f.act("w-a", step).await;
                assert_eq!(
                    out.error.unwrap().code,
                    ErrorCode::ApprovalDenied,
                    "{mode}/{sensitive}"
                );
                assert_eq!(out.action.delivery, gcu_protocol::result::Delivery::None);
                let oracle = f.driver.oracle().await.unwrap();
                assert!(oracle.presses.is_empty());
                assert!(oracle.foreground.is_none());
                assert_eq!(oracle.held_input, 0);
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn auto_app_grants_intersect_scope_and_stop_and_never_write_always() {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        for mode in MODES {
            let f = Fixture::open(mode, config(), true, SCENE).await;
            for (app, window) in [
                ("fixture.c", "w-c"),
                ("fixture.d", "w-d"),
                ("fixture.a", "w-a3"),
                ("fixture.a", "forged-window"),
            ] {
                let error = f.observe(app, window).await.unwrap_err();
                assert_eq!(error.code, ErrorCode::AppDenied);
                assert_eq!(error.message, SCOPE_DENIED_MESSAGE);
                assert!(error.state.is_none());
            }
            for (app, window) in [
                ("fixture.a", "w-a"),
                ("fixture.a", "w-a2"),
                ("fixture.b", "w-b"),
            ] {
                let seen = f.observe(app, window).await.unwrap();
                let mut call = ToolCall::new("allowed", app, click(go(&seen.text)));
                call.window = Some(window.into());
                call.settle_budget_ms = Some(0);
                let out = f.router.execute(&f.session, call).await;
                assert!(out.error.is_none(), "{mode}/{window}: {out:?}");
            }
            assert!(!f.home.path().join("approvals.json").exists());
            assert!(f.policy.load().always.is_empty());
            let presses = f.driver.oracle().await.unwrap().presses;
            f.activity.request_stop();
            f.lease.rebind(scope(&f.evidence, 2).await).unwrap();
            let error = f.observe("fixture.c", "w-c").await.unwrap_err();
            assert_eq!(error.code, ErrorCode::StoppedByUser);
            let restarted =
                Session::open("new-session", f.activity.clone(), f.router.input_release());
            assert!(restarted.started_stopped());
            let error = f.router.list_apps(&restarted).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::StoppedByUser);
            let out = f.act("w-a", click(1)).await;
            assert_eq!(out.error.unwrap().code, ErrorCode::StoppedByUser);
            assert_eq!(out.action.delivery, gcu_protocol::result::Delivery::None);
            assert_eq!(f.driver.oracle().await.unwrap().presses, presses);
            assert!(f.activity.latched_stop());
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn auto_app_grants_cannot_override_builtin_deny_or_execution_host_identity() {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        for mode in MODES {
            for (image, expected) in [
                ("/usr/bin/xterm", ErrorCode::AppDenied),
                ("/usr/bin/python", ErrorCode::HandoffRequired),
            ] {
                let scene = SCENE.replace("/fixtures/a", image);
                let f = Fixture::open(mode, config(), true, &scene).await;
                let error = f.observe("fixture.a", "w-a").await.unwrap_err();
                assert_eq!(error.code, expected, "{mode}/{image}: {error:?}");
                assert!(f.driver.oracle().await.unwrap().presses.is_empty());
                assert!(f.policy.load().always.is_empty());
            }
        }
    })
    .await
    .unwrap();
}
