use super::{
    bundle::{Bundle, Installation, ShellStamp},
    lock::PublicationLock,
    HelperAction, HelperStatus,
};
use grok_computer_use_wayland::{GnomeHelperControl, HelperHealth, HelperSnapshot};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn root() -> Result<PathBuf, String> {
    let root = match std::env::var_os("XDG_DATA_HOME").filter(|s| !s.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => {
            PathBuf::from(std::env::var_os("HOME").ok_or("computer_use_helper_files: no HOME")?)
                .join(".local/share")
        }
    };
    let root = root.join("gnome-shell/extensions");
    super::bundle::safe_path(&root)?;
    Ok(root)
}
fn stamp(control: &GnomeHelperControl) -> ShellStamp {
    ShellStamp {
        bus: control.bus_id.clone(),
        owner: control.shell_owner.clone(),
        process: control.shell_process.clone(),
    }
}
fn our_path(bundle: &Bundle, snapshot: &HelperSnapshot) -> bool {
    snapshot
        .extension
        .as_ref()
        .is_none_or(|e| e.per_user && std::path::Path::new(&e.path) == bundle.path())
}

fn present(
    bundle: &Bundle,
    installation: Installation,
    snapshot: &HelperSnapshot,
    shell: &ShellStamp,
) -> HelperStatus {
    let mut status = HelperStatus::unavailable("unavailable");
    status.available = true;
    status.shell_version = Some(snapshot.shell_version.clone());
    status.installation = Some(
        serde_json::to_value(installation)
            .expect("unit enum serializes")
            .as_str()
            .unwrap()
            .to_owned(),
    );
    let mut actions = Vec::new();
    status.state = if !snapshot.supported() {
        "unsupported"
    } else if !our_path(bundle, snapshot) || installation == Installation::Conflict {
        "conflict"
    } else {
        match installation {
            Installation::Missing => {
                if snapshot.inactive() {
                    actions.push(HelperAction::Install);
                }
                "missing"
            }
            Installation::Modified | Installation::RecoveryRequired => {
                if snapshot.inactive() {
                    actions.push(HelperAction::Repair);
                }
                if snapshot.extension.is_some() && !snapshot.inactive() {
                    actions.push(HelperAction::Disable);
                }
                "repair_required"
            }
            Installation::Current => {
                if snapshot.extension.is_some() {
                    actions.push(HelperAction::Disable);
                }
                if snapshot.inactive() {
                    actions.push(HelperAction::Repair);
                }
                if bundle.restart_required(shell)
                    || snapshot.extension.is_none()
                    || snapshot.needs_restart()
                {
                    "restart_required"
                } else if !snapshot.user_extensions_enabled {
                    "global_disabled"
                } else if snapshot.active() {
                    "ready"
                } else if snapshot.enableable() {
                    actions.push(HelperAction::Enable);
                    "disabled"
                } else if snapshot.health == HelperHealth::Blocked {
                    "blocked"
                } else {
                    "unconfirmed"
                }
            }
            Installation::Conflict => unreachable!(),
        }
    };
    if !status.busy && !status.feature_enabled {
        status.actions = actions;
    }
    status
}

pub(super) async fn status() -> Result<HelperStatus, String> {
    if !super::super::linux_portal::selected() {
        return Ok(HelperStatus::unavailable("unavailable"));
    }
    let control = GnomeHelperControl::connect().await?;
    let snapshot = control.snapshot().await?;
    let shell = stamp(&control);
    let root = root()?;
    tauri::async_runtime::spawn_blocking(move || {
        let bundle = Bundle::new(root);
        Ok(present(&bundle, bundle.inspect()?, &snapshot, &shell))
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(super) async fn act(action: HelperAction) -> Result<(), String> {
    if !super::super::linux_portal::selected() {
        return Err("computer_use_helper_unavailable".into());
    }
    let _transition = super::super::feature_lifecycle::transition_lock()
        .lock()
        .await;
    if super::super::feature::feature_enabled() {
        return Err("computer_use_helper_turn_off_first".into());
    }
    let registry = super::super::linux_portal::registry();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !registry.quiesce_for_maintenance()? {
        if Instant::now() >= deadline {
            return Err("computer_use_helper_cleanup_pending".into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let control = GnomeHelperControl::connect().await?;
    let snapshot = control.snapshot().await?;
    if !snapshot.supported() {
        return Err("computer_use_helper_unsupported".into());
    }
    let root = root()?;
    let (bundle, _lock) = tauri::async_runtime::spawn_blocking(move || {
        let lock = PublicationLock::acquire(&root)?;
        Ok::<_, String>((Bundle::new(root), lock))
    })
    .await
    .map_err(|e| e.to_string())??;
    let snapshot = control.snapshot().await?;
    let installation = bundle.inspect()?;
    if !our_path(&bundle, &snapshot) || installation == Installation::Conflict {
        return Err("computer_use_helper_conflict".into());
    }
    let shell = stamp(&control);
    let eligible = match action {
        HelperAction::Install => installation == Installation::Missing && snapshot.inactive(),
        HelperAction::Repair => installation != Installation::Missing && snapshot.inactive(),
        HelperAction::Enable => {
            installation == Installation::Current
                && !bundle.restart_required(&shell)
                && snapshot.enableable()
        }
        HelperAction::Disable => snapshot.extension.is_some(),
    };
    if !eligible {
        return Err("computer_use_helper_action_not_ready".into());
    }
    match action {
        HelperAction::Install | HelperAction::Repair => {
            // Replacing files never reloads cached GJS modules. A later Shell
            // owner AND discovery/readback are required before explicit Enable.
            tauri::async_runtime::spawn_blocking(move || {
                bundle.install(action == HelperAction::Repair, &shell)
            })
            .await
            .map_err(|e| e.to_string())??;
            let after = control.snapshot().await?;
            if !after.inactive() {
                return Err("computer_use_helper_state_changed_after_publication".into());
            }
            Ok(())
        }
        HelperAction::Enable => control.set_enabled(true).await,
        HelperAction::Disable => control.set_enabled(false).await,
    }
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;

#[cfg(all(test, feature = "computer-use-wayland-preview"))]
#[path = "live_shell_tests.rs"]
mod live_shell_tests;

#[cfg(all(test, feature = "computer-use-wayland-preview"))]
#[path = "vm_shell_tests.rs"]
mod vm_shell_tests;
