//! Error cleanup delivers the original held callback before joining its caller.

use super::*;

pub(super) struct Completion {
    view: tauri::Webview,
    scope: Arc<Probe>,
    world: Arc<DownloadWorld>,
    run_id: String,
    armed: bool,
}

impl Completion {
    pub(super) fn new(
        live: &SideBrowserLiveView,
        scope: &Arc<Probe>,
        world: Arc<DownloadWorld>,
        run_id: &str,
    ) -> Self {
        Self {
            view: live.webview.clone(),
            scope: scope.clone(),
            world,
            run_id: run_id.into(),
            armed: true,
        }
    }

    pub(super) fn release(&mut self) -> Result<(), String> {
        if !self.armed {
            return Ok(());
        }
        let scope = self.scope.clone();
        io::native(&self.view, move |_| scope.release_on_native_thread())?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        if self.armed {
            // Stop the original authority, then deliver only its captured
            // callback on its original controller. No label lookup or replay.
            let _ = self.world.broker.request_stop(&self.run_id);
            if let Err(error) = self.release() {
                eprintln!("original held App callback cleanup failed: {error}");
            }
        }
    }
}
