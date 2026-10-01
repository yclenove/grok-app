// Owned installed-VM guard. No permission or input is granted here.
pub async fn verify() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::Path;
    use zbus::{fdo::DBusProxy, zvariant::OwnedObjectPath, Connection, Proxy};

    const VM: &str = "812400f8-a6c6-4c38-a735-2b8d0ef8d3e2";
    let marker = Path::new("/etc/cu-owned-vm-id");
    let metadata = marker.symlink_metadata()?;
    if std::env::var("GROK_CU_OWNED_VM_ACCEPTANCE").as_deref() != Ok(VM)
        || std::env::var("XDG_RUNTIME_DIR").as_deref() != Ok("/run/user/1000")
        || unsafe { libc::geteuid() } != 1000
        || !metadata.is_file()
        || metadata.uid() != 0
        || metadata.permissions().mode() & 0o7777 != 0o444
        || std::fs::read_to_string(marker)? != VM
    {
        return Err("explicit owned installed VM required".into());
    }
    let file = std::env::args()
        .nth(1)
        .ok_or("native identity file required")?;
    let expected: serde_json::Value = serde_json::from_slice(&std::fs::read(file)?)?;
    let session_id = expected["session"].as_str().ok_or("session required")?;
    let shell_owner = expected["owner"].as_str().ok_or("Shell owner required")?;
    let shell_pid = expected["shellPid"].as_u64().ok_or("Shell PID required")?;
    let connection = Connection::session().await?;
    let dbus = DBusProxy::new(&connection).await?;
    if dbus
        .get_name_owner("org.gnome.Shell".try_into()?)
        .await?
        .as_str()
        != shell_owner
        || u64::from(
            dbus.get_connection_unix_process_id(shell_owner.try_into()?)
                .await?,
        ) != shell_pid
    {
        return Err("original Shell changed".into());
    }
    let system = Connection::system().await?;
    let system_dbus = DBusProxy::new(&system).await?;
    let login_owner = system_dbus
        .get_name_owner("org.freedesktop.login1".try_into()?)
        .await?;
    let manager = Proxy::new(
        &system,
        login_owner.as_str(),
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await?;
    let session_path: OwnedObjectPath = manager.call("GetSession", &(session_id,)).await?;
    let session = Proxy::new(
        &system,
        login_owner.as_str(),
        session_path.as_str(),
        "org.freedesktop.login1.Session",
    )
    .await?;
    let user: (u32, OwnedObjectPath) = session.get_property("User").await?;
    let seat: (String, OwnedObjectPath) = session.get_property("Seat").await?;
    if user.0 != 1000
        || seat.0 != "seat0"
        || session.get_property::<String>("Type").await? != "wayland"
        || !session.get_property::<bool>("Active").await?
        || session.get_property::<bool>("Remote").await?
        || session.get_property::<bool>("LockedHint").await?
    {
        return Err("owned local unlocked Wayland session required".into());
    }
    Ok(())
}
