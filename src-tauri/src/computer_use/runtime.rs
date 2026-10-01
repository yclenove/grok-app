//! Host wrapper: Computer Use runtime lives under the App private data dir.
use grok_computer_use_core::runtime::{required_runtime_components, RuntimeIssue, RuntimeStore};
use std::path::{Path, PathBuf};

pub use grok_computer_use_core::runtime::{
    BROWSER_WORKER, CHROMIUM, DRIVER, JS_RUNTIME, MCP_SERVER, PLAYWRIGHT, PLAYWRIGHT_ARCHIVE,
    PLAYWRIGHT_RUNTIME,
};

const SCRIPT: &str = include_str!("../../../tools/computer-use-mcp/server.mjs");
const PROTOCOL: &str = include_str!("../../../tools/computer-use-mcp/protocol.mjs");

pub fn runtime_root() -> PathBuf {
    crate::paths::app_data_root()
        .join("computer-use")
        .join("runtime")
}

fn store() -> RuntimeStore {
    RuntimeStore::new(runtime_root())
}

pub fn resolve(component_id: &str) -> Result<PathBuf, String> {
    store().resolve(component_id)
}

pub fn product_paths() -> Result<grok_computer_use_core::runtime::ProductRuntimePaths, String> {
    store().product_runtime_paths()
}

pub fn active_manifest() -> Result<grok_computer_use_core::runtime::RuntimeManifest, String> {
    store().load_active_manifest()
}

pub fn ensure_embedded_mcp(script: &str, protocol: &str) -> Result<(), String> {
    store().ensure_embedded_mcp(script, protocol)
}

pub fn find_installer_seed(resource_dir: &Path, exe_dir: &Path) -> Result<PathBuf, String> {
    let candidates = [
        resource_dir.join("computer-use").join("seed"),
        resource_dir
            .join("resources")
            .join("computer-use")
            .join("seed"),
        exe_dir.join("computer-use").join("seed"),
    ];
    candidates
        .into_iter()
        .find(|p| p.join("manifest.json").is_file())
        .ok_or_else(|| {
            "Computer Use installer seed is missing. Repair or reinstall Grok App. System Node on PATH is not used.".into()
        })
}

pub fn diagnose() -> Vec<RuntimeIssue> {
    let mut issues = store().diagnose(required_runtime_components());
    if issues
        .iter()
        .any(|i| i.component == MCP_SERVER || i.component == "mcp-protocol")
    {
        // MCP scripts are embedded in the App binary; try to materialize them first.
        let _ = store().ensure_embedded_mcp(SCRIPT, PROTOCOL);
        issues = store().diagnose(required_runtime_components());
    }
    issues
}

pub fn repair_from_install(resource_dir: &Path, exe_dir: &Path) -> Result<String, String> {
    let seed = find_installer_seed(resource_dir, exe_dir)?;
    let pack = store().repair_from_bundle(&seed)?;
    store().ensure_embedded_mcp(SCRIPT, PROTOCOL)?;
    let leftover = store().diagnose(required_runtime_components());
    if !leftover.is_empty() {
        return Err(format!(
            "Computer Use repair finished but runtime is still invalid ({:?}). Reinstall Grok App. System Node on PATH is not used.",
            leftover.iter().map(|i| &i.code).collect::<Vec<_>>()
        ));
    }
    Ok(pack)
}

pub fn rollback() -> Result<String, String> {
    store().rollback_pack()
}

pub fn can_rollback() -> bool {
    store().has_previous_pack()
}

