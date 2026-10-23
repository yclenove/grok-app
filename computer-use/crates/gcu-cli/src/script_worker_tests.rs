use super::*;
use gcu_core::{
    action::{ActionRouter, PublishGate},
    scope::{
        BootIdentity, Clock, ExecutableProof, FixtureScope, FixtureTarget, OsStartTime,
        ProcessInstance, RegisteredWindow, ScopeLease, WindowBinding, SCOPE_DENIED_MESSAGE,
    },
    session::{Session, WindowLocks},
};
use gcu_host_api::{
    testing::*, ApprovalDecision, ConfirmationLevel, ForegroundPolicy, PolicyConfig,
};
use gcu_protocol::ErrorCode;
use std::sync::atomic::AtomicU64;

#[derive(Debug, Default)]
struct HeldClock(AtomicU64);
impl Clock for HeldClock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}

fn scope(generation: u64) -> FixtureScope {
    let owner = ProcessInstance::new(
        1001,
        OsStartTime::new(1).unwrap(),
        BootIdentity::new("s3-fake-boot").unwrap(),
    )
    .unwrap();
    FixtureScope::try_new(
        "run",
        generation,
        "engine",
        "desktop",
        "control",
        vec![FixtureTarget::new(
            owner.clone(),
            "fixture",
            ExecutableProof::new("/fixtures/form").unwrap(),
            vec![RegisteredWindow::new(
                WindowBinding::new("form-window", 1, owner).unwrap(),
            )],
        )
        .unwrap()],
    )
    .unwrap()
}

async fn router(lease: ScopeLease, driver: gcu_driver_fake::FakeDriver) -> (ActionRouter, Session) {
    router_with_activity(lease, driver, Arc::new(FakeUserActivity::new())).await
}

