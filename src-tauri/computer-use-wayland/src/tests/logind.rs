//! Private real D-Bus transport; no system login session is modified by tests.
#[path = "gnome_session.rs"]
pub(super) mod gnome_session;
#[path = "logind_lifetime.rs"]
mod lifetime;
use super::{adapter::FixturePolicy, Bus};
use crate::{LogindSessionWatch, PortalInputPolicy};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};
use zbus::{
    zvariant::{OwnedObjectPath, OwnedValue, Value},
    Connection,
};

pub(super) const NAME: &str = "org.freedesktop.login1";
pub(super) const ROOT: &str = "/org/freedesktop/login1";
pub(super) const PATH: &str = "/org/freedesktop/login1/session/c42";
pub(super) const MANAGER: &str = "org.freedesktop.login1.Manager";
const SESSION: &str = "org.freedesktop.login1.Session";
pub(super) const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

struct Snapshot {
    active: bool,
    locked: bool,
    remote: bool,
    uid: u32,
    kind: &'static str,
    class: &'static str,
    state: &'static str,
    sleeping: bool,
    shutting_down: bool,
    startup_pulse: bool,
    reject_pid: bool,
    seen_pid: AtomicU32,
    pulse_sent: AtomicBool,
    peer_other_session: bool,
    pid_queries: AtomicU32,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            active: true,
            locked: false,
            remote: false,
            uid: unsafe { libc::geteuid() },
            kind: "wayland",
            class: "user",
            state: "active",
            sleeping: false,
            shutting_down: false,
            startup_pulse: false,
            reject_pid: false,
            seen_pid: AtomicU32::new(0),
            pulse_sent: AtomicBool::new(false),
            peer_other_session: false,
            pid_queries: AtomicU32::new(0),
        }
    }
}

struct Manager(Arc<Snapshot>);
#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl Manager {
    #[zbus(name = "GetSessionByPID")]
    async fn get_session_by_pid(
        &self,
        pid: u32,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        self.0.seen_pid.store(pid, Ordering::SeqCst);
        if self.0.reject_pid {
            return Err(zbus::fdo::Error::Failed("no session for this PID".into()));
        }
        if self.0.startup_pulse {
            for member in ["Lock", "Unlock"] {
                connection
                    .emit_signal(None::<&str>, PATH, SESSION, member, &())
                    .await
                    .unwrap();
            }
            self.0.pulse_sent.store(true, Ordering::SeqCst);
        }
        if self.0.pid_queries.fetch_add(1, Ordering::SeqCst) > 0 && self.0.peer_other_session {
            return Ok("/org/freedesktop/login1/session/c99".try_into().unwrap());
        }
        Ok(PATH.try_into().unwrap())
    }
    #[zbus(property)]
    fn preparing_for_sleep(&self) -> bool {
        self.0.sleeping
    }
    #[zbus(property)]
    fn preparing_for_shutdown(&self) -> bool {
        self.0.shutting_down
    }
}

struct Session(Arc<Snapshot>);
#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl Session {
    #[zbus(property)]
    fn active(&self) -> bool {
        self.0.active
    }
    #[zbus(property)]
    fn locked_hint(&self) -> bool {
        self.0.locked
    }
    #[zbus(property)]
    fn remote(&self) -> bool {
        self.0.remote
    }
    #[zbus(property)]
    fn user(&self) -> (u32, OwnedObjectPath) {
        (
            self.0.uid,
            "/org/freedesktop/login1/user/u42".try_into().unwrap(),
        )
    }
    #[zbus(property, name = "Type")]
    fn session_type(&self) -> &str {
        self.0.kind
    }
    #[zbus(property)]
    fn class(&self) -> &str {
        self.0.class
    }
    #[zbus(property)]
    fn state(&self) -> &str {
        self.0.state
    }
}

