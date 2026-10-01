use super::*;
use std::sync::{Condvar, Mutex};

struct HeldHost {
    hold: AtomicBool,
    entered: AtomicBool,
    release: (Mutex<bool>, Condvar),
}
impl PortalInputPolicy for HeldHost {
    fn input_available(&self) -> bool {
        if self.hold.load(Ordering::SeqCst) {
            self.entered.store(true, Ordering::SeqCst);
            let (_guard, timeout) = self
                .release
                .1
                .wait_timeout_while(
                    self.release.0.lock().unwrap(),
                    Duration::from_secs(3),
                    |released| !*released,
                )
                .unwrap();
            assert!(
                !timeout.timed_out(),
                "test failed to release original Host callback"
            );
        }
        true
    }
    fn user_input_active(&self) -> bool {
        false
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forwarded_policy_response_cannot_outlive_native_monitor_owner() {
    let fixture = Fixture::new(Snapshot::default()).await;
    let host = Arc::new(HeldHost {
        hold: AtomicBool::new(false),
        entered: AtomicBool::new(false),
        release: (Mutex::new(false), Condvar::new()),
    });
    let watch = fixture.watch_with(host.clone()).await.unwrap();
    let policy = watch.input_policy();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(stopped));
    ready(&policy).await;
    host.hold.store(true, Ordering::SeqCst);
    let forwarded = std::thread::spawn(move || policy.input_available());
    tokio::time::timeout(Duration::from_secs(1), async {
        while !host.entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    stop.send(()).unwrap();
    owner.await.unwrap().unwrap();
    *host.release.0.lock().unwrap() = true;
    host.release.1.notify_all();
    assert!(
        !forwarded.join().unwrap(),
        "late Host bool published authority after original OS monitor joined"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lock_signal_closes_original_pending_portal_without_another_model_call() {
    use crate::tests::registry::{closed, options};
    use grok_computer_use_core::adapter::ComputerUseAdapter;
    let login = Fixture::new(Snapshot::default()).await;
    let watch = login.watch().await.unwrap();
    let policy = watch.input_policy();
    let (_stop, stopped) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(stopped));
    ready(&policy).await;
    let portal = crate::tests::Fixture::new(crate::tests::Mode::SilentStart).await;
    let registry = crate::PortalRegistry::new(policy.clone());
    let address = portal.bus.address.clone();
    let ticket = registry
        .select_with(options("native-logind-pending"), 1, 1, move |o| {
            crate::PortalSession::on_test_bus(o, address)
        })
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while !portal.shared.called("Start") {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    login.changed("LockedHint", true.into()).await;
    assert!(tokio::time::timeout(Duration::from_secs(1), owner)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    closed(&registry, &ticket).await;
    assert!(registry.is_idle("native-logind-pending"));
    portal.assert_sessions_closed();
    login.changed("LockedHint", false.into()).await;
    assert!(
        !policy.input_available(),
        "unlock signal revived old OS policy"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn untrusted_bus_peer_cannot_spoof_the_pinned_login1_session() {
    let f = Fixture::new(Snapshot::default()).await;
    let watch = f.watch().await.unwrap();
    let policy = watch.input_policy();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(stopped));
    ready(&policy).await;
    let peer = zbus::connection::Builder::address(f.bus.address.as_str())
        .unwrap()
        .build()
        .await
        .unwrap();
    peer.emit_signal(None::<&str>, PATH, SESSION, "Lock", &())
        .await
        .unwrap();
    peer.emit_signal(None::<&str>, ROOT, MANAGER, "PrepareForSleep", &(true,))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        policy.input_available(),
        "unowned peer changed the pinned OS session"
    );
    stop.send(()).unwrap();
    owner.await.unwrap().unwrap();
}
