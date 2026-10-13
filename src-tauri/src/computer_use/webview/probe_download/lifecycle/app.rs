//! Real App main IPC / child Wry download callbacks; no Host-owned bridge.

use super::*;
use crate::computer_use::webview::download::probe::{self, Probe};
use crate::computer_use::webview::SideBrowserLiveView;
use tauri::Manager;

pub(crate) mod before_start;
pub(crate) mod buffer_crash;
pub(crate) mod buffer_retirement;
pub(crate) mod error_unwind;
mod held;
pub(crate) mod idle_exit;
pub(in crate::computer_use::webview) mod io;
pub(crate) mod publication;

const KEEPER: &str = "resource-browser-download-keeper";
const TARGET: &str = "resource-browser-download-target";

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
    // Always attempt the real off-UI close before the normal App exit barrier.
    let target_close = crate::side_browser_host::close(&app, TARGET.into());
    let keeper_close = crate::side_browser_host::close(&app, KEEPER.into());
    serde_json::json!({
        "ok": result.is_ok() && target_close.is_ok() && keeper_close.is_ok(),
        "mode":"webview-download-close", "nativeProductSideBrowser":true,
        "realMainIpc":result.is_ok(), "realWryDownloadCallbacks":result.is_ok(), "taskGrantPath":true,
        "hostApprovalSynthetic":true, "realAcp":false, "realMcp":false, "realModel":false,
        "loginDataImported":false, "result": result.as_ref().ok(), "error":result.err(),
        "targetCloseError": target_close.err(), "keeperCloseError":keeper_close.err(),
    })
}

fn check(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let mut scenarios = Vec::new();
    for timeout in [false, true] {
        let mut server = Server::spawn()?;
        let result = scenario(app, &server, timeout);
        let cleanup = server.stop_and_join();
        scenarios.push(result?);
        cleanup?;
    }
    Ok(serde_json::json!({"scenarios":scenarios}))
}

fn scenario(
    app: &tauri::AppHandle,
    server: &Server,
    timeout: bool,
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
    let scope = Probe::watch(live.instance, true)?;
    let result = check_download(app, &live, &scope, pid, server, timeout, None);
    // Close fences routing before physically closing the controller. Reopening
    // by label would reject that retained original and skip callback cleanup.
    let counts = scope.counts();
    let released = if result.is_err() && counts.wry_finished > counts.wry_processed {
        let release_scope = scope.clone();
        io::native(&live.webview, move |_| {
            release_scope.release_on_native_thread()
        })
    } else {
        Ok(())
    };
    let closed = crate::side_browser_host::close(app, TARGET.into());
    // The successful path already closed this exact view. Dispatching to its
    // retired native controller is unnecessary; its callback was witnessed.
    match result {
        Ok(value) => {
            closed?;
            Ok(value)
        }
        Err(error) => Err(format!(
            "{error}; original_callbacks={}; release_error={:?}; close_error={:?}",
            serde_json::to_string(&scope.counts()).map_err(|_| "native counters invalid")?,
            released.err(),
            closed.err()
        )),
    }
}

fn observe_app(
    world: &DownloadWorld,
    grant: &DownloadGrant,
    name: &str,
) -> Result<ObservationCursor, String> {
    let (target, _) = world
        .broker
        .authorized_target(&grant.run_id)
        .map_err(|error| error.to_string())?;
    let result = world.call(
        grant,
        "computer_observe",
        serde_json::json!({"runId":grant.run_id,"targetId":target,"screenshot":false}),
    )?;
    cursor_from_observation(&text_payload(&result, "App download observation")?, name)
}