struct Fixture {
    bus: Bus,
    server: Connection,
    snapshot: Arc<Snapshot>,
    host: Arc<FixturePolicy>,
}
impl Fixture {
    async fn new(snapshot: Snapshot) -> Self {
        let bus = Bus::start();
        let snapshot = Arc::new(snapshot);
        let server = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name(NAME)
            .unwrap()
            .serve_at(ROOT, Manager(snapshot.clone()))
            .unwrap()
            .serve_at(PATH, Session(snapshot.clone()))
            .unwrap()
            .build()
            .await
            .unwrap();
        Self {
            bus,
            server,
            snapshot,
            host: Arc::new(FixturePolicy::default()),
        }
    }
    async fn watch(&self) -> Result<LogindSessionWatch, String> {
        self.watch_with(self.host.clone()).await
    }
    async fn watch_with(
        &self,
        host: Arc<dyn PortalInputPolicy>,
    ) -> Result<LogindSessionWatch, String> {
        let connection = zbus::connection::Builder::address(self.bus.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(3),
            LogindSessionWatch::prepare(
                connection,
                std::process::id(),
                unsafe { libc::geteuid() },
                host,
            ),
        )
        .await
        .unwrap()
    }
    async fn changed(&self, key: &str, value: OwnedValue) {
        let values = HashMap::from([(key.to_owned(), value)]);
        self.server
            .emit_signal(
                None::<&str>,
                PATH,
                PROPERTIES,
                "PropertiesChanged",
                &(SESSION, values, Vec::<String>::new()),
            )
            .await
            .unwrap();
    }
}

