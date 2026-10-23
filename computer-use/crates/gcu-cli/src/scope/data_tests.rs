use super::*;
use gcu_core::scope::{Clock, FixtureScope, Monotonic};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::AsyncReadExt;

#[derive(Debug, Default)]
struct HeldClock(AtomicU64);
impl Clock for HeldClock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}
fn scope() -> FixtureScope {
    FixtureScope::try_new("run", 1, "engine", "desktop", "control", vec![]).unwrap()
}

#[tokio::test]
async fn bridge_checks_both_byte_directions_with_every_watcher_held_back() {
    tokio::time::timeout(Duration::from_secs(5), async {
        for outbound in [false, true] {
            let clock = Arc::new(HeldClock::default());
            let lease =
                ScopeLease::install_with_deadline(scope(), clock.clone(), Duration::from_secs(3));
            let authority = lease.arm_attempt().unwrap();
            let (mut network, socket) = tokio::io::duplex(4096);
            let (bridge, mut mcp) = tokio::io::duplex(4096);
            // The exact production copy future, with NO changed()/watch/timer
            // future. Its per-poll usable check must refuse the ready bytes.
            let copy = tokio::spawn(copy_authorized(socket, bridge, authority));
            network.write_all(b"in").await.unwrap();
            let mut inbound = [0; 2];
            mcp.read_exact(&mut inbound).await.unwrap();
            assert_eq!(&inbound, b"in");
            mcp.write_all(b"out").await.unwrap();
            let mut outbound_bytes = [0; 3];
            network.read_exact(&mut outbound_bytes).await.unwrap();
            assert_eq!(&outbound_bytes, b"out");
            clock.0.store(4, Ordering::SeqCst);
            if outbound {
                mcp.write_all(b"CANARY").await.unwrap();
            } else {
                network.write_all(b"CANARY").await.unwrap();
            }
            assert!(copy.await.unwrap().is_err());
            let mut leaked = Vec::new();
            if outbound {
                network.read_to_end(&mut leaked).await.unwrap();
            } else {
                mcp.read_to_end(&mut leaked).await.unwrap();
            }
            assert!(
                leaked.is_empty(),
                "bridge forwarded expired bytes: {leaked:?}"
            );
        }
    })
    .await
    .unwrap();
}

#[tokio::test(start_paused = true)]
async fn idle_bridge_wakes_at_the_engine_deadline_without_control_watching() {
    tokio::time::timeout(Duration::from_secs(6), async {
        let clock = Arc::new(Monotonic::default());
        let lease = ScopeLease::install_with_deadline(scope(), clock, Duration::from_secs(3));
        let authority = lease.arm_attempt().unwrap();
        let (mut network, socket) = tokio::io::duplex(4096);
        let (bridge, _mcp) = tokio::io::duplex(4096);
        let copy = tokio::spawn(copy_scoped(socket, bridge, authority));
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(3)).await;
        assert!(copy.await.unwrap().is_err());
        let mut bytes = Vec::new();
        network.read_to_end(&mut bytes).await.unwrap();
        assert!(bytes.is_empty());
    })
    .await
    .unwrap();
}
