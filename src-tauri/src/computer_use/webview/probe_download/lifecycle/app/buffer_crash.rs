//! Publish a real held-buffer witness, then let an external owner kill this App.
//! Without that crash, unwind the original callback and fail, never claim success.

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
        Err("App crash gate requires independent isolated settings".into())
    };
    let target_close = crate::side_browser_host::close(&app, TARGET.into());
    let keeper_close = crate::side_browser_host::close(&app, KEEPER.into());
    serde_json::json!({
        "ok":false,"mode":"webview-download-buffer-crash",
        "error":result.err(),"targetCloseError":target_close.err(),
        "keeperCloseError":keeper_close.err(),"externalCrashObserved":false,
        "realModel":false,"fullDeliveryAcceptance":false
    })
}

fn check(app: &tauri::AppHandle) -> Result<(), String> {
    let mut server = Server::spawn()?;
    let result = scenario(app, &server);
    let cleanup = server.stop_and_join();
    result?;
    cleanup
}

fn scenario(app: &tauri::AppHandle, server: &Server) -> Result<(), String> {
    use std::io::Write;

    io::create(app, KEEPER, &server.page_url(), 20.0)?;
    io::ready(app, KEEPER, false)?;
    io::create(app, TARGET, &server.page_url(), 400.0)?;
    io::ready(app, TARGET, false)?;
    let live = Arc::new(SideBrowserLiveView::open(app.clone(), TARGET.into())?);
    let browser_pid = io::pid(&live.webview)?;
    let keeper = app.get_webview(KEEPER).ok_or("crash keeper missing")?;
    if browser_pid != io::pid(&keeper)? {
        return Err("crash views do not share the original browser".into());
    }
    let scope = Probe::watch(live.instance, true)?;
    let root = crate::paths::app_data_root()
        .join("native-download-crash")
        .join("original-broker-long-path-remains-authoritative-without-shortening");
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let world = Arc::new(DownloadWorld::new(live.clone(), root.clone())?);
    let grant = world.grant()?;
    let cursor = observe_app(&world, &grant, "下载慢速文件")?;
    let args = download_args(&grant, &cursor, "crash-held", "held.bin");
    let pending = PendingDownload::spawn(world.clone(), &grant, args, server.transfer.clone());
    let _held = held::Completion::new(&live, &scope, world.clone(), &grant.run_id);
    server.transfer.wait_started()?;
    let (native_path, buffered) =
        probe::native_staging(live.instance).ok_or("crash buffer registry missing")?;
    if !buffered {
        return Err("crash fixture did not use a private long-path buffer".into());
    }
    server.transfer.release();
    server.transfer.wait_finished()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        pending.assert_waiting()?;
        let counts = scope.counts();
        if counts.native_terminal == 1 && counts.wry_finished == 1 {
            break;
        }
        if Instant::now() >= deadline {
            return Err("crash fixture lacks authentic completed callbacks".into());
        }
        io::eval(app, KEEPER, "document.readyState")?;
    }
    let data = std::fs::read(&native_path).map_err(|error| error.to_string())?;
    if data.len() != 1024 * 1024
        || data.iter().any(|byte| *byte != b'x')
        || scope.counts().wry_processed != 0
        || world.adapter.is_idle(&grant.run_id)
        || !staged_files(&root)?.is_empty()
        || server.downloads() != 1
    {
        return Err("crash fixture buffer is incomplete, published or already retired".into());
    }
    let report = serde_json::json!({
        "mode":"webview-download-buffer-crash","appProcessId":std::process::id(),
        "browserProcessId":browser_pid,"nativeInstance":live.instance,
        "nativePath":native_path,"buffered":true,"realPrivateFileBytes":data.len(),
        "originalCallbacks":scope.counts(),"downloadRequests":server.downloads(),
        "brokerStagingEmpty":true,"realMainIpc":true,"realWryDownloadCallbacks":true,
        "hostApprovalSynthetic":true,"realModel":false,"fullDeliveryAcceptance":false
    });
    let bytes = serde_json::to_vec(&report).map_err(|_| "crash witness invalid")?;
    let mut ready = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(crate::paths::app_data_root().join("native-download-crash-ready.json"))
        .map_err(|_| "crash witness creation failed")?;
    ready
        .write_all(&bytes)
        .and_then(|()| ready.sync_all())
        .map_err(|_| "crash witness publication failed")?;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        pending.assert_waiting()?;
        io::eval(app, KEEPER, "document.readyState")?;
        std::thread::sleep(Duration::from_millis(20));
    }
    Err("external fixture did not crash the original App within its bounded hold".into())
}
