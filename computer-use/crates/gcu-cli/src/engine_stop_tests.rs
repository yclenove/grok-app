//! Stop never waits for ordinary session-map ownership.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;

#[tokio::test]
async fn stop_locked_session_map_acknowledges_and_releases_independently() {
    let (a, b) = tokio::io::duplex(8192);
    let host = AuthConfig::current(Role::Host, Channel::Stop).unwrap();
    let mut engine = host.clone();
    engine.role = Role::Engine;
    let secret = Secret::random();
    let other = secret.clone();
    let expected = host.instance.clone();
    let ep = expected.clone();
    let job =
        tokio::spawn(async move { Peer::connect(a, &host, &expected, &secret).await.unwrap() });
    let (engine, incoming, _) = Peer::connect(b, &engine, &ep, &other).await.unwrap();
    let (host, _, _) = job.await.unwrap();
    let sessions: Sessions = Arc::new(RwLock::new(HashMap::new()));
    let locked = sessions.write().await;
    let stop = StopMirror::new(false);
    let driver = Arc::new(gcu_driver_fake::FakeDriver::contract_fixture().unwrap());
    let (_keep_sender, local_stop) = tokio::sync::mpsc::channel(1);
    let marker = tempfile::tempdir().unwrap();
    let control = tokio::spawn(control(
        engine,
        incoming,
        stop.clone(),
        sessions.clone(),
        driver.clone(),
        local_stop,
        Some(marker.path().to_path_buf()),
    ));
    let _: w::Empty = host
        .call(
            "engine.session.stop",
            &w::Stop { session_id: None },
            Duration::from_millis(500),
        )
        .await
        .unwrap();
    assert!(stop.stopped("fixture"));
    // The session-map guard is still held; no ordinary lifecycle operation can
    // have run. The independent release task must nevertheless be scheduled.
    for _ in 0..100 {
        if driver.release_calls() > 0 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(driver.release_calls() > 0);
    assert!(!marker.path().join("stop-reason").exists());
    drop(locked);
    host.close();
    control.await.unwrap();
}

#[tokio::test]
async fn local_stop_latches_releases_writes_marker_then_disconnects() {
    let (a, b) = tokio::io::duplex(8192);
    let host_auth = AuthConfig::current(Role::Host, Channel::Stop).unwrap();
    let mut engine_auth = host_auth.clone();
    engine_auth.role = Role::Engine;
    let secret = Secret::random();
    let other = secret.clone();
    let expected = host_auth.instance.clone();
    let ep = expected.clone();
    let job = tokio::spawn(async move {
        Peer::connect(a, &host_auth, &expected, &secret)
            .await
            .unwrap()
    });
    let (engine, incoming, _) = Peer::connect(b, &engine_auth, &ep, &other).await.unwrap();
    let (host, _, _) = job.await.unwrap();
    let sessions: Sessions = Arc::new(RwLock::new(HashMap::new()));
    let locked = sessions.write().await;
    let stop = StopMirror::new(false);
    let driver = Arc::new(gcu_driver_fake::FakeDriver::contract_fixture().unwrap());
    let marker = tempfile::tempdir().unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let control = tokio::spawn(control(
        engine,
        incoming,
        stop.clone(),
        sessions.clone(),
        driver.clone(),
        rx,
        Some(marker.path().to_path_buf()),
    ));
    tx.send(gcu_host_protocol::stop_reason::StopReason::Hotkey)
        .await
        .unwrap();
    for _ in 0..200 {
        if stop.stopped("fixture") && driver.release_calls() > 0 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(stop.stopped("fixture"));
    assert!(driver.release_calls() > 0);
    drop(locked);
    let path = marker.path().join("stop-reason");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline && host.connected() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!host.connected());
    assert_eq!(std::fs::read(&path).unwrap(), b"hotkey\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dir_mode = std::fs::metadata(marker.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
    }
    control.await.unwrap();
}

#[tokio::test]
async fn dropped_local_stop_sender_is_not_a_stop() {
    let (a, b) = tokio::io::duplex(8192);
    let host_auth = AuthConfig::current(Role::Host, Channel::Stop).unwrap();
    let mut engine_auth = host_auth.clone();
    engine_auth.role = Role::Engine;
    let secret = Secret::random();
    let other = secret.clone();
    let expected = host_auth.instance.clone();
    let ep = expected.clone();
    let job = tokio::spawn(async move {
        Peer::connect(a, &host_auth, &expected, &secret)
            .await
            .unwrap()
    });
    let (engine, incoming, _) = Peer::connect(b, &engine_auth, &ep, &other).await.unwrap();
    let (host, _, _) = job.await.unwrap();
    let sessions: Sessions = Arc::new(RwLock::new(HashMap::new()));
    let stop = StopMirror::new(false);
    let driver = Arc::new(gcu_driver_fake::FakeDriver::contract_fixture().unwrap());
    let marker = tempfile::tempdir().unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let control = tokio::spawn(control(
        engine,
        incoming,
        stop.clone(),
        sessions,
        driver,
        rx,
        Some(marker.path().to_path_buf()),
    ));
    drop(tx);
    let _: w::Empty = host
        .call(
            "engine.session.stop",
            &w::Stop { session_id: None },
            Duration::from_millis(500),
        )
        .await
        .unwrap();
    assert!(host.connected());
    assert!(!marker.path().join("stop-reason").exists());
    assert!(stop.stopped("fixture"));
    host.close();
    control.await.unwrap();
}
