//! Deterministic fake Windows API for portable discovery and safety tests.
use crate::{
    api::{ImageInfo, NativeWindow, Shortcut, WinApi},
    gate::{IntegrityLevel, ProcessSecurity},
    identity::{SignatureInfo, SignatureState},
};
use gcu_core::driver::{DriverError, Result};
use std::{
    collections::BTreeMap,
    sync::{Condvar, Mutex},
    time::Duration,
};

mod blocking;
pub use blocking::ControlledBlock;

/// Mutable fake OS state. No real account, window, process or filesystem access.
#[derive(Clone, Debug)]
pub struct FakeState {
    /// Engine token/session/desktop state.
    pub engine: ProcessSecurity,
    /// Target token/session states.
    pub processes: BTreeMap<u32, ProcessSecurity>,
    /// Process executable paths.
    pub paths: BTreeMap<u32, String>,
    /// Running process AppUserModelIds; discovery hints only.
    pub aumids: BTreeMap<u32, String>,
    /// File metadata indexed by query path.
    pub images: BTreeMap<String, ImageInfo>,
    /// Enumerated HWNDs.
    pub windows: Vec<NativeWindow>,
    /// Virtual desktop observations.
    pub desktops: BTreeMap<u64, Option<bool>>,
    /// Parsed .lnk fixtures.
    pub links: Vec<Shortcut>,
    /// Serialized cache.
    pub cache: Option<Vec<u8>>,
    /// Index scan delay to prove first-call nonblocking behavior.
    pub scan_delay: Duration,
    /// Delayed enumeration to prove list_apps does not wait for running-image IO.
    pub enum_delay: Duration,
    /// Invalid .lnk entries interspersed with working ones.
    pub broken_links: Vec<String>,
    /// Simulate an app refusing a nonactivating restoration request.
    pub refuse_restore: bool,
    /// Recorded launch targets.
    pub launches: Vec<Shortcut>,
    /// Recorded restoration HWNDs.
    pub restores: Vec<u64>,
    /// Full image-verification calls, for detecting redundant hashing/signatures.
    pub image_reads: Vec<String>,
    /// Top-level metadata enumeration count (never UIA; background catalog excluded).
    pub enumerations: usize,
    /// Windows/native metadata probe failure.
    pub failure: Option<DriverError>,
}
impl Default for FakeState {
    fn default() -> Self {
        Self {
            engine: medium_security(),
            processes: BTreeMap::new(),
            paths: BTreeMap::new(),
            aumids: BTreeMap::new(),
            images: BTreeMap::new(),
            windows: Vec::new(),
            desktops: BTreeMap::new(),
            links: Vec::new(),
            cache: None,
            scan_delay: Duration::ZERO,
            enum_delay: Duration::ZERO,
            broken_links: Vec::new(),
            refuse_restore: false,
            launches: Vec::new(),
            restores: Vec::new(),
            image_reads: Vec::new(),
            enumerations: 0,
            failure: None,
        }
    }
}
/// Injectable fake with a real bounded notification primitive.
#[derive(Default)]
pub struct FakeWinApi {
    state: Mutex<FakeState>,
    changed: Mutex<bool>,
    notify: Condvar,
    scan_block: Mutex<Option<std::sync::Arc<ControlledBlock>>>,
    cache_block: Mutex<Option<std::sync::Arc<ControlledBlock>>>,
    enum_block: Mutex<Option<std::sync::Arc<ControlledBlock>>>,
    image_blocks: Mutex<BTreeMap<String, std::sync::Arc<ControlledBlock>>>,
}
impl FakeWinApi {
    /// Hold the Start Menu scan until the returned gate is released.
    pub fn block_index(&self) -> std::sync::Arc<ControlledBlock> {
        let block = std::sync::Arc::new(ControlledBlock::default());
        *self.scan_block.lock().unwrap_or_else(|e| e.into_inner()) = Some(block.clone());
        block
    }
    /// Hold startup cache IO independently of discovery queues.
    pub fn block_cache(&self) -> std::sync::Arc<ControlledBlock> {
        let block = std::sync::Arc::new(ControlledBlock::default());
        *self.cache_block.lock().unwrap_or_else(|e| e.into_inner()) = Some(block.clone());
        block
    }
    /// Hold enumeration until released; no real-time fake delay required.
    pub fn block_enumeration(&self) -> std::sync::Arc<ControlledBlock> {
        let block = std::sync::Arc::new(ControlledBlock::default());
        *self.enum_block.lock().unwrap_or_else(|e| e.into_inner()) = Some(block.clone());
        block
    }
    /// Hold only one executable's signature/hash, without retaining fake state.
    pub fn block_image(&self, path: &str) -> std::sync::Arc<ControlledBlock> {
        let block = std::sync::Arc::new(ControlledBlock::default());
        self.image_blocks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(path.to_lowercase(), block.clone());
        block
    }

