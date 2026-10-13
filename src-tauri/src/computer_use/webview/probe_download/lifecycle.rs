//! Physical download cancellation, followed by a fresh owner on the same native label.

use super::*;
use grok_computer_use_core::broker::StopState;
use std::sync::mpsc::{self, Receiver, TryRecvError};

pub(crate) mod app;
mod server;
pub(crate) mod shared;
use server::{Server, Transfer};

#[derive(Clone, Copy, Debug)]
enum Control {
    Pause,
    Stop,
    Navigate,
    Close,
}

struct PendingDownload {
    world: Arc<DownloadWorld>,
    grant: DownloadGrant,
    transfer: Arc<Transfer>,
    result: Receiver<Result<serde_json::Value, String>>,
    join: Option<JoinHandle<()>>,
}

impl PendingDownload {
    fn spawn(
        world: Arc<DownloadWorld>,
        grant: &DownloadGrant,
        args: serde_json::Value,
        transfer: Arc<Transfer>,
    ) -> Self {
        let (tx, result) = mpsc::channel();
        let worker_world = world.clone();
        let worker_grant = grant.clone();
        let join = thread::spawn(move || {
            let _ = tx.send(worker_world.call(&worker_grant, "computer_download", args));
        });
        Self {
            world,
            grant: grant.clone(),
            transfer,
            result,
            join: Some(join),
        }
    }

    fn assert_waiting(&self) -> Result<(), String> {
        match self.result.try_recv() {
            Err(TryRecvError::Empty) => Ok(()),
            outcome => Err(format!(
                "slow download caller returned before the control: {outcome:?}"
            )),
        }
    }

    fn terminal(&mut self, started: Instant) -> Result<serde_json::Value, String> {
        let result = self.result.recv_timeout(Duration::from_secs(5).saturating_sub(started.elapsed()))
            .map_err(|error| format!("download control missed terminal deadline: {error}; elapsed_ms={} native_busy={}",
                started.elapsed().as_millis(), !self.world.adapter.is_idle(&self.grant.run_id)))??;
        if started.elapsed() > Duration::from_secs(5) {
            return Err("download control returned after its five-second deadline".into());
        }
        if let Some(join) = self.join.take() {
            join.join().map_err(|_| "slow download caller panicked")?;
        }
        Ok(result)
    }
}

