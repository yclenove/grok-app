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
    let control = tokio::spawn(control(
        engine,
        incoming,
        stop.clone(),
        sessions.clone(),
        driver.clone(),
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
    drop(locked);
    host.close();
    control.await.unwrap();
}
