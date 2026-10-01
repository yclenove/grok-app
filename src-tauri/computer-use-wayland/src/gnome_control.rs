//! Explicit per-user helper management, not desktop authority. Only this fixed
//! UUID is mutable. Never calls ReloadExtension (unsupported by GNOME 46),
//! toggles global extensions, evaluates Shell code, or installs remote code.
use std::collections::HashMap;
use std::time::Duration;
use zbus::{zvariant::OwnedValue, Connection, Proxy};

pub const GNOME_HELPER_UUID: &str = "computer-use@grok-app.local";
const SHELL: &str = "org.gnome.Shell";
const PATH: &str = "/org/gnome/Shell";
const INTERFACE: &str = "org.gnome.Shell.Extensions";
const HELPER_PATH: &str = "/org/grok/ComputerUse/NativePolicy";
const HELPER_INTERFACE: &str = "org.grok.ComputerUse.NativePolicy1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HelperHealth {
    Absent,
    Ready,
    Blocked,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct HelperExtension {
    pub state: u32,
    /// GNOME's resolved enable setting, separate from its sticky ERROR state.
    pub enabled: bool,
    pub path: String,
    pub per_user: bool,
}

#[derive(Clone, Debug)]
pub struct HelperSnapshot {
    pub shell_version: String,
    pub user_extensions_enabled: bool,
    pub extension: Option<HelperExtension>,
    pub health: HelperHealth,
}
impl HelperSnapshot {
    pub fn supported(&self) -> bool {
        self.shell_version.split('.').next() == Some("46")
    }
    pub fn inactive(&self) -> bool {
        self.extension
            .as_ref()
            .is_none_or(|e| !e.enabled && matches!(e.state, 2 | 3 | 4 | 6 | 99))
            && self.health == HelperHealth::Absent
    }
    pub fn enableable(&self) -> bool {
        self.supported()
            && self.user_extensions_enabled
            && self.inactive()
            && self
                .extension
                .as_ref()
                .is_some_and(|e| matches!(e.state, 2 | 6))
    }
    pub fn needs_restart(&self) -> bool {
        self.extension
            .as_ref()
            .is_some_and(|e| matches!(e.state, 3 | 4 | 99))
    }
    pub fn active(&self) -> bool {
        self.extension
            .as_ref()
            .is_some_and(|e| e.enabled && e.state == 1)
            && self.health == HelperHealth::Ready
    }
}

/// A pinned same-UID Shell endpoint. This is NOT the login1/native input grant
/// verifier; each actual desktop selection still requires those stronger checks.
pub struct GnomeHelperControl {
    connection: Connection,
    pub bus_id: String,
    pub shell_owner: String,
    pub shell_process: String,
}

fn process_identity(pid: u32) -> Result<String, String> {
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").map_err(error)?;
    let boot = uuid::Uuid::parse_str(boot.trim()).map_err(error)?;
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).map_err(error)?;
    let (_, rest) = stat
        .rsplit_once(") ")
        .ok_or_else(|| error("malformed Shell process identity"))?;
    let start = rest
        .split_whitespace()
        .nth(19)
        .ok_or_else(|| error("missing Shell start time"))?
        .parse::<u64>()
        .map_err(error)?;
    if pid == 0 || start == 0 {
        return Err(error("invalid Shell process identity"));
    }
    Ok(format!("{boot}:{pid}:{start}"))
}
fn error(e: impl std::fmt::Display) -> String {
    format!("computer_use_helper_shell: {e}")
}

fn number(info: &HashMap<String, OwnedValue>, name: &str) -> Result<u32, String> {
    let n = f64::try_from(
        info.get(name)
            .ok_or_else(|| error("missing extension field"))?,
    )
    .map_err(error)?;
    if !n.is_finite() || n < 0.0 || n.fract() != 0.0 || n > u32::MAX as f64 {
        return Err(error("invalid extension number"));
    }
    Ok(n as u32)
}
fn string(info: &HashMap<String, OwnedValue>, name: &str) -> Result<String, String> {
    let value = <&str>::try_from(
        info.get(name)
            .ok_or_else(|| error("missing extension field"))?,
    )
    .map_err(error)?;
    if value.len() > 4096 {
        return Err(error("extension field too long"));
    }
    Ok(value.to_owned())
}
fn parse_extension(info: HashMap<String, OwnedValue>) -> Result<Option<HelperExtension>, String> {
    if info.is_empty() {
        return Ok(None);
    }
    if string(&info, "uuid")? != GNOME_HELPER_UUID {
        return Err(error("wrong extension UUID"));
    }
    let kind = number(&info, "type")?;
    if !matches!(kind, 1 | 2) {
        return Err(error("unknown extension type"));
    }
    let state = number(&info, "state")?;
    if !matches!(state, 1..=8 | 99) {
        return Err(error("unknown extension state"));
    }
    Ok(Some(HelperExtension {
        state,
        enabled: bool::try_from(
            info.get("enabled")
                .ok_or_else(|| error("missing extension enabled field"))?,
        )
        .map_err(error)?,
        path: string(&info, "path")?,
        per_user: kind == 2,
    }))
}

