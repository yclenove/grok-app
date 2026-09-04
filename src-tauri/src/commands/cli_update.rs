// from PR #63

/// Run resolved `grok update --check --json` and return a typed DTO.
///
/// Also attaches App compatibility fields (#1009): current App version, absolute
/// floor, and a best-effort GitHub App-update probe so the UI can warn before
/// installing a newer CLI while the App bundle is behind.
#[tauri::command]
pub async fn cli_update_check() -> Result<crate::cli_update::CliUpdateCheck, String> {
    let mut dto = tauri::async_runtime::spawn_blocking(|| {
        let settings = store::load_settings();
        crate::cli_update::check_cli_update(settings.manual_cli_path.as_deref())
    })
    .await
    .map_err(|e| e.to_string())??;

    let app_ver = env!("CARGO_PKG_VERSION");
    let (latest_app, app_update_available) =
        match crate::app_update::check_app_update().await {
            Ok(check) => (Some(check.latest_version), Some(check.update_available)),
            Err(_) => (None, None),
        };
    crate::cli_update::enrich_cli_update_check_app_compat(
        &mut dto,
        app_ver,
        latest_app.as_deref(),
        app_update_available,
    );
    Ok(dto)
}

// from PR #63 / channel UX (CLI >= 0.2.117)

/// Install CLI update / switch channel / pin version.
///
/// Optional `channel` (`stable`|`alpha`), `version` pin, and `force` reinstall.
/// Channel switch and version pin are mutually exclusive; unknown channels error
/// (never invented). Plain update still falls back to App install trust-chain.
///
/// When the App is behind (#1009), pass `acknowledge_app_behind: true` after the
/// UI warning; otherwise Host soft-fails with an `APP_BEHIND:` error.
#[tauri::command]
pub async fn cli_update_install(
    app: tauri::AppHandle,
    channel: Option<String>,
    version: Option<String>,
    force: Option<bool>,
    acknowledge_app_behind: Option<bool>,
) -> Result<crate::cli_install::CliInstallResult, String> {
    let opts = crate::cli_update::CliUpdateInstallOpts {
        channel,
        version,
        force: force.unwrap_or(false),
        acknowledge_app_behind: acknowledge_app_behind.unwrap_or(false),
    };
    crate::cli_update::install_cli_update(app, opts).await
}

/// Recycle every warm agent process so the next send spawns fresh binaries.
/// Used after a CLI upgrade: running children keep executing the old image
/// until restarted (NEW-05). Chat history is untouched; sessions reconnect
/// lazily on the next send.
#[tauri::command]
pub async fn agents_recycle_all(
    app: tauri::AppHandle,
    mgr: State<'_, Arc<SessionManager>>,
) -> Result<(), String> {
    mgr.recycle_all_agents(&app, "cli_upgrade").await;
    Ok(())
}
