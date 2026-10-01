use super::{fixture::*, lost};
use crate::PortalInputPolicy;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::Duration,
};
use zbus::zvariant::{OwnedValue, Value};

async fn blocked(policy: &Arc<dyn PortalInputPolicy>) {
    tokio::time::timeout(Duration::from_secs(1), async {
        while policy.input_available() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn held_get_user_resumes_on_the_original_server_executor() {
    let f = Fixture::new(State::default(), Session::default()).await;
    f.state.hold_user();
    let connection = zbus::connection::Builder::address(f.bus.address.as_str())
        .unwrap()
        .build()
        .await
        .unwrap();
    let request = tokio::spawn(async move {
        let proxy = zbus::Proxy::new(
            &connection,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .await
        .unwrap();
        proxy
            .call::<_, _, zbus::zvariant::OwnedObjectPath>(
                "GetUser",
                &(unsafe { libc::geteuid() },),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while f.state.user_lookup_reads.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    let pending = !request.is_finished();
    f.state.release_user();
    let response = tokio::time::timeout(Duration::from_secs(1), request)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(pending, "held request must not fail or complete early");
    assert_eq!(response.as_str(), USER_PATH);
    assert_eq!(f.state.user_lookup_reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn stalled_inventory_read_expires_without_rebinding() {
    let f = Fixture::new(State::default(), Session::default()).await;
    let (policy, _stop, owner) = f.started().await;
    f.state.hold_user();
    f.manager_signal("SessionNew", session_ref("ssh", SSH))
        .await;
    blocked(&policy).await;
    assert!(lost(owner).await.contains("deadline"));
    f.state.release_user();
    assert!(!policy.input_available());
}

#[tokio::test]
async fn original_owner_loss_interrupts_pending_inventory_read() {
    let f = Fixture::new(State::default(), Session::default()).await;
    let (policy, _stop, owner) = f.started().await;
    f.state.hold_user();
    f.manager_signal("SessionNew", session_ref("ssh", SSH))
        .await;
    blocked(&policy).await;
    f.server
        .release_name("org.freedesktop.login1")
        .await
        .unwrap();
    let error = tokio::time::timeout(Duration::from_millis(300), owner)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.contains("owner lost"));
    assert!(!policy.input_available());
    f.state.release_user();
}

#[tokio::test]
async fn pid_reassociation_and_signal_carried_ambiguity_are_permanent_loss() {
    for mode in 0..3 {
        let f = Fixture::new(State::default(), Session::default()).await;
        let (policy, _stop, owner) = f.started().await;
        match mode {
            0 => {
                f.state.foreign_user.store(true, Ordering::SeqCst);
                f.manager_signal("SessionNew", session_ref("ssh", SSH))
                    .await;
            }
            1 => {
                f.state
                    .direct
                    .lock()
                    .unwrap()
                    .insert(std::process::id(), SSH.try_into().unwrap());
                f.manager_signal("SessionNew", session_ref("ssh", SSH))
                    .await;
            }
            _ => {
                let refs = vec![
                    session_ref("c42", super::super::logind::PATH),
                    session_ref("other", OTHER),
                ];
                f.changed(
                    HashMap::from([(
                        "Sessions".into(),
                        OwnedValue::try_from(Value::from(refs)).unwrap(),
                    )]),
                    vec![],
                )
                .await;
            }
        }
        let error = lost(owner).await;
        assert!(
            error.contains(
                [
                    "association changed",
                    "different login session",
                    "graphical"
                ][mode]
            ),
            "{error}"
        );
        assert!(!policy.input_available());
    }
}

#[tokio::test]
async fn queued_display_loss_is_not_overwritten_before_first_poll() {
    let f = Fixture::new(State::default(), Session::default()).await;
    let watch = f.watch().await.unwrap();
    let policy = watch.input_policy();
    f.changed(HashMap::new(), vec!["Display".into()]).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    let (_stop, recv) = tokio::sync::oneshot::channel();
    assert!(watch
        .run_until(recv)
        .await
        .unwrap_err()
        .contains("invalidated"));
    assert!(!policy.input_available());
}

struct HeldHost {
    hold: AtomicBool,
    entered: AtomicBool,
    release: (Mutex<bool>, Condvar),
}
impl PortalInputPolicy for HeldHost {
    fn input_available(&self) -> bool {
        if self.hold.load(Ordering::SeqCst) {
            self.entered.store(true, Ordering::SeqCst);
            let (_held, timed) = self
                .release
                .1
                .wait_timeout_while(
                    self.release.0.lock().unwrap(),
                    Duration::from_secs(4),
                    |v| !*v,
                )
                .unwrap();
            assert!(!timed.timed_out());
        }
        true
    }
    fn user_input_active(&self) -> bool {
        false
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_refresh_callback_cannot_publish_late_true_after_reopen() {
    let f = Fixture::new(State::default(), Session::default()).await;
    let host = Arc::new(HeldHost {
        hold: AtomicBool::new(false),
        entered: AtomicBool::new(false),
        release: (Mutex::new(false), Condvar::new()),
    });
    let watch = f.watch_with(host.clone()).await.unwrap();
    let policy = watch.input_policy();
    let (stop, recv) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(recv));
    super::super::logind::ready(&policy).await;
    host.hold.store(true, Ordering::SeqCst);
    let forwarded = policy.clone();
    let original = std::thread::spawn(move || forwarded.input_available());
    tokio::time::timeout(Duration::from_secs(1), async {
        while !host.entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    let reads = f.state.user_reads.load(Ordering::SeqCst);
    f.manager_signal("SessionNew", session_ref("ssh", SSH))
        .await;
    f.wait_reads(reads + 2).await;
    host.hold.store(false, Ordering::SeqCst);
    super::super::logind::ready(&policy).await;
    *host.release.0.lock().unwrap() = true;
    host.release.1.notify_all();
    assert!(
        !original.join().unwrap(),
        "pre-fence positive escaped after refresh/reopen"
    );
    assert!(policy.input_available());
    stop.send(()).unwrap();
    owner.await.unwrap().unwrap();
    assert!(!policy.input_available());
}
