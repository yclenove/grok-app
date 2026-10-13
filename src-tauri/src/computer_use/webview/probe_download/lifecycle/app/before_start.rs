//! Real App download requests held before HTTP response headers / native start.

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
        Err("App download gate requires independent isolated settings".into())
    };
    let target_close = crate::side_browser_host::close(&app, TARGET.into());
    let keeper_close = crate::side_browser_host::close(&app, KEEPER.into());
    serde_json::json!({
        "ok":result.is_ok() && target_close.is_ok() && keeper_close.is_ok(),
        "mode":"webview-download-before-start","nativeProductSideBrowser":true,
        "realMainIpc":result.is_ok(),"realWryDownloadCallbacks":result.is_ok(),
        "taskGrantPath":true,"controlDispatch":"original-broker",
        "hostApprovalSynthetic":true,"realAcp":false,"realMcp":false,"realModel":false,
        "loginDataImported":false,"result":result.as_ref().ok(),"error":result.err(),
        "targetCloseError":target_close.err(),"keeperCloseError":keeper_close.err()
    })
}

fn check(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let mut scenarios = Vec::new();
    for (action, grace) in [
        (Control::Pause, false),
        (Control::Stop, false),
        (Control::Stop, true),
        (Control::Navigate, false),
        (Control::Close, false),
    ] {
        let mut server = Server::spawn_before_start()?;
        let result = scenario(app, &server, action, grace)
            .map_err(|error| format!("before-start {action:?} grace={grace}: {error}"));
        let cleanup = server.stop_and_join();
        scenarios.push(result?);
        cleanup?;
    }
    Ok(serde_json::json!({"scenarios":scenarios}))
}

fn scenario(
    app: &tauri::AppHandle,
    server: &Server,
    action: Control,
    grace: bool,
) -> Result<serde_json::Value, String> {
    if !crate::side_browser_host::exists(app, KEEPER) {
        io::create(app, KEEPER, &server.page_url(), 20.0)?;
        io::ready(app, KEEPER, false)?;
    }
    io::create(app, TARGET, &server.page_url(), 400.0)?;
    io::ready(app, TARGET, false)?;
    let live = Arc::new(SideBrowserLiveView::open(app.clone(), TARGET.into())?);
    let pid = io::pid(&live.webview)?;
    let keeper = app.get_webview(KEEPER).ok_or("App keeper missing")?;
    if pid != io::pid(&keeper)? {
        return Err("App views do not share a physical browser".into());
    }
    let scope = Probe::watch(live.instance, false)?;
    let result = check_download(app, &live, &scope, pid, server, action, grace);
    let closed = crate::side_browser_host::close(app, TARGET.into());
    match result {
        Ok(value) => {
            closed?;
            Ok(value)
        }
        Err(error) => Err(format!(
            "{action:?}: {error}; original_callbacks={}; close_error={:?}",
            serde_json::to_string(&scope.counts()).map_err(|_| "native counters invalid")?,
            closed.err()
        )),
    }
}

fn zero_callbacks(counts: &probe::Counts) -> bool {
    counts.native_started == 0
        && counts.wry_requested == 0
        && counts.native_terminal == 0
        && counts.wry_finished == 0
        && counts.wry_processed == 0
        && counts.browser_exited == 0
}

