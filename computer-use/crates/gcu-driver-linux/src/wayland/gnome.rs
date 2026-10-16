//! GNOME Shell inventory and policy-gated activation on a pinned unique D-Bus owner.
//!
//! Ported v1's existing-Shell-connection export, unique-owner credential checks,
//! and helper-generation fence; no Shell Eval, input hooks or native-policy grants.

use super::{windows::*, Compositor};
use crate::identity::{
    inspection::INSPECTION_TIMEOUT, needs_reauth, same_process_instance, AppIdentity,
};
use async_trait::async_trait;
use std::sync::Arc;

pub(crate) const PATH: &str = "/org/grok/ComputerUse/Windows";
pub(crate) const INTERFACE: &str = "org.grok.ComputerUse.Windows1";
/// Protocol-v1 record: exact ID, lifetime, title, app ID, PID and peer-credential proof.
pub type WireWindow = (String, u64, String, String, u32, bool);

/// D-Bus boundary, including credentials; fakes can exercise native adapters on any OS.
#[async_trait]
pub trait CompositorBus: Send + Sync {
    /// Resolve the exact well-known service to a unique owner; never cache across uses.
    async fn owner(&self, name: &str) -> Result<String, IdentityError>;
    /// Read the unique owner's bus-authenticated OS PID.
    async fn pid(&self, owner: &str) -> Result<u32, IdentityError>;
    /// Read the optional GNOME extension on the pinned unique owner.
    async fn list(&self, owner: &str) -> Result<(u32, String, Vec<WireWindow>), IdentityError>;
    /// Activate with a compositor-side epoch/object/PID fence. Lost replies are
    /// maybe-executed; never retry. Caller supplies the Router's foreground permission.
    async fn activate(
        &self,
        owner: &str,
        epoch: &str,
        window: &WindowRecord,
    ) -> Result<bool, IdentityError>;
    /// Live connection for exporting the optional KWin receiver; fakes need none.
    fn connection(&self) -> Option<&zbus::Connection> {
        None
    }
}

/// Live retained zbus connection. All calls are awaited by the bounded backend;
/// cancellation drops the call future and never detaches a native task.
pub struct ZbusCompositorBus(zbus::Connection);
impl ZbusCompositorBus {
    /// Wrap an existing session bus connection; no new connection or service is created.
    pub fn new(connection: zbus::Connection) -> Self {
        Self(connection)
    }
}
#[async_trait]
impl CompositorBus for ZbusCompositorBus {
    async fn owner(&self, name: &str) -> Result<String, IdentityError> {
        Ok(zbus::fdo::DBusProxy::new(&self.0)
            .await
            .map_err(transport)?
            .get_name_owner(name.try_into().map_err(transport)?)
            .await
            .map_err(transport)?
            .to_string())
    }
    async fn pid(&self, owner: &str) -> Result<u32, IdentityError> {
        zbus::fdo::DBusProxy::new(&self.0)
            .await
            .map_err(transport)?
            .get_connection_unix_process_id(owner.try_into().map_err(transport)?)
            .await
            .map_err(transport)
    }
    async fn list(&self, owner: &str) -> Result<(u32, String, Vec<WireWindow>), IdentityError> {
        zbus::Proxy::new(&self.0, owner, PATH, INTERFACE)
            .await
            .map_err(transport)?
            .call("ListWindows", &())
            .await
            .map_err(transport)
    }
    async fn activate(
        &self,
        owner: &str,
        epoch: &str,
        window: &WindowRecord,
    ) -> Result<bool, IdentityError> {
        zbus::Proxy::new(&self.0, owner, PATH, INTERFACE)
            .await
            .map_err(transport)?
            .call(
                "Activate",
                &(
                    epoch,
                    window.id.as_str(),
                    window.generation,
                    window.pid.ok_or(IdentityError::Handoff)?,
                    true,
                ),
            )
            .await
            .map_err(|_| IdentityError::Transport {
                maybe_executed: true,
            })
    }
    fn connection(&self) -> Option<&zbus::Connection> {
        Some(&self.0)
    }
}

/// A compositor endpoint pinned to an independently inspected OS process image.
/// A well-known name, desktop environment string or same UID alone is not authority.
pub struct CompositorPeer {
    bus: Arc<dyn CompositorBus>,
    name: &'static str,
    owner: String,
    image: AppIdentity,
    processes: Arc<dyn ProcessSource>,
}

impl CompositorPeer {
    /// Pin GNOME/KWin's bus credentials to the trusted launch/session process anchor.
    /// Drop cancels inspection; cold image hashing has a separate 185s budget.
    /// The caller must obtain `expected` from its trusted compositor/session resolver.
    pub async fn pin(
        bus: Arc<dyn CompositorBus>,
        compositor: Compositor,
        expected: AppIdentity,
        processes: Arc<dyn ProcessSource>,
    ) -> Result<Self, IdentityError> {
        let name = match compositor {
            Compositor::Gnome => "org.gnome.Shell",
            Compositor::Kde => "org.kde.KWin",
            _ => return Err(IdentityError::Unsupported),
        };
        tokio::time::timeout(INSPECTION_TIMEOUT, async {
            let owner = native_read(bus.owner(name)).await?;
            if !owner.starts_with(':') {
                return Err(IdentityError::Handoff);
            }
            let peer = Self {
                bus,
                name,
                owner,
                image: expected,
                processes,
            };
            peer.check().await?;
            Ok(peer)
        })
        .await
        .map_err(|_| IdentityError::Timeout {
            maybe_executed: false,
        })?
    }

    /// Unique owner used for routing and script report authentication.
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// Retained bus connection. Does not establish a new connection.
    pub fn connection(&self) -> Option<&zbus::Connection> {
        self.bus.connection()
    }

