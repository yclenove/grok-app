//! Desktop auto-update helpers (Tauri updater plugin + process relaunch).
//!
//! Runtime registration only when release CI injects `GROK_UPDATER_PUBLIC_KEY` +
//! `GROK_UPDATER_ENDPOINT` (`build.rs` → `cfg(grok_updater_enabled)`) on a
//! non-debug binary. The crate itself is always a hard dependency so Tauri ACL
//! can resolve `updater:allow-*` permissions at build time.
//!
//! Local / unsigned builds keep the manual GitHub path via the frontend state
//! machine (`app_check_update`).
//!
//! ## Teardown ordering (P0)
//!
//! For installers that return, call [`prepare_for_app_update`] only after
//! `update.install()` succeeds and before `relaunch()`. Failed staging must not
//! tear down services. The pinned Windows updater patch invokes the App-managed
//! fallible guard after verified staging and before checked installer launch.
//! A rejected guard/launch retains the original candidate and cannot exit.
//! Source/library verification is not signed installed-product acceptance.

use std::sync::Arc;

mod shutdown_gate;

use tauri::{AppHandle, State};
use tracing::info;

use crate::mirror::MirrorHost;
use crate::remote_im::RemoteImState;
use crate::session_manager::SessionManager;
use crate::voice_host::VoiceHost;

/// Process-wide guard so prepare-for-update does not race with itself.
///
/// Concurrent callers await the same result. A later attempt rechecks cleanup;
/// neither a prior failure nor a prior success grants an unconditional restart.
static UPDATE_SHUTDOWN: shutdown_gate::ShutdownGate = shutdown_gate::ShutdownGate::new();

/// Installed before any webview loads. The patched updater invokes this on its
/// owned blocking installer task, after verified staging and before OS launch.
#[cfg(windows)]
pub(crate) fn register_windows_install_guard(app: &AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let app_handle = app.clone();
    let installed = app.manage(tauri_plugin_updater::WindowsInstallGuard::new(move || {
        let app = app_handle.clone();
        let mgr = app
            .try_state::<Arc<SessionManager>>()
            .ok_or("App update SessionManager unavailable")?
            .inner()
            .clone();
        let mirror = app
            .try_state::<Arc<MirrorHost>>()
            .ok_or("App update MirrorHost unavailable")?
            .inner()
            .clone();
        let voice = app
            .try_state::<Arc<VoiceHost>>()
            .ok_or("App update VoiceHost unavailable")?
            .inner()
            .clone();
        let remote_im = app
            .try_state::<Arc<RemoteImState>>()
            .ok_or("App update Remote IM unavailable")?
            .inner()
            .clone();
        // The plugin owns this blocking task even when its IPC caller vanishes.
        tauri::async_runtime::block_on(
            UPDATE_SHUTDOWN
                .run(async move { prepare_owned(app, mgr, mirror, voice, remote_im).await }),
        )
    }));
    if installed {
        Ok(())
    } else {
        Err("App update install guard already registered".into())
    }
}

/// Returns `true` when the running install supports Tauri's auto-updater.
///
/// On Linux, Tauri's updater only works for AppImage bundles. The AppImage
/// runtime sets `APPIMAGE` when the binary is executed from an AppImage.
/// `.deb` / `.rpm` packages surface a manual-download path instead.
///
/// On macOS and Windows every supported install format is auto-updatable.
#[tauri::command]
pub fn is_auto_update_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var("APPIMAGE").is_ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// True when this binary was built with pubkey + endpoint injected
/// (`GROK_UPDATER_*` at compile time) and is not a debug build.
#[tauri::command]
pub fn is_updater_plugin_enabled() -> bool {
    cfg!(grok_updater_enabled) && !cfg!(debug_assertions)
}

/// Snapshot for About / Doctor: which update path this binary can use.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterStatusDto {
    /// Platform packaging supports silent install (e.g. not Linux .deb).
    pub platform_supported: bool,
    /// Release binary built with signing pubkey + endpoint.
    pub plugin_enabled: bool,
    /// `silent` when plugin path is live; `unsupported` when plugin is on but
    /// the package type cannot auto-update (e.g. Linux non-AppImage);
    /// otherwise `github_manual` (unsigned / local / plugin off).
    pub channel: String,
    /// Compile-time endpoint (empty when plugin off).
    pub endpoint: String,
}