impl GnomeHelperControl {
    pub async fn connect() -> Result<Self, String> {
        tokio::time::timeout(Duration::from_secs(3), async {
            Self::on_connection(Connection::session().await.map_err(error)?).await
        })
        .await
        .map_err(|_| error("status deadline"))?
    }
    pub(super) async fn on_connection(connection: Connection) -> Result<Self, String> {
        let dbus = zbus::fdo::DBusProxy::new(&connection)
            .await
            .map_err(error)?;
        let shell_owner = dbus
            .get_name_owner(SHELL.try_into().map_err(error)?)
            .await
            .map_err(error)?
            .to_string();
        let uid = dbus
            .get_connection_unix_user(shell_owner.as_str().try_into().map_err(error)?)
            .await
            .map_err(error)?;
        if uid != unsafe { libc::geteuid() } {
            return Err(error("Shell user mismatch"));
        }
        let bus_id = dbus.get_id().await.map_err(error)?.to_string();
        let pid = dbus
            .get_connection_unix_process_id(shell_owner.as_str().try_into().map_err(error)?)
            .await
            .map_err(error)?;
        let shell_process = process_identity(pid)?;
        let control = Self {
            connection,
            bus_id,
            shell_owner,
            shell_process,
        };
        control.check_owner().await?;
        Ok(control)
    }
    async fn check_owner(&self) -> Result<(), String> {
        let dbus = zbus::fdo::DBusProxy::new(&self.connection)
            .await
            .map_err(error)?;
        if dbus
            .get_name_owner(SHELL.try_into().map_err(error)?)
            .await
            .map_err(error)?
            .as_str()
            != self.shell_owner
        {
            return Err(error("Shell owner changed; refresh required"));
        }
        Ok(())
    }
    async fn extension_proxy(&self) -> Result<Proxy<'_>, String> {
        Proxy::new(&self.connection, self.shell_owner.as_str(), PATH, INTERFACE)
            .await
            .map_err(error)
    }
    pub async fn snapshot(&self) -> Result<HelperSnapshot, String> {
        tokio::time::timeout(Duration::from_secs(3), self.read_snapshot())
            .await
            .map_err(|_| error("status deadline"))?
    }
    async fn read_snapshot(&self) -> Result<HelperSnapshot, String> {
        self.check_owner().await?;
        // Fresh proxies, with no retained property cache used as permission.
        let proxy = self.extension_proxy().await?;
        let shell_version: String = proxy.get_property("ShellVersion").await.map_err(error)?;
        let user_extensions_enabled = proxy
            .get_property("UserExtensionsEnabled")
            .await
            .map_err(error)?;
        let info = proxy
            .call("GetExtensionInfo", &(GNOME_HELPER_UUID,))
            .await
            .map_err(error)?;
        let extension = parse_extension(info)?;
        let helper = Proxy::new(
            &self.connection,
            self.shell_owner.as_str(),
            HELPER_PATH,
            HELPER_INTERFACE,
        )
        .await
        .map_err(error)?;
        let state: Result<(u32, String, u32, bool), zbus::Error> =
            helper.call("GetState", &()).await;
        let health = match state {
            Ok((1, epoch, _, blocked))
                if uuid::Uuid::parse_str(&epoch).is_ok_and(|id| id.to_string() == epoch) =>
            {
                if blocked {
                    HelperHealth::Blocked
                } else {
                    HelperHealth::Ready
                }
            }
            Err(zbus::Error::MethodError(name, _, _))
                if matches!(
                    name.as_str(),
                    "org.freedesktop.DBus.Error.UnknownObject"
                        | "org.freedesktop.DBus.Error.UnknownMethod"
                        | "org.freedesktop.DBus.Error.UnknownInterface"
                ) =>
            {
                HelperHealth::Absent
            }
            _ => HelperHealth::Unknown,
        };
        self.check_owner().await?;
        Ok(HelperSnapshot {
            shell_version,
            user_extensions_enabled,
            extension,
            health,
        })
    }
    /// Keep this ORIGINAL future alive through reply and readback. A true
    /// method reply only records settings, not successful activation/retirement.
    pub async fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        let before = self.snapshot().await?;
        if !before.supported() || before.extension.is_none() {
            return Err(error("unsupported or undiscovered helper"));
        }
        if enabled && !before.user_extensions_enabled {
            return Err(error("global extensions disabled"));
        }
        if enabled && !before.enableable() {
            return Err(error(
                "helper is not ready to enable; refresh or restart Shell",
            ));
        }
        self.check_owner().await?;
        let accepted: bool = self
            .extension_proxy()
            .await?
            .call(
                if enabled {
                    "EnableExtension"
                } else {
                    "DisableExtension"
                },
                &(GNOME_HELPER_UUID,),
            )
            .await
            .map_err(|e| error(format!("mutation outcome unconfirmed: {e}")))?;
        if !accepted {
            return Err(error("Shell refused extension setting"));
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let current = self.snapshot().await?;
            if if enabled {
                current.active()
            } else {
                current.inactive()
            } {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(error(
                    "mutation readback unconfirmed; refresh, do not assume completion",
                ));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[cfg(test)]
#[path = "gnome_control_tests.rs"]
mod tests;
