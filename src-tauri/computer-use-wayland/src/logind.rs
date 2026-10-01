//! Read-only login1 session fencing, not a physical-input or portal detector.
//! API: systemd's org.freedesktop.login1, PID/User binding and concrete Session
//! signals. Never watch the caller-relative /session/self convenience path.
#[path = "logind_binding.rs"]
mod binding;
#[path = "logind_binding_signal.rs"]
mod binding_signal;
use crate::PortalInputPolicy;
use futures_util::{FutureExt, StreamExt};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};
use zbus::{
    message::Type,
    zvariant::{OwnedObjectPath, OwnedValue},
    Connection, MatchRule, Message, MessageStream, Proxy,
};

const NAME: &str = "org.freedesktop.login1";
const ROOT: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";
const SESSION: &str = "org.freedesktop.login1.Session";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
type Properties = HashMap<String, OwnedValue>;

struct Guard {
    // 0: unpolled owner, 1: monitoring, 2: irrevocably closed.
    phase: AtomicU8,
    // Revalidation may close then reopen the same monitoring phase. A Host
    // callback started before that fence cannot publish a late positive (ABA).
    epoch: AtomicU64,
    host: Arc<dyn PortalInputPolicy>,
}

impl PortalInputPolicy for Guard {
    fn input_available(&self) -> bool {
        // The original monitor can stop while the Host callback is in flight.
        // Never publish that callback's late true after the OS owner is gone.
        let epoch = self.epoch.load(Ordering::Acquire);
        self.phase.load(Ordering::Acquire) == 1
            && self.host.input_available()
            && self.phase.load(Ordering::Acquire) == 1
            && self.epoch.load(Ordering::Acquire) == epoch
    }
    fn user_input_active(&self) -> bool {
        self.host.user_input_active()
    }
}

/// Owns a pinned, read-only system-bus subscription for this process's exact
/// local Wayland login session, or its unambiguous login1 user-manager Display.
/// Missing login1/session/properties and multiple graphical logins fail closed.
///
/// The Host must retain and await `run_until` on its granting reactor. Dropping
/// that future revokes this guard; constructing it without polling never grants
/// input. No task is internally detached. Recovery requires a new watch AND
/// explicit portal consent. Physical takeover and portal permissions remain
/// mandatory parts of the supplied Host policy; this is not their replacement.
pub struct LogindSessionWatch {
    _connection: Connection,
    owner: String,
    uid: u32,
    path: OwnedObjectPath,
    binding: Option<binding::UserBinding>,
    signals: MessageStream,
    owners: MessageStream,
    guard: Arc<Guard>,
}

fn error(e: impl std::fmt::Display) -> String {
    format!("login1 session monitor: {e}")
}

fn boolean(p: &Properties, key: &str) -> Result<bool, String> {
    bool::try_from(p.get(key).ok_or_else(|| error(format!("missing {key}")))?).map_err(error)
}

fn string<'a>(p: &'a Properties, key: &str) -> Result<&'a str, String> {
    <&str>::try_from(p.get(key).ok_or_else(|| error(format!("missing {key}")))?).map_err(error)
}

fn initial_session(p: &Properties, uid: u32) -> Result<(), String> {
    let user: (u32, OwnedObjectPath) = p
        .get("User")
        .ok_or_else(|| error("missing User"))?
        .try_clone()
        .map_err(error)?
        .try_into()
        .map_err(error)?;
    if user.0 != uid
        || boolean(p, "Remote")?
        || !boolean(p, "Active")?
        || boolean(p, "LockedHint")?
        || string(p, "Type")? != "wayland"
        || string(p, "Class")? != "user"
        || string(p, "State")? != "active"
    {
        return Err(error(
            "not this user's unlocked active local Wayland session",
        ));
    }
    Ok(())
}

async fn all(
    connection: &Connection,
    owner: &str,
    path: &str,
    interface: &str,
) -> Result<Properties, String> {
    Proxy::new(connection, owner, path, PROPERTIES)
        .await
        .map_err(error)?
        .call("GetAll", &(interface,))
        .await
        .map_err(error)
}

