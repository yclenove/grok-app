//! Closing one real controller must retire downloads while its browser stays alive.

use super::*;
use crate::computer_use::webview_host::SharedBrowser;

fn check(browser: &SharedBrowser, server: &Server, root: &Path) -> Result<(), String> {
    browser.keeper.fixture_ready()?;
    let live = browser.view("old", &server.page_url())?;
    let primary = check_view(browser, &live, server, root);
    live.finish_probe(primary)
}

fn check_view(
    browser: &SharedBrowser,
    live: &Arc<HostOwnedWebView>,
    server: &Server,
    root: &Path,
) -> Result<(), String> {
    live.fixture_ready()?;
    let pid = live.browser_pid();
    if pid == 0 || pid != browser.keeper.browser_pid() {
        return Err("shared controllers did not use the same physical browser process".into());
    }
    let world = Arc::new(DownloadWorld::new(live.clone(), root.to_path_buf())?);
    let grant = world.grant()?;
    let (target, _) = world
        .broker
        .authorized_target(&grant.run_id)
        .map_err(|error| error.to_string())?;
    let cursor = observe(
        "shared-slow-first-bytes",
        live,
        &world,
        &grant,
        &target,
        "下载慢速文件",
    )?;
    let args = download_args(&grant, &cursor, "shared-held", "held.bin");
    let mut pending =
        PendingDownload::spawn(world.clone(), &grant, args.clone(), server.transfer.clone());
    wait_in_flight(live, &pending, server)?;
    eprintln!(
        "gate: webview_download_shared_inflight same_browser=1 pid={pid} native={:?}",
        live.download_counts()
    );
    let started = Instant::now();
    live.shutdown()?;
    let terminal = pending.terminal(started)?;
    require_tool_error(&terminal, "shared controller download cancellation")?;
    if tool_error_kind(&terminal) == "unknown"
        || tool_error_kind(&terminal) == "native-terminal-timeout"
    {
        return Err(
            "shared close used an unknown or timeout result instead of native cancellation".into(),
        );
    }
    let idle = world.adapter.is_idle(&grant.run_id);
    let native = live.download_counts();
    if native != (1, 1, 1)
        || !idle
        || !server.transfer.is_held()
        || live.browser_exited()
        || browser.keeper.browser_exited()
        || !staged_files(root)?.is_empty()
    {
        return Err(format!("shared close lacked physical retirement: idle={idle} native={native:?} view_exit={} keeper_exit={}", live.browser_exited(), browser.keeper.browser_exited()));
    }
    let elapsed = started.elapsed().as_millis();
    if browser.keeper.host_script("document.readyState")? != "\"complete\"" {
        return Err("shared keeper stopped responding after one view closed".into());
    }
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
        return Err("shared close left a stopped run occupied".into());
    }
    server.transfer.release();
    server.transfer.wait_finished()?;
    if !staged_files(root)?.is_empty()
        || live.download_counts() != native
        || server.downloads() != 1
    {
        return Err("shared close replay or late body revived the retired download".into());
    }
    let replacement = browser.view("new", &server.page_url())?;
    let result = if replacement.browser_pid() != pid {
        Err("shared replacement changed browser process instead of preserving the keeper".into())
    } else {
        check_fresh_owner(&replacement, server, root, &world, &grant, &args)
    };
    replacement.finish_probe(result)?;
    if browser.keeper.browser_exited() {
        return Err("closing the replacement terminated the keeper's browser".into());
    }
    println!("gate: webview_download_shared_close terminal_ms={elapsed} same_browser=1 native_terminal=1 owner_exit=0 keeper_alive=1 cleanup=1 replay=0 late_body=0 fresh_owner=1");
    Ok(())
}

pub(crate) fn run() -> Result<(), String> {
    let root =
        std::env::temp_dir().join(format!("grok-cu-download-shared-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).map_err(|error| error.to_string())?;
    let _root = ProbeRoot(root.clone());
    eprintln!(
        "gate: webview_download_shared_start profile_root={}",
        root.display()
    );
    let mut server = Server::spawn()?;
    let browser = SharedBrowser::spawn(&root.join("shared-profile"), &server.page_url())?;
    let result = check(&browser, &server, &root)
        .map_err(|error| format!("{error}; profile_root={}", root.display()));
    let result = browser.keeper.finish_probe(result);
    let cleanup = server.stop_and_join();
    shutdown_result(result, cleanup)
}

fn shutdown_result(primary: Result<(), String>, cleanup: Result<(), String>) -> Result<(), String> {
    match (primary, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(format!("{error}; fixture cleanup: {cleanup}")),
    }
}
