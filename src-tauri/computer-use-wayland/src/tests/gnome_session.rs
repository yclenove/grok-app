//! Two actual private D-Bus connections; never invoke Lock on the user's Shell.
#[path = "gnome_native.rs"]
pub(in crate::tests) mod native;
#[path = "gnome_owner_lifetime.rs"]
mod owner_lifetime;
use super::*;
use crate::GnomeSessionWatch;
const SHELL: &str = "org.gnome.Shell";
const SHIELD: &str = "org.gnome.Shell.ScreenShield";
const SHIELD_PATH: &str = "/org/gnome/ScreenSaver";
const IFACE: &str = "org.gnome.ScreenSaver";

struct Screen {
    active: bool,
    pulse: bool,
    fail: bool,
}
#[zbus::interface(name = "org.gnome.ScreenSaver")]
impl Screen {
    async fn get_active(
        &self,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<bool> {
        if self.fail {
            return Err(zbus::fdo::Error::Failed("shield snapshot failed".into()));
        }
        if self.pulse {
            for active in [true, false] {
                connection
                    .emit_signal(
                        None::<&str>,
                        SHIELD_PATH,
                        IFACE,
                        "ActiveChanged",
                        &(active,),
                    )
                    .await
                    .unwrap();
            }
        }
        Ok(self.active)
    }
}

struct Shell {
    bus: Bus,
    server: Connection,
}
impl Shell {
    async fn new(active: bool, pulse: bool, fail: bool) -> Self {
        Self::on_bus(Bus::start(), active, pulse, fail).await
    }
    async fn on_bus(bus: Bus, active: bool, pulse: bool, fail: bool) -> Self {
        let server = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name(SHELL)
            .unwrap()
            .name(SHIELD)
            .unwrap()
            .serve_at(
                SHIELD_PATH,
                Screen {
                    active,
                    pulse,
                    fail,
                },
            )
            .unwrap()
            .build()
            .await
            .unwrap();
        Self { bus, server }
    }
    async fn client(&self) -> Connection {
        zbus::connection::Builder::address(self.bus.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap()
    }
    async fn watch(&self, login: &Fixture) -> Result<GnomeSessionWatch, String> {
        GnomeSessionWatch::prepare(login.watch().await?, self.client().await).await
    }
    async fn active(&self, value: bool) {
        self.server
            .emit_signal(None::<&str>, SHIELD_PATH, IFACE, "ActiveChanged", &(value,))
            .await
            .unwrap();
    }
}
async fn ended(owner: tokio::task::JoinHandle<Result<(), String>>) -> String {
    tokio::time::timeout(Duration::from_secs(2), owner)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err()
}

pub(in crate::tests) struct NativePolicySource {
    _login: Fixture,
    shell: Shell,
}
impl NativePolicySource {
    pub(in crate::tests) async fn start() -> (Self, GnomeSessionWatch) {
        let login = Fixture::new(Snapshot::default()).await;
        let shell = Shell::new(false, false, false).await;
        let watch = shell.watch(&login).await.unwrap();
        (
            Self {
                _login: login,
                shell,
            },
            watch,
        )
    }
    pub(in crate::tests) async fn set_active(&self, active: bool) {
        self.shell.active(active).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn late_host_reply_cannot_outlive_either_composite_subscription() {
    use std::sync::{Condvar, Mutex};
    struct HeldHost {
        entered: AtomicBool,
        release: (Mutex<bool>, Condvar),
    }
    impl PortalInputPolicy for HeldHost {
        fn input_available(&self) -> bool {
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
            assert!(!timeout.timed_out());
            true
        }
        fn user_input_active(&self) -> bool {
            false
        }
    }
    let login = Fixture::new(Snapshot::default()).await;
    let shell = Shell::new(false, false, false).await;
    let host = Arc::new(HeldHost {
        entered: AtomicBool::new(false),
        release: (Mutex::new(false), Condvar::new()),
    });
    let watch = GnomeSessionWatch::prepare(
        login.watch_with(host.clone()).await.unwrap(),
        shell.client().await,
    )
    .await
    .unwrap();
    let policy = watch.input_policy();
    let (_stop, stopped) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(stopped));
    let forwarded = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            let value = policy.input_available();
            if value || policy.user_input_active() {
                return value;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while !host.entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    shell.active(true).await;
    assert!(ended(owner).await.contains("activated"));
    *host.release.0.lock().unwrap() = true;
    host.release.1.notify_all();
    assert!(
        !forwarded.join().unwrap(),
        "late callback revived a closed composite"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn composite_requires_both_original_monitors_and_closes_both_on_stop() {
    let login = Fixture::new(Snapshot::default()).await;
    let shell = Shell::new(false, false, false).await;
    let login_watch = login.watch().await.unwrap();
    let login_policy = login_watch.input_policy();
    let watch = GnomeSessionWatch::prepare(login_watch, shell.client().await)
        .await
        .unwrap();
    let policy = watch.input_policy();
    assert!(!policy.input_available());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(stopped));
    ready(&policy).await;
    assert_eq!(
        login.snapshot.pid_queries.load(Ordering::SeqCst),
        2,
        "Shell peer was not bound to login1"
    );
    login.host.deny.store(true, Ordering::SeqCst);
    assert!(!policy.input_available());
    login.host.deny.store(false, Ordering::SeqCst);
    login.host.takeover.store(true, Ordering::SeqCst);
    assert!(policy.user_input_active(), "physical Host policy bypassed");
    stop.send(()).unwrap();
    owner.await.unwrap().unwrap();
    login.host.takeover.store(false, Ordering::SeqCst);
    assert!(!policy.input_available());
    assert!(
        !login_policy.input_available(),
        "composite leaked its original login1 future"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_failed_missing_mismatched_or_foreign_session_never_grants() {
    for case in 0..6 {
        let login = Fixture::new(Snapshot {
            peer_other_session: case == 5,
            ..Snapshot::default()
        })
        .await;
        let shell = Shell::new(case == 0, false, case == 1).await;
        let mut replacement = None;
        if case == 2 {
            shell.server.release_name(SHIELD).await.unwrap();
        }
        if matches!(case, 3 | 4) {
            shell.server.release_name(SHELL).await.unwrap();
        }
        if case == 4 {
            let other = shell.client().await;
            other.request_name(SHELL).await.unwrap();
            replacement = Some(other);
        }
        let err = match shell.watch(&login).await {
            Ok(_) => panic!("unsafe case {case} accepted"),
            Err(e) => e,
        };
        let expected = match case {
            0 => "screen shield is active",
            1 => "shield snapshot failed",
            2 | 3 => "NameHasNoOwner",
            4 => "owner mismatch",
            5 => "different login session",
            _ => unreachable!(),
        };
        assert!(err.contains(expected), "case {case}: {err}");
        drop(replacement);
    }
    let login = Fixture::new(Snapshot::default()).await;
    let mut watch = login.watch().await.unwrap();
    assert!(watch
        .verify_peer(0, unsafe { libc::geteuid() })
        .await
        .unwrap_err()
        .contains("invalid PID"));
    assert!(watch
        .verify_peer(
            std::process::id(),
            unsafe { libc::geteuid() }.wrapping_add(1)
        )
        .await
        .unwrap_err()
        .contains("different user"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_activation_pulse_survives_unlocked_snapshot() {
    let login = Fixture::new(Snapshot::default()).await;
    let shell = Shell::new(false, true, false).await;
    let watch = shell.watch(&login).await.unwrap();
    let policy = watch.input_policy();
    let (_stop, stopped) = tokio::sync::oneshot::channel();
    assert!(watch
        .run_until(stopped)
        .await
        .unwrap_err()
        .contains("activated"));
    assert!(!policy.input_available());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_then_inactive_closes_and_never_rearms_old_owner() {
    let login = Fixture::new(Snapshot::default()).await;
    let shell = Shell::new(false, false, false).await;
    let watch = shell.watch(&login).await.unwrap();
    let policy = watch.input_policy();
    let (_stop, stopped) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(stopped));
    ready(&policy).await;
    shell.active(true).await;
    shell.active(false).await;
    assert!(ended(owner).await.contains("activated"));
    assert!(!policy.input_available());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login1_loss_closes_the_composite_while_shield_remains_inactive() {
    let login = Fixture::new(Snapshot::default()).await;
    let shell = Shell::new(false, false, false).await;
    let watch = shell.watch(&login).await.unwrap();
    let policy = watch.input_policy();
    let (_stop, stopped) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(stopped));
    ready(&policy).await;
    login.changed("Active", false.into()).await;
    assert!(ended(owner).await.contains("session safety state changed"));
    shell.active(false).await;
    assert!(!policy.input_available());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn either_shell_name_loss_is_terminal_and_replacement_is_not_recovery() {
    for name in [SHELL, SHIELD] {
        let login = Fixture::new(Snapshot::default()).await;
        let shell = Shell::new(false, false, false).await;
        let watch = shell.watch(&login).await.unwrap();
        let policy = watch.input_policy();
        let (_stop, stopped) = tokio::sync::oneshot::channel();
        let owner = tokio::spawn(watch.run_until(stopped));
        ready(&policy).await;
        shell.server.release_name(name).await.unwrap();
        let replacement = shell.client().await;
        replacement.request_name(name).await.unwrap();
        assert!(ended(owner).await.contains("owner lost or replaced"));
        assert!(!policy.input_available());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_bus_disconnect_closes_both_original_monitors() {
    let login = Fixture::new(Snapshot::default()).await;
    let mut shell = Shell::new(false, false, false).await;
    let login_watch = login.watch().await.unwrap();
    let login_policy = login_watch.input_policy();
    let watch = GnomeSessionWatch::prepare(login_watch, shell.client().await)
        .await
        .unwrap();
    let policy = watch.input_policy();
    let (_stop, stopped) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(stopped));
    ready(&policy).await;
    shell.bus.child.kill().unwrap();
    shell.bus.child.wait().unwrap();
    ended(owner).await;
    assert!(!policy.input_available());
    assert!(!login_policy.input_available());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unrelated_sender_path_interface_and_inactive_signals_do_not_revoke() {
    let login = Fixture::new(Snapshot::default()).await;
    let shell = Shell::new(false, false, false).await;
    let watch = shell.watch(&login).await.unwrap();
    let policy = watch.input_policy();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(stopped));
    ready(&policy).await;
    let peer = shell.client().await;
    peer.emit_signal(None::<&str>, SHIELD_PATH, IFACE, "ActiveChanged", &(true,))
        .await
        .unwrap();
    shell
        .server
        .emit_signal(
            None::<&str>,
            "/org/gnome/Other",
            IFACE,
            "ActiveChanged",
            &(true,),
        )
        .await
        .unwrap();
    shell
        .server
        .emit_signal(
            None::<&str>,
            SHIELD_PATH,
            "org.gnome.Other",
            "ActiveChanged",
            &(true,),
        )
        .await
        .unwrap();
    peer.request_name("org.gnome.Unrelated").await.unwrap();
    shell.active(false).await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(policy.input_available());
    stop.send(()).unwrap();
    owner.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_active_signal_fails_closed() {
    let login = Fixture::new(Snapshot::default()).await;
    let shell = Shell::new(false, false, false).await;
    let watch = shell.watch(&login).await.unwrap();
    let policy = watch.input_policy();
    let (_stop, stopped) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(stopped));
    ready(&policy).await;
    shell
        .server
        .emit_signal(
            None::<&str>,
            SHIELD_PATH,
            IFACE,
            "ActiveChanged",
            &("invalid",),
        )
        .await
        .unwrap();
    assert!(ended(owner).await.contains("GNOME session monitor"));
    assert!(!policy.input_available());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn drop_unpolled_precancel_and_abort_all_revoke_original_owners() {
    for case in 0..3 {
        let login = Fixture::new(Snapshot::default()).await;
        let shell = Shell::new(false, false, false).await;
        let watch = shell.watch(&login).await.unwrap();
        let policy = watch.input_policy();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        match case {
            0 => drop(watch.run_until(stopped)),
            1 => {
                drop(stop);
                watch.run_until(stopped).await.unwrap();
            }
            _ => {
                let owner = tokio::spawn(watch.run_until(stopped));
                ready(&policy).await;
                owner.abort();
                assert!(owner.await.unwrap_err().is_cancelled());
            }
        }
        assert!(!policy.input_available());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shield_closes_original_pending_consent_without_a_model_call() {
    use crate::tests::registry::{closed, options};
    use grok_computer_use_core::adapter::ComputerUseAdapter;
    let login = Fixture::new(Snapshot::default()).await;
    let shell = Shell::new(false, false, false).await;
    let watch = shell.watch(&login).await.unwrap();
    let policy = watch.input_policy();
    let (_stop, stopped) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(stopped));
    ready(&policy).await;
    let portal = crate::tests::Fixture::new(crate::tests::Mode::SilentStart).await;
    let registry = crate::PortalRegistry::new(policy.clone());
    let address = portal.bus.address.clone();
    let ticket = registry
        .select_with(options("gnome-pending"), 1, 1, move |o| {
            crate::PortalSession::on_test_bus(o, address)
        })
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !portal.shared.called("Start") {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    shell.active(true).await;
    assert!(ended(owner).await.contains("activated"));
    closed(&registry, &ticket).await;
    portal.assert_sessions_closed();
    assert!(registry.is_idle("gnome-pending"));
    shell.active(false).await;
    assert!(!policy.input_available());
}
