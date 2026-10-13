//! Baseline normal App exit with real main/side views and no download request.

use super::*;

pub(crate) fn run(app: tauri::AppHandle) -> serde_json::Value {
    let settings = crate::store::load_settings();
    let isolated = crate::paths::isolated_app_instance_id().is_some()
        && settings.session_data_mode == "independent"
        && !settings.close_to_tray
        && !settings.keep_tray_for_schedules;
    let result = if isolated {
        io::ready(&app, "main", true).and_then(|()| check(&app))
    } else {
        Err("idle exit gate requires independent isolated settings".into())
    };
    let target_close = crate::side_browser_host::close(&app, TARGET.into());
    let keeper_close = crate::side_browser_host::close(&app, KEEPER.into());
    serde_json::json!({
        "ok":result.is_ok() && target_close.is_ok() && keeper_close.is_ok(),
        "mode":"webview-exit-idle","result":result.as_ref().ok(),"error":result.err(),
        "targetCloseError":target_close.err(),"keeperCloseError":keeper_close.err(),
        "realMainWebView":true,"realSideBrowser":true,"downloadRequests":0,
        "realModel":false,"fullDeliveryAcceptance":false
    })
}

fn check(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let mut server = Server::spawn()?;
    let result = scenario(app, &server);
    let cleanup = server.stop_and_join();
    let result = result?;
    cleanup?;
    Ok(result)
}

fn scenario(app: &tauri::AppHandle, server: &Server) -> Result<serde_json::Value, String> {
    io::create(app, KEEPER, &server.page_url(), 20.0)?;
    io::ready(app, KEEPER, false)?;
    io::create(app, TARGET, &server.page_url(), 400.0)?;
    io::ready(app, TARGET, false)?;
    let main = app.get_webview("main").ok_or("idle main view missing")?;
    let target = Arc::new(SideBrowserLiveView::open(app.clone(), TARGET.into())?);
    let keeper = Arc::new(SideBrowserLiveView::open(app.clone(), KEEPER.into())?);
    let main_pid = io::pid(&main)?;
    let target_pid = io::pid(&target.webview)?;
    let keeper_pid = io::pid(&keeper.webview)?;
    if target_pid != keeper_pid || server.downloads() != 0 {
        return Err("idle baseline browser identity or zero-effect check failed".into());
    }
    let target_instance = target.instance;
    let keeper_instance = keeper.instance;
    let installed = io::native(&main, move |_| {
        Ok(
            !crate::computer_use::webview::probe_download_resources_retired(target_instance)
                && !crate::computer_use::webview::probe_download_resources_retired(keeper_instance),
        )
    })?;
    if !installed {
        return Err("idle baseline native observers never installed".into());
    }
    crate::side_browser_host::close(app, TARGET.into())?;
    crate::side_browser_host::close(app, KEEPER.into())?;
    let retired = io::native(&main, move |_| {
        Ok(
            crate::computer_use::webview::probe_download_resources_retired(target_instance)
                && crate::computer_use::webview::probe_download_resources_retired(keeper_instance),
        )
    })?;
    if !retired || server.downloads() != 0 {
        return Err("idle baseline controllers did not retire through normal close".into());
    }
    Ok(serde_json::json!({
        "mainBrowserProcessId":main_pid,"sideBrowserProcessId":target_pid,
        "mainAndSideShareBrowser":main_pid==target_pid,"nativeObserversInstalled":installed,
        "originalNativeResourcesRetired":retired,"downloadRequests":server.downloads(),
        "targetExistsAfterClose":crate::side_browser_host::exists(app,TARGET),
        "keeperExistsAfterClose":crate::side_browser_host::exists(app,KEEPER)
    }))
}