#[tauri::command]
pub fn updater_status() -> UpdaterStatusDto {
    let platform_supported = is_auto_update_supported();
    let plugin_enabled = is_updater_plugin_enabled();
    let channel = if plugin_enabled && platform_supported {
        "silent".to_string()
    } else if plugin_enabled && !platform_supported {
        // Signed binary but this install type cannot silent-update.
        "unsupported".to_string()
    } else {
        "github_manual".to_string()
    };
    // Endpoint is only meaningful when the plugin was compiled in; avoid leaking
    // build-time env into debug strings beyond the non-secret public URL.
    let endpoint = if plugin_enabled {
        option_env!("GROK_UPDATER_ENDPOINT")
            .unwrap_or("")
            .to_string()
    } else {
        String::new()
    };
    UpdaterStatusDto {
        platform_supported,
        plugin_enabled,
        channel,
        endpoint,
    }
}

/// Stop managed agent children / hosts before process relaunch after a staged install.
///
/// Runs after a returning `update.install()` succeeds and before `relaunch()`.
/// It cannot guard a platform installer that exits before returning to the UI.
///
/// `remote_im.inner` is held only for the duration of `stop_async`. That method
/// uses a separate global `runtime_slot` mutex (not `remote_im.inner`) and
/// parking_lot fields on `BridgeRuntime` — audited: no re-lock of `inner`.
#[tauri::command]
pub async fn prepare_for_app_update(
    app: AppHandle,
    mgr: State<'_, Arc<SessionManager>>,
    mirror: State<'_, Arc<MirrorHost>>,
    voice: State<'_, Arc<VoiceHost>>,
    remote_im: State<'_, Arc<RemoteImState>>,
) -> Result<(), String> {
    let mgr = Arc::clone(mgr.inner());
    let mirror = Arc::clone(mirror.inner());
    let voice = Arc::clone(voice.inner());
    let remote_im = Arc::clone(remote_im.inner());
    UPDATE_SHUTDOWN
        .run(async move { prepare_owned(app, mgr, mirror, voice, remote_im).await })
        .await
}

async fn prepare_owned(
    app: AppHandle,
    mgr: Arc<SessionManager>,
    mirror: Arc<MirrorHost>,
    voice: Arc<VoiceHost>,
    remote_im: Arc<RemoteImState>,
) -> Result<(), String> {
    info!(target: "grok_app::updater", "stopping managed processes before app relaunch");

    // Relaunch requests cannot be prevented by Tauri's ExitRequested API.
    // Fence dispatch now; the bounded catalog barrier runs after interactive
    // network surfaces have stopped and while ACP endpoints are still indexed.
    crate::computer_use::shutdown::fence_all();

    // Voice realtime session first (network + tool delegation).
    // Pass SessionManager so keep_agents_on_end=false can cancel delegated turns.
    let mut errors = Vec::new();
    let _ = voice.stop(&app, &mgr).await;

    // Remote IM connectors (Feishu / Weixin / …).
    // Hold `inner` only while stop_async runs; stop_async does not re-enter `inner`.
    {
        let mut rt = remote_im.inner.lock().await;
        if let Err(e) = rt.stop_async().await {
            tracing::warn!(target: "grok_app::updater", error = %e, "remote_im stop during prepare_for_app_update");
            errors.push(format!("Remote IM cleanup: {e}"));
        }
    }

    let computer_use_shutdown = crate::computer_use::shutdown::shutdown_with_catalog_barrier(
        &mgr,
        std::time::Duration::from_millis(2_500),
    )
    .await;

    // Kill live + background ACP agent processes; session metadata stays on disk.
    mgr.recycle_all_agents(&app, "app_update").await;

    // Mirror HTTP host + cloudflared tunnel.
    mirror.stop_sync();

    if !computer_use_shutdown.is_clean() {
        errors.push(format!(
            "Computer Use cleanup incomplete ({}/{}): {}",
            computer_use_shutdown.settled,
            computer_use_shutdown.attempted,
            computer_use_shutdown.errors.join("; ")
        ));
    }
    if errors.is_empty() {
        info!(target: "grok_app::updater", "managed cleanup reports settled; ready for relaunch");
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_update_supported_is_bool() {
        let _ = is_auto_update_supported();
        assert!(!is_updater_plugin_enabled() || cfg!(grok_updater_enabled));
    }

    #[test]
    fn updater_status_channel_matches_flags() {
        let s = updater_status();
        assert!(
            s.channel == "silent" || s.channel == "github_manual" || s.channel == "unsupported"
        );
        if s.plugin_enabled && s.platform_supported {
            assert_eq!(s.channel, "silent");
        } else if s.plugin_enabled && !s.platform_supported {
            assert_eq!(s.channel, "unsupported");
        } else {
            assert_eq!(s.channel, "github_manual");
        }
    }

    #[test]
    fn plugin_enabled_false_in_debug_without_cfg() {
        if cfg!(debug_assertions) {
            assert!(!is_updater_plugin_enabled());
        }
    }
}