fn check_download(
    app: &tauri::AppHandle,
    live: &Arc<SideBrowserLiveView>,
    scope: &Arc<Probe>,
    pid: u32,
    server: &Server,
    timeout: bool,
    fault: Option<&mut error_unwind::Fault>,
) -> Result<serde_json::Value, String> {
    let root = crate::paths::app_data_root().join(if timeout {
        "download-timeout"
    } else {
        "download-concurrent"
    });
    std::fs::create_dir(&root).map_err(|error| error.to_string())?;
    let world = Arc::new(DownloadWorld::new(live.clone(), root.clone())?);
    let grant = world.grant()?;
    let cursor = observe_app(&world, &grant, "下载慢速文件")?;
    let args = download_args(&grant, &cursor, "app-held", "held.bin");
    let mut pending =
        PendingDownload::spawn(world.clone(), &grant, args.clone(), server.transfer.clone());
    // Declared after pending so failure cleanup releases its real callback
    // before PendingDownload drops and joins the original caller.
    let mut held = held::Completion::new(live, scope, world.clone(), &grant.run_id);
    server.transfer.wait_started()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let received = loop {
        pending.assert_waiting()?;
        let instance = live.instance;
        let received = io::native(&live.webview, move |_| {
            probe::received_bytes_on_native_thread(instance)
        })?;
        let counts = scope.counts();
        if counts.native_started == 1 && counts.wry_requested == 1 && received > 0 {
            break received;
        }
        if Instant::now() >= deadline {
            return Err("App slow download lacked real native bytes/handlers".into());
        }
    };
    if world.adapter.is_idle(&grant.run_id) || !server.transfer.is_held() || server.downloads() != 1
    {
        return Err("App slow download did not retain physical occupancy".into());
    }
    let started = Instant::now();
    io::start_close(app, TARGET, if timeout { 1 } else { 2 })?;
    let deadline = started + Duration::from_secs(5);
    while scope.counts().wry_finished != 1 || scope.counts().native_terminal != 1 {
        pending.assert_waiting()?;
        io::heartbeat(app)?;
        if Instant::now() >= deadline {
            return Err("App cancellation lacked original native/Wry terminal callbacks".into());
        }
    }
    let heartbeat = io::heartbeat(app)?;
    if heartbeat["completed"] != 0
        || heartbeat["failed"] != 0
        || heartbeat["heartbeat"].as_u64().unwrap_or(0) == 0
    {
        return Err("App close acknowledged before the held original Wry callback".into());
    }
    if io::eval(app, KEEPER, "document.readyState")? != "complete" {
        return Err("App keeper blocked during asynchronous close".into());
    }
    pending.assert_waiting()?;
    let original_hidden = !io::visible(&live.webview)?;
    if app.get_webview(TARGET).is_none()
        || scope.counts().wry_processed != 0
        || world.adapter.is_idle(&grant.run_id)
        || !original_hidden
    {
        return Err("App discarded the physical owner before Wry delivery".into());
    }
    if let Some(fault) = fault {
        // This isolated probe fails while the real completion is still held.
        // Only the original RAII guard may release it before caller Drop/join.
        return fault.inject(&mut pending, args, received, scope.counts());
    }
    let timeout_ms = if timeout {
        // Keep the authentic callback until the unmodified five-second close
        // deadline expires. No timeout override, synthetic terminal or sleep.
        // Observe the IPC reply after the unchanged five-second native cutoff.
        let state = io::close_result_with_observation_budget(app, 1, Duration::from_secs(8))?;
        if state["completed"] != 0 || state["failed"] != 1 || app.get_webview(TARGET).is_none() {
            return Err("failed App close discarded its original native owner".into());
        }
        if io::create(app, TARGET, &server.page_url(), 400.0).is_ok() {
            return Err("App reused a native view whose close was still pending".into());
        }
        Some(started.elapsed().as_millis())
    } else {
        None
    };
    let released_at = Instant::now();
    held.release()?;
    if timeout {
        io::start_close(app, TARGET, 1)?;
    }
    let state = io::close_result(app, if timeout { 1 } else { 2 })?;
    if state["failed"] != 0 || state["completed"] != if timeout { 1 } else { 2 } {
        return Err("App concurrent/explicit retry close did not succeed".into());
    }
    let terminal = pending.terminal(released_at)?;
    require_tool_error(&terminal, "App close interrupted download")?;
    if matches!(
        tool_error_kind(&terminal),
        "unknown" | "native-terminal-timeout"
    ) {
        return Err("App close used an unknown/timeout as physical cancellation".into());
    }
    let counts = scope.counts();
    let instance = live.instance;
    let keeper = app
        .get_webview(KEEPER)
        .ok_or("App keeper missing after close")?;
    let resources_retired = io::native(&keeper, move |_| {
        Ok(crate::computer_use::webview::probe_download_resources_retired(instance))
    })?;
    if counts.native_started != 1
        || counts.wry_requested != 1
        || counts.native_terminal != 1
        || counts.wry_finished != 1
        || counts.wry_processed != 1
        || counts.browser_exited != 0
        || !world.adapter.is_idle(&grant.run_id)
        || !server.transfer.is_held()
        || !staged_files(&root)?.is_empty()
        || app.get_webview(TARGET).is_some()
        || !resources_retired
    {
        return Err(
            "App close lacked exact physical retirement, cleanup or original callbacks".into(),
        );
    }
    let close_ms = released_at.elapsed().as_millis();
    rejected_call(world.call(&grant, "computer_download", args.clone()))?;
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
        return Err("App stopped run remained occupied".into());
    }
    server.transfer.release();
    server.transfer.wait_finished()?;
    if server.downloads() != 1 || !staged_files(&root)?.is_empty() {
        return Err("old App download replay/body revived effects".into());
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
    Ok(
        serde_json::json!({"timeoutScenario":timeout,"sameBrowser":true,"receivedBytes":received,
        "ipcCallCount":if timeout {1} else {2},"uiResponsiveWhileClosePending":true,
        "keeperResponsiveWhileClosePending":true,"originalRetainedViewHidden":original_hidden,"timeoutMs":timeout_ms,"closeAfterReleaseMs":close_ms,
        "originalCallbacks":counts,"originalAdapterIdle":true,"originalNativeResourcesRetired":resources_retired,"sameLabelFreshOwner":fresh,
        "lateBodyRevival":false,"replayDispatched":false,"stagingEmpty":true}),
    )
}

