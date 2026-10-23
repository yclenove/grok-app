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
                move |lease| async move {
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
            |_| std::future::pending::<Result<(), Failure>>(),
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
            move |lease| async move {
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
            move |_| async move {
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
