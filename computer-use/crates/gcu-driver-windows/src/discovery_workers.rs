//! Independent queues: identity enrichment must never precede window/UIA reads.
use crate::{
    api::WinApi,
    apps::AppIndex,
    desktop::{visible_windows, window_process},
    discovery::{executable_name, Entry, State},
    gate::check_engine,
    identity::normalize_path,
};
use gcu_core::driver::*;
use gcu_protocol::AppInfo;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::oneshot;

pub(super) type Task = Box<dyn FnOnce(&mut State) + Send>;
type Entries = Arc<Mutex<BTreeMap<String, Entry>>>;

pub(super) struct Discovery {
    api: Arc<dyn WinApi>,
    pub(super) index: Arc<AppIndex>,
    pub(super) running: mpsc::SyncSender<Task>,
    pub(super) installed: mpsc::SyncSender<Task>,
    catalog: Catalog,
    entries: Entries,
}
impl Discovery {
    pub(super) async fn start(api: Arc<dyn WinApi>) -> Result<(Self, mpsc::SyncSender<Task>)> {
        check_api(api.clone()).await?;
        // The index owns cache/Start Menu IO, including its initial cache read.
        let index = Arc::new(AppIndex::start(api.clone())?);
        let entries = Arc::new(Mutex::new(BTreeMap::new()));
        let read = start(api.clone(), index.clone(), entries.clone(), "gcu-win-read").await?;
        let running = start(
            api.clone(),
            index.clone(),
            entries.clone(),
            "gcu-win-running",
        )
        .await?;
        let installed = start(
            api.clone(),
            index.clone(),
            entries.clone(),
            "gcu-win-installed",
        )
        .await?;
        let catalog = Catalog::start(api.clone())?;
        Ok((
            Self {
                api,
                index,
                running,
                installed,
                catalog,
                entries,
            },
            read,
        ))
    }
    pub(super) fn indexing(&self) -> bool {
        self.index.indexing() || self.catalog.indexing.load(Ordering::Acquire)
    }
    pub(super) async fn check(&self) -> Result<()> {
        // The live engine gate is independent of identity/index/UIA queues.
        check_api(self.api.clone()).await
    }
    pub(super) fn list(&self) -> Result<Vec<AppInfo>> {
        let mut apps = self.catalog.apps.lock().map_err(|_| crashed())?.clone();
        // Resolved live applications appear immediately, including ones opened
        // after the catalog's last snapshot. Entries remain hints here; every
        // observation/action still validates live process/image/window identity.
        for entry in self.entries.lock().map_err(|_| crashed())?.values() {
            if entry.info.pid.is_some() {
                apps.insert(entry.info.id.clone(), entry.info.clone());
            } else {
                // An installed-only resolution must not hide a newer live hint.
                apps.entry(entry.info.id.clone())
                    .or_insert_with(|| entry.info.clone());
            }
        }
        for shortcut in self.index.shortcuts()? {
            // Do not duplicate a running executable using its Start Menu AUMID.
            if apps
                .values()
                .any(|a| a.id.eq_ignore_ascii_case(&shortcut.target))
            {
                continue;
            }
            let id = shortcut
                .aumid
                .unwrap_or_else(|| shortcut.target.to_lowercase());
            apps.entry(id.clone()).or_insert(AppInfo {
                id,
                name: shortcut.name,
                pid: None,
                identity: None,
            });
        }
        self.catalog.refresh();
        Ok(apps.into_values().collect())
    }
    pub(super) fn issues(&self) -> Vec<String> {
        self.catalog
            .issues
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| vec!["App catalog unavailable".into()])
    }
}

async fn check_api(api: Arc<dyn WinApi>) -> Result<()> {
    tokio::time::timeout(
        DRIVER_DEADLINE,
        tokio::task::spawn_blocking(move || check_engine(&api.engine_security()?)),
    )
    .await
    .map_err(|_| DriverError::Timeout {
        maybe_executed: false,
    })?
    .map_err(|_| crashed())?
}