#[allow(clippy::too_many_arguments)]
fn check_download(
    app: &tauri::AppHandle,
    live: &Arc<SideBrowserLiveView>,
    scope: &Arc<Probe>,
    pid: u32,
    server: &Server,
    action: Control,
    grace: bool,
) -> Result<serde_json::Value, String> {
    let root = crate::paths::app_data_root().join(match (action, grace) {
        (Control::Pause, false) => "download-before-start-pause",
        (Control::Stop, false) => "download-before-start-stop",
        (Control::Stop, true) => "download-before-start-stop-grace",
        (Control::Navigate, false) => "download-before-start-navigate",
        (Control::Close, false) => "download-before-start-close",
        _ => return Err("unsupported before-start control".into()),
    });
    std::fs::create_dir(&root).map_err(|error| error.to_string())?;
    let world = Arc::new(DownloadWorld::new(live.clone(), root.clone())?);
    let grant = world.grant()?;
    let cursor = observe_app(&world, &grant, "下载慢速文件")?;
    let args = download_args(&grant, &cursor, "app-before-start", "held.bin");
    let mut pending =
        PendingDownload::spawn(world.clone(), &grant, args.clone(), server.transfer.clone());
    server.transfer.wait_requested()?;
    let (native_path, native_buffered) =
        probe::native_staging(live.instance).ok_or("original native staging owner missing")?;
    if grace && !native_buffered {
        return Err("long-path grace did not exercise the private native buffer".into());
    }
    pending.assert_waiting()?;
    if !zero_callbacks(&scope.counts())
        || !server.transfer.is_before_start_held()
        || world.adapter.is_idle(&grant.run_id)
        || server.downloads() != 1
        || !staged_files(&root)?.is_empty()
    {
        return Err("original HTTP request was not held before the real native start".into());
    }
    let control_at = Instant::now();
    match action {
        Control::Pause => {
            world
                .broker
                .pause(&grant.run_id)
                .map_err(|error| error.to_string())?;
            if !world
                .broker
                .is_paused(&grant.run_id)
                .map_err(|error| error.to_string())?
            {
                return Err("Pause did not fence original task authority".into());
            }
        }
        Control::Stop => {
            let ticket = world
                .broker
                .fence_stop(&grant.run_id)
                .map_err(|error| error.to_string())?
                .ok_or("original Stop did not produce its cleanup ticket")?;
            if world
                .broker
                .finish_stop_cleanup(&ticket)
                .map_err(|error| error.to_string())?
                != StopState::StopRequested
            {
                return Err("Stop acknowledged retirement before original download start".into());
            }
        }
        Control::Navigate => {
            let generation = live.navigation_generation();
            live.webview
                .navigate(tauri::Url::parse(&server.page_url()).map_err(|e| e.to_string())?)
                .map_err(|error| error.to_string())?;
            let deadline = control_at + Duration::from_secs(5);
            while live.navigation_generation() == generation {
                pending.assert_waiting()?;
                if Instant::now() >= deadline {
                    return Err("pre-start original navigation did not retire authority".into());
                }
                io::eval(app, "main", "document.readyState")?;
            }
            io::ready(app, TARGET, false)?;
        }
        Control::Close => io::start_close(app, TARGET, 1)?,
    }
    let deadline = control_at + Duration::from_secs(5);
    while !probe::cancelled_before_start(live.instance) {
        pending.assert_waiting()?;
        if Instant::now() >= deadline {
            return Err(
                "original download cancellation fence was not witnessed before headers".into(),
            );
        }
        io::eval(app, "main", "document.readyState")?;
    }
    let before = scope.counts();
    pending.assert_waiting()?;
    rejected_call(world.call(&grant, "computer_download", args.clone()))?;
    if !zero_callbacks(&before)
        || !zero_callbacks(&scope.counts())
        || !server.transfer.is_before_start_held()
        || world.adapter.is_idle(&grant.run_id)
        || server.downloads() != 1
        || !staged_files(&root)?.is_empty()
        || io::eval(app, KEEPER, "document.readyState")? != "complete"
    {
        return Err(
            "cancelled pre-start download released occupancy, replayed or blocked keeper".into(),
        );
    }
    let fence_ms = control_at.elapsed().as_millis();
    eprintln!("gate: before_start control={action:?} grace={grace} fence_ms={fence_ms} counts={} stages={}",
        serde_json::to_string(&before).map_err(|_| "invalid counters")?,
        serde_json::to_string(&scope.stages()).map_err(|_| "invalid stage times")?);
    let close_timeout_ms = if matches!(action, Control::Close) {
        // The backend keeps its original five-second retirement deadline. Its
        // failed IPC promise is delivered afterwards; an observation timer
        // started before native admission must not race that same deadline.
        let state = io::close_result_with_observation_budget(app, 1, Duration::from_secs(8))?;
        let close_elapsed = control_at.elapsed();
        pending.assert_waiting()?;
        if state["completed"] != 0
            || state["failed"] != 1
            || close_elapsed < Duration::from_secs(5)
            || close_elapsed > Duration::from_secs(6)
            || app.get_webview(TARGET).is_none()
            || io::visible(&live.webview)?
            || world.adapter.is_idle(&grant.run_id)
            || !zero_callbacks(&scope.counts())
            || probe::native_staging(live.instance) != Some((native_path.clone(), native_buffered))
        {
            return Err("pre-start Close discarded the original pending download owner".into());
        }
        if io::create(app, TARGET, &server.page_url(), 400.0).is_ok() {
            return Err("pre-start pending Close admitted same-label replacement".into());
        }
        Some(close_elapsed.as_millis())
    } else {
        None
    };
    let grace_result = if grace {
        let expiry = scope.wait_cancel_expiry()?;
        if expiry < probe::cancellation_grace() {
            return Err("native cancellation expired before its original grace".into());
        }
        let result = pending.terminal(Instant::now())?;
        require_tool_error(&result, "App original cancellation grace")?;
        if tool_error_kind(&result) != "native-terminal-timeout"
            || before.dispatch_acknowledged != 1
            || !zero_callbacks(&scope.counts())
            || !server.transfer.is_before_start_held()
            || world.adapter.is_idle(&grant.run_id)
            || probe::native_staging(live.instance) != Some((native_path.clone(), native_buffered))
            || (native_buffered && !native_path.parent().is_some_and(Path::is_dir))
            || world
                .broker
                .stop_state(&grant.run_id)
                .map_err(|error| error.to_string())?
                != StopState::StopRequested
            || !staged_files(&root)?.is_empty()
        {
            return Err(format!(
                "original grace reply discarded physical occupancy or claimed success; reply={result}; before={}; current={}; stages={}",
                serde_json::to_string(&before).unwrap_or_default(),
                serde_json::to_string(&scope.counts()).unwrap_or_default(),
                serde_json::to_string(&scope.stages()).unwrap_or_default()
            ));
        }
        rejected_call(world.call(&grant, "computer_download", args.clone()))?;
        if server.downloads() != 1 || !zero_callbacks(&scope.counts()) {
            return Err("original grace reply allowed a replay".into());
        }
        Some((control_at.elapsed().as_millis(), result, expiry.as_millis()))
    } else {
        None
    };
    let released_at = Instant::now();
    server.transfer.release_headers();
    if grace {
        let deadline = released_at + Duration::from_secs(5);
        loop {
            let counts = scope.counts();
            if counts.native_terminal == 1
                && counts.wry_processed == 1
                && world.adapter.is_idle(&grant.run_id)
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err("late response did not physically retire after original grace".into());
            }
            io::eval(app, "main", "document.readyState")?;
        }
    } else {
        let terminal = pending.terminal(released_at)?;
        require_tool_error(&terminal, "App download cancelled before native start")?;
        if matches!(
            tool_error_kind(&terminal),
            "unknown" | "native-terminal-timeout"
        ) {
            return Err("pre-start control used unknown/timeout as physical cancellation".into());
        }
    }
    server.transfer.wait_started()?;
    if native_buffered && native_path.parent().is_some_and(Path::exists) {
        return Err("retired pre-start download left its private native buffer directory".into());
    }
    let counts = scope.counts();
    if counts.native_started != 1
        || counts.wry_requested != 1
        || counts.native_terminal != 1
        || counts.wry_finished != 1
        || counts.wry_processed != 1
        || counts.browser_exited != 0
        || !world.adapter.is_idle(&grant.run_id)
        || !server.transfer.is_held()
        || !staged_files(&root)?.is_empty()
    {
        return Err(
            "late response lacked authentic cancellation/retirement or published bytes".into(),
        );
    }
    let terminal_ms = released_at.elapsed().as_millis();
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
        return Err("original pre-start run remained occupied after terminal callbacks".into());
    }
    rejected_call(world.call(&grant, "computer_download", args.clone()))?;
    server.transfer.release();
    server.transfer.wait_finished()?;
    if server.downloads() != 1 || !staged_files(&root)?.is_empty() {
        return Err("late pre-start body revived a cancelled download".into());
    }
    io::start_close(app, TARGET, 1)?;
    let close = io::close_result(app, 1)?;
    if close["failed"] != 0 || close["completed"] != 1 || app.get_webview(TARGET).is_some() {
        return Err("original App close IPC did not retire cancelled download view".into());
    }
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
    if grace && fresh["nativeDestinationUtf16Length"].as_u64().unwrap_or(0) <= 260 {
        return Err("long-path regression was replaced by a short destination".into());
    }
    if grace
        && (fresh["destinationReceipt"]["buffered"] != true
            || fresh["destinationReceipt"]["nativeUtf16Length"]
                .as_u64()
                .unwrap_or(260)
                >= 260)
    {
        return Err("long Broker destination did not use the compatible native transfer".into());
    }
    Ok(serde_json::json!({
        "control":format!("{action:?}"),"requestBeforeHeaders":true,"beforeCallbacks":before,
        "originalGraceMs":probe::cancellation_grace().as_millis(),
        "graceStage":if grace { Some("original-native-cancel-wait") } else { None },
        "graceScenario":grace,"unknownAfterMs":grace_result.as_ref().map(|(elapsed, _, _)| elapsed),
        "nativeGraceElapsedMs":grace_result.as_ref().map(|(_, _, elapsed)| elapsed),
        "graceReply":grace_result.map(|(_, reply, _)| reply),
        "originalCancellationWitnessedBeforeStart":true,"occupancyRetainedBeforeStart":true,
        "keeperResponsive":true,"controlFenceMs":fence_ms,"terminalAfterHeadersMs":terminal_ms,
        "preStartCloseTimeoutMs":close_timeout_ms,"explicitCloseRetry":matches!(action, Control::Close),
        "originalCallbacks":counts,"originalAdapterIdle":true,"lateBodyRevival":false,
        "originalStageTimesMs":scope.stages(),
        "nativeBuffered":native_buffered,"nativeBufferRetainedDuringGrace":grace,
        "nativeBufferRetiredAfterCallbacks":native_buffered,
        "replayDispatched":false,"stagingEmpty":true,"sameLabelFreshOwner":fresh
    }))
}
