//! A real completed private file outlives the held original Wry callback.
//! Exercise both caller orders, including the unchanged eight-second grace.

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
        Err("App buffer gate requires independent isolated settings".into())
    };
    let target_close = crate::side_browser_host::close(&app, TARGET.into());
    let keeper_close = crate::side_browser_host::close(&app, KEEPER.into());
    serde_json::json!({
        "ok":result.is_ok() && target_close.is_ok() && keeper_close.is_ok(),
        "mode":"webview-download-buffer-retirement","nativeProductSideBrowser":true,
        "realMainIpc":true,"realWryDownloadCallbacks":true,"taskGrantPath":true,
        "hostApprovalSynthetic":true,"realAcp":false,"realMcp":false,"realModel":false,
        "loginDataImported":false,"result":result.as_ref().ok(),"error":result.err(),
        "targetCloseError":target_close.err(),"keeperCloseError":keeper_close.err()
    })
}

fn check(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let mut scenarios = Vec::new();
    for cancel in [false, true] {
        let mut server = Server::spawn()?;
        let result = scenario(app, &server, cancel);
        let cleanup = server.stop_and_join();
        scenarios.push(result?);
        cleanup?;
    }
    Ok(serde_json::json!({"scenarios":scenarios}))
}

fn verify_buffer(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| format!("original buffer missing: {error}"))?;
    if bytes.len() != 1024 * 1024 || bytes.iter().any(|byte| *byte != b'x') {
        return Err("original complete private buffer bytes changed".into());
    }
    Ok(())
}

fn scenario(
    app: &tauri::AppHandle,
    server: &Server,
    cancel: bool,
) -> Result<serde_json::Value, String> {
    if !crate::side_browser_host::exists(app, KEEPER) {
        io::create(app, KEEPER, &server.page_url(), 20.0)?;
        io::ready(app, KEEPER, false)?;
    }
    io::create(app, TARGET, &server.page_url(), 400.0)?;
    io::ready(app, TARGET, false)?;
    let live = Arc::new(SideBrowserLiveView::open(app.clone(), TARGET.into())?);
    let pid = io::pid(&live.webview)?;
    let keeper = app.get_webview(KEEPER).ok_or("buffer keeper missing")?;
    if pid != io::pid(&keeper)? {
        return Err("buffer views did not share the original browser".into());
    }
    let scope = Probe::watch(live.instance, true)?;
    let root = crate::paths::app_data_root()
        .join(if cancel {
            "buffer-caller-first"
        } else {
            "buffer-callback-first"
        })
        .join("original-broker-long-path-remains-authoritative-without-shortening");
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let world = Arc::new(DownloadWorld::new(live.clone(), root.clone())?);
    let grant = world.grant()?;
    let cursor = observe_app(&world, &grant, "下载慢速文件")?;
    let args = download_args(&grant, &cursor, "buffer-held", "held.bin");
    let mut pending =
        PendingDownload::spawn(world.clone(), &grant, args.clone(), server.transfer.clone());
    // RAII releases the exact captured callback before joining the caller on errors.
    let mut held = held::Completion::new(&live, &scope, world.clone(), &grant.run_id);
    server.transfer.wait_started()?;
    let (native_path, buffered) =
        probe::native_staging(live.instance).ok_or("original buffer registry missing")?;
    if !buffered {
        return Err("buffer gate did not exercise a real private long-path file".into());
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
            return Err("complete buffer lacked authentic held native/Wry callbacks".into());
        }
        io::eval(app, KEEPER, "document.readyState")?;
    }
    verify_buffer(&native_path)?;
    if scope.counts().wry_processed != 0 || world.adapter.is_idle(&grant.run_id) {
        return Err("first native terminal retired the original callback owner".into());
    }
    let unknown = if cancel {
        world
            .broker
            .request_stop(&grant.run_id)
            .map_err(|error| error.to_string())?;
        let expiry = scope.wait_cancel_expiry()?;
        let result = pending.terminal(Instant::now())?;
        require_tool_error(&result, "held buffer original caller grace")?;
        verify_buffer(&native_path)?;
        if expiry < probe::cancellation_grace()
            || tool_error_kind(&result) != "native-terminal-timeout"
            || scope.counts().wry_processed != 0
            || world.adapter.is_idle(&grant.run_id)
            || probe::native_staging(live.instance) != Some((native_path.clone(), true))
            || !staged_files(&root)?.is_empty()
        {
            return Err(
                "unknown caller released or published the held original private file".into(),
            );
        }
        rejected_call(world.call(&grant, "computer_download", args.clone()))?;
        Some(expiry.as_millis())
    } else {
        None
    };
    let held_counts = scope.counts();
    let released = Instant::now();
    held.release()?;
    if !cancel {
        let result = pending.terminal(released)?;
        let (path, bytes) = completed_path(&result)?;
        let mut published = PublishedFiles::new(root.clone());
        published.retain(path.clone());
        if bytes != 1024 * 1024 {
            return Err("private buffer copy length changed".into());
        }
        verify_buffer(&path)?;
        drop(published);
    }
    let deadline = released + Duration::from_secs(5);
    while !world.adapter.is_idle(&grant.run_id) {
        if Instant::now() >= deadline {
            return Err("released original buffer did not retire".into());
        }
        io::eval(app, KEEPER, "document.readyState")?;
    }
    if native_path.exists()
        || native_path.parent().is_some_and(Path::exists)
        || !staged_files(&root)?.is_empty()
        || scope.counts().wry_processed != 1
        || scope.counts().browser_exited != 0
        || server.downloads() != 1
    {
        return Err("joined original buffer was leaked, republished or replayed".into());
    }
    world
        .broker
        .request_stop(&grant.run_id)
        .map_err(|error| error.to_string())?;
    if world
        .broker
        .wait_stopped(&grant.run_id, Duration::from_secs(5))
        .map_err(|error| error.to_string())?
        != StopState::Stopped
    {
        return Err("original buffer run remained occupied after callbacks".into());
    }
    io::start_close(app, TARGET, 1)?;
    let close = io::close_result(app, 1)?;
    let instance = live.instance;
    let retired = io::native(&keeper, move |_| {
        Ok(crate::computer_use::webview::probe_download_resources_retired(instance))
    })?;
    if close["completed"] != 1 || close["failed"] != 0 || !retired {
        return Err("original buffer controller did not close through main IPC".into());
    }
    let counts = scope.counts();
    let fresh = fresh_owner(
        app,
        pid,
        server,
        &root,
        &world,
        &grant,
        &args,
        live.instance,
    )?;
    Ok(serde_json::json!({
        "cancelledAfterNativeComplete":cancel,"destination":scope.destination(),
        "realPrivateFileBytes":1024*1024,"bufferContentVerified":true,
        "heldOriginalCallbacks":held_counts,"originalCallbacks":counts,
        "originalGraceMs":unknown,"originalCallerJoined":true,"originalAdapterIdle":true,
        "privateBufferRemovedOnlyAfterOriginalCallback":true,"originalNativeResourcesRetired":retired,
        "sameLabelFreshOwner":fresh,"ipcResult":close,"stagingEmpty":true,
        "replayDispatched":false,"lateBodyRevival":false
    }))
}