/// This-branch seed → isolated GROK_APP_HOME. Never PATH, never official Grok home.
#[cfg(feature = "computer-use-probe")]
pub fn run_isolated_repair_gate() -> Result<(), String> {
    let home = std::env::var("GROK_APP_HOME")
        .map_err(|_| "GROK_APP_HOME is required for isolated repair".to_string())?;
    let home_path = PathBuf::from(&home);
    let home_s = home.replace('\\', "/").to_ascii_lowercase();
    if home_s.contains("appdata/local/grok")
        || home_s.ends_with("/.grok")
        || home_s.contains("/.grok/")
    {
        return Err("isolated GROK_APP_HOME must not be official Grok or shared ~/.grok".into());
    }
    let resource = std::env::var("GROK_CU_RESOURCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources"));
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| resource.clone());
    let pack = repair_from_install(&resource, &exe_dir)?;
    let leftover = diagnose();
    if !leftover.is_empty() {
        return Err(format!("isolated diagnose leftover {leftover:?}"));
    }
    let js = resolve(JS_RUNTIME)?;
    let pw = resolve(PLAYWRIGHT_ARCHIVE)?;
    let modules = resolve(PLAYWRIGHT_RUNTIME)?;
    if !js.starts_with(&home_path)
        || !pw.starts_with(&home_path)
        || !modules.starts_with(&home_path)
    {
        return Err(format!(
            "resolved runtime escaped isolated home js={} pw={} home={}",
            js.display(),
            pw.display(),
            home_path.display()
        ));
    }
    grok_computer_use_core::runtime_prepare::check_module_import(&js, &modules)
        .map_err(|e| format!("isolated runtime import {}: {}", e.code, e.message))?;
    let pw_bytes = std::fs::read(&pw).map_err(|e| e.to_string())?;
    if pw_bytes.len() < 1_000_000 || !pw_bytes.starts_with(&[0x1f, 0x8b]) {
        return Err(format!(
            "isolated playwright is not a gzip tarball ({} bytes)",
            pw_bytes.len()
        ));
    }
    println!("gate: isolated_runtime_repair pack={pack}");
    println!("gate: isolated_home={}", home_path.display());
    println!(
        "gate: js_runtime={} version=v{} module_import=passed",
        js.display(),
        grok_computer_use_core::runtime::REQUIRED_JS_RUNTIME_VERSION
    );
    println!(
        "gate: playwright={} bytes={} gzip=1f8b",
        pw.display(),
        pw_bytes.len()
    );
    println!("gate: isolated_runtime_repair PASS");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use grok_computer_use_core::runtime::{JS_RUNTIME, PLAYWRIGHT_ARCHIVE};
    use std::ffi::{OsStr, OsString};

    #[test]
    fn portable_resource_layout_is_discoverable_beside_the_executable() {
        // Layout-only fixture: this does not execute a portable App or repair a
        // real user runtime. Runtime integrity is checked separately.
        let root =
            std::env::temp_dir().join(format!("cu-portable-layout-{}", uuid::Uuid::new_v4()));
        let seed = root.join("resources").join("computer-use").join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        std::fs::write(seed.join("manifest.json"), b"{}").unwrap();
        assert_eq!(find_installer_seed(&root, &root).unwrap(), seed);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn executable_only_portable_layout_has_no_runtime_seed() {
        let root = std::env::temp_dir().join(format!("cu-portable-empty-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Grok.exe"), b"inert fixture, never executed").unwrap();
        assert!(find_installer_seed(&root, &root).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    struct EnvGuard {
        saved: Vec<(OsString, Option<OsString>)>,
    }

    impl EnvGuard {
        fn new() -> Self {
            Self { saved: Vec::new() }
        }

        fn set(&mut self, key: &str, value: impl AsRef<OsStr>) {
            let key = OsString::from(key);
            if !self.saved.iter().any(|(saved, _)| saved == &key) {
                self.saved.push((key.clone(), std::env::var_os(&key)));
            }
            std::env::set_var(key, value);
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..).rev() {
                match value {
                    Some(value) => std::env::set_var(&key, value),
                    None => std::env::remove_var(&key),
                }
            }
        }
    }

    #[test]
    fn shipped_seed_repair_from_install_activates_official_node_and_ignores_path_node() {
        let _home = crate::paths::APP_HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("grok-cu-s24-host-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let decoy = tmp.join("path-decoy");
        std::fs::create_dir_all(&decoy).unwrap();
        std::fs::write(decoy.join("node.exe"), b"SYSTEM-NODE").unwrap();
        std::fs::write(decoy.join("node"), b"SYSTEM-NODE").unwrap();
        let prev_home = std::env::var_os("GROK_APP_HOME");
        let prev_path = std::env::var_os("PATH");
        let mut env = EnvGuard::new();
        env.set("GROK_APP_HOME", &tmp);
        env.set("PATH", decoy.as_os_str());
        let resource = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        let result = repair_from_install(&resource, &resource);
        let issues = diagnose();
        let resolved = resolve(JS_RUNTIME);
        result.expect("repair_from_install must succeed for official Node seed");
        assert!(
            issues
                .iter()
                .all(|i| !(i.code == "placeholder" && i.component == JS_RUNTIME)),
            "{issues:?}"
        );
        let js = resolved.expect("js-runtime must resolve after repair");
        assert!(js.starts_with(&tmp), "{}", js.display());
        assert!(!js.starts_with(&decoy), "{}", js.display());
        let bytes = std::fs::read(&js).unwrap();
        assert!(
            grok_computer_use_core::runtime::looks_like_native_binary(&bytes),
            "got {} bytes",
            bytes.len()
        );
        let pw =
            resolve(PLAYWRIGHT_ARCHIVE).expect("playwright-core archive must resolve after repair");
        assert!(pw.starts_with(&tmp), "{}", pw.display());
        assert!(!pw.starts_with(&decoy), "{}", pw.display());
        let pw_bytes = std::fs::read(&pw).unwrap();
        assert_ne!(
            pw_bytes.as_slice(),
            grok_computer_use_core::runtime::PLACEHOLDER_PLAYWRIGHT
        );
        drop(env);
        assert_eq!(std::env::var_os("GROK_APP_HOME"), prev_home);
        assert_eq!(std::env::var_os("PATH"), prev_path);
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn env_guard_restores_after_unwind() {
        let _home = crate::paths::APP_HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        const KEY: &str = "GROK_CU_ENV_GUARD_TEST";
        let previous = std::env::var_os(KEY);
        let result = std::panic::catch_unwind(|| {
            let mut env = EnvGuard::new();
            env.set(KEY, "temporary");
            assert_eq!(std::env::var_os(KEY), Some(OsString::from("temporary")));
            panic!("exercise EnvGuard unwind restoration");
        });
        assert!(result.is_err());
        assert_eq!(std::env::var_os(KEY), previous);
    }
}
