//! Deterministic fake macOS API. No real process, window, file, or account access.

use crate::{
    api::{AxWindow, BundleFacts, CgWindow, LaunchRecord, MacApi, RunningApp, SpotlightHit},
    identity::{SignatureInfo, SignatureState},
};
use gcu_core::driver::{DriverError, Result};
use gcu_protocol::Rect;
use std::{
    collections::BTreeMap,
    sync::Mutex,
    time::{Duration, SystemTime},
};

/// Mutable fake OS state.
#[derive(Clone, Debug)]
pub struct FakeState {
    /// Engine effective user id. Zero is root.
    pub euid: u32,
    /// Accessibility trust, without a prompt.
    pub accessibility: bool,
    /// Screen Recording preflight.
    pub screen_recording: bool,
    /// Running applications.
    pub running: Vec<RunningApp>,
    /// Frontmost pid. Launch and unhide must not change it.
    pub frontmost: Option<u32>,
    /// Bundle facts keyed by the path passed to [`MacApi::bundle_facts`].
    pub facts: BTreeMap<String, BundleFacts>,
    /// Spotlight hits revealed after [`Self::spotlight_delay`].
    pub spotlight: Vec<SpotlightHit>,
    /// How long [`MacApi::spotlight_recent`] blocks.
    pub spotlight_delay: Duration,
    /// Core Graphics windows.
    pub cg_windows: Vec<CgWindow>,
    /// Accessibility windows by pid.
    pub ax_windows: BTreeMap<u32, Vec<AxWindow>>,
    /// Windows added to the Core Graphics list on unhide.
    pub reveal_on_unhide: Vec<CgWindow>,
    /// Recorded launches. `activates` is false.
    pub launches: Vec<LaunchRecord>,
    /// Recorded `AXRaise` calls.
    pub raises: Vec<(u32, u32)>,
    /// Recorded unhide pids.
    pub unhides: Vec<u32>,
    /// When true, `AXRaise` leaves the window minimized.
    pub keep_minimized: bool,
    /// Injected failure for every read.
    pub failure: Option<DriverError>,
}

impl Default for FakeState {
    fn default() -> Self {
        Self {
            euid: 501,
            accessibility: true,
            screen_recording: true,
            running: Vec::new(),
            frontmost: Some(1),
            facts: BTreeMap::new(),
            spotlight: Vec::new(),
            spotlight_delay: Duration::ZERO,
            cg_windows: Vec::new(),
            ax_windows: BTreeMap::new(),
            reveal_on_unhide: Vec::new(),
            launches: Vec::new(),
            raises: Vec::new(),
            unhides: Vec::new(),
            keep_minimized: false,
            failure: None,
        }
    }
}

/// Injectable fake. Lock poisoning surfaces as a driver crash.
#[derive(Default)]
pub struct FakeMacApi {
    state: Mutex<FakeState>,
}

impl FakeMacApi {
    /// Mutate fixture state.
    pub fn update(&self, update: impl FnOnce(&mut FakeState)) {
        if let Ok(mut state) = self.state.lock() {
            update(&mut state);
        }
    }

    /// Snapshot fixture state.
    pub fn state(&self) -> Result<FakeState> {
        let state = self.state.lock().map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
        if let Some(error) = &state.failure {
            return Err(error.clone());
        }
        Ok(state.clone())
    }

    /// Install facts and a running process.
    pub fn add_running(&self, pid: u32, facts: &BundleFacts) {
        let app = running_app(pid, facts);
        self.update(|state| {
            state.facts.insert(facts.bundle_path.clone(), facts.clone());
            state
                .facts
                .insert(facts.executable_path.clone(), facts.clone());
            state.running.push(app);
        });
    }
}

impl MacApi for FakeMacApi {
    fn engine_euid(&self) -> Result<u32> {
        Ok(self.state()?.euid)
    }

    fn accessibility_trusted(&self) -> Result<bool> {
        Ok(self.state()?.accessibility)
    }

    fn screen_recording_ok(&self) -> Result<bool> {
        Ok(self.state()?.screen_recording)
    }

    fn running_apps(&self) -> Result<Vec<RunningApp>> {
        Ok(self.state()?.running)
    }

    fn frontmost_pid(&self) -> Result<Option<u32>> {
        Ok(self.state()?.frontmost)
    }

    fn bundle_facts(&self, path: &str) -> Result<BundleFacts> {
        let state = self.state()?;
        state
            .facts
            .get(path)
            .cloned()
            .or_else(|| {
                state
                    .facts
                    .values()
                    .find(|facts| {
                        facts.bundle_path.eq_ignore_ascii_case(path)
                            || facts.executable_path.eq_ignore_ascii_case(path)
                    })
                    .cloned()
            })
            .ok_or(DriverError::AppNotFound)
    }

