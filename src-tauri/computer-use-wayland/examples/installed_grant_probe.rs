//! Explicit owned installed-VM acceptance of the production registry and portal.
//! Experimental observer only; NOT the App/ACP/MCP or a shipping authorization UI.
#[cfg(target_os = "linux")]
#[path = "installed_grant/identity.rs"]
mod identity;
#[cfg(target_os = "linux")]
#[path = "installed_grant/pointer.rs"]
mod pointer;
#[cfg(target_os = "linux")]
#[path = "installed_grant/recovery.rs"]
mod recovery;
#[cfg(target_os = "linux")]
#[path = "installed_grant/run.rs"]
mod run;
#[cfg(target_os = "linux")]
#[path = "installed_grant/ui.rs"]
mod ui;

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(identity::verify())?;
    ui::run(runtime)
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("installed GNOME grant acceptance is Linux-only");
    std::process::exit(1);
}
