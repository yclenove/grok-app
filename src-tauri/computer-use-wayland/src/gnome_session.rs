//! Read-only GNOME ScreenShield + login1 lifetime. No global input hook.
//!
//! GNOME Shell 49.0 exports org.gnome.ScreenSaver on its own
//! org.gnome.Shell.ScreenShield name. org.gnome.ScreenSaver is a separate proxy;
//! pin the Shell owner, not that proxy. Upstream screenShield.js explicitly
//! delays ActiveChanged during its lock animation. This is an additional loss
//! source, NOT proof of instantaneous lock exclusion or physical-user takeover.
use crate::{LogindSessionWatch, PortalInputPolicy};
use futures_util::{FutureExt, StreamExt};
use std::{
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};
use zbus::{message::Type, Connection, MatchRule, Message, MessageStream, Proxy};

const SHELL: &str = "org.gnome.Shell";
const SHIELD: &str = "org.gnome.Shell.ScreenShield";
const PATH: &str = "/org/gnome/ScreenSaver";
const INTERFACE: &str = "org.gnome.ScreenSaver";

fn error(e: impl std::fmt::Display) -> String {
    format!("GNOME session monitor: {e}")
}

struct Guard {
    // 0: original future unpolled; 1: subscribed; 2: permanently closed.
    phase: AtomicU8,
    login: Arc<dyn PortalInputPolicy>,
}
impl PortalInputPolicy for Guard {
    fn input_available(&self) -> bool {
        self.phase.load(Ordering::Acquire) == 1
            && self.login.input_available()
            && self.phase.load(Ordering::Acquire) == 1
    }
    fn user_input_active(&self) -> bool {
        self.login.user_input_active()
    }
}

struct ScreenShieldWatch {
    _connection: Connection,
    owner: String,
    owners: futures_util::stream::Select<MessageStream, MessageStream>,
    active: MessageStream,
    guard: Arc<Guard>,
}

/// Owns both original OS subscriptions. Must be retained and polled by the Host
/// reactor. Either source's loss, cancellation, drop, or end closes the composite
/// policy before return; recovery requires a NEW watch and explicit consent.
///
/// The required Host policy must still provide portal permission and physical
/// takeover fencing. This type must not be used with an allow-all production
/// policy. It does not enable the currently disabled native Wayland App route.
pub struct GnomeSessionWatch {
    login: LogindSessionWatch,
    shield: ScreenShieldWatch,
}

impl GnomeSessionWatch {
    pub async fn connect(host: Arc<dyn PortalInputPolicy>) -> Result<Self, String> {
        tokio::time::timeout(Duration::from_secs(3), async {
            let login = LogindSessionWatch::connect(host).await?;
            let connection = Connection::session().await.map_err(error)?;
            Self::prepare(login, connection).await
        })
        .await
        .map_err(|_| error("initialization deadline"))?
    }