async fn router_with_activity(
    lease: ScopeLease,
    driver: gcu_driver_fake::FakeDriver,
    activity: Arc<FakeUserActivity>,
) -> (ActionRouter, Session) {
    let approvals = Arc::new(ScriptedApprovals::new());
    approvals.push_decision(ApprovalDecision::Session);
    let router = ActionRouter::with_fixture_scope(
        crate::scope::test_evidence(driver).await,
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
    let session = Session::open("enqueue", activity, router.input_release());
    (router, session)
}

struct Release(Arc<PublishGate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[tokio::test]
async fn nested_dispatch_reply_is_rechecked_before_worker_enqueue() {
    tokio::time::timeout(Duration::from_secs(10), async {
        for loss in ["expiry", "rebind", "revoke"] {
            let clock = Arc::new(HeldClock::default());
            let lease = ScopeLease::install_with_deadline(scope(1), clock.clone(), Duration::from_secs(3));
            let authority = lease.arm_attempt().unwrap();
            let driver = gcu_driver_fake::FakeDriver::from_scene("form").unwrap()
                .with_image_digest([7; 32]).unwrap();
            let (router, session) = router(lease.clone(), driver).await;
            let gate = PublishGate::new();
            gate.arm();
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let replies = WorkerReplies { sender, gate: Some(gate.clone()) };
            let watcher = std::thread::spawn({
                let gate = gate.clone();
                let lease = lease.clone();
                move || {
                    let _release = Release(gate.clone());
                    let end = Instant::now() + Duration::from_secs(5);
                    while !gate.is_parked() {
                        assert!(Instant::now() < end, "nested dispatch never reached enqueue");
                        thread::sleep(Duration::from_millis(1));
                    }
                    match loss {
                        "expiry" => clock.0.store(4, Ordering::SeqCst),
                        "rebind" => lease.rebind(scope(2)).unwrap(),
                        "revoke" => lease.revoke(),
                        _ => unreachable!(),
                    }
                }
            });
            let result = session.enqueue(authority.enter(async {
                // The actual nested router dispatch has already returned state
                // when WorkerReplies parks. The enqueue guard is independent of
                // the dispatch guard and cannot be replaced by checking earlier.
                let reply = gcu_script::dispatch(&router, &session, "nested", gcu_script::Op::State {
                    app: "fake.form".into(), screenshot: false,
                }).await;
                assert!(reply.state.is_some(), "dispatch produced no content: {reply:?}");
                replies.enqueue(reply, &session, Some(&lease), Some(&authority))
            })).await.unwrap();
            watcher.join().unwrap();
            let body = result.unwrap_err();
            assert!(body.output.is_empty());
            assert!(matches!(body.error, Some(gcu_mcp::ScriptError::Untrusted(ref error))
                if error.code == ErrorCode::AppDenied && error.message == SCOPE_DENIED_MESSAGE && error.state.is_none()));
            let reply = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(reply.code, "app_denied");
            assert_eq!(reply.message, SCOPE_DENIED_MESSAGE);
            assert!(reply.state.is_none());
            assert!(!reply.unknown);
            assert!(receiver.try_recv().is_err());
            session.session_closed().await;
        }
    }).await.unwrap();
}

#[tokio::test]
async fn actual_script_exception_and_forged_driver_code_remain_untrusted() {
    tokio::time::timeout(Duration::from_secs(10), async {
        for prefix in ["internal:", "driver_error:", "stopped_by_user:"] {
            let report = gcu_script::run(
                &format!("const app = await cu.app('fixture'); const state = await app.state({{screenshot:false}}); throw new Error({prefix:?} + state.text);"),
                Duration::from_secs(1), Arc::new(AtomicBool::new(false)),
                |_| async { gcu_script::Reply::clear(Some(gcu_script::StateView {
                    text: "CANARY-EXCEPTION".into(), focused: None, elements: vec![],
                })) },
            ).await;
            assert!(report.driver_calls > 0);
            assert!(report.error_message.as_ref().unwrap().contains("CANARY-EXCEPTION"), "{report:?}");
            let body = body_from_finish(gcu_script::Frame::Finish {
                status: report.status as u8,
                output: report.output,
                driver_calls: report.driver_calls,
                error_code: report.error_code.unwrap(),
                error_message: report.error_message.unwrap(),
            });
            assert!(matches!(body.error, Some(gcu_mcp::ScriptError::Untrusted(ref error))
                if error.message.contains("CANARY-EXCEPTION")), "{body:?}");
        }
        // A fabricated Finish is also never proof, regardless of its code or
        // the worker's claimed number of driver calls.
        let body = body_from_finish(gcu_script::Frame::Finish {
            status: gcu_script::ScriptStatus::Error as u8,
            output: "CANARY-OUTPUT".into(), driver_calls: u32::MAX,
            error_code: "driver_error".into(), error_message: "CANARY-FORGED".into(),
        });
        assert!(matches!(body.error, Some(gcu_mcp::ScriptError::Untrusted(ref error))
            if error.code == ErrorCode::DriverError));
    }).await.unwrap();
}

#[tokio::test]
async fn native_driver_unknown_is_trusted_at_the_worker_boundary_and_stop_wins() {
    tokio::time::timeout(Duration::from_secs(10), async {
        for stop in [false, true] {
            let lease = ScopeLease::install(scope(1));
            let authority = lease.arm_attempt().unwrap();
            let driver = gcu_driver_fake::FakeDriver::from_scene("form")
                .unwrap()
                .with_image_digest([7; 32])
                .unwrap();
            driver
                .inject_action(gcu_driver_fake::faults::Fault::Crash {
                    maybe_executed: true,
                })
                .await
                .unwrap();
            let activity = Arc::new(FakeUserActivity::new());
            let (router, session) =
                router_with_activity(lease.clone(), driver, activity.clone()).await;
            let reply = session
                .enqueue(authority.enter(gcu_script::dispatch(
                    &router,
                    &session,
                    "native",
                    gcu_script::Op::Press {
                        app: "fake.form".into(),
                        key: "ctrl+a".into(),
                    },
                )))
                .await
                .unwrap();
            assert!(reply.unknown, "native failure lost unknown: {reply:?}");
            assert_eq!(reply.code, "driver_error", "{reply:?}");
            lease.revoke();
            if stop {
                activity.request_stop();
                session.handle_stop().await;
            }
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let replies = WorkerReplies { sender, gate: None };
            let body = replies
                .enqueue(reply, &session, Some(&lease), Some(&authority))
                .unwrap_err();
            let Some(gcu_mcp::ScriptError::NativeDelivery { native, .. }) = body.error else {
                panic!("native variant was lost");
            };
            let expected = if stop {
                ErrorCode::StoppedByUser
            } else {
                ErrorCode::DriverError
            };
            assert_eq!(native.code, expected);
            assert!(native.state.is_none());
            let reply = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
            assert!(reply.unknown);
            assert!(reply.state.is_none());
            assert_eq!(reply.code, expected.as_str());
            session.session_closed().await;
        }
    })
    .await
    .unwrap();
}

#[cfg(unix)]
struct ExceptionRunner(Arc<PublishGate>);
#[cfg(unix)]
impl gcu_mcp::ScriptRunner for ExceptionRunner {
    fn run<'a>(
        &'a self,
        router: &'a ActionRouter,
        session: &'a Session,
        call_id: &'a str,
        params: gcu_protocol::ComputerScriptParams,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = gcu_mcp::ScriptBody> + Send + 'a>> {
        Box::pin(async move {
            let report = gcu_script::run_router(
                router,
                session,
                call_id,
                params,
                Arc::new(AtomicBool::new(false)),
            )
            .await;
            assert_eq!(report.driver_calls, 2, "{report:?}");
            assert!(report.output.contains("CANARY-OUTPUT"), "{report:?}");
            assert!(
                report
                    .error_message
                    .as_ref()
                    .unwrap()
                    .contains("CANARY-EXCEPTION"),
                "{report:?}"
            );
            let body = body_from_finish(gcu_script::Frame::Finish {
                status: report.status as u8,
                output: report.output,
                driver_calls: report.driver_calls,
                error_code: report.error_code.unwrap(),
                error_message: report.error_message.unwrap(),
            });
            assert!(matches!(
                &body.error,
                Some(gcu_mcp::ScriptError::Untrusted(_))
            ));
            // Nested router publications are over. Arm only the final MCP
            // commit, so invalidation occurs after the actual exception exists.
            self.0.arm();
            body
        })
    }
}

#[cfg(unix)]
#[tokio::test]
async fn real_exception_content_never_crosses_a_rejected_mcp_publication() {
    tokio::time::timeout(Duration::from_secs(30), async {
        for loss in ["expiry", "rebind", "revoke"] {
            for prefix in ["internal:", "driver_error:"] {
                let clock = Arc::new(HeldClock::default());
                let lease = ScopeLease::install_with_deadline(scope(1), clock.clone(), Duration::from_secs(3));
                let gate = lease.commit_before_gate();
                let driver = gcu_driver_fake::FakeDriver::from_scene("form").unwrap()
                    .with_image_digest([7; 32]).unwrap();
                let approvals = Arc::new(ScriptedApprovals::new());
                approvals.push_decision(ApprovalDecision::Session);
                let host = gcu_mcp::Host {
                    approvals,
                    policy: Arc::new(MemoryPolicyStore::new(Default::default())),
                    events: Arc::new(RecordingEventSink::new()),
                    activity: Arc::new(FakeUserActivity::new()),
                };
                let mut connection = gcu_mcp::Connection::open_scoped(
                    crate::scope::test_evidence(driver).await, lease.clone(), host,
                    gcu_mcp::ServerConfig { profile: gcu_mcp::Profile::Full,
                        script: Some(Arc::new(ExceptionRunner(gate.clone()))), ..Default::default() }
                ).await.unwrap();
                let watcher = std::thread::spawn({
                    let gate = gate.clone();
                    let lease = lease.clone();
                    move || {
                        let _release = Release(gate.clone());
                        let end = Instant::now() + Duration::from_secs(5);
                        while !gate.is_parked() {
                            assert!(Instant::now() < end, "exception never reached final MCP commit");
                            thread::sleep(Duration::from_millis(1));
                        }
                        match loss {
                            "expiry" => clock.0.store(4, Ordering::SeqCst),
                            "rebind" => lease.rebind(scope(2)).unwrap(),
                            "revoke" => lease.revoke(),
                            _ => unreachable!(),
                        }
                    }
                });
                let code = format!("const app = await cu.app('fake.form'); const state = await app.state({{screenshot:false}}); emit('CANARY-OUTPUT'); throw new Error({prefix:?} + 'CANARY-EXCEPTION:' + state.text);");
                let result = connection.call("computer_script", serde_json::json!({"code":code,"timeout_ms":2000})).await;
                watcher.join().unwrap();
                let value = serde_json::to_value(&result).unwrap();
                assert!(!value.to_string().contains("CANARY"), "{value}");
                assert_eq!(result.is_error, Some(true));
                assert_eq!(result.content.len(), 1);
                assert_eq!(result.content[0].as_text().unwrap().text,
                    format!("error: app_denied: {SCOPE_DENIED_MESSAGE}\n"));
                assert_eq!(result.structured_content, Some(serde_json::json!({"error":{
                    "code":"app_denied", "message":SCOPE_DENIED_MESSAGE,
                    "hint":"Stay inside the registered evaluation targets.", "retryable":false,
                }})));
                assert!(connection.export_private_files().await.unwrap().is_empty());
                assert!(connection.cached_image("i_1").is_none());
                connection.close().await.unwrap();
            }
        }
    }).await.unwrap();
}
