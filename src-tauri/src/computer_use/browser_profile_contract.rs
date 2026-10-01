//! Real owned Chromium open/close with a gate delaying Host publication of
//! the open response. This is not a claim of cancelling a native launch.
use grok_computer_use_core::browser::{
    ExistingTabHost, ManagedBrowserWorker, ManagedDownload, ManagedPage, ManagedProfile,
    WorkerError, WorkerRunPhase, WorkerRunRevision,
};
use grok_computer_use_core::error::BrokerError;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use super::LoopbackPlaywrightWorker;

struct DelayedOpen {
    worker: Arc<LoopbackPlaywrightWorker>,
    ready: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl ManagedBrowserWorker for DelayedOpen {
    fn goto(
        &self,
        _owner: &str,
        _profile: &str,
        _page: &str,
        _generation: u64,
        _action: &str,
        _url: &str,
    ) -> Result<ManagedPage, WorkerError> {
        Err(WorkerError::not_started(
            400,
            "unused",
            "fixture does not navigate",
        ))
    }

    fn download(
        &self,
        _owner: &str,
        _profile: &str,
        _page: &str,
        _generation: u64,
        _snapshot: &str,
        _action: &str,
        _name: &str,
        _element: Option<&str>,
    ) -> Result<ManagedDownload, WorkerError> {
        Err(WorkerError::not_started(
            400,
            "unused",
            "fixture does not download",
        ))
    }

    fn open_profile(&self, owner: &str, profile: &str) -> Result<ManagedProfile, WorkerError> {
        let opened = self.worker.open_profile(owner, profile)?;
        if owner == "host-opening-contract" {
            self.ready
                .send(())
                .map_err(|_| WorkerError::transport("fixture gate lost"))?;
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .map_err(|_| WorkerError::timeout("fixture publication gate timed out"))?;
        }
        Ok(opened)
    }

    fn cancel_run(&self, owner: &str) -> Result<(), WorkerError> {
        self.worker.cancel_run(owner)
    }
}

pub fn verify_stop_during_open(
    worker: Arc<LoopbackPlaywrightWorker>,
    profile_root: &std::path::Path,
) -> Result<(), String> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let host = Arc::new(ExistingTabHost::new());
    host.set_profile_root(profile_root.to_path_buf());
    host.set_worker(Arc::new(DelayedOpen {
        worker: worker.clone(),
        ready: ready_tx,
        release: Mutex::new(release_rx),
    }));
    let other = host
        .open_managed_profile("other-session", "host-other-contract", "host-other")
        .map_err(|e| e.to_string())?;
    let opening_host = host.clone();
    let opening = std::thread::spawn(move || {
        opening_host.open_managed_profile(
            "opening-session",
            "host-opening-contract",
            "host-opening",
        )
    });
    let ready = ready_rx.recv_timeout(Duration::from_secs(10));
    let stopped = if ready.is_ok() {
        Some(host.cancel_run("host-opening-contract"))
    } else {
        None
    };
    let _ = release_tx.send(());
    let opened = opening
        .join()
        .map_err(|_| "Host opening fixture panicked")?;
    ready.map_err(|_| "real Chromium open never reached publication gate")?;
    if !matches!(stopped, Some(Err(BrokerError::BrowserWorker(ref e))) if e.code == "run_cleanup_pending")
        || !matches!(opened, Err(BrokerError::StopRequested))
        || !host
            .list_managed_for_run("host-opening-contract")
            .is_empty()
    {
        return Err("Host reported stopped early or published a late real browser open".into());
    }
    host.cancel_run("host-opening-contract")
        .map_err(|e| e.to_string())?;
    let terminal = worker
        .run_status("host-opening-contract", WorkerRunRevision::INITIAL)
        .map_err(|e| e.to_string())?;
    if terminal.phase != WorkerRunPhase::Stopped || !terminal.is_idle() {
        return Err("stopped worker owner did not acknowledge terminal idle state".into());
    }
    // cancel-run permanently opts the owner into versioned admission; this
    // legacy business client omits runRevision and must now be rejected there.
    if !matches!(worker.list_pages("host-opening-contract", "host-opening"), Err(ref e) if e.code == "stale_run_revision")
    {
        return Err("stopped worker owner retained browser access".into());
    }
    if host.managed_target_info(&other.tab_id).is_none()
        || worker
            .list_pages("host-other-contract", "host-other")
            .map_err(|e| e.to_string())?
            .is_empty()
    {
        return Err("stopping one Host owner affected another real browser".into());
    }
    // Successful reuse by another owner proves the old slot was closed and
    // released; the terminal old owner must still be unable to reopen it.
    host.open_managed_profile("other-session", "host-other-contract", "host-opening")
        .map_err(|e| e.to_string())?;
    if host
        .open_managed_profile("opening-session", "host-opening-contract", "late-profile")
        .is_ok()
    {
        return Err("stopped Host owner reopened a browser profile".into());
    }
    host.cancel_run("host-other-contract")
        .map_err(|e| e.to_string())?;
    println!("gate: host_profile real-Chromium open/Stop/late-publication/reuse/two-run-isolation=passed");
    Ok(())
}
