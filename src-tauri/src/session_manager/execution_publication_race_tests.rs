//! Slow real store locks must not block runtime access or handshake cancellation.
use super::*;
use std::{sync::mpsc, thread};

fn cancel_during_store_contention(background: bool) {
    let fixture = Fixture::new(false);
    runtime().block_on(async {
        let (client, mut peer) =
            test_transport::client_for_current_route(false, SessionMcpPolicy::default(), None);
        open(&client, &mut peer, Some("grok-4.6")).await;
        let manager = Arc::new(fixture.manager(&client, "process-a"));
        let foreign = if background {
            let target = manager.inner.lock().take().unwrap();
            manager
                .background
                .lock()
                .insert(fixture.meta.id.clone(), target);
            let foreign = store::create_session(None, Some("Busy foreign".into()), false).unwrap();
            let mut busy = routing_tests::bare_live_session(&foreign.id, "foreign-process");
            busy.meta = foreign.clone();
            busy.fsm.begin_stream().unwrap();
            busy.prompt_in_flight = true;
            *manager.inner.lock() = Some(busy);
            Some(foreign)
        } else {
            None
        };
        let before = fs::read(crate::paths::sessions_index_file()).unwrap();
        // This is the actual process-global store mutex, not a mocked writer.
        let held =
            crate::store_lock::lock_exclusive(&fixture.root.join("slow-writer.json")).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (worker_manager, worker_client, expected) =
            (manager.clone(), client.clone(), fixture.meta.clone());
        let worker = thread::spawn(move || {
            started_tx.send(()).unwrap();
            worker_manager.publish_opened_execution(
                &expected,
                &worker_client,
                "process-a",
                "synthetic-opened-agent",
                false,
            )
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        // Give the worker a scheduling opportunity while IO remains blocked.
        thread::sleep(Duration::from_millis(100));
        let (cancel_tx, cancel_rx) = mpsc::channel();
        let (cancel_manager, target) = (manager.clone(), fixture.meta.id.clone());
        let cancel = thread::spawn(move || {
            let result = cancel_manager.with_session_mut(&target, |session| {
                session.turn_open_epoch = session.turn_open_epoch.wrapping_add(1);
                let result = session.fsm.connect_failed(crate::error::AgentError::new(
                    crate::error::AgentErrorCode::ConnectFailed,
                    "connect cancelled",
                ));
                if result.is_ok() {
                    session.acp = None;
                }
                result.is_ok()
            });
            cancel_tx.send(result == Some(true)).unwrap();
        });
        let cancelled_before_release = cancel_rx.recv_timeout(Duration::from_secs(1));
        // Release and join BEFORE asserting, even in the genuine red case.
        drop(held);
        let published = worker.join().unwrap();
        cancel.join().unwrap();
        let current_state = manager
            .with_session_mut(&fixture.meta.id, |s| s.fsm.state())
            .unwrap();
        let after = fs::read(crate::paths::sessions_index_file()).unwrap();
        if let Some(foreign) = foreign {
            let guard = manager.inner.lock();
            let live = guard.as_ref().unwrap();
            assert_eq!(live.app_session_id, foreign.id);
            assert_eq!(live.fsm.state(), SessionState::Streaming);
            assert!(live.prompt_in_flight);
        }
        client.kill().await;
        assert!(
            cancelled_before_release.is_ok_and(|value| value),
            "Stop waited for the unrelated store writer"
        );
        assert!(published.is_err(), "cancelled connection published Ready");
        assert_eq!(current_state, SessionState::Disconnected);
        assert_eq!(after, before, "cancelled worker changed execution history");
    });
}

#[test]
fn live_stop_can_cancel_publication_while_another_store_writer_is_slow() {
    cancel_during_store_contention(false);
}

#[test]
fn background_stop_can_cancel_publication_without_blocking_foreign_focus() {
    cancel_during_store_contention(true);
}

#[test]
fn dropped_async_completion_revokes_worker_before_timeout_cleanup() {
    let fixture = Fixture::new(false);
    let rt = runtime();
    let (manager, before, timed_out, revoked, runtime_available) = rt.block_on(async {
        let (client, mut peer) =
            test_transport::client_for_current_route(false, SessionMcpPolicy::default(), None);
        open(&client, &mut peer, Some("grok-4.6")).await;
        let manager = Arc::new(fixture.manager(&client, "process-a"));
        let lease = manager
            .with_session_mut(&fixture.meta.id, |s| s.fsm.connection_lease().unwrap())
            .unwrap();
        let before = fs::read(crate::paths::sessions_index_file()).unwrap();
        let held =
            crate::store_lock::lock_exclusive(&fixture.root.join("slow-writer.json")).unwrap();
        let result = tokio::time::timeout(
            Duration::from_millis(80),
            manager.complete_opened_execution(
                &fixture.meta,
                &client,
                "process-a",
                "synthetic-opened-agent",
                false,
            ),
        )
        .await;
        let revoked = lease.check().is_err();
        let runtime_available = manager.inner.try_lock().is_some();
        drop(held);
        client.kill().await;
        (manager, before, result.is_err(), revoked, runtime_available)
    });
    // Runtime destruction joins its blocking workers; inspect final state only
    // AFTER that, not from a sleep or an assumed detached-worker completion.
    drop(rt);
    assert!(timed_out);
    assert!(
        revoked,
        "dropping the real async future did not revoke publication"
    );
    assert!(runtime_available);
    assert_eq!(
        fs::read(crate::paths::sessions_index_file()).unwrap(),
        before
    );
    assert_eq!(
        manager
            .with_session_mut(&fixture.meta.id, |s| s.fsm.state())
            .unwrap(),
        SessionState::Connecting
    );
}

#[test]
fn publication_process_mutex_wait_is_bounded_without_blocking_runtime() {
    let fixture = Fixture::new(false);
    runtime().block_on(async {
        let (client, mut peer) =
            test_transport::client_for_current_route(false, SessionMcpPolicy::default(), None);
        open(&client, &mut peer, Some("grok-4.6")).await;
        let manager = Arc::new(fixture.manager(&client, "process-a"));
        let before = fs::read(crate::paths::sessions_index_file()).unwrap();
        let held =
            crate::store_lock::lock_exclusive(&fixture.root.join("slow-writer.json")).unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let (worker_manager, worker_client, expected) =
            (manager.clone(), client.clone(), fixture.meta.clone());
        let worker = thread::spawn(move || {
            let result = worker_manager.publish_opened_execution(
                &expected,
                &worker_client,
                "process-a",
                "synthetic-opened-agent",
                false,
            );
            done_tx.send(result).unwrap();
        });
        let result = done_rx.recv_timeout(Duration::from_secs(5));
        let runtime_available = manager.inner.try_lock().is_some();
        drop(held);
        worker.join().unwrap();
        client.kill().await;
        assert!(runtime_available);
        let error = result
            .expect("publication waited indefinitely for the process store mutex")
            .unwrap_err();
        assert!(crate::store_lock::is_lock_busy(&error), "{error}");
        assert_eq!(
            fs::read(crate::paths::sessions_index_file()).unwrap(),
            before
        );
        assert_eq!(
            manager
                .with_session_mut(&fixture.meta.id, |s| s.fsm.state())
                .unwrap(),
            SessionState::Connecting
        );
    });
}

#[test]
fn invalid_settings_are_not_migrated_quarantined_or_replaced_with_defaults() {
    let fixture = Fixture::new(false);
    runtime().block_on(async {
        let (client, mut peer) =
            test_transport::client_for_current_route(false, SessionMcpPolicy::default(), None);
        open(&client, &mut peer, Some("grok-4.6")).await;
        let manager = Arc::new(fixture.manager(&client, "process-a"));
        let before = fs::read(crate::paths::sessions_index_file()).unwrap();
        fs::write(crate::paths::settings_file(), b"{invalid").unwrap();
        assert!(manager
            .complete_opened_execution(
                &fixture.meta,
                &client,
                "process-a",
                "synthetic-opened-agent",
                false,
            )
            .await
            .is_err());
        assert_eq!(
            fs::read(crate::paths::settings_file()).unwrap(),
            b"{invalid"
        );
        assert_eq!(
            fs::read(crate::paths::sessions_index_file()).unwrap(),
            before
        );
        assert_eq!(
            manager
                .with_session_mut(&fixture.meta.id, |s| s.fsm.state())
                .unwrap(),
            SessionState::Connecting
        );
        client.kill().await;
    });
}

#[test]
fn late_publication_preserves_newer_runtime_preferences_and_title() {
    let fixture = Fixture::new(false);
    runtime().block_on(async {
        let (client, mut peer) =
            test_transport::client_for_current_route(false, SessionMcpPolicy::default(), None);
        open(&client, &mut peer, Some("grok-4.6")).await;
        let manager = Arc::new(fixture.manager(&client, "process-a"));
        let held =
            crate::store_lock::lock_exclusive(&fixture.root.join("slow-writer.json")).unwrap();
        let (worker_manager, worker_client, expected) =
            (manager.clone(), client.clone(), fixture.meta.clone());
        let worker = thread::spawn(move || {
            worker_manager.publish_opened_execution(
                &expected,
                &worker_client,
                "process-a",
                "synthetic-opened-agent",
                false,
            )
        });
        thread::sleep(Duration::from_millis(100));
        // Busy/connecting desired intent is not an execution observation.
        let runtime_updated = manager
            .inner
            .try_lock_for(Duration::from_secs(1))
            .map(|mut slot| {
                let live = slot.as_mut().unwrap();
                live.meta.provider_id = Some("latest-desired-channel".into());
                live.meta.model_id = Some("latest-desired-model".into());
                live.meta.title = "Latest runtime title".into();
            })
            .is_some();
        drop(held);
        let result = worker.join().unwrap();
        client.kill().await;
        assert!(runtime_updated);
        result.unwrap();
        let guard = manager.inner.lock();
        let live = guard.as_ref().unwrap();
        assert_eq!(
            live.meta.provider_id.as_deref(),
            Some("latest-desired-channel")
        );
        assert_eq!(live.meta.model_id.as_deref(), Some("latest-desired-model"));
        assert_eq!(live.meta.title, "Latest runtime title");
        assert_eq!(live.fsm.state(), SessionState::Ready);
        assert_eq!(
            live.meta
                .agent_execution
                .as_ref()
                .unwrap()
                .model_id
                .as_deref(),
            Some("grok-4.6")
        );
    });
}
