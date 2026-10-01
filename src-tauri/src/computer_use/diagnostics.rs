//! Computer Use support bundle and separated cleanup. Host paths only.

use grok_computer_use_core::privacy::{
    admit_cleanup_path, clear_owned_tree, export_support_bundle, write_owner, BundleSpec,
    CleanupKind, OWNER_MARKER,
};
use std::path::PathBuf;

fn cu_root() -> PathBuf {
    crate::paths::app_data_root().join("computer-use")
}

fn ensure_root() -> Result<PathBuf, String> {
    let root = cu_root();
    write_owner(&root, "grok-app")?;
    Ok(root)
}

pub fn export_bundle() -> Result<PathBuf, String> {
    let root = ensure_root()?;
    let cap = crate::computer_use::ensure_host_runtime().capabilities();
    let digest = crate::computer_use::runtime::active_manifest()
        .ok()
        .map(|m| m.pack_id)
        .unwrap_or_else(|| "none".into());
    let spec = BundleSpec {
        app_version: env!("CARGO_PKG_VERSION").into(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        capability: serde_json::json!({
            "backend": cap.backend_id,
            "notes": cap.notes,
        }),
        runtime_digest: digest,
        grants_summary: serde_json::json!({
            "featureEnabled": crate::computer_use::ensure_host_runtime().feature_enabled(),
        }),
        worker_health: "host".into(),
        recent_errors: Vec::new(),
        schema_version: "1".into(),
    };
    export_support_bundle(&root, "grok-app", &spec)
}

pub fn clear(kind: CleanupKind) -> Result<(), String> {
    let root = ensure_root()?;
    let dir = root.join(kind.dir_name());
    if !dir.exists() {
        write_owner(&dir, "grok-app")?;
        return Ok(());
    }
    admit_cleanup_path(&dir, "grok-app")?;
    clear_owned_tree(&dir, "grok-app")?;
    write_owner(&dir, "grok-app")?;
    let _ = OWNER_MARKER;
    Ok(())
}

#[cfg(feature = "computer-use-probe")]
pub fn run_host_privacy_gates() -> Result<(), String> {
    grok_computer_use_core::privacy::run_privacy_gates()?;
    let _lock = crate::paths::APP_HOME_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let prev = std::env::var("GROK_APP_HOME").ok();
    let tmp = std::env::temp_dir().join(format!(
        "cu-privacy-host-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let _ = std::fs::create_dir_all(&tmp);
    std::env::set_var("GROK_APP_HOME", &tmp);
    let result = (|| {
        let root = ensure_root()?;
        write_owner(&root.join(CleanupKind::Traces.dir_name()), "grok-app")?;
        write_owner(&root.join(CleanupKind::Staging.dir_name()), "grok-app")?;
        let spec = BundleSpec {
            app_version: env!("CARGO_PKG_VERSION").into(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            capability: serde_json::json!({"backend": "host-gate"}),
            runtime_digest: "gate".into(),
            grants_summary: serde_json::json!({"authorized": false}),
            worker_health: "gate".into(),
            recent_errors: Vec::new(),
            schema_version: "1".into(),
        };
        let zip = export_support_bundle(&root, "grok-app", &spec)?;
        grok_computer_use_core::privacy::rescan_archive(&zip)?;
        clear(CleanupKind::Traces)?;
        clear(CleanupKind::Staging)?;
        let _ = std::fs::remove_file(&zip);
        Ok::<_, String>(())
    })();
    match prev {
        Some(v) => std::env::set_var("GROK_APP_HOME", v),
        None => std::env::remove_var("GROK_APP_HOME"),
    }
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    println!("gate: host_privacy_bundle_and_cleanup");
    Ok(())
}
