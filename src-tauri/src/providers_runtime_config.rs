//! Main-agent runtime preparation. Configuration text is private, never Debug/UI.
use super::*;
use crate::{extensions::ExtensionsPrefs, store::AppSettings};

pub(super) fn runtime_text(
    text: &str,
    settings: &AppSettings,
    permission: &str,
    inject: bool,
    prefs: &ExtensionsPrefs,
) -> Result<String, String> {
    ConfigSnapshot::parse(text)?.runtime_text(settings, permission, inject, prefs)
}

pub(crate) fn permission_text(text: &str, permission: &str) -> Result<String, String> {
    ConfigSnapshot::parse(text)?.permission_text(permission)
}

pub(crate) fn compat_text(text: &str) -> Result<String, String> {
    ConfigSnapshot::parse(text)?.compat_text()
}

fn target_home(home: &Path) -> Result<PathBuf, String> {
    let actual = home
        .canonicalize()
        .map_err(|_| "agent runtime home is unavailable")?;
    if home == agent_home_dir() {
        let base = crate::paths::app_data_root()
            .canonicalize()
            .map_err(|_| "agent runtime base is unavailable")?;
        if actual == base.join("agent-home") {
            return Ok(actual);
        }
    }
    Err("agent runtime home is outside the owned namespace".into())
}

fn checked_path(home: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = home.join(relative);
    let mut ancestor = path.as_path();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                let actual = ancestor
                    .canonicalize()
                    .map_err(|_| "agent runtime path is unavailable")?;
                if actual != ancestor {
                    return Err("agent runtime path redirects its namespace".into());
                }
                if ancestor == path && !metadata.is_file() || ancestor != path && !metadata.is_dir()
                {
                    return Err("agent runtime path has an invalid file type".into());
                }
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor
                    .parent()
                    .ok_or("agent runtime path is unavailable")?;
            }
            Err(_) => return Err("agent runtime path is unavailable".into()),
        }
    }
    Ok(path)
}

fn write_if_changed(home: &Path, relative: &str, bytes: &[u8]) -> Result<(), String> {
    let path = checked_path(home, relative)?;
    match fs::read(&path) {
        Ok(previous) if previous == bytes => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("agent runtime file could not be read".into()),
    }
    crate::store_lock::write_bytes_atomic(&path, bytes)
        .map_err(|_| "agent runtime file could not be published".into())
}

fn claude_settings(home: &Path, permission: &str) -> Result<Vec<u8>, String> {
    let path = checked_path(home, ".claude/settings.json")?;
    let mut document = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)
            .map_err(|_| "agent permission settings are invalid")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(_) => return Err("agent permission settings could not be read".into()),
    };
    let root = document
        .as_object_mut()
        .ok_or("agent permission settings are invalid")?;
    let permissions = root
        .entry("permissions")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or("agent permission settings are invalid")?;
    permissions.insert(
        "defaultMode".into(),
        crate::agent_prefs::claude_default_mode(permission).into(),
    );
    serde_json::to_vec_pretty(&document).map_err(|_| "agent permission settings are invalid".into())
}

fn checked_write_paths(home: &Path, relative: &str) -> Result<(), String> {
    for candidate in [
        relative.to_owned(),
        format!("{relative}.tmp"),
        format!("{relative}.lock"),
    ] {
        checked_path(home, &candidate)?;
    }
    Ok(())
}

pub(crate) fn sync_compat_runtime(mode: &str) -> Result<(), String> {
    if crate::agent_home_config::normalize_mode(mode) == "shared" {
        return Ok(());
    }
    ensure_app_dirs().map_err(|_| "agent runtime home is unavailable")?;
    crate::agent_home_config::with_config_write_lock(|| {
        let home = target_home(&agent_home_dir())?;
        checked_write_paths(&home, "config.toml")?;
        let text = ProcessConfigBinding::read_config(&home.join("config.toml"))?;
        write_if_changed(
            &home,
            "config.toml",
            crate::agent_home_config::ensure_compat_mcp_disabled(&text)?.as_bytes(),
        )
    })
}

