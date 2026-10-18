//! `computer_script` uses the same router entry as a single tool and batch.
#![cfg(all(feature = "script", unix))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_core::{
    action::ActionRouter,
    driver::Driver,
    session::{Session, WindowLocks},
};
use gcu_driver_fake::FakeDriver;
use gcu_host_api::{
    testing::{FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals},
    ApprovalDecision, ConfirmationLevel, ForegroundPolicy, PolicyConfig,
};
use gcu_protocol::ComputerScriptParams;
use gcu_script::{Frame, ScriptStatus};
use std::os::unix::process::CommandExt;
use std::{
    process::{Command, Stdio},
    sync::{atomic::AtomicBool, Arc},
};

fn open(
    driver: Arc<dyn Driver>,
) -> (
    ActionRouter,
    Session,
    Arc<FakeUserActivity>,
    Arc<ScriptedApprovals>,
) {
    let approvals = Arc::new(ScriptedApprovals::new());
    for _ in 0..8 {
        approvals.push_decision(ApprovalDecision::Session);
    }
    let router = ActionRouter::new(
        driver,
        approvals.clone(),
        Arc::new(MemoryPolicyStore::new(PolicyConfig {
            sensitive_action_confirmation: ConfirmationLevel::Off,
            allow_foreground: ForegroundPolicy::Always,
            ..PolicyConfig::default()
        })),
        Arc::new(RecordingEventSink::new()),
        WindowLocks::new(),
    );
    let activity = Arc::new(FakeUserActivity::new());
    let session = Session::open("script-router", activity.clone(), router.input_release());
    session.begin_turn().unwrap();
    (router, session, activity, approvals)
}

#[tokio::test]
async fn script_set_value_reaches_fake_desktop_through_router() {
    let driver = Arc::new(FakeDriver::from_scene("form").unwrap());
    let (router, session, _activity, _approvals) = open(driver.clone());
    let report = gcu_script::run_router(
        &router,
        &session,
        "set",
        ComputerScriptParams {
            code: r#"
                const app = await cu.app("fake.form");
                const s = await app.state({ screenshot: false });
                const field = s.find({ role: "text_field" }) || { index: 1 };
                await app.setValue(field, "季度总结");
                return { ok: true };
            "#
            .into(),
            timeout_ms: 5_000,
        },
        Arc::new(AtomicBool::new(false)),
    )
    .await;
    assert_eq!(report.status, ScriptStatus::Done, "{report:?}");
    assert!(report.output.contains("ok"), "{report:?}");
    let oracle = driver.oracle().await.unwrap();
    assert_eq!(oracle.text["text"], "季度总结");
}

#[tokio::test]
async fn stop_before_script_does_not_change_the_desktop() {
    let driver = Arc::new(FakeDriver::from_scene("form").unwrap());
    let (router, session, activity, _approvals) = open(driver.clone());
    activity.request_stop();
    let report = gcu_script::run_router(
        &router,
        &session,
        "stop",
        ComputerScriptParams {
            code: r#"
                try { await cu.app("fake.form"); } catch (e) {}
                try {
                  const app = await cu.app("fake.form");
                  const s = await app.state({ screenshot: false });
                  const field = s.find({ role: "text_field" }) || { index: 1 };
                  await app.setValue(field, "季度总结");
                } catch (e) {}
                return "after";
            "#
            .into(),
            timeout_ms: 5_000,
        },
        Arc::new(AtomicBool::new(false)),
    )
    .await;
    assert_eq!(report.status, ScriptStatus::Stopped, "{report:?}");
    assert!(report.driver_calls <= 1, "{report:?}");
    let oracle = driver.oracle().await.unwrap();
    assert_ne!(
        oracle.text.get("text").map(String::as_str),
        Some("季度总结")
    );
}

#[cfg(unix)]
#[test]
fn script_worker_process_returns_under_address_limit() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gcu"));
    command
        .arg("script-worker")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: same child-only address cap as the production launcher.
    unsafe {
        command.pre_exec(|| {
            let limit = libc::rlimit {
                rlim_cur: gcu_script::WORKER_AS_BYTES as libc::rlim_t,
                rlim_max: gcu_script::WORKER_AS_BYTES as libc::rlim_t,
            };
            let rc = libc::setrlimit(libc::RLIMIT_AS, &limit);
            if rc == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    let mut child = command.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    gcu_script::write_frame(
        &mut stdin,
        &Frame::Start {
            timeout_ms: 5_000,
            code: "return {ok:true};".into(),
        },
    )
    .unwrap();
    drop(stdin);
    let mut stdout = child.stdout.take().unwrap();
    let frame = gcu_script::read_frame(&mut stdout);
    let status = child.wait().unwrap();
    match frame {
        Ok(Frame::Finish {
            status: code,
            output,
            ..
        }) => {
            assert_eq!(code, 0, "{output}");
            assert!(output.contains("ok"), "{output}");
            assert!(status.success(), "{status}");
        }
        other => panic!("worker frame {other:?}, exit {status}"),
    }
}