impl Drop for PendingDownload {
    fn drop(&mut self) {
        // Fence the original authority before releasing the fixture or joining its caller.
        let _ = self.world.broker.request_stop(&self.grant.run_id);
        self.transfer.release();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn rejected_call(result: Result<serde_json::Value, String>) -> Result<(), String> {
    match result {
        Err(_) => Ok(()),
        Ok(result) => require_tool_error(&result, "retired download replay"),
    }
}

fn wait_in_flight(
    live: &HostOwnedWebView,
    pending: &PendingDownload,
    server: &Server,
) -> Result<(), String> {
    server.transfer.wait_started()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while live.download_counts() != (1, 1, 0) {
        pending.assert_waiting()?;
        if Instant::now() >= deadline {
            return Err(format!(
                "slow attachment never reached the original native handlers: {:?}",
                live.download_counts()
            ));
        }
        // An actual round trip through the original STA witnesses progress; no timing sleep.
        live.host_script("document.readyState")?;
    }
    pending.assert_waiting()?;
    if !server.transfer.is_held()
        || server.downloads() != 1
        || pending.world.adapter.is_idle(&pending.grant.run_id)
    {
        return Err("slow attachment did not retain real native occupancy and held bytes".into());
    }
    Ok(())
}

fn control(
    world: &DownloadWorld,
    grant: &DownloadGrant,
    live: &HostOwnedWebView,
    server: &Server,
    action: Control,
) -> Result<(), String> {
    match action {
        Control::Pause => world
            .broker
            .pause(&grant.run_id)
            .map_err(|error| error.to_string()),
        Control::Stop => {
            let ticket = world
                .broker
                .fence_stop(&grant.run_id)
                .map_err(|error| error.to_string())?
                .ok_or("in-flight download Stop unexpectedly had no cleanup ticket")?;
            if world
                .broker
                .stop_state(&grant.run_id)
                .map_err(|error| error.to_string())?
                != StopState::StopRequested
            {
                return Err("logical Stop acknowledged physical retirement before cleanup".into());
            }
            world
                .broker
                .finish_stop_cleanup(&ticket)
                .map_err(|error| error.to_string())?;
            Ok(())
        }
        Control::Navigate => live.navigate(&server.page_url()).map(|_| ()),
        Control::Close => live.shutdown(),
    }
}

fn check(
    live: &Arc<HostOwnedWebView>,
    server: &Server,
    root: &Path,
    action: Control,
) -> Result<(), String> {
    live.fixture_ready()?;
    let world = Arc::new(DownloadWorld::new(live.clone(), root.to_path_buf())?);
    let grant = world.grant()?;
    let (target_id, _) = world
        .broker
        .authorized_target(&grant.run_id)
        .map_err(|error| error.to_string())?;
    let cursor = observe(
        "slow-first-bytes",
        live,
        &world,
        &grant,
        &target_id,
        "下载慢速文件",
    )?;
    let args = download_args(&grant, &cursor, "held-download", "held.bin");
    let mut pending =
        PendingDownload::spawn(world.clone(), &grant, args.clone(), server.transfer.clone());
    wait_in_flight(live, &pending, server)?;

    let started = Instant::now();
    control(&world, &grant, live, server, action)?;
    let terminal = pending.terminal(started)?;
    require_tool_error(&terminal, "cancelled slow WebView download")?;
    // Read idle first: a terminal callback arriving between observations cannot
    // make an outstanding operation look as if it retired before that callback.
    let idle = world.adapter.is_idle(&grant.run_id);
    let native = live.download_counts();
    let closed_without_terminal = matches!(action, Control::Close)
        && native == (1, 1, 0)
        && tool_error_kind(&terminal) == "unknown";
    if !server.transfer.is_held()
        || !idle
        || (native != (1, 1, 1) && !closed_without_terminal)
        || !staged_files(root)?.is_empty()
    {
        return Err(format!("{action:?} did not retire before held body release: idle={idle} native={native:?} staged={}", staged_files(root)?.len()));
    }
    if tool_error_kind(&terminal) == "native-terminal-timeout" {
        return Err(format!(
            "{action:?} returned unknown timeout instead of native cancellation"
        ));
    }
    let elapsed = started.elapsed().as_millis();
    rejected_call(world.call(&grant, "computer_download", args.clone()))?;
    if server.downloads() != 1 || live.download_counts() != native {
        return Err("retired slow download replay reached another native dispatch".into());
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
        return Err("slow download remained pending after native retirement".into());
    }
    server.transfer.release();
    server.transfer.wait_finished()?;
    if !staged_files(root)?.is_empty() || live.download_counts() != native {
        return Err("held body release revived the cancelled download".into());
    }

    let replacement = if matches!(action, Control::Close) {
        HostOwnedWebView::spawn_disposable(
            "resource-browser-dl-life",
            "new",
            &server.page_url(),
            &root.join("new-profile"),
        )?
    } else {
        live.clone()
    };
    let result = check_fresh_owner(&replacement, server, root, &world, &grant, &args);
    let result = if matches!(action, Control::Close) {
        replacement.finish_probe(result)
    } else {
        result
    };
    result?;
    println!("gate: webview_download_lifecycle control={action:?} terminal_ms={elapsed} held_native=1 native_terminal={} owner_exit={} cleanup=1 replay=0 late_body=0 fresh_owner=1", native.2, matches!(action, Control::Close));
    Ok(())
}

fn check_fresh_owner(
    live: &Arc<HostOwnedWebView>,
    server: &Server,
    root: &Path,
    old: &DownloadWorld,
    old_grant: &DownloadGrant,
    old_args: &serde_json::Value,
) -> Result<(), String> {
    live.fixture_ready()?;
    let next_root = root.join("next-owner");
    std::fs::create_dir(&next_root).map_err(|error| error.to_string())?;
    let _root = ProbeRoot(next_root.clone());
    let next = DownloadWorld::new(live.clone(), next_root)?;
    let grant = next.grant()?;
    if next.owner == old.owner || grant.run_id == old_grant.run_id {
        return Err("download replacement reused the original authority".into());
    }
    let (target_id, _) = next
        .broker
        .authorized_target(&grant.run_id)
        .map_err(|error| error.to_string())?;
    old.broker
        .request_stop(&old_grant.run_id)
        .map_err(|error| error.to_string())?;
    rejected_call(old.call(old_grant, "computer_download", old_args.clone()))?;
    let before = live.download_counts();
    let cursor = observe(
        "fresh-owner",
        live,
        &next,
        &grant,
        &target_id,
        "下载测试文件",
    )?;
    let result = next.call(
        &grant,
        "computer_download",
        download_args(&grant, &cursor, "new-download", "new.bin"),
    )?;
    let (path, bytes) = completed_path(&result)?;
    let mut files = PublishedFiles::new(next.staging_root.clone());
    files.retain(path.clone());
    verify_file(&path, bytes)?;
    old.broker
        .request_stop(&old_grant.run_id)
        .map_err(|error| error.to_string())?;
    verify_file(&path, bytes)?;
    if server.downloads() != 2
        || live.download_counts() != (before.0 + 1, before.1 + 1, before.2 + 1)
        || !next.adapter.is_idle(&grant.run_id)
    {
        return Err("late old Stop affected the fresh owner's exactly-once download".into());
    }
    Ok(())
}

pub(crate) fn run() -> Result<(), String> {
    for action in [
        Control::Pause,
        Control::Stop,
        Control::Navigate,
        Control::Close,
    ] {
        let root = std::env::temp_dir().join(format!(
            "grok-cu-download-lifecycle-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&root).map_err(|error| error.to_string())?;
        eprintln!(
            "gate: webview_download_lifecycle_start control={action:?} profile_root={}",
            root.display()
        );
        let _root = ProbeRoot(root.clone());
        let mut server = Server::spawn()?;
        let live = HostOwnedWebView::spawn_disposable(
            "resource-browser-dl-life",
            "old",
            &server.page_url(),
            &root.join("old-profile"),
        )?;
        let result = check(&live, &server, &root, action)
            .map_err(|error| format!("{action:?}: {error}; profile_root={}", root.display()));
        let result = live.finish_probe(result);
        let cleanup = server.stop_and_join();
        match (result, cleanup) {
            (Ok(()), Ok(())) => {}
            (Err(error), Ok(())) | (Ok(()), Err(error)) => return Err(error),
            (Err(error), Err(cleanup)) => {
                return Err(format!("{error}; fixture cleanup: {cleanup}"))
            }
        }
    }
    Ok(())
}