    /// Mutate fixture state. Poisoned fake state is surfaced as a driver crash.
    pub fn update(&self, update: impl FnOnce(&mut FakeState)) {
        if let Ok(mut state) = self.state.lock() {
            update(&mut state);
        }
    }
    /// Snapshot fixture state without retaining the lock.
    pub fn state(&self) -> Result<FakeState> {
        let state = self.state.lock().map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
        if let Some(error) = &state.failure {
            return Err(error.clone());
        }
        Ok(state.clone())
    }
    /// Add an executable fixture.
    pub fn image(&self, path: &str, image: ImageInfo) {
        self.update(|s| {
            s.images.insert(path.into(), image);
        });
    }
    /// Trigger a Start Menu notification.
    pub fn notify_change(&self) {
        if let Ok(mut changed) = self.changed.lock() {
            *changed = true;
            self.notify.notify_all();
        }
    }
}
impl WinApi for FakeWinApi {
    fn engine_security(&self) -> Result<ProcessSecurity> {
        Ok(self.state()?.engine)
    }
    fn process_security(&self, pid: u32) -> Result<ProcessSecurity> {
        self.state()?
            .processes
            .get(&pid)
            .cloned()
            .ok_or(DriverError::WindowGone)
    }
    fn enum_windows(&self) -> Result<Vec<NativeWindow>> {
        // The background catalog refresh (spawned per resolve/list) enumerates on
        // its own thread at an unordered time; only caller-thread passes count.
        let background = std::thread::current().name() == Some("gcu-win-catalog");
        self.update(|s| s.enumerations += usize::from(!background));
        let block = self
            .enum_block
            .lock()
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?
            .clone();
        if let Some(block) = block {
            block.wait();
        }
        let state = self.state()?;
        // `park_timeout` may return early on a spurious wakeup, which would let a
        // "stalled" enumeration finish inside the budget the caller is testing.
        let until = std::time::Instant::now() + state.enum_delay;
        while let Some(left) = until.checked_duration_since(std::time::Instant::now()) {
            std::thread::park_timeout(left);
        }
        Ok(state.windows)
    }
    fn process_path(&self, pid: u32) -> Result<String> {
        self.state()?
            .paths
            .get(&pid)
            .cloned()
            .ok_or(DriverError::AppNotFound)
    }
    fn process_aumid(&self, pid: u32) -> Result<Option<String>> {
        Ok(self.state()?.aumids.get(&pid).cloned())
    }
    fn image(&self, path: &str) -> Result<ImageInfo> {
        self.update(|s| s.image_reads.push(path.to_lowercase()));
        let block = self
            .image_blocks
            .lock()
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?
            .get(&path.to_lowercase())
            .cloned();
        if let Some(block) = block {
            block.wait();
        }
        let state = self.state()?;
        state
            .images
            .get(path)
            .cloned()
            .or_else(|| {
                state
                    .images
                    .values()
                    .find(|i| i.canonical_path.eq_ignore_ascii_case(path))
                    .cloned()
            })
            .ok_or(DriverError::AppNotFound)
    }
    fn on_current_desktop(&self, hwnd: u64) -> Result<Option<bool>> {
        Ok(self.state()?.desktops.get(&hwnd).copied().flatten())
    }
    fn read_link(&self, path: &str) -> Result<Shortcut> {
        self.state()?
            .links
            .into_iter()
            .find(|s| s.link == path)
            .ok_or(DriverError::AppNotFound)
    }
    fn start_menu_links(&self) -> Result<Vec<String>> {
        let block = self
            .scan_block
            .lock()
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?
            .clone();
        if let Some(block) = block {
            block.wait();
        }
        let state = self.state()?;
        std::thread::park_timeout(state.scan_delay);
        Ok(state
            .links
            .into_iter()
            .map(|s| s.link)
            .chain(state.broken_links)
            .collect())
    }
    fn read_cache(&self) -> Result<Option<Vec<u8>>> {
        let block = self
            .cache_block
            .lock()
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?
            .clone();
        if let Some(block) = block {
            block.wait();
        }
        Ok(self.state()?.cache)
    }
    fn write_cache(&self, bytes: &[u8]) -> Result<()> {
        self.update(|s| s.cache = Some(bytes.to_vec()));
        Ok(())
    }
    fn links_changed(&self, timeout: Duration) -> Result<bool> {
        let changed = self.changed.lock().map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
        let (mut changed, _) = self
            .notify
            .wait_timeout_while(changed, timeout, |c| !*c)
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
        let result = *changed;
        *changed = false;
        Ok(result)
    }
    fn launch(
        &self,
        shortcut: &Shortcut,
        expected: &crate::identity::WindowsIdentity,
    ) -> Result<()> {
        if crate::identity::identity_for_path(self, &shortcut.target)? != *expected {
            return Err(DriverError::AppNotFound);
        }
        self.update(|s| s.launches.push(shortcut.clone()));
        Ok(())
    }
    fn restore(&self, expected: &NativeWindow) -> Result<()> {
        let hwnd = expected.hwnd;
        let current = self
            .state()?
            .windows
            .into_iter()
            .find(|w| w.hwnd == hwnd)
            .ok_or(DriverError::WindowGone)?;
        crate::desktop::HwndStamp::from(expected).validate(&current)?;
        self.update(|s| {
            s.restores.push(hwnd);
            if !s.refuse_restore {
                if let Some(w) = s.windows.iter_mut().find(|w| w.hwnd == hwnd) {
                    w.minimized = false;
                }
            }
        });
        Ok(())
    }
}
/// Normal interactive token fixture.
pub fn medium_security() -> ProcessSecurity {
    ProcessSecurity {
        integrity: IntegrityLevel::Medium,
        ui_access: false,
        session_id: 1,
        interactive: true,
        secure_desktop: false,
        created: 42,
    }
}
/// Valid signer/image fixture; same publisher can be paired with different certs.
pub fn signed_image(path: &str) -> ImageInfo {
    ImageInfo {
        canonical_path: path.into(),
        sha256: "a".repeat(64),
        writable_load_path: false,
        signature: SignatureInfo {
            state: SignatureState::Valid,
            publisher: Some("Example Publisher".into()),
            certificate_sha256: Some("1".repeat(64)),
            product: Some("Editor.exe / Editor".into()),
        },
    }
}
/// Visible top-level HWND fixture.
pub fn window(hwnd: u64, pid: u32) -> NativeWindow {
    NativeWindow {
        hwnd,
        pid,
        process_created: 42,
        created: 99,
        class: "FixtureWindow".into(),
        title: "Editor".into(),
        visible: true,
        tool_window: false,
        app_window: false,
        owner: None,
        minimized: false,
        foreground: false,
        frame: gcu_protocol::Rect {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 600.0,
        },
        scale: 1.0,
        child_pids: Vec::new(),
    }
}