    pub(crate) fn is_kwin(&self) -> bool {
        self.name == "org.kde.KWin"
    }

    pub(crate) async fn check(&self) -> Result<(), IdentityError> {
        if native_read(self.bus.owner(self.name)).await? != self.owner {
            return Err(IdentityError::StaleWindow);
        }
        let pid = native_read(self.bus.pid(&self.owner)).await?;
        if pid != self.image.pid {
            return Err(IdentityError::Handoff);
        }
        let actual = bounded_inspection(
            &gcu_core::driver::Cancel::default(),
            self.processes.inspect(pid),
        )
        .await?;
        if needs_reauth(&self.image, &actual) || !same_process_instance(&self.image, &actual) {
            return Err(IdentityError::Reauthorize);
        }
        Ok(())
    }
}

/// Native GNOME backend. The optional extension must already be installed/enabled.
pub struct GnomeBackend {
    peer: CompositorPeer,
}

impl GnomeBackend {
    /// Construct from a pinned GNOME endpoint; never installs extensions or grants access.
    pub fn new(peer: CompositorPeer) -> Result<Self, IdentityError> {
        if peer.name != "org.gnome.Shell" {
            return Err(IdentityError::Unsupported);
        }
        Ok(Self { peer })
    }

    async fn read(&self) -> Result<WindowSnapshot, IdentityError> {
        self.peer.check().await?;
        let (version, epoch, records): (u32, String, Vec<WireWindow>) =
            native_read(self.peer.bus.list(self.peer.owner())).await?;
        self.peer.check().await?;
        if version != 1 {
            return Err(IdentityError::Unsupported);
        }
        decode_snapshot(Compositor::Gnome, self.peer.owner(), epoch, records)
    }
}

#[async_trait]
impl WindowBackend for GnomeBackend {
    fn compositor(&self) -> Compositor {
        Compositor::Gnome
    }

    async fn snapshot(&self) -> Result<WindowSnapshot, IdentityError> {
        tokio::time::timeout(INSPECTION_TIMEOUT, self.read())
            .await
            .map_err(|_| IdentityError::Timeout {
                maybe_executed: false,
            })?
    }

    async fn activate(
        &self,
        expected: &WindowSnapshot,
        window: &WindowRecord,
        image: &AppIdentity,
    ) -> Result<(), IdentityError> {
        // Preflight timeout is definitely unexecuted. Dispatch has its own conservative bound.
        let fresh = self.snapshot().await?;
        if fresh.epoch != expected.epoch || !fresh.windows.iter().any(|w| same_window(w, window)) {
            return Err(IdentityError::StaleWindow);
        }
        let epoch = expected
            .epoch
            .strip_prefix(&format!("{}/", self.peer.owner()))
            .ok_or(IdentityError::StaleWindow)?;
        let pid = window
            .pid
            .filter(|_| window.peer_pid)
            .ok_or(IdentityError::Handoff)?;
        // The compositor read may have taken time. Revalidate the opened image
        // and its cache key immediately before IPC, not only before that read.
        let current_image =
            tokio::time::timeout(INSPECTION_TIMEOUT, self.peer.processes.inspect(pid))
                .await
                .map_err(|_| IdentityError::Timeout {
                    maybe_executed: false,
                })??;
        if needs_reauth(image, &current_image) || !same_process_instance(image, &current_image) {
            return Err(IdentityError::Reauthorize);
        }
        tokio::time::timeout(IDENTITY_TIMEOUT, async {
            let accepted = self
                .peer
                .bus
                .activate(self.peer.owner(), epoch, window)
                .await?;
            if !accepted {
                return Err(IdentityError::StaleWindow);
            }
            self.peer
                .check()
                .await
                .map_err(|_| IdentityError::Transport {
                    maybe_executed: true,
                })
        })
        .await
        .map_err(|_| IdentityError::Timeout {
            maybe_executed: true,
        })?
    }
}

fn same_window(a: &WindowRecord, b: &WindowRecord) -> bool {
    a.id == b.id && a.generation == b.generation && a.pid == b.pid && a.peer_pid == b.peer_pid
}

pub(crate) fn transport(_: impl std::fmt::Display) -> IdentityError {
    IdentityError::Transport {
        maybe_executed: false,
    }
}

pub(crate) fn decode_snapshot(
    compositor: Compositor,
    owner: &str,
    epoch: String,
    records: Vec<WireWindow>,
) -> Result<WindowSnapshot, IdentityError> {
    if epoch.is_empty() || epoch.len() > 128 || records.len() > 4096 {
        return Err(IdentityError::Handoff);
    }
    let mut ids = std::collections::HashSet::new();
    let mut windows = Vec::with_capacity(records.len());
    for (id, generation, title, app_id, pid, peer_pid) in records {
        if id.is_empty()
            || id.len() > 256
            || generation == 0
            || generation > (1u64 << 53) - 1
            || title.len() > 16384
            || app_id.len() > 4096
            || !ids.insert(id.clone())
            || (peer_pid && pid == 0)
        {
            return Err(IdentityError::Handoff);
        }
        windows.push(WindowRecord {
            id,
            generation,
            title,
            app_id,
            pid: (pid > 0).then_some(pid),
            peer_pid,
        });
    }
    Ok(WindowSnapshot {
        compositor,
        epoch: format!("{owner}/{epoch}"),
        windows,
    })
}

async fn native_read<T>(
    future: impl std::future::Future<Output = Result<T, IdentityError>>,
) -> Result<T, IdentityError> {
    tokio::time::timeout(IDENTITY_TIMEOUT, future)
        .await
        .unwrap_or(Err(IdentityError::Timeout {
            maybe_executed: false,
        }))
}