impl LogindSessionWatch {
    pub async fn connect(host: Arc<dyn PortalInputPolicy>) -> Result<Self, String> {
        tokio::time::timeout(Duration::from_secs(3), async {
            let connection = Connection::system().await.map_err(error)?;
            // getuid is process identity, never an agent/environment selection.
            Self::prepare(
                connection,
                std::process::id(),
                unsafe { libc::geteuid() },
                host,
            )
            .await
        })
        .await
        .map_err(|_| error("initialization deadline"))?
    }

    pub(super) async fn prepare(
        connection: Connection,
        pid: u32,
        uid: u32,
        host: Arc<dyn PortalInputPolicy>,
    ) -> Result<Self, String> {
        let dbus = zbus::fdo::DBusProxy::new(&connection)
            .await
            .map_err(error)?;
        let owner = dbus
            .get_name_owner(NAME.try_into().map_err(error)?)
            .await
            .map_err(error)?
            .to_string();
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender("org.freedesktop.DBus")
            .map_err(error)?
            .interface("org.freedesktop.DBus")
            .map_err(error)?
            .member("NameOwnerChanged")
            .map_err(error)?
            .add_arg(NAME)
            .map_err(error)?
            .build();
        let owners = MessageStream::for_match_rule(rule, &connection, Some(64))
            .await
            .map_err(error)?;
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(owner.as_str())
            .map_err(error)?
            .path_namespace(ROOT)
            .map_err(error)?
            .build();
        // Subscribe BEFORE resolving the session and fetching its snapshot, so
        // lock/unlock pulses during initialization cannot disappear in GetAll.
        let signals = MessageStream::for_match_rule(rule, &connection, Some(128))
            .await
            .map_err(error)?;
        let (path, binding) = binding::resolve(&connection, &owner, pid, uid).await?;
        initial_session(
            &all(&connection, &owner, path.as_str(), SESSION).await?,
            uid,
        )?;
        let manager = all(&connection, &owner, ROOT, MANAGER).await?;
        if boolean(&manager, "PreparingForSleep")? || boolean(&manager, "PreparingForShutdown")? {
            return Err(error("system is preparing to sleep or shut down"));
        }
        if dbus
            .get_name_owner(NAME.try_into().map_err(error)?)
            .await
            .map_err(error)?
            .as_str()
            != owner
        {
            return Err(error("login1 owner changed during initialization"));
        }
        Ok(Self {
            _connection: connection,
            owner,
            uid,
            path,
            binding,
            signals,
            owners,
            guard: Arc::new(Guard {
                phase: AtomicU8::new(0),
                epoch: AtomicU64::new(0),
                host,
            }),
        })
    }

    pub fn input_policy(&self) -> Arc<dyn PortalInputPolicy> {
        self.guard.clone()
    }

    /// Bind a session-bus service to the same concrete login session. A user's
    /// session bus can span several graphical logins; matching UID is not enough.
    pub(crate) async fn verify_peer(&mut self, pid: u32, uid: u32) -> Result<(), String> {
        if uid != self.uid || pid == 0 {
            return Err(error("peer has a different user or invalid PID"));
        }
        let (path, peer) = binding::resolve(&self._connection, &self.owner, pid, uid).await?;
        if path != self.path {
            return Err(error("peer belongs to a different login session"));
        }
        initial_session(
            &all(&self._connection, &self.owner, path.as_str(), SESSION).await?,
            uid,
        )?;
        if let Some(peer) = peer {
            if let Some(binding) = &mut self.binding {
                if binding.path != peer.path || binding.display != peer.display {
                    return Err(error("peer has a different user-manager Display"));
                }
                if !binding.pids.contains(&pid) {
                    binding.pids.push(pid);
                }
            } else {
                self.binding = Some(peer);
            }
        }
        Ok(())
    }

