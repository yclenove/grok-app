//! Original staged file commit/Drop after task, binding or native retirement.

use super::*;
use crate::computer_use::webview::download::publication_probe::PublicationProbe;

#[derive(Clone, Copy, Debug)]
enum Boundary {
    Publish,
    Pause,
    Stop,
    Navigate,
    Unbind,
    Rebind,
    Close,
}

struct HeldPublication {
    gate: PublicationProbe,
    pending: PendingDownload,
}

impl HeldPublication {
    fn spawn(
        world: Arc<DownloadWorld>,
        grant: &DownloadGrant,
        live: &SideBrowserLiveView,
        args: serde_json::Value,
        transfer: Arc<Transfer>,
    ) -> Result<Self, String> {
        let gate =
            PublicationProbe::arm(&grant.run_id, format!("{}|{}", live.label(), live.tab_id()))?;
        let pending = PendingDownload::spawn(world, grant, args, transfer);
        Ok(Self { gate, pending })
    }
}

impl Drop for HeldPublication {
    fn drop(&mut self) {
        // Error cleanup must fence, then release, before PendingDownload joins.
        // Neither field order nor a control's successful path provides that.
        let _ = self
            .pending
            .world
            .broker
            .request_stop(&self.pending.grant.run_id);
        self.gate.release();
    }
}

pub(crate) fn run(app: tauri::AppHandle) -> serde_json::Value {
    let settings = crate::store::load_settings();
    let isolated = crate::paths::isolated_app_instance_id().is_some()
        && settings.session_data_mode == "independent"
        && !settings.close_to_tray
        && !settings.keep_tray_for_schedules;
    let result = if isolated {
        io::ready(&app, "main", true).and_then(|()| check(&app))
    } else {
        Err("App publication gate requires independent isolated settings".into())
    };
    let target_close = crate::side_browser_host::close(&app, TARGET.into());
    let keeper_close = crate::side_browser_host::close(&app, KEEPER.into());
    serde_json::json!({
        "ok":result.is_ok() && target_close.is_ok() && keeper_close.is_ok(),
        "mode":"webview-download-publication","nativeProductSideBrowser":true,
        "realMainIpc":result.is_ok(),"realWryDownloadCallbacks":result.is_ok(),
        "taskGrantPath":true,"controlDispatch":"original-broker-or-native-view",
        "hostApprovalSynthetic":true,"realAcp":false,"realMcp":false,"realModel":false,
        "loginDataImported":false,"result":result.as_ref().ok(),"error":result.err(),
        "targetCloseError":target_close.err(),"keeperCloseError":keeper_close.err()
    })
}

fn check(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let mut scenarios = Vec::new();
    for boundary in [
        Boundary::Publish,
        Boundary::Pause,
        Boundary::Stop,
        Boundary::Navigate,
        Boundary::Unbind,
        Boundary::Rebind,
        Boundary::Close,
    ] {
        let mut server = Server::spawn()?;
        let result = scenario(app, &server, boundary);
        let cleanup = server.stop_and_join();
        scenarios.push(result?);
        cleanup?;
    }
    Ok(serde_json::json!({"scenarios":scenarios}))
}

fn scenario(
    app: &tauri::AppHandle,
    server: &Server,
    boundary: Boundary,
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
        return Err("publication views do not share the original browser".into());
    }
    let scope = Probe::watch(live.instance, false)?;
    let result = check_download(app, &live, &scope, pid, server, boundary);
    let closed = crate::side_browser_host::close(app, TARGET.into());
    match result {
        Ok(value) => {
            closed?;
            Ok(value)
        }
        Err(error) => Err(format!(
            "{boundary:?}: {error}; callbacks={}; close_error={:?}",
            serde_json::to_string(&scope.counts()).unwrap_or_default(),
            closed.err()
        )),
    }
}