    fn spotlight_recent(&self) -> Result<Vec<SpotlightHit>> {
        let (delay, hits) = {
            let state = self.state.lock().map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
            if let Some(error) = &state.failure {
                return Err(error.clone());
            }
            (state.spotlight_delay, state.spotlight.clone())
        };
        std::thread::park_timeout(delay);
        Ok(hits)
    }

    fn windows(&self) -> Result<Vec<CgWindow>> {
        Ok(self.state()?.cg_windows)
    }

    fn ax_windows(&self, pid: u32) -> Result<Vec<AxWindow>> {
        let state = self.state()?;
        Ok(state.ax_windows.get(&pid).cloned().unwrap_or_default())
    }

    fn ax_raise(&self, pid: u32, index: u32) -> Result<()> {
        self.update(|state| {
            state.raises.push((pid, index));
            if state.keep_minimized {
                return;
            }
            if let Some(windows) = state.ax_windows.get_mut(&pid) {
                if let Some(window) = windows.iter_mut().find(|window| window.index == index) {
                    window.minimized = false;
                }
            }
        });
        Ok(())
    }

    fn unhide(&self, pid: u32) -> Result<()> {
        self.update(|state| {
            state.unhides.push(pid);
            let extra = std::mem::take(&mut state.reveal_on_unhide);
            state.cg_windows.extend(extra);
            for app in &mut state.running {
                if app.pid == pid {
                    app.hidden = false;
                }
            }
        });
        Ok(())
    }

    fn launch(&self, path: &str) -> Result<()> {
        self.update(|state| {
            state.launches.push(LaunchRecord {
                path: path.to_owned(),
                activates: false,
            });
        });
        Ok(())
    }
}

/// Sixty-four hex characters of one byte.
pub fn hex_byte(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

/// Forty hex characters of one byte.
pub fn hex_cdhash(byte: u8) -> String {
    format!("{byte:02x}").repeat(20)
}

/// A team-signed bundle. The localized name defaults to the bundle file stem.
pub fn signed_facts(bundle_id: &str, bundle_path: &str) -> BundleFacts {
    let stem = bundle_path
        .rsplit('/')
        .next()
        .unwrap_or(bundle_path)
        .trim_end_matches(".app");
    let executable_path = format!("{bundle_path}/Contents/MacOS/{stem}");
    BundleFacts {
        bundle_id: Some(bundle_id.to_owned()),
        localized_name: stem.to_owned(),
        bundle_name: stem.to_owned(),
        bundle_path: bundle_path.to_owned(),
        executable_path,
        signature: SignatureInfo {
            state: SignatureState::Valid,
            team_id: Some("TEAMID1234".to_owned()),
            signing_id: Some(bundle_id.to_owned()),
            subject: Some("Example Signer".to_owned()),
            certificate_sha256: Some(hex_byte(0x11)),
            product: Some(stem.to_owned()),
            build: Some("1".to_owned()),
        },
        cdhash: Some(hex_cdhash(0x22)),
        sha256: hex_byte(0x33),
        writable_load_path: false,
    }
}

/// An unsigned bundle. The grant binds path, SHA-256, and cdhash.
pub fn unsigned_facts(bundle_path: &str) -> BundleFacts {
    let mut facts = signed_facts("com.example.Unsigned", bundle_path);
    facts.bundle_id = None;
    facts.signature.state = SignatureState::Unsigned;
    facts.signature.team_id = None;
    facts.signature.subject = None;
    facts.signature.certificate_sha256 = None;
    facts.signature.signing_id = None;
    facts
}

/// A running-app row copied from facts.
pub fn running_app(pid: u32, facts: &BundleFacts) -> RunningApp {
    RunningApp {
        pid,
        bundle_id: facts.bundle_id.clone(),
        localized_name: facts.localized_name.clone(),
        bundle_path: facts.bundle_path.clone(),
        executable_path: facts.executable_path.clone(),
        hidden: false,
    }
}

/// A layer-0 on-screen window.
pub fn cg_window(id: u32, pid: u32, title: &str, frame: Rect) -> CgWindow {
    CgWindow {
        window_id: id,
        pid,
        layer: 0,
        title: title.to_owned(),
        owner_name: title.to_owned(),
        frame,
        alpha: 1.0,
        scale: 2.0,
        on_current_desktop: None,
    }
}

/// An Accessibility window with the same rectangle as a Core Graphics row.
pub fn ax_window(index: u32, title: &str, frame: Rect) -> AxWindow {
    AxWindow {
        index,
        title: title.to_owned(),
        frame,
        minimized: false,
        main: true,
    }
}

/// A Spotlight hit last used at `now`.
pub fn recent_hit(name: &str, path: &str, bundle_id: Option<&str>) -> SpotlightHit {
    SpotlightHit {
        path: path.to_owned(),
        name: name.to_owned(),
        localized_name: name.to_owned(),
        bundle_id: bundle_id.map(str::to_owned),
        last_used: Some(SystemTime::now()),
    }
}

/// Rectangle helper for tests.
pub fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect { x, y, w, h }
}
