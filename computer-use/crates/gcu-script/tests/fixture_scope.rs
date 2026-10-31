//! Scoped `computer_script` hard-stop. The private control socket is not_run.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{atomic::AtomicBool, Arc};

use gcu_core::driver::Driver;
use gcu_core::scope::{ScopeLease, SCOPE_DENIED_MESSAGE};
use gcu_core::testing::host::scoped_router;
use gcu_core::testing::probe::ScopeProbe;
use gcu_protocol::tools::ComputerScriptParams;
use gcu_script::{run_router, ScriptStatus};

#[test]
fn script_scope_denial_stops_before_a_second_call() {
    for (prefix, allowed_calls) in [("", 0), ("await cu.app(\"Fixture Allowed\");", 1)] {
        let probe = ScopeProbe::external_canary();
        let lease = ScopeLease::install(ScopeProbe::allowed_scope());
        let (router, session) = scoped_router(probe.clone(), lease);
        let report = gcu_core::testing::block_on(run_router(
            &router,
            &session,
            "scope",
            ComputerScriptParams {
                code: format!(
                    "{prefix}\ntry {{ await cu.app(\"CANARY-EXTERNAL-C\"); }} catch (e) {{}}\n\
                 await cu.app(\"Fixture Allowed\"); return \"after\";"
                ),
                timeout_ms: 5_000,
            },
            Arc::new(AtomicBool::new(false)),
        ))
        .unwrap();
        assert_eq!(report.driver_calls, allowed_calls + 1, "{report:?}");
        assert_eq!(report.status, ScriptStatus::Stopped, "{report:?}");
        assert_eq!(
            report.error_code.as_deref(),
            Some("app_denied"),
            "{report:?}"
        );
        assert_eq!(
            SCOPE_DENIED_MESSAGE,
            "Target unavailable in this evaluation scope."
        );
        assert_eq!(
            report.error_message.as_deref(),
            Some("Target unavailable in this evaluation scope."),
            "{report:?}"
        );
        if allowed_calls == 0 {
            assert_eq!(probe.content(), 0, "script read external content");
        } else {
            assert!(
                probe.content() > 0,
                "prefix did not observe the allowed target"
            );
        }
        assert!(!report.output.contains("after"), "{report:?}");
        assert_eq!(probe.capture(), 0, "script captured external pixels");
        assert_eq!(probe.perform(), 0, "script delivered input");
        assert_eq!(probe.launch(), 0, "script launched");
        assert_eq!(probe.restores(), 0, "script restored a window");
    }
}

#[test]
fn inner_probe_resolves_and_reads_the_external_canary() {
    let probe = ScopeProbe::external_canary();
    let read = gcu_core::testing::block_on(async {
        let app = probe
            .resolve_app(&gcu_core::driver::AppQuery::from("CANARY-EXTERNAL-C"))
            .await
            .unwrap();
        let win = gcu_core::driver::WindowRef {
            app,
            id: "w-c".into(),
        };
        probe
            .snapshot_tree(
                &win,
                &gcu_core::driver::TreeOpts::default(),
                &gcu_core::driver::Cancel::default(),
            )
            .await
            .unwrap()
    })
    .unwrap();
    let name = read.root.unwrap().name.unwrap();
    assert_eq!(name, "CANARY-EXTERNAL-C");
    assert_eq!(probe.content(), 1, "inner driver did not read the canary");
}

#[test]
fn allowed_script_observes_without_restoring_the_hidden_window() {
    let probe = ScopeProbe::external_canary();
    let lease = ScopeLease::install(ScopeProbe::allowed_scope());
    let (router, session) = scoped_router(probe.clone(), lease);
    let report = gcu_core::testing::block_on(run_router(
        &router,
        &session,
        "allowed",
        ComputerScriptParams {
            code: "const app = await cu.app(\"Fixture Allowed\"); return await app.state({screenshot:false});"
                .into(),
            timeout_ms: 5_000,
        },
        Arc::new(AtomicBool::new(false)),
    ))
    .unwrap();
    assert_eq!(report.status, ScriptStatus::Done, "{report:?}");
    assert_eq!(report.driver_calls, 2, "{report:?}");
    assert!(
        report.output.contains("Allowed Target"),
        "scope wrapper hid the allowed target: {report:?}"
    );
    assert!(probe.content() > 0, "allowed observation did not read");
    assert_eq!(probe.restores(), 0, "cu.app restored the hidden key window");
    assert_eq!(probe.capture(), 0);
    assert_eq!(probe.perform(), 0);
    assert_eq!(probe.launch(), 0);
    assert!(!report.output.contains("CANARY-EXTERNAL-C"), "{report:?}");
}

#[test]
fn queued_script_operation_keeps_connection_authority_and_refuses_reply() {
    use gcu_core::scope::{
        BootIdentity, ExecutableProof, FixtureScope, FixtureTarget, OsStartTime, ProcessInstance,
        RegisteredWindow, WindowBinding,
    };
    use std::{
        future::Future,
        task::Poll,
        time::{Duration, Instant},
    };
    gcu_core::testing::block_on(async {
        let probe = ScopeProbe::external_canary();
        let initial = ScopeProbe::allowed_scope();
        let lease = ScopeLease::install(initial.clone());
        let authority = lease.arm_attempt().unwrap();
        let (router, session) = scoped_router(probe.clone(), lease.clone());
        let queued = authority.enter(gcu_script::dispatch(
            &router,
            &session,
            "queued",
            gcu_script::Op::App {
                name: "Fixture Allowed".into(),
            },
        ));
        let owner = ProcessInstance::new(
            1101,
            OsStartTime::new(10).unwrap(),
            BootIdentity::new("boot-1").unwrap(),
        )
        .unwrap();
        let target = FixtureTarget::new(
            owner.clone(),
            "allowed",
            ExecutableProof::new("/fixtures/allowed").unwrap(),
            vec![RegisteredWindow::new(
                WindowBinding::new("w-allowed", 1, owner).unwrap(),
            )],
        )
        .unwrap();
        lease
            .rebind(
                FixtureScope::try_new(
                    initial.run_id(),
                    2,
                    initial.engine_instance(),
                    initial.desktop_session(),
                    initial.control_lease(),
                    vec![target],
                )
                .unwrap(),
            )
            .unwrap();
        let mut queued = std::pin::pin!(queued);
        let end = Instant::now() + Duration::from_secs(5);
        let reply = std::future::poll_fn(|cx| {
            assert!(Instant::now() < end, "queued script operation hung");
            match queued.as_mut().poll(cx) {
                Poll::Ready(reply) => Poll::Ready(reply),
                Poll::Pending => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
        })
        .await;
        assert_eq!(reply.code, "app_denied");
        assert_eq!(reply.message, SCOPE_DENIED_MESSAGE);
        assert!(reply.state.is_none());
        assert!(reply.hard_stop);
        assert_eq!(probe.content(), 0);
        assert_eq!(probe.capture(), 0);
        assert_eq!(probe.perform(), 0);
    })
    .unwrap();
}