fn apply_boundary(
    app: &tauri::AppHandle,
    live: &Arc<SideBrowserLiveView>,
    world: &DownloadWorld,
    grant: &DownloadGrant,
    server: &Server,
    boundary: Boundary,
) -> Result<(), String> {
    match boundary {
        Boundary::Publish => {}
        Boundary::Pause => world
            .broker
            .pause(&grant.run_id)
            .map_err(|error| error.to_string())?,
        Boundary::Stop => {
            world
                .broker
                .request_stop(&grant.run_id)
                .map_err(|error| error.to_string())?;
        }
        Boundary::Navigate => {
            let generation = live.navigation_generation();
            // Let the original Wry NavigationStarting callback retire authority.
            // Do not synthesize a lifecycle generation in this fixture.
            live.webview
                .navigate(tauri::Url::parse(&server.page_url()).map_err(|e| e.to_string())?)
                .map_err(|error| error.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(5);
            while live.navigation_generation() == generation {
                if Instant::now() >= deadline {
                    return Err("original navigation did not retire document authority".into());
                }
                io::eval(app, "main", "document.readyState")?;
            }
            io::ready(app, TARGET, false)?;
        }
        Boundary::Unbind => {
            if !world.adapter.unbind_for_run(&world.owner, &grant.run_id) {
                return Err("original run binding was not removed".into());
            }
        }
        Boundary::Rebind => {
            let selection = WebViewSelection::capture(live.clone())?;
            world
                .adapter
                .bind_selected(&selection, &world.owner, &grant.run_id)?;
        }
        Boundary::Close => {
            io::start_close(app, TARGET, 1)?;
            let state = io::close_result(app, 1)?;
            if state["failed"] != 0 || state["completed"] != 1 || live.is_alive() {
                return Err("original main close IPC did not retire the native view".into());
            }
        }
    }
    Ok(())
}

fn check_download(
    app: &tauri::AppHandle,
    live: &Arc<SideBrowserLiveView>,
    scope: &Arc<Probe>,
    pid: u32,
    server: &Server,
    boundary: Boundary,
) -> Result<serde_json::Value, String> {
    let root = crate::paths::app_data_root().join(format!("download-publication-{boundary:?}"));
    std::fs::create_dir(&root).map_err(|error| error.to_string())?;
    let world = Arc::new(DownloadWorld::new(live.clone(), root.clone())?);
    let grant = world.grant()?;
    let cursor = observe_app(&world, &grant, "下载测试文件")?;
    let args = download_args(&grant, &cursor, "app-publication", "held.bin");
    let mut held = HeldPublication::spawn(
        world.clone(),
        &grant,
        live,
        args.clone(),
        server.transfer.clone(),
    )?;
    let staged = held.gate.wait_staged()?;
    held.pending.assert_waiting()?;
    verify_file(&staged.path, DOWNLOAD_BYTES.len() as u64)?;
    let counts = scope.counts();
    if counts.native_started != 1
        || counts.wry_requested != 1
        || counts.native_terminal != 1
        || counts.wry_finished != 1
        || counts.wry_processed != 1
        || server.downloads() != 1
        || staged_files(&root)?.len() != 1
        || held.gate.arrivals() != 1
        || !world.adapter.is_idle(&grant.run_id)
    {
        return Err("publication hold lacked authentic completed file/callbacks".into());
    }
    let controlled_at = Instant::now();
    apply_boundary(app, live, &world, &grant, server, boundary)?;
    if controlled_at.elapsed() > Duration::from_secs(5)
        || io::eval(app, KEEPER, "document.readyState")? != "complete"
    {
        return Err("publication control blocked Broker or original keeper".into());
    }
    held.pending.assert_waiting()?;
    let control_ms = controlled_at.elapsed().as_millis();
    let released_at = Instant::now();
    held.gate.release();
    let terminal = held.pending.terminal(released_at)?;
    let published = matches!(boundary, Boundary::Publish);
    if published {
        let (path, bytes) = completed_path(&terminal)?;
        if path != staged.path {
            return Err("publication returned a different staged file".into());
        }
        verify_file(&path, bytes)?;
        let mut files = PublishedFiles::new(root.clone());
        files.retain(path);
        let replay = world.call(&grant, "computer_download", args.clone())?;
        if replay != terminal {
            return Err("successful publication replay changed its receipt".into());
        }
        drop(files);
    } else {
        require_tool_error(&terminal, "retired staged publication")?;
        let rejected_at_commit = terminal["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|part| part["text"].as_str())
            .any(|text| text.contains("staging publication failed"));
        if !rejected_at_commit {
            return Err(format!(
                "staged download did not reach original final commit: {terminal}"
            ));
        }
        rejected_call(world.call(&grant, "computer_download", args.clone()))?;
    }
    if staged.path.exists()
        || !staged_files(&root)?.is_empty()
        || server.downloads() != 1
        || held.gate.arrivals() != 1
        || !world.adapter.is_idle(&grant.run_id)
    {
        return Err("old staged publication leaked, replayed or retained native occupancy".into());
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
        return Err("publication run did not finish original Stop cleanup".into());
    }
    crate::side_browser_host::close(app, TARGET.into())?;
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
        "boundary":format!("{boundary:?}"),"authenticFileCompletedBeforeControl":true,
        "bytes":DOWNLOAD_BYTES.len(),"hashVerifiedBeforeControl":true,"originalCallbacks":counts,
        "nativeIdleBeforePublication":true,"publicationStillPendingBeforeRelease":true,
        "controlMs":control_ms,"published":published,"finalCommitRejected":!published,
        "stagingEmpty":true,"replayDispatched":false,"keeperResponsive":true,
        "sameLabelFreshOwner":fresh
    }))
}
