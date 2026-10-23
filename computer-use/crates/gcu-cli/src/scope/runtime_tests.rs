//! The production bootstrap/watch/cleanup loop with a fake data-plane future.
use super::*;
use gcu_core::{
    driver::{Cancel, DriverAction, KeyRoute},
    scope::ScopeLease,
};
use gcu_host_protocol::scope::{Scope, LEASE, VERSION};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;
fn install() -> Control {
    let scope = Scope {
        engine_instance: "e".into(),
        desktop_session: "d".into(),
        control_lease: "c".into(),
        targets: vec![],
    };
    Control::Install {
        version: VERSION,
        run_id: "r".into(),
        generation: 1,
        scope_digest: scope.digest("r", 1).unwrap(),
        scope,
    }
}
async fn held_driver() -> gcu_driver_fake::FakeDriver {
    let driver = gcu_driver_fake::FakeDriver::from_scene("form").unwrap();
    let win = driver.fixture_window("fake.form").await.unwrap();
    driver
        .inject_action(gcu_driver_fake::faults::Fault::Crash {
            maybe_executed: true,
        })
        .await
        .unwrap();
    let req = DriverAction::PressKeys {
        win,
        chord: gcu_protocol::KeyChord::parse("ctrl+a").unwrap(),
        route: KeyRoute::TargetedOnly,
    };
    assert!(driver.perform(&req, &Cancel::default()).await.is_err());
    assert_ne!(driver.oracle().await.unwrap().held_input, 0);
    driver
}
#[tokio::test]
async fn eof_or_malformed_control_releases_held_input_and_cancels_data_work() {
    tokio::time::timeout(Duration::from_secs(5), async {
        for malformed in [false, true] {
            let driver = held_driver().await;
            let observed = driver.clone();
            let (mut supervisor, engine) = tokio::io::duplex(4096);
            let captured = Arc::new(Mutex::new(None::<ScopeLease>));
            let copy = captured.clone();
            let task = tokio::spawn(run_controlled(
                Box::new(engine),
                driver,
                Machine::new(Monotonic::default()),
                move |lease, _children| async move {
                    *copy.lock().unwrap() = Some(lease);
                    std::future::pending::<Result<(), Failure>>().await
                },
            ));
            codec::write_frame(&mut supervisor, &install())
                .await
                .unwrap();
            let _: Ack = codec::read_frame(&mut supervisor).await.unwrap().unwrap();
            // Bounded wait until the data future holds its actual S1 lease.
            while captured.lock().unwrap().is_none() {
                tokio::task::yield_now().await;
            }
            let lease = captured.lock().unwrap().clone().unwrap();
            let mut authority = lease.arm_attempt().unwrap();
            if malformed {
                codec::write_frame(
                    &mut supervisor,
                    &serde_json::json!({"type":"Renew","run_id":"r"}),
                )
                .await
                .unwrap();
            } else {
                supervisor.shutdown().await.unwrap();
            }
            assert!(task.await.unwrap().is_err());
            assert!(lease.current().is_none());
            assert!(!authority.confirm(&lease));
            assert!(observed.release_calls() > 0);
            assert_eq!(observed.oracle().await.unwrap().held_input, 0);
        }
    })
    .await
    .unwrap();
}
#[tokio::test(start_paused = true)]
async fn lease_expiry_cancels_partial_control_read_and_releases_input() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let driver = held_driver().await;
        let observed = driver.clone();
        let (mut supervisor, engine) = tokio::io::duplex(4096);
        let task = tokio::spawn(run_controlled(
            Box::new(engine),
            driver,
            Machine::new(Monotonic::default()),
            |_, _children| std::future::pending::<Result<(), Failure>>(),
        ));
        codec::write_frame(&mut supervisor, &install())
            .await
            .unwrap();
        let _: Ack = codec::read_frame(&mut supervisor).await.unwrap().unwrap();
        supervisor.write_all(&[0, 0, 0, 100, b'{']).await.unwrap();
        tokio::time::advance(LEASE + Duration::from_millis(1)).await;
        assert!(task.await.unwrap().is_err());
        assert_eq!(observed.oracle().await.unwrap().held_input, 0);
        assert!(observed.release_calls() > 0);
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn cancellation_revokes_before_release_fallback() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let driver = held_driver().await;
        let observed = driver.clone();
        let (mut supervisor, engine) = tokio::io::duplex(4096);
        let captured = Arc::new(Mutex::new(None::<ScopeLease>));
        let copy = captured.clone();
        let task = tokio::spawn(run_controlled(
            Box::new(engine),
            driver,
            Machine::new(Monotonic::default()),
            move |lease, _children| async move {
                *copy.lock().unwrap() = Some(lease);
                std::future::pending::<Result<(), Failure>>().await
            },
        ));
        codec::write_frame(&mut supervisor, &install())
            .await
            .unwrap();
        let _: Ack = codec::read_frame(&mut supervisor).await.unwrap().unwrap();
        while captured.lock().unwrap().is_none() {
            tokio::task::yield_now().await;
        }
        task.abort();
        let _ = task.await;
        assert!(captured
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .current()
            .is_none());
        while observed.release_calls() == 0 {
            tokio::task::yield_now().await;
        }
        assert_eq!(observed.oracle().await.unwrap().held_input, 0);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn data_factory_is_not_polled_until_ack_is_fully_flushed() {
    use std::sync::atomic::{AtomicBool, Ordering};
    tokio::time::timeout(Duration::from_secs(5), async {
        // One output byte of capacity forces the engine's ACK write to wait.
        // A normal socket can buffer a whole ACK even when nobody reads it.
        let (mut supervisor, engine) = tokio::io::duplex(1);
        let opened = Arc::new(AtomicBool::new(false));
        let seen = opened.clone();
        let driver = gcu_driver_fake::FakeDriver::from_scene("form").unwrap();
        let task = tokio::spawn(run_controlled(
            Box::new(engine),
            driver,
            Machine::new(Monotonic::default()),
            move |_, _children| async move {
                seen.store(true, Ordering::SeqCst);
                std::future::pending::<Result<(), Failure>>().await
            },
        ));
        codec::write_frame(&mut supervisor, &install())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(
            !opened.load(Ordering::SeqCst),
            "data opened while ACK was blocked"
        );
        let _: Ack = codec::read_frame(&mut supervisor).await.unwrap().unwrap();
        while !opened.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        supervisor.shutdown().await.unwrap();
        assert!(task.await.unwrap().is_err());
    })
    .await
    .unwrap();
}

