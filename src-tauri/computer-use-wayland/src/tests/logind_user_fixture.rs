//! Real private D-Bus with login1's exact NoSessionForPID error name.
use super::super::logind::{ready, MANAGER, NAME, PATH, PROPERTIES, ROOT};
use super::super::{adapter::FixturePolicy, Bus};
use crate::{LogindSessionWatch, PortalInputPolicy};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use zbus::{
    zvariant::{OwnedObjectPath, OwnedValue},
    Connection,
};

pub(super) const USER: &str = "org.freedesktop.login1.User";
pub(super) const USER_PATH: &str = "/org/freedesktop/login1/user/u42";
pub(super) const SSH: &str = "/org/freedesktop/login1/session/ssh";
pub(super) const OTHER: &str = "/org/freedesktop/login1/session/other";
pub(super) type SessionRef = (String, OwnedObjectPath);
pub(super) fn session_ref(id: &str, path: &str) -> SessionRef {
    (id.into(), path.try_into().unwrap())
}

pub(super) struct State {
    pub uid: u32,
    pub display: Mutex<SessionRef>,
    pub sessions: Mutex<Vec<SessionRef>>,
    pub direct: Mutex<HashMap<u32, OwnedObjectPath>>,
    pub generic_error: AtomicBool,
    pub foreign_user: AtomicBool,
    pub user_reads: AtomicU32,
    pub pid_user_reads: AtomicU32,
    pub user_lookup_reads: AtomicU32,
    pub(super) hold_user: AtomicBool,
    pub(super) user_released: tokio::sync::Notify,
}
impl Default for State {
    fn default() -> Self {
        Self {
            uid: unsafe { libc::geteuid() },
            display: Mutex::new(session_ref("c42", PATH)),
            sessions: Mutex::new(vec![session_ref("c42", PATH)]),
            direct: Mutex::new(HashMap::new()),
            generic_error: AtomicBool::new(false),
            foreign_user: AtomicBool::new(false),
            user_reads: AtomicU32::new(0),
            pid_user_reads: AtomicU32::new(0),
            user_lookup_reads: AtomicU32::new(0),
            hold_user: AtomicBool::new(false),
            user_released: tokio::sync::Notify::new(),
        }
    }
}
impl State {
    pub fn hold_user(&self) {
        self.hold_user.store(true, Ordering::SeqCst);
    }
    pub fn release_user(&self) {
        self.hold_user.store(false, Ordering::SeqCst);
        self.user_released.notify_waiters();
    }
}
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.login1")]
enum LoginError {
    NoSessionForPID(String),
    Failed(String),
}
struct Manager(Arc<State>);
#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl Manager {
    #[zbus(name = "GetSessionByPID")]
    fn get_session_by_pid(&self, pid: u32) -> Result<OwnedObjectPath, LoginError> {
        if self.0.generic_error.load(Ordering::SeqCst) {
            return Err(LoginError::Failed("no session for this PID".into()));
        }
        self.0
            .direct
            .lock()
            .unwrap()
            .get(&pid)
            .cloned()
            .ok_or_else(|| LoginError::NoSessionForPID("process belongs to user manager".into()))
    }
    #[zbus(name = "GetUserByPID")]
    fn get_user_by_pid(&self, _pid: u32) -> OwnedObjectPath {
        self.0.pid_user_reads.fetch_add(1, Ordering::SeqCst);
        if self.0.foreign_user.load(Ordering::SeqCst) {
            "/org/freedesktop/login1/user/foreign"
        } else {
            USER_PATH
        }
        .try_into()
        .unwrap()
    }
    #[zbus(name = "GetUser")]
    async fn get_user(&self, _uid: u32) -> OwnedObjectPath {
        self.0.user_lookup_reads.fetch_add(1, Ordering::SeqCst);
        loop {
            // zbus may dispatch on its own async-io executor, including while
            // the test runtime shuts down. A runtime-independent notification
            // avoids turning an intended pending reply into an executor panic.
            let released = self.0.user_released.notified();
            tokio::pin!(released);
            released.as_mut().enable();
            if !self.0.hold_user.load(Ordering::SeqCst) {
                break;
            }
            released.await;
        }
        USER_PATH.try_into().unwrap()
    }
    #[zbus(property)]
    fn preparing_for_sleep(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn preparing_for_shutdown(&self) -> bool {
        false
    }
}
struct User(Arc<State>);
#[zbus::interface(name = "org.freedesktop.login1.User")]
impl User {
    #[zbus(property, name = "UID")]
    fn uid(&self) -> u32 {
        self.0.uid
    }
    #[zbus(property)]
    fn display(&self) -> SessionRef {
        self.0.user_reads.fetch_add(1, Ordering::SeqCst);
        self.0.display.lock().unwrap().clone()
    }
    #[zbus(property)]
    fn sessions(&self) -> Vec<SessionRef> {
        self.0.sessions.lock().unwrap().clone()
    }
}
pub(super) struct Session {
    pub id: &'static str,
    pub kind: &'static str,
    pub class: &'static str,
    pub remote: bool,
    pub locked: bool,
    pub active: bool,
    pub uid: u32,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            id: "c42",
            kind: "wayland",
            class: "user",
            remote: false,
            locked: false,
            active: true,
            uid: unsafe { libc::geteuid() },
        }
    }
}
#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl Session {
    #[zbus(property)]
    fn id(&self) -> &str {
        self.id
    }
    #[zbus(property)]
    fn active(&self) -> bool {
        self.active
    }
    #[zbus(property)]
    fn locked_hint(&self) -> bool {
        self.locked
    }
    #[zbus(property)]
    fn remote(&self) -> bool {
        self.remote
    }
    #[zbus(property)]
    fn user(&self) -> (u32, OwnedObjectPath) {
        (self.uid, USER_PATH.try_into().unwrap())
    }
    #[zbus(property, name = "Type")]
    fn kind(&self) -> &str {
        self.kind
    }
    #[zbus(property)]
    fn class(&self) -> &str {
        self.class
    }
    #[zbus(property)]
    fn state(&self) -> &str {
        if self.active {
            "active"
        } else {
            "online"
        }
    }
}
pub(super) struct Fixture {
    pub bus: Bus,
    pub server: Connection,
    pub state: Arc<State>,
    pub host: Arc<FixturePolicy>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.state.release_user();
    }
}
impl Fixture {
    pub async fn new(state: State, session: Session) -> Self {
        let bus = Bus::start();
        let state = Arc::new(state);
        let server = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name(NAME)
            .unwrap()
            .serve_at(ROOT, Manager(state.clone()))
            .unwrap()
            .serve_at(USER_PATH, User(state.clone()))
            .unwrap()
            .serve_at(PATH, session)
            .unwrap()
            .serve_at(
                SSH,
                Session {
                    id: "ssh",
                    kind: "tty",
                    remote: true,
                    active: false,
                    ..Session::default()
                },
            )
            .unwrap()
            .serve_at(
                OTHER,
                Session {
                    id: "other",
                    active: false,
                    ..Session::default()
                },
            )
            .unwrap()
            .build()
            .await
            .unwrap();
        Self {
            bus,
            server,
            state,
            host: Arc::new(FixturePolicy::default()),
        }
    }
    pub async fn watch(&self) -> Result<LogindSessionWatch, String> {
        self.watch_with(self.host.clone()).await
    }
    pub async fn watch_with(
        &self,
        host: Arc<dyn PortalInputPolicy>,
    ) -> Result<LogindSessionWatch, String> {
        let c = zbus::connection::Builder::address(self.bus.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(3),
            LogindSessionWatch::prepare(c, std::process::id(), unsafe { libc::geteuid() }, host),
        )
        .await
        .unwrap()
    }
    pub async fn changed(&self, changed: HashMap<String, OwnedValue>, invalidated: Vec<String>) {
        self.server
            .emit_signal(
                None::<&str>,
                USER_PATH,
                PROPERTIES,
                "PropertiesChanged",
                &(USER, changed, invalidated),
            )
            .await
            .unwrap();
    }
    pub async fn manager_signal(&self, member: &str, value: SessionRef) {
        self.server
            .emit_signal(None::<&str>, ROOT, MANAGER, member, &value)
            .await
            .unwrap();
    }
    pub async fn wait_reads(&self, count: u32) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while self.state.user_reads.load(Ordering::SeqCst) < count {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
    }
    pub async fn started(
        &self,
    ) -> (
        Arc<dyn PortalInputPolicy>,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<Result<(), String>>,
    ) {
        let watch = self.watch().await.unwrap();
        let policy = watch.input_policy();
        assert!(!policy.input_available());
        let (send, recv) = tokio::sync::oneshot::channel();
        let owner = tokio::spawn(watch.run_until(recv));
        ready(&policy).await;
        (policy, send, owner)
    }
}