    pub(super) async fn prepare(
        mut login: LogindSessionWatch,
        connection: Connection,
    ) -> Result<Self, String> {
        let dbus = zbus::fdo::DBusProxy::new(&connection)
            .await
            .map_err(error)?;
        let owner = dbus
            .get_name_owner(SHIELD.try_into().map_err(error)?)
            .await
            .map_err(error)?
            .to_string();
        // Match each exact name at the bus, not after receiving every service's
        // churn. A full unrelated-signal queue can backpressure the shared
        // connection before the original monitor is first polled, preventing
        // the helper snapshot/heartbeat from completing. Keep both original
        // subscriptions before the snapshot and final owner readback: an ABA
        // release/reacquire of either name must still permanently retire us.
        let owners = futures_util::stream::select(
            owner_changes(&connection, SHELL).await?,
            owner_changes(&connection, SHIELD).await?,
        );
        let active = MessageStream::for_match_rule(
            MatchRule::builder()
                .msg_type(Type::Signal)
                .sender(owner.as_str())
                .map_err(error)?
                .path(PATH)
                .map_err(error)?
                .interface(INTERFACE)
                .map_err(error)?
                .member("ActiveChanged")
                .map_err(error)?
                .build(),
            &connection,
            Some(64),
        )
        .await
        .map_err(error)?;
        // Exact unique owner credentials come from the bus, never the model or
        // an XDG session environment variable. login1 resolves its real PID.
        let bus_name = owner.as_str().try_into().map_err(error)?;
        let uid = dbus
            .get_connection_unix_user(bus_name)
            .await
            .map_err(error)?;
        let bus_name = owner.as_str().try_into().map_err(error)?;
        let pid = dbus
            .get_connection_unix_process_id(bus_name)
            .await
            .map_err(error)?;
        login.verify_peer(pid, uid).await?;
        let proxy = Proxy::new(&connection, owner.as_str(), PATH, INTERFACE)
            .await
            .map_err(error)?;
        let is_active: bool = proxy.call("GetActive", &()).await.map_err(error)?;
        if is_active {
            return Err(error("screen shield is active"));
        }
        for name in [SHELL, SHIELD] {
            if dbus
                .get_name_owner(name.try_into().map_err(error)?)
                .await
                .map_err(error)?
                .as_str()
                != owner
            {
                return Err(error(
                    "Shell and ScreenShield owner mismatch or replacement",
                ));
            }
        }
        let guard = Arc::new(Guard {
            phase: AtomicU8::new(0),
            login: login.input_policy(),
        });
        Ok(Self {
            login,
            shield: ScreenShieldWatch {
                _connection: connection,
                owner,
                owners,
                active,
                guard,
            },
        })
    }

    pub fn input_policy(&self) -> Arc<dyn PortalInputPolicy> {
        self.shield.guard.clone()
    }

    pub(super) fn native_endpoint(&self) -> (Connection, String) {
        (self.shield._connection.clone(), self.shield.owner.clone())
    }

    pub async fn run_until(self, stop: tokio::sync::oneshot::Receiver<()>) -> Result<(), String> {
        let Self { login, shield } = self;
        let (_send, receive) = tokio::sync::oneshot::channel();
        // These are original in-place futures, not detached spawned tasks.
        // select drops both subscription owners before the outer future returns.
        tokio::select! {
            biased;
            _ = stop => Ok(()),
            result = login.run_until(receive) => result,
            result = shield.run() => result,
        }
    }
}

async fn owner_changes(connection: &Connection, name: &str) -> Result<MessageStream, String> {
    MessageStream::for_match_rule(
        MatchRule::builder()
            .msg_type(Type::Signal)
            .sender("org.freedesktop.DBus")
            .map_err(error)?
            .path("/org/freedesktop/DBus")
            .map_err(error)?
            .interface("org.freedesktop.DBus")
            .map_err(error)?
            .member("NameOwnerChanged")
            .map_err(error)?
            .add_arg(name)
            .map_err(error)?
            .build(),
        connection,
        Some(64),
    )
    .await
    .map_err(error)
}

impl ScreenShieldWatch {
    async fn run(mut self) -> Result<(), String> {
        while let Some(message) = self.owners.next().now_or_never() {
            Self::owner(message)?;
        }
        while let Some(message) = self.active.next().now_or_never() {
            Self::active(message)?;
        }
        self.guard.phase.store(1, Ordering::Release);
        loop {
            tokio::select! {
                biased;
                message = self.owners.next() => Self::owner(message)?,
                message = self.active.next() => Self::active(message)?,
            }
        }
    }
    fn owner(message: Option<zbus::Result<Message>>) -> Result<(), String> {
        let message = message
            .ok_or_else(|| error("owner watch ended"))?
            .map_err(error)?;
        let (name, _, _): (String, String, String) = message.body().deserialize().map_err(error)?;
        if matches!(name.as_str(), SHELL | SHIELD) {
            return Err(error("Shell or ScreenShield owner lost or replaced"));
        }
        Ok(())
    }
    fn active(message: Option<zbus::Result<Message>>) -> Result<(), String> {
        let message = message
            .ok_or_else(|| error("screen shield watch ended"))?
            .map_err(error)?;
        let (active,): (bool,) = message.body().deserialize().map_err(error)?;
        if active {
            return Err(error("screen shield activated"));
        }
        Ok(())
    }
}
impl Drop for ScreenShieldWatch {
    fn drop(&mut self) {
        self.guard.phase.store(2, Ordering::Release);
    }
}