#[cfg(all(unix, feature = "script"))]
#[test]
#[ignore = "subprocess JS worker fixture"]
fn owned_js_worker_helper() {
    let code = gcu_script::worker_main(std::io::stdin(), std::io::stdout());
    std::process::exit(code);
}

#[cfg(all(unix, feature = "script"))]
#[test]
fn runtime_shutdown_reaps_detached_script_worker() {
    use std::{
        process::{Command, Stdio},
        sync::mpsc,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (pid_tx, pid_rx) = mpsc::sync_channel(1);
    let task = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            let (mut supervisor, engine) = tokio::io::duplex(4096);
            let driver = gcu_driver_fake::FakeDriver::from_scene("form").unwrap();
            let task = tokio::spawn(run_controlled(
                Box::new(engine),
                driver,
                Machine::new(Monotonic::default()),
                move |_, children| async move {
                    let mut command = Command::new(std::env::current_exe().unwrap());
                    command
                        .args([
                            "--ignored",
                            "--exact",
                            "scope::runtime_tests::owned_js_worker_helper",
                            "--nocapture",
                        ])
                        .stdin(Stdio::piped())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());
                    let owner = children.spawn(&mut command).unwrap();
                    let pid = {
                        let mut child = owner.child.lock().unwrap();
                        gcu_script::write_frame(
                            child.stdin.as_mut().unwrap(),
                            &gcu_script::Frame::Start {
                                timeout_ms: 30000,
                                code: "for (;;) {}".into(),
                            },
                        )
                        .unwrap();
                        child.id()
                    };
                    // Keep the child in a detached handler, as rmcp does. Only the
                    // engine registry can guarantee draining it at runtime shutdown.
                    tokio::spawn(async move {
                        let _owner = owner;
                        std::future::pending::<()>().await;
                    });
                    pid_tx.send(pid).unwrap();
                    std::future::pending::<Result<(), Failure>>().await
                },
            ));
            codec::write_frame(&mut supervisor, &install())
                .await
                .unwrap();
            let _: Ack = codec::read_frame(&mut supervisor).await.unwrap().unwrap();
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if let Ok(pid) = pid_rx.try_recv() {
                        break pid;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .map(|pid| (task, supervisor, pid))
            .unwrap()
        })
        .await
        .unwrap()
    });
    let (task, supervisor, pid) = task;
    runtime.block_on(async { tokio::time::sleep(Duration::from_millis(100)).await });
    assert_eq!(unsafe { libc::kill(i32::try_from(pid).unwrap(), 0) }, 0);
    // No control EOF: destroy the engine's entire runtime while data work lives.
    runtime.shutdown_timeout(Duration::from_secs(3));
    drop((task, supervisor));
    assert_eq!(
        unsafe { libc::kill(i32::try_from(pid).unwrap(), 0) },
        -1,
        "worker survived runtime shutdown"
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}