pub(super) async fn ready(policy: &Arc<dyn PortalInputPolicy>) {
    tokio::time::timeout(Duration::from_secs(1), async {
        while !policy.input_available() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("watch owner never reached monitoring");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exact_local_pid_monitor_requires_polled_owner_and_explicit_join() {
    let f = Fixture::new(Snapshot::default()).await;
    let watch = f.watch().await.unwrap();
    let policy = watch.input_policy();
    assert_eq!(
        f.snapshot.seen_pid.load(Ordering::SeqCst),
        std::process::id()
    );
    assert!(
        !policy.input_available(),
        "unpolled watch is not OS monitoring"
    );
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(stopped));
    ready(&policy).await;
    f.host.deny.store(true, Ordering::SeqCst);
    assert!(!policy.input_available());
    f.host.deny.store(false, Ordering::SeqCst);
    f.host.takeover.store(true, Ordering::SeqCst);
    assert!(policy.user_input_active());
    stop.send(()).unwrap();
    owner.await.unwrap().unwrap();
    f.host.takeover.store(false, Ordering::SeqCst);
    assert!(
        !policy.input_available(),
        "stopped native monitor cannot revive"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initial_missing_or_unsafe_session_fails_without_a_fallback() {
    for case in 0..10 {
        let mut s = Snapshot::default();
        match case {
            0 => s.active = false,
            1 => s.locked = true,
            2 => s.remote = true,
            3 => s.uid = s.uid.wrapping_add(1),
            4 => s.kind = "x11",
            5 => s.class = "greeter",
            6 => s.state = "closing",
            7 => s.sleeping = true,
            8 => s.shutting_down = true,
            9 => s.reject_pid = true,
            _ => unreachable!(),
        }
        let f = Fixture::new(s).await;
        let failure = f.watch().await.err().expect("unsafe session was accepted");
        let expected = match case {
            7 | 8 => "preparing to sleep or shut down",
            9 => "no session for this PID",
            _ => "not this user's unlocked active local Wayland session",
        };
        assert!(
            failure.contains(expected),
            "wrong rejection for case {case}: {failure}"
        );
    }
    let bus = Bus::start();
    let connection = zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .build()
        .await
        .unwrap();
    assert!(LogindSessionWatch::prepare(
        connection,
        std::process::id(),
        unsafe { libc::geteuid() },
        Arc::new(FixturePolicy::default())
    )
    .await
    .is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_lock_unlock_pulse_is_not_overwritten_by_unlocked_snapshot() {
    let f = Fixture::new(Snapshot {
        startup_pulse: true,
        ..Snapshot::default()
    })
    .await;
    let watch = f.watch().await.unwrap();
    let policy = watch.input_policy();
    assert!(f.snapshot.pulse_sent.load(Ordering::SeqCst));
    let (_stop, stopped) = tokio::sync::oneshot::channel();
    let result = tokio::time::timeout(Duration::from_secs(1), watch.run_until(stopped))
        .await
        .unwrap();
    assert!(result.unwrap_err().contains("lock requested"));
    assert!(!policy.input_available());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn safety_signals_revoke_without_an_input_operation_or_permission_reset() {
    for case in 0..9 {
        let f = Fixture::new(Snapshot::default()).await;
        let watch = f.watch().await.unwrap();
        let policy = watch.input_policy();
        let (_stop, stopped) = tokio::sync::oneshot::channel();
        let owner = tokio::spawn(watch.run_until(stopped));
        ready(&policy).await;
        match case {
            0 => {
                f.changed("LockedHint", true.into()).await;
                f.changed("LockedHint", false.into()).await;
            }
            1 => {
                f.changed("Active", false.into()).await;
                f.changed("Active", true.into()).await;
            }
            2 => {
                f.changed("State", Value::from("closing").try_into().unwrap())
                    .await
            }
            3 => {
                f.changed("LockedHint", Value::from("false").try_into().unwrap())
                    .await
            }
            4 => f
                .server
                .emit_signal(
                    None::<&str>,
                    PATH,
                    PROPERTIES,
                    "PropertiesChanged",
                    &(
                        SESSION,
                        HashMap::<String, OwnedValue>::new(),
                        vec!["Active"],
                    ),
                )
                .await
                .unwrap(),
            5 => f
                .server
                .emit_signal(None::<&str>, ROOT, MANAGER, "PrepareForSleep", &(true,))
                .await
                .unwrap(),
            6 => f
                .server
                .emit_signal(None::<&str>, ROOT, MANAGER, "PrepareForShutdown", &(true,))
                .await
                .unwrap(),
            7 => f
                .server
                .emit_signal(
                    None::<&str>,
                    ROOT,
                    MANAGER,
                    "SessionRemoved",
                    &("c42", OwnedObjectPath::try_from(PATH).unwrap()),
                )
                .await
                .unwrap(),
            8 => f
                .server
                .emit_signal(None::<&str>, PATH, SESSION, "Lock", &())
                .await
                .unwrap(),
            _ => unreachable!(),
        }
        let result = tokio::time::timeout(Duration::from_secs(1), owner)
            .await
            .expect("native watch did not revoke")
            .unwrap();
        assert!(result.is_err(), "case {case} returned clean stop");
        assert!(!policy.input_available());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn other_sessions_and_positive_signals_do_not_retarget_this_owner() {
    let f = Fixture::new(Snapshot::default()).await;
    let watch = f.watch().await.unwrap();
    let policy = watch.input_policy();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(stopped));
    ready(&policy).await;
    f.server
        .emit_signal(
            None::<&str>,
            "/org/freedesktop/login1/session/c99",
            SESSION,
            "Lock",
            &(),
        )
        .await
        .unwrap();
    f.server
        .emit_signal(None::<&str>, PATH, SESSION, "Unlock", &())
        .await
        .unwrap();
    f.server
        .emit_signal(None::<&str>, ROOT, MANAGER, "PrepareForSleep", &(false,))
        .await
        .unwrap();
    f.server
        .emit_signal(
            None::<&str>,
            ROOT,
            MANAGER,
            "SessionRemoved",
            &(
                "c99",
                OwnedObjectPath::try_from("/org/freedesktop/login1/session/c99").unwrap(),
            ),
        )
        .await
        .unwrap();
    f.changed("LockedHint", false.into()).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(policy.input_available());
    stop.send(()).unwrap();
    owner.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_service_loss_or_bus_disconnect_revokes_without_reconnect() {
    for bus_loss in [false, true] {
        let mut f = Fixture::new(Snapshot::default()).await;
        let watch = f.watch().await.unwrap();
        let policy = watch.input_policy();
        let (_stop, stopped) = tokio::sync::oneshot::channel();
        let owner = tokio::spawn(watch.run_until(stopped));
        ready(&policy).await;
        if bus_loss {
            f.bus.child.kill().unwrap();
            f.bus.child.wait().unwrap();
        } else {
            assert!(f.server.release_name(NAME).await.unwrap());
        }
        let result = tokio::time::timeout(Duration::from_secs(1), owner)
            .await
            .expect("disconnected monitor remained live")
            .unwrap();
        assert!(result.is_err());
        assert!(!policy.input_available());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropped_owner_and_pre_cancelled_watch_never_leave_authority() {
    let f = Fixture::new(Snapshot::default()).await;
    let watch = f.watch().await.unwrap();
    let policy = watch.input_policy();
    drop(watch);
    assert!(!policy.input_available());
    let watch = f.watch().await.unwrap();
    let policy = watch.input_policy();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    drop(stop);
    watch.run_until(stopped).await.unwrap();
    assert!(!policy.input_available());
    let watch = f.watch().await.unwrap();
    let policy = watch.input_policy();
    let (_stop, stopped) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(stopped));
    ready(&policy).await;
    owner.abort();
    assert!(owner.await.unwrap_err().is_cancelled());
    assert!(!policy.input_available());
}
