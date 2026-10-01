//! Explicit owned-VM, read-only native monitor lifecycle acceptance.
//! It locks only the authenticated local test session. No portal, EI or App grant.
#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use grok_computer_use_wayland::{GnomeNativePolicyWatch, PortalInputPolicy};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::{path::Path, sync::Arc, time::Duration};
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
    struct ReadOnlyHost;
    impl PortalInputPolicy for ReadOnlyHost {
        fn input_available(&self) -> bool {
            false
        }
        fn user_input_active(&self) -> bool {
            false
        }
    }
    let watch = GnomeNativePolicyWatch::connect(Arc::new(ReadOnlyHost)).await?;
    let policy = watch.input_policy();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let mut run = Box::pin(watch.run_until(stopped));
    tokio::select! {
        result = &mut run => return Err(format!("original monitor ended before lock: {result:?}").into()),
        _ = tokio::time::sleep(Duration::from_millis(350)) => {}
    }
    assert!(
        !policy.input_available(),
        "read-only Host must never grant input"
    );
    let lock_started = std::time::Instant::now();
    let request: Result<(), zbus::Error> = manager.call("LockSession", &(session_id,)).await;
    if let Err(error) = request {
        let _ = stop.send(());
        let _ = run.await;
        return Err(error.into());
    }
    let result = match tokio::time::timeout(Duration::from_secs(4), &mut run).await {
        Ok(result) => result,
        Err(_) => {
            let _ = stop.send(());
            let joined = run.await;
            return Err(format!(
                "native lock monitor deadline; original cancellation joined: {joined:?}"
            )
            .into());
        }
    };
    let invalidated_after_ms = lock_started.elapsed().as_millis();
    let error = result.expect_err("lock must invalidate rather than normally stop the monitor");
    if ![
        "session lock requested",
        "session safety state changed",
        "helper changed; new consent required",
        "screen shield activated",
    ]
    .iter()
    .any(|reason| error.contains(reason))
    {
        return Err(format!("unrelated monitor failure is not lock proof: {error}").into());
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        while !session.get_property::<bool>("LockedHint").await? {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok::<(), zbus::Error>(())
    })
    .await??;
    assert!(!policy.input_available());
    if dbus
        .get_name_owner("org.gnome.Shell".try_into()?)
        .await?
        .as_str()
        != shell_owner
        || u64::from(
            dbus.get_connection_unix_process_id(shell_owner.try_into()?)
                .await?,
        ) != shell_pid
        || !session.get_property::<bool>("Active").await?
        || session.get_property::<bool>("Remote").await?
    {
        return Err("original active local Shell changed during lock".into());
    }
    println!(
        "{}",
        serde_json::json!({
            "originalMonitorTerminatedAndJoined": true, "readOnlyHostThroughout": true,
            "originalShellOwner": shell_owner, "originalShellPid": shell_pid,
            "originalSession": session_id, "lockedHint": true,
            "invalidation": error, "monitorSurvivedBeforeLockMs": 350,
            "invalidationAfterLockRequestMs": invalidated_after_ms,
            "inputAvailableAfter": policy.input_available(), "actualGrantVerified": false,
            "atomicStopVerified": false, "unlockRecoveryVerified": false,
            "probePid": std::process::id()
        })
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("installed native lock acceptance is Linux-only");
    std::process::exit(1);
}
