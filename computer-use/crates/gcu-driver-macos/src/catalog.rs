//! In-memory list hints, enriched on a thread separate from resolution and AX.

use crate::{
    api::{MacApi, RunningApp},
    apps::{list_catalog, resolved_from},
    spotlight::SpotlightIndex,
};
use gcu_core::driver::{DriverError, Result};
use gcu_protocol::AppInfo;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, SystemTime},
};

#[derive(Default)]
struct Snapshot {
    running: Vec<(RunningApp, AppInfo)>,
    issues: Vec<String>,
}

pub(super) struct Catalog {
    snapshot: Arc<Mutex<Snapshot>>,
    indexing: Arc<AtomicBool>,
    refresh: mpsc::SyncSender<()>,
}

impl Catalog {
    pub(super) fn start(api: Arc<dyn MacApi>) -> Result<Self> {
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let indexing = Arc::new(AtomicBool::new(true));
        let (refresh, requests) = mpsc::sync_channel(1);
        let cache = snapshot.clone();
        let flag = indexing.clone();
        std::thread::Builder::new()
            .name("gcu-macos-catalog".into())
            .spawn(move || loop {
                flag.store(true, Ordering::Release);
                if let Err(error) = refresh_catalog(api.as_ref(), &cache) {
                    if let Ok(mut snapshot) = cache.lock() {
                        snapshot.issues.push(error.to_string());
                        snapshot.issues.truncate(8);
                    }
                }
                flag.store(false, Ordering::Release);
                match requests.recv_timeout(Duration::from_secs(60)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            })
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
        Ok(Self {
            snapshot,
            indexing,
            refresh,
        })
    }

    pub(super) fn indexing(&self) -> bool {
        self.indexing.load(Ordering::Acquire)
    }

    /// No native calls and no wait for enrichment. Request a coalesced refresh.
    pub(super) fn list(&self, spotlight: &SpotlightIndex) -> Result<Vec<AppInfo>> {
        let running = self
            .snapshot
            .lock()
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?
            .running
            .clone();
        let hits: Vec<_> = spotlight
            .hits()?
            .into_iter()
            .filter(|hit| {
                !running.iter().any(|(app, _)| {
                    app.bundle_id.as_ref().is_some_and(|id| {
                        hit.bundle_id
                            .as_ref()
                            .is_some_and(|other| id.eq_ignore_ascii_case(other))
                    }) || app.bundle_path.eq_ignore_ascii_case(&hit.path)
                })
            })
            .collect();
        let mut apps: Vec<_> = running.iter().map(|(_, info)| info.clone()).collect();
        apps.extend(list_catalog(&[], &hits, SystemTime::now()));
        apps.sort_by(|left, right| left.id.cmp(&right.id));
        if !self.indexing.swap(true, Ordering::AcqRel) && self.refresh.try_send(()).is_err() {
            self.indexing.store(false, Ordering::Release);
        }
        Ok(apps)
    }

    pub(super) fn issues(&self) -> Vec<String> {
        self.snapshot
            .lock()
            .map(|cache| cache.issues.clone())
            .unwrap_or_else(|_| vec!["App catalog unavailable".into()])
    }
}

fn refresh_catalog(api: &dyn MacApi, cache: &Mutex<Snapshot>) -> Result<()> {
    let running = api.running_apps()?;
    // Publish names and pids before any signature or executable IO. Hints have
    // no authorization identity until their facts have been verified.
    let hints = running.iter().map(|app| (app.clone(), hint(app))).collect();
    cache
        .lock()
        .map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?
        .running = hints;
    for app in running {
        let path = if app.bundle_path.is_empty() {
            &app.executable_path
        } else {
            &app.bundle_path
        };
        let result = api
            .bundle_facts(path)
            .and_then(|facts| resolved_from(facts, Some(app.pid), app.hidden));
        let mut snapshot = cache.lock().map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
        match result {
            Ok(resolved) => {
                if let Some((_, info)) = snapshot
                    .running
                    .iter_mut()
                    .find(|(row, _)| row.pid == app.pid)
                {
                    *info = resolved.info;
                }
            }
            Err(error) => {
                snapshot.issues.push(error.to_string());
                snapshot.issues.truncate(8);
            }
        }
    }
    Ok(())
}

fn hint(app: &RunningApp) -> AppInfo {
    let id = app
        .bundle_id
        .clone()
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| app.executable_path.clone());
    let name = if app.localized_name.trim().is_empty() {
        app.executable_path
            .rsplit('/')
            .next()
            .unwrap_or(&id)
            .to_owned()
    } else {
        app.localized_name.clone()
    };
    AppInfo {
        id,
        name,
        pid: Some(app.pid),
        identity: None,
    }
}
