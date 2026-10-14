//! Injected physical activity and remote-disconnect Stop tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use gcu_host_protocol::{
    auth::*,
    server::{Incoming, Peer},
};
struct Physical;
impl PhysicalActivity for Physical {
    fn ms_since_physical_input(&self) -> Option<u64> {
        Some(42)
    }
}
async fn connected() -> (Peer, Incoming, Peer) {
    let (a, b) = tokio::io::duplex(8192);
    let c = AuthConfig::current(Role::Host, Channel::Rpc).unwrap();
    let mut e = c.clone();
    e.role = Role::Engine;
    let expected = c.instance.clone();
    let ep = expected.clone();
    let secret = Secret::random();
    let other = secret.clone();
    let task = tokio::spawn(async move { Peer::connect(a, &c, &expected, &secret).await.unwrap() });
    let (engine, _, _) = Peer::connect(b, &e, &ep, &other).await.unwrap();
    let (host, incoming, _) = task.await.unwrap();
    (host, incoming, engine)
}
#[tokio::test]
async fn physical_input_is_engine_injected_and_disconnect_latches_stop() {
    let (host, _, peer) = connected().await;
    let stop = StopMirror::new(false);
    let remote = RemoteHost {
        peer,
        session_id: "app-chat".into(),
        policy: Arc::new(Mutex::new((1, PolicyConfig::default()))),
        physical: Arc::new(Physical),
        stop: stop.clone(),
    };
    assert_eq!(remote.ms_since_physical_input(), Some(42));
    assert!(!remote.stop_requested());
    stop.latch(None);
    assert!(remote.stop_requested());
    remote.clear_stop_by_user();
    assert!(!remote.latched_stop());
    host.close();
    remote.peer.disconnected().await;
    assert!(remote.latched_stop());
    remote.clear_stop_by_user();
    assert!(remote.latched_stop());
    let driver = gcu_driver_fake::FakeDriver::from_scene("form").unwrap();
    use gcu_core::driver::Driver;
    let report = remote.doctor(driver.doctor().await);
    assert!(!report
        .issues
        .iter()
        .any(|s| s == "user_activity_guard: unavailable"));
}
#[tokio::test]
async fn unavailable_physical_source_reports_degradation_and_keeps_foreground_asking() {
    let (host, _, peer) = connected().await;
    let remote = RemoteHost {
        peer,
        session_id: "app-chat".into(),
        policy: Arc::new(Mutex::new((1, PolicyConfig::default()))),
        physical: Arc::new(super::super::UnavailableActivity),
        stop: StopMirror::new(false),
    };
    let driver = gcu_driver_fake::FakeDriver::from_scene("form").unwrap();
    use gcu_core::driver::Driver;
    let report = remote.doctor(driver.doctor().await);
    assert!(!report.capabilities.user_activity_guard);
    assert!(report
        .issues
        .iter()
        .any(|s| s == "user_activity_guard: unavailable"));
    assert_eq!(remote.load().allow_foreground, ForegroundPolicy::Ask);
    host.close();
}