#[allow(clippy::too_many_arguments)]
fn fresh_owner(
    app: &tauri::AppHandle,
    pid: u32,
    server: &Server,
    root: &Path,
    old: &DownloadWorld,
    old_grant: &DownloadGrant,
    old_args: &serde_json::Value,
    old_instance: u64,
) -> Result<serde_json::Value, String> {
    io::create(app, TARGET, &server.page_url(), 400.0)?;
    io::ready(app, TARGET, false)?;
    let live = Arc::new(SideBrowserLiveView::open(app.clone(), TARGET.into())?);
    if live.instance == old_instance || io::pid(&live.webview)? != pid {
        return Err(
            "App same-label replacement did not preserve a fresh owner in the shared browser"
                .into(),
        );
    }
    let scope = Probe::watch(live.instance, false)?;
    let world = DownloadWorld::new(live.clone(), root.to_path_buf())?;
    let grant = world.grant()?;
    rejected_call(old.call(old_grant, "computer_download", old_args.clone()))?;
    old.broker
        .request_stop(&old_grant.run_id)
        .map_err(|error| error.to_string())?;
    let cursor = observe_app(&world, &grant, "下载测试文件")?;
    let result = world.call(
        &grant,
        "computer_download",
        download_args(&grant, &cursor, "app-fresh", "fresh.bin"),
    )?;
    if result.get("isError").and_then(serde_json::Value::as_bool) == Some(true) {
        return Err(format!(
            "App fresh download failed: instance={} reply={result} callbacks={} stages={}",
            live.instance,
            serde_json::to_string(&scope.counts()).unwrap_or_default(),
            serde_json::to_string(&scope.stages()).unwrap_or_default()
        ));
    }
    let (path, bytes) = completed_path(&result)?;
    let mut published = PublishedFiles::new(root.to_path_buf());
    published.retain(path.clone());
    verify_file(&path, bytes)?;
    // Download destinations are canonical paths (extended-length on Windows).
    // Compare resolved filesystem identities, not differently prefixed strings.
    let canonical_root = std::fs::canonicalize(root.join("computer-use-staging"))
        .map_err(|error| error.to_string())?;
    let canonical_path = std::fs::canonicalize(&path).map_err(|error| error.to_string())?;
    let native_path_length = canonical_path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .count();
    if canonical_path == canonical_root
        || !canonical_path.starts_with(&canonical_root)
        || server.downloads() != 2
    {
        return Err(format!(
            "App fresh download escaped staging or repeated network effects: path={canonical_path:?}, root={canonical_root:?}, downloads={}",
            server.downloads()
        ));
    }
    let counts = scope.counts();
    if counts.native_started != 1
        || counts.wry_requested != 1
        || counts.native_terminal != 1
        || counts.wry_finished != 1
        || counts.wry_processed != 1
        || counts.browser_exited != 0
    {
        return Err("App fresh download did not join the original native/Wry callbacks".into());
    }
    crate::side_browser_host::close(app, TARGET.into())?;
    if io::eval(app, KEEPER, "document.readyState")? != "complete" {
        return Err("App fresh owner close damaged its keeper".into());
    }
    drop(published);
    if !staged_files(root)?.is_empty() {
        return Err("App published fixture cleanup left files".into());
    }
    Ok(
        serde_json::json!({"differentNativeInstance":true,"sameBrowser":true,"bytes":bytes,"hashVerified":true,
        "nativeDestinationUtf16Length":native_path_length,"destinationReceipt":scope.destination(),"originalCallbacks":counts}),
    )
}