async fn start(
    api: Arc<dyn WinApi>,
    index: Arc<AppIndex>,
    entries: Entries,
    name: &str,
) -> Result<mpsc::SyncSender<Task>> {
    let (tx, rx) = mpsc::sync_channel::<Task>(32);
    let (ready, initialized) = oneshot::channel();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let state = (|| {
                check_engine(&api.engine_security()?)?;
                Ok(State {
                    _thread_context: api.enter_thread()?,
                    api,
                    index,
                    entries,
                    issues: Vec::new(),
                    windows: BTreeMap::new(),
                })
            })();
            match state {
                Ok(mut state) => {
                    if ready.send(Ok(())).is_err() {
                        return;
                    }
                    while let Ok(task) = rx.recv() {
                        task(&mut state);
                    }
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                }
            }
        })
        .map_err(|_| crashed())?;
    tokio::time::timeout(DRIVER_DEADLINE, initialized)
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })?
        .map_err(|_| crashed())??;
    Ok(tx)
}

struct Catalog {
    apps: Arc<Mutex<BTreeMap<String, AppInfo>>>,
    issues: Arc<Mutex<Vec<String>>>,
    indexing: Arc<AtomicBool>,
    refresh: mpsc::SyncSender<()>,
}
impl Catalog {
    fn start(api: Arc<dyn WinApi>) -> Result<Self> {
        let apps = Arc::new(Mutex::new(BTreeMap::new()));
        let issues = Arc::new(Mutex::new(Vec::new()));
        let indexing = Arc::new(AtomicBool::new(true));
        let (refresh, requests) = mpsc::sync_channel(1);
        let (cache, notes, flag) = (apps.clone(), issues.clone(), indexing.clone());
        std::thread::Builder::new()
            .name("gcu-win-catalog".into())
            .spawn(move || {
                let _context = match api.enter_thread() {
                    Ok(context) => context,
                    Err(error) => {
                        note(&notes, error);
                        flag.store(false, Ordering::Release);
                        return;
                    }
                };
                loop {
                    flag.store(true, Ordering::Release);
                    if let Err(error) = refresh_catalog(api.as_ref(), &cache, &notes) {
                        note(&notes, error);
                    }
                    flag.store(false, Ordering::Release);
                    match requests.recv_timeout(Duration::from_secs(60)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => (),
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .map_err(|_| crashed())?;
        Ok(Self {
            apps,
            issues,
            indexing,
            refresh,
        })
    }
    fn refresh(&self) {
        if !self.indexing.swap(true, Ordering::AcqRel) && self.refresh.try_send(()).is_err() {
            self.indexing.store(false, Ordering::Release);
        }
    }
}

fn refresh_catalog(
    api: &dyn WinApi,
    cache: &Mutex<BTreeMap<String, AppInfo>>,
    notes: &Mutex<Vec<String>>,
) -> Result<()> {
    check_engine(&api.engine_security()?)?;
    let mut hints = BTreeMap::new();
    for window in visible_windows(api.enum_windows()?) {
        let hint = (|| {
            let pid = window_process(api, &window)?;
            let path = normalize_path(&api.process_path(pid)?)?;
            Ok(AppInfo {
                name: executable_name(&path),
                id: path,
                pid: Some(pid),
                identity: None,
            })
        })();
        match hint {
            Ok(info) => {
                hints.insert(info.id.clone(), info);
            }
            Err(error) => {
                if let Ok(mut notes) = notes.lock() {
                    note_window(&mut notes, error);
                }
            }
        }
    }
    // Discovery must not hash/sign unrelated images: one blocked verifier used
    // to prevent all later running-window refreshes. Only resolve authorizes.
    *cache.lock().map_err(|_| crashed())? = hints;
    Ok(())
}
fn note(notes: &Mutex<Vec<String>>, error: DriverError) {
    if let Ok(mut notes) = notes.lock() {
        notes.push(error.to_string());
        notes.truncate(8);
    }
}

// EnumWindows is a snapshot: unrelated processes may exit before path/token
// lookup. These per-window outcomes are diagnostics, not engine health issues.
pub(super) fn note_window(issues: &mut Vec<String>, error: DriverError) {
    tracing::debug!(target: "gcu_windows_discovery", %error, "window discovery omitted candidate");
    if matches!(error, DriverError::AppNotFound | DriverError::WindowGone) {
        return;
    }
    let issue = format!("window discovery: {error}");
    if issues.len() < 8 && !issues.contains(&issue) {
        issues.push(issue);
    }
}
fn crashed() -> DriverError {
    DriverError::Crashed {
        maybe_executed: false,
    }
}