    pub async fn run_until(
        mut self,
        mut stop: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<(), String> {
        // Consume loss queued during initial method calls before publishing
        // monitoring. Positive snapshots never overwrite a negative signal.
        loop {
            while let Some(message) = self.owners.next().now_or_never() {
                self.owner(message)?;
            }
            while let Some(message) = self.signals.next().now_or_never() {
                if !self.process(message, &mut stop).await? {
                    return Ok(());
                }
            }
            // An inventory read can await while the service owner is lost.
            while let Some(message) = self.owners.next().now_or_never() {
                self.owner(message)?;
            }
            if stop.try_recv() != Err(tokio::sync::oneshot::error::TryRecvError::Empty) {
                return Ok(());
            }
            // Reopen only after all signals queued during a refresh are checked.
            self.guard.phase.store(1, Ordering::Release);
            tokio::select! {
                biased;
                _ = &mut stop => return Ok(()),
                message = self.owners.next() => self.owner(message)?,
                message = self.signals.next() => {
                    if !self.process(message, &mut stop).await? { return Ok(()); }
                },
            }
        }
    }

    async fn process(
        &mut self,
        message: Option<zbus::Result<Message>>,
        stop: &mut tokio::sync::oneshot::Receiver<()>,
    ) -> Result<bool, String> {
        let message = message
            .ok_or_else(|| error("session watch ended"))?
            .map_err(error)?;
        self.signal(Some(Ok(message.clone())))?;
        let Some(binding) = &self.binding else {
            return Ok(true);
        };
        let Some(inspect) = binding.signal(&message, self.uid)? else {
            return Ok(true);
        };
        self.guard.phase.store(0, Ordering::Release);
        self.guard.epoch.fetch_add(1, Ordering::AcqRel);
        let refresh = tokio::time::timeout(
            Duration::from_secs(1),
            binding::refresh(&self._connection, &self.owner, self.uid, binding, &inspect),
        );
        let next = tokio::select! {
            biased;
            _ = stop => return Ok(false),
            message = self.owners.next() => {
                self.owner(message)?;
                return Err(error("unexpected owner event during user-manager validation"));
            },
            result = refresh => result.map_err(|_| error("user-manager validation deadline"))??,
        };
        self.binding = Some(next);
        Ok(true)
    }

    fn owner(&self, message: Option<zbus::Result<Message>>) -> Result<(), String> {
        let message = message
            .ok_or_else(|| error("owner watch ended"))?
            .map_err(error)?;
        let (name, _, _): (String, String, String) = message.body().deserialize().map_err(error)?;
        if name == NAME {
            return Err(error("login1 owner lost or replaced"));
        }
        Ok(())
    }

    fn signal(&self, message: Option<zbus::Result<Message>>) -> Result<(), String> {
        let message = message
            .ok_or_else(|| error("session watch ended"))?
            .map_err(error)?;
        let header = message.header();
        let path = header.path().map(|v| v.as_str());
        let interface = header.interface().map(|v| v.as_str());
        let member = header.member().map(|v| v.as_str());
        if path == Some(self.path.as_str()) {
            if interface == Some(SESSION) && member == Some("Lock") {
                return Err(error("session lock requested"));
            }
            if interface == Some(PROPERTIES) && member == Some("PropertiesChanged") {
                let (changed_interface, changed, invalidated): (String, Properties, Vec<String>) =
                    message.body().deserialize().map_err(error)?;
                if changed_interface == SESSION {
                    let keys = [
                        "Active",
                        "LockedHint",
                        "State",
                        "Type",
                        "Class",
                        "User",
                        "Remote",
                        "Id",
                    ];
                    if invalidated.iter().any(|key| keys.contains(&key.as_str())) {
                        return Err(error("session safety property invalidated"));
                    }
                    if (changed.contains_key("Active") && !boolean(&changed, "Active")?)
                        || (changed.contains_key("LockedHint") && boolean(&changed, "LockedHint")?)
                        || (changed.contains_key("State") && string(&changed, "State")? != "active")
                        || ["Type", "Class", "User", "Remote", "Id"]
                            .iter()
                            .any(|key| changed.contains_key(*key))
                    {
                        return Err(error("session safety state changed"));
                    }
                }
            }
        }
        if path == Some(ROOT) && interface == Some(MANAGER) {
            match member {
                Some("PrepareForSleep" | "PrepareForShutdown") => {
                    let (active,): (bool,) = message.body().deserialize().map_err(error)?;
                    if active {
                        return Err(error("system sleep or shutdown requested"));
                    }
                }
                Some("SessionRemoved") => {
                    let (_, removed): (String, OwnedObjectPath) =
                        message.body().deserialize().map_err(error)?;
                    if removed == self.path {
                        return Err(error("session removed"));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl Drop for LogindSessionWatch {
    fn drop(&mut self) {
        self.guard.phase.store(2, Ordering::Release);
    }
}
