//! Background Start Menu indexing. Cache records are hints, never authorization.
use crate::api::{Shortcut, WinApi};
use gcu_core::driver::{DriverError, Result};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cache {
    version: u32,
    shortcuts: Vec<Shortcut>,
}

/// Decode a versioned, size-bounded cache. Invalid data triggers a fresh scan.
pub fn decode_cache(bytes: &[u8]) -> Result<Vec<Shortcut>> {
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(DriverError::InvalidRequest);
    }
    let cache: Cache = serde_json::from_slice(bytes).map_err(|_| DriverError::InvalidRequest)?;
    if cache.version != 1 || cache.shortcuts.len() > 10_000 {
        return Err(DriverError::InvalidRequest);
    }
    Ok(cache.shortcuts)
}
/// Encode discovery hints; neither signatures nor authorization grants are cached.
pub fn encode_cache(shortcuts: &[Shortcut]) -> Result<Vec<u8>> {
    serde_json::to_vec(&Cache {
        version: 1,
        shortcuts: shortcuts.to_vec(),
    })
    .map_err(|_| DriverError::InvalidRequest)
}

#[derive(Default)]
struct IndexState {
    shortcuts: Vec<Shortcut>,
    issues: Vec<String>,
}
/// Background index with a bounded notification wait. Drop requests shutdown;
/// no caller waits for Start Menu scans or notification delivery.
pub struct AppIndex {
    state: Arc<Mutex<IndexState>>,
    indexing: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}
impl AppIndex {
    /// Start cache loading, rebuilding and watching on a dedicated thread.
    /// Returns immediately; no cache IO or scan runs on a discovery read queue.
    pub fn start(api: Arc<dyn WinApi>) -> Result<Self> {
        let index = Self {
            state: Arc::new(Mutex::new(IndexState::default())),
            indexing: Arc::new(AtomicBool::new(true)),
            stop: Arc::new(AtomicBool::new(false)),
        };
        let shared = index.state.clone();
        let indexing = index.indexing.clone();
        let stop = index.stop.clone();
        std::thread::Builder::new()
            .name("gcu-win-app-index".into())
            .spawn(move || {
                let _context = match api.enter_thread() {
                    Ok(context) => context,
                    Err(error) => {
                        if let Ok(mut state) = shared.lock() {
                            state.issues.push(error.to_string());
                        }
                        indexing.store(false, Ordering::Release);
                        return;
                    }
                };
                let cached = api
                    .read_cache()
                    .and_then(|bytes| bytes.map(|b| decode_cache(&b)).transpose());
                match cached {
                    Ok(Some(shortcuts)) => {
                        if let Ok(mut state) = shared.lock() {
                            state.shortcuts = shortcuts;
                        }
                    }
                    Ok(None) => (),
                    Err(error) => {
                        if let Ok(mut state) = shared.lock() {
                            state.issues.push(format!(
                                "App cache invalid or unreadable; rebuilding: {error}"
                            ));
                        }
                    }
                }
                // Establish notifications before scanning so changes during the
                // initial rebuild remain signaled on the persistent handles.
                if let Err(error) = api.links_changed(Duration::ZERO) {
                    if let Ok(mut state) = shared.lock() {
                        state.issues.push(error.to_string());
                    }
                }
                loop {
                    let result = scan(api.as_ref(), &stop);
                    match result {
                        Ok((shortcuts, issues)) => {
                            if let Ok(mut state) = shared.lock() {
                                state.shortcuts = shortcuts;
                                state.issues.extend(issues);
                                state.issues.truncate(8);
                            }
                        }
                        Err(error) => {
                            if let Ok(mut state) = shared.lock() {
                                state.issues.push(error.to_string());
                                state.issues.truncate(8);
                            }
                        }
                    }
                    indexing.store(false, Ordering::Release);
                    loop {
                        if stop.load(Ordering::Acquire) {
                            return;
                        }
                        match api.links_changed(Duration::from_millis(250)) {
                            Ok(true) => {
                                indexing.store(true, Ordering::Release);
                                break;
                            }
                            Ok(false) => {}
                            Err(error) => {
                                if let Ok(mut state) = shared.lock() {
                                    state.issues.push(error.to_string());
                                    state.issues.truncate(8);
                                }
                                // Notification failures are reported; bounded periodic
                                // rescanning recovers without a tight failing-FFI loop.
                                std::thread::park_timeout(Duration::from_millis(250));
                                indexing.store(true, Ordering::Release);
                                break;
                            }
                        }
                    }
                }
            })
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
        Ok(index)
    }
    /// Clone only in-memory discovery hints; never holds a mutex during OS IO.
    pub fn shortcuts(&self) -> Result<Vec<Shortcut>> {
        Ok(self
            .state
            .lock()
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?
            .shortcuts
            .clone())
    }
    /// Nonblocking rebuild flag.
    pub fn indexing(&self) -> bool {
        self.indexing.load(Ordering::Acquire)
    }
    /// Metadata-only cache/watcher diagnostics.
    pub fn issues(&self) -> Vec<String> {
        self.state
            .lock()
            .map(|s| s.issues.clone())
            .unwrap_or_else(|_| vec!["App index unavailable".into()])
    }
}
impl Drop for AppIndex {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
fn scan(api: &dyn WinApi, stop: &AtomicBool) -> Result<(Vec<Shortcut>, Vec<String>)> {
    let mut shortcuts = Vec::new();
    let mut issues = Vec::new();
    for path in api.start_menu_links()? {
        if stop.load(Ordering::Acquire) {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        // Broken/non-executable links do not hide the rest of the Start Menu.
        // Record their metadata-only failure rather than discarding a full scan.
        match api.read_link(&path) {
            Ok(shortcut) => shortcuts.push(shortcut),
            Err(error) => {
                if issues.len() < 8 {
                    issues.push(error.to_string());
                }
            }
        }
    }
    shortcuts.sort_by(|a, b| a.link.cmp(&b.link));
    shortcuts.dedup_by(|a, b| a.link == b.link);
    if let Err(error) = api.write_cache(&encode_cache(&shortcuts)?) {
        issues.push(error.to_string());
    }
    Ok((shortcuts, issues))
}
