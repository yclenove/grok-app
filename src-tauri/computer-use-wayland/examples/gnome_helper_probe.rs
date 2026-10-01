//! Opt-in real-Shell probe, NOT a product command or desktop grant.
//! Run only inside the private PID/mount/network/D-Bus acceptance namespace.
#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use grok_computer_use_wayland::GnomeHelperControl;
    use std::io::{BufRead, Write};

    if std::env::var("GROK_CU_PRIVATE_SHELL_ACCEPTANCE").as_deref() != Ok("1")
        || std::env::var("HOME").as_deref() != Ok("/tmp/cu-home")
        || std::env::var("XDG_RUNTIME_DIR").as_deref() != Ok("/tmp/cu-runtime")
        || !matches!(
            std::fs::read_to_string("/tmp/cu-home/.private-shell-acceptance").as_deref(),
            Ok("owned isolated GNOME Shell acceptance\n")
        )
    {
        return Err("private Shell acceptance environment required".into());
    }
    let action = std::env::args().nth(1).ok_or("probe action required")?;
    if !matches!(
        action.as_str(),
        "snapshot" | "enable" | "disable" | "owner-loss"
    ) {
        return Err("unknown probe action".into());
    }
    let control = GnomeHelperControl::connect().await?;
    if action == "owner-loss" {
        println!("{}", serde_json::json!({"pinned": control.shell_process}));
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        if line != "check\n" {
            return Err("explicit owner-loss checkpoint required".into());
        }
        match control.snapshot().await {
            Ok(_) => return Err("old owner unexpectedly remained valid".into()),
            Err(error) => println!("{}", serde_json::json!({"oldOwnerRejected": error})),
        }
        return Ok(());
    }
    if action != "snapshot" {
        control.set_enabled(action == "enable").await?;
    }
    let snapshot = control.snapshot().await?;
    println!(
        "{}",
        serde_json::json!({
            "action": action,
            "bus": control.bus_id,
            "owner": control.shell_owner,
            "process": control.shell_process,
            "shellVersion": snapshot.shell_version,
            "globalEnabled": snapshot.user_extensions_enabled,
            "health": format!("{:?}", snapshot.health),
            "active": snapshot.active(),
            "inactive": snapshot.inactive(),
            "enableable": snapshot.enableable(),
            "needsRestart": snapshot.needs_restart(),
            "extension": snapshot.extension.as_ref().map(|extension| serde_json::json!({
                "state": extension.state, "enabled": extension.enabled,
                "path": extension.path, "perUser": extension.per_user,
            })),
        })
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("GNOME Shell acceptance is Linux-only");
    std::process::exit(1);
}
