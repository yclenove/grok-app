//! First local chat prepares bundled files, without starting a control surface.
use super::*;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::sync::OnceLock;

#[derive(Clone)]
struct Installation {
    resource: PathBuf,
    executable: PathBuf,
}

static INSTALLATION: OnceLock<Installation> = OnceLock::new();
static MUTATION: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// Probe-only equivalent of setup's trusted bundle-path registration. This
/// records an installation; the first App session still prepares the runtime.
#[cfg(feature = "computer-use-probe")]
pub(crate) fn register_probe_installation(
    resource: PathBuf,
    executable: PathBuf,
) -> Result<(), String> {
    INSTALLATION
        .set(Installation {
            resource,
            executable,
        })
        .map_err(|_| "probe installation already registered".into())
}

/// Record trusted App paths only. No copying, browser, driver or permission UI.
pub(crate) fn register_installation(app: &tauri::AppHandle) {
    use tauri::Manager;
    let paths = (|| -> Result<Installation, String> {
        let resource = app.path().resource_dir().map_err(|e| e.to_string())?;
        let executable = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or("Computer Use executable directory unavailable")?
            .to_path_buf();
        Ok(Installation {
            resource,
            executable,
        })
    })();
    match paths {
        Ok(paths) => {
            let _ = INSTALLATION.set(paths);
        }
        Err(error) => tracing::warn!(%error, "computer-use bundle paths unavailable"),
    }
}

pub(super) struct MutationGuard {
    _local: parking_lot::MutexGuard<'static, ()>,
    file: File,
}

impl Drop for MutationGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub(super) fn mutation_guard(root: &Path) -> Result<MutationGuard, String> {
    let local = MUTATION.lock();
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".runtime.lock"))
        .map_err(|e| e.to_string())?;
    file.try_lock_exclusive()
        .map_err(|_| "Computer Use runtime is being prepared by another App process".to_string())?;
    Ok(MutationGuard {
        _local: local,
        file,
    })
}

pub(crate) fn ensure_request_runtime() -> Result<(), String> {
    let root = runtime_root();
    initialize(&root, INSTALLATION.get())
}

fn initialize(root: &Path, installation: Option<&Installation>) -> Result<(), String> {
    let _guard = mutation_guard(root)?;
    let store = RuntimeStore::new(root.to_path_buf());
    if store.resolve(JS_RUNTIME).is_ok() {
        return Ok(());
    }
    // A failed old request/diagnostic could have created only the two embedded
    // MCP files. This exact, verified App pack still needs its bundled runtime.
    // Broken, foreign or otherwise incomplete active packs need explicit repair.
    let manifest = store.load_active_manifest();
    let embedded_only = manifest.as_ref().is_ok_and(|manifest| {
        manifest.components.len() == 2
            && [MCP_SERVER, grok_computer_use_core::runtime::MCP_PROTOCOL]
                .iter()
                .all(|id| store.resolve(id).is_ok())
            && store
                .resolve(MCP_SERVER)
                .is_ok_and(|file| std::fs::read(file).is_ok_and(|bytes| bytes == SCRIPT.as_bytes()))
            && store
                .resolve(grok_computer_use_core::runtime::MCP_PROTOCOL)
                .is_ok_and(|file| {
                    std::fs::read(file).is_ok_and(|bytes| bytes == PROTOCOL.as_bytes())
                })
    });
    let pointer_exists = match std::fs::symlink_metadata(root.join("current.json")) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.to_string()),
    };
    if pointer_exists && !embedded_only {
        return store.resolve(JS_RUNTIME).map(|_| ());
    }
    let staging = root.join(".staging");
    match std::fs::symlink_metadata(&staging) {
        Ok(metadata) => {
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || std::fs::read_dir(staging)
                    .map_err(|e| e.to_string())?
                    .next()
                    .is_some()
            {
                return Err(
                    "Computer Use runtime has an interrupted preparation. Repair it in Settings."
                        .into(),
                );
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let installation = installation.ok_or(
        "Computer Use installer paths unavailable. Repair or reinstall Grok App. System Node on PATH is not used.",
    )?;
    let seed = find_installer_seed(&installation.resource, &installation.executable)?;
    store.repair_from_bundle(&seed)?;
    store.ensure_embedded_mcp(SCRIPT, PROTOCOL)?;
    store.resolve(JS_RUNTIME).map(|_| ())
}

#[cfg(test)]
mod tests;