pub(crate) fn sync_permission_runtime(mode: &str, permission: &str) -> Result<(), String> {
    if crate::agent_home_config::normalize_mode(mode) == "shared" {
        return Ok(());
    }
    ensure_app_dirs().map_err(|_| "agent runtime home is unavailable")?;
    crate::agent_home_config::with_config_write_lock(|| {
        let home = target_home(&agent_home_dir())?;
        for relative in ["config.toml", ".claude/settings.json"] {
            checked_write_paths(&home, relative)?;
        }
        let text = ProcessConfigBinding::read_config(&home.join("config.toml"))?;
        let next = crate::agent_prefs::set_permission_in_toml(&text, permission)?;
        let claude = claude_settings(&home, permission)?;
        write_if_changed(&home, "config.toml", next.as_bytes())?;
        write_if_changed(&home, ".claude/settings.json", &claude)
    })
}

fn text_only(snapshot: &ConfigSnapshot) -> Result<bool, String> {
    let Some(id) = snapshot.default_model()? else {
        return Ok(false);
    };
    let Some(fields) = snapshot.model_fields(id)? else {
        return Ok(false);
    };
    let model = fields.get("model").map(String::as_str).unwrap_or(id);
    let models = decode_app_models(fields.get(APP_MODELS_KEY).map(String::as_str), model, model);
    Ok(crate::models_aux::custom_model_is_text_only(
        id,
        fields.get("name").map(String::as_str).unwrap_or(id),
        model,
        &models,
        supports_vision_from_fields(&fields),
    ))
}

/// Legacy independent startup only. New generations require the unpublished
/// capability below; a marker is never permission to rewrite a live profile.
/// No auth-store, global-settings or history lookup occurs in this consumer.
pub(crate) fn prepare_main_runtime(
    home: &Path,
    settings: &AppSettings,
    permission: &str,
    expected_custom: bool,
    official_available: bool,
    prefs: &ExtensionsPrefs,
) -> Result<bool, String> {
    crate::agent_home_config::with_config_write_lock(|| {
        let home = target_home(home)?;
        prepare_runtime_files(
            &home,
            settings,
            permission,
            expected_custom,
            official_available,
            prefs,
        )
    })
}

pub(super) fn prepare_pending_runtime(
    pending: &super::execution_home::PendingHome,
    settings: &AppSettings,
    permission: &str,
    expected_custom: bool,
    official_available: bool,
    prefs: &ExtensionsPrefs,
) -> Result<bool, String> {
    pending.check()?;
    let home = pending
        .home()
        .canonicalize()
        .map_err(|_| "execution preparation directory is unavailable")?;
    let inject = prepare_runtime_files(
        &home,
        settings,
        permission,
        expected_custom,
        official_available,
        prefs,
    )?;
    pending.check()?;
    Ok(inject)
}

fn prepare_runtime_files(
    home: &Path,
    settings: &AppSettings,
    permission: &str,
    expected_custom: bool,
    official_available: bool,
    prefs: &ExtensionsPrefs,
) -> Result<bool, String> {
    let managed = [
        "config.toml",
        ".claude/settings.json",
        "hooks/grok-app-block-native-imagine.json",
        "hooks/grok-app-block-native-imagine.sh",
    ];
    // Reject all redirects and malformed settings before changing any file.
    for relative in managed {
        checked_write_paths(home, relative)?;
    }
    let path = checked_path(home, "config.toml")?;
    let text = ProcessConfigBinding::read_config(&path)?;
    if ProcessConfigBinding::custom_from_text(&text)? != expected_custom {
        return Err("provider route changed during runtime preparation".into());
    }
    let inject = expected_custom && settings.official_aux_inject && official_available;
    let block_image_read = expected_custom && text_only(&ConfigSnapshot::parse(&text)?)?;
    let next = runtime_text(&text, settings, permission, inject, prefs)?;
    let claude = claude_settings(home, permission)?;
    write_if_changed(home, "config.toml", next.as_bytes())?;
    write_if_changed(home, ".claude/settings.json", &claude)?;
    if inject {
        write_if_changed(
            home,
            managed[2],
            crate::official_aux::native_media_block_hook_json().as_bytes(),
        )?;
        write_if_changed(
            home,
            managed[3],
            crate::official_aux::native_media_block_hook_script_body_with(block_image_read)
                .as_bytes(),
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(home.join(managed[3]), fs::Permissions::from_mode(0o755))
                .map_err(|_| "agent runtime hook could not be prepared")?;
        }
    } else {
        for relative in &managed[2..] {
            match fs::remove_file(checked_path(home, relative)?) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("agent runtime hook could not be removed".into()),
            }
        }
    }
    if next != text {
        crate::extensions::invalidate_mcp_cache();
    }
    Ok(inject)
}

#[cfg(test)]
#[path = "providers_runtime_config_tests.rs"]
mod tests;
