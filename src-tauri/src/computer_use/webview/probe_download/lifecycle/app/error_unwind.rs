//! Deliberate error while the original cancelled Wry completion is still held.
//! A successful verification must still leave the actual App report/exit failed.

use super::*;

pub(super) const INJECTED: &str = "isolated fixture error while original Wry completion is held";

#[derive(Default)]
pub(super) struct Fault {
    witness: Option<Witness>,
}

struct Witness {
    world: Arc<DownloadWorld>,
    grant: DownloadGrant,
    args: serde_json::Value,
    original_result: Receiver<Result<serde_json::Value, String>>,
    held_counts: probe::Counts,
    received: u64,
    started: Instant,
}

impl Fault {
    pub(super) fn inject(
        &mut self,
        pending: &mut PendingDownload,
        args: serde_json::Value,
        received: u64,
        held_counts: probe::Counts,
    ) -> Result<serde_json::Value, String> {
        // Retain the original worker's receiver, not a synthetic tool result.
        // PendingDownload still owns and joins the original worker in Drop.
        let (_, unused_receiver) = mpsc::channel();
        self.witness = Some(Witness {
            world: pending.world.clone(),
            grant: pending.grant.clone(),
            args,
            original_result: std::mem::replace(&mut pending.result, unused_receiver),
            held_counts,
            received,
            started: Instant::now(),
        });
        Err(INJECTED.into())
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
        Err("App download error gate requires independent isolated settings".into())
    };
    let target_close = crate::side_browser_host::close(&app, TARGET.into());
    let keeper_close = crate::side_browser_host::close(&app, KEEPER.into());
    let verified = result.is_ok() && target_close.is_ok() && keeper_close.is_ok();
    let error = match &result {
        Ok(_) => INJECTED.to_string(),
        Err(error) => error.clone(),
    };
    serde_json::json!({
        "ok":false,"mode":"webview-download-error-unwind","error":error,
        "errorUnwindVerified":verified,"result":result.ok(),
        "nativeProductSideBrowser":true,"realMainIpc":verified,
        "realWryDownloadCallbacks":verified,"taskGrantPath":true,
        "hostApprovalSynthetic":true,"realAcp":false,"realMcp":false,"realModel":false,
        "loginDataImported":false,"targetCloseError":target_close.err(),
        "keeperCloseError":keeper_close.err(),
    })
}

fn check(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let mut server = Server::spawn()?;
    let result = scenario(app, &server);
    let cleanup = server.stop_and_join();
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(format!("{error}; fixture cleanup: {cleanup}")),
    }
}

fn scenario(app: &tauri::AppHandle, server: &Server) -> Result<serde_json::Value, String> {
    io::create(app, KEEPER, &server.page_url(), 20.0)?;
    io::ready(app, KEEPER, false)?;
    io::create(app, TARGET, &server.page_url(), 400.0)?;
    io::ready(app, TARGET, false)?;
    let live = Arc::new(SideBrowserLiveView::open(app.clone(), TARGET.into())?);
    let pid = io::pid(&live.webview)?;
    let keeper = app
        .get_webview(KEEPER)
        .ok_or("error unwind keeper missing")?;
    if pid != io::pid(&keeper)? {
        return Err("error unwind views did not share the original browser".into());
    }
    let scope = Probe::watch(live.instance, true)?;
    let mut fault = Fault::default();
    let original_error = check_download(app, &live, &scope, pid, server, false, Some(&mut fault));
    if original_error.as_ref().err().map(String::as_str) != Some(INJECTED) {
        return Err(format!(
            "held-callback fault was not reached: {original_error:?}"
        ));
    }
    let witness = fault
        .witness
        .ok_or("original error unwind witness missing")?;
    if witness.held_counts.native_terminal != 1
        || witness.held_counts.wry_finished != 1
        || witness.held_counts.wry_processed != 0
        || witness.received == 0
    {
        return Err("injected error lacked a real held original completion".into());
    }
    // check_download returned only after Completion then PendingDownload Drop.
    // The retained original receiver must already contain its authentic result.
    let terminal = witness.original_result.try_recv().map_err(|error| {
        format!("original caller did not retire during error unwind: {error}")
    })??;
    require_tool_error(&terminal, "error unwind original cancelled download")?;
    if matches!(
        tool_error_kind(&terminal),
        "unknown" | "native-terminal-timeout"
    ) {
        return Err("error unwind used unknown/timeout as native retirement".into());
    }
    let state = io::close_result(app, 2)?;
    let counts = scope.counts();
    let instance = live.instance;
    let resources_retired = io::native(&keeper, move |_| {
        Ok(crate::computer_use::webview::probe_download_resources_retired(instance))
    })?;
    let root = &witness.world.staging_root;
    let stop_state = witness
        .world
        .broker
        .wait_stopped(&witness.grant.run_id, Duration::from_secs(5))
        .map_err(|error| error.to_string())?;
    if state["completed"] != 2
        || state["failed"] != 0
        || counts.native_started != 1
        || counts.wry_requested != 1
        || counts.native_terminal != 1
        || counts.wry_finished != 1
        || counts.wry_processed != 1
        || counts.browser_exited != 0
        || !witness.world.adapter.is_idle(&witness.grant.run_id)
        || stop_state != StopState::Stopped
        || !resources_retired
        || app.get_webview(TARGET).is_some()
        || !staged_files(root)?.is_empty()
    {
        return Err("original error unwind lacked exact callback/owner/staging retirement".into());
    }
    server.transfer.wait_finished()?;
    rejected_call(
        witness
            .world
            .call(&witness.grant, "computer_download", witness.args.clone()),
    )?;
    if server.downloads() != 1 || io::eval(app, KEEPER, "document.readyState")? != "complete" {
        return Err("error unwind replay or late body damaged the original keeper".into());
    }
    let error_to_retired_ms = witness.started.elapsed().as_millis();
    let fresh = fresh_owner(
        app,
        pid,
        server,
        root,
        &witness.world,
        &witness.grant,
        &witness.args,
        live.instance,
    )?;
    Ok(serde_json::json!({
        "injectedWhileOriginalCallbackHeld":true,"receivedBytes":witness.received,
        "heldOriginalCallbacks":witness.held_counts,"originalCallbacks":counts,
        "originalCallerJoined":true,"originalCallerOutcome":tool_error_kind(&terminal),
        "originalAdapterIdle":true,"originalNativeResourcesRetired":resources_retired,
        "errorToRetiredMs":error_to_retired_ms,"ipcCallCount":2,"ipcResult":state,
        "sameLabelFreshOwner":fresh,"stagingEmpty":true,"replayDispatched":false,
        "keeperResponsive":true,"lateBodyRevival":false,
    }))
}
