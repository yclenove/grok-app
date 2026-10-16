//! Discovery worker state. OS calls go through [`crate::api::MacApi`].

use crate::capture::{self, SourceWindow};
use crate::{
    api::{BundleFacts, MacApi, RunningApp, SpotlightHit},
    apps::{self, hit_could_match, resolve_loaded, ResolveOutcome, ResolvedApp},
    gate::{self, MacGate},
    identity::{identity_from_facts, needs_reauth},
    spotlight::SpotlightIndex,
    windows::{self, align_pid, AlignedWindow},
};
use gcu_core::{
    driver::{AppQuery, AppRef, DriverError, Result, WindowRef},
    model::Geometry,
};
use gcu_protocol::WindowInfo;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::SystemTime,
};

pub(super) struct State {
    pub(super) api: Arc<dyn MacApi>,
    pub(super) spotlight: Arc<SpotlightIndex>,
    entries: Arc<Mutex<BTreeMap<String, ResolvedApp>>>,
    windows: BTreeMap<String, AlignedWindow>,
    pub(super) issues: Vec<String>,
}

impl State {
    pub(super) fn new(api: Arc<dyn MacApi>, spotlight: Arc<SpotlightIndex>) -> Self {
        Self {
            api,
            spotlight,
            entries: Arc::new(Mutex::new(BTreeMap::new())),
            windows: BTreeMap::new(),
            issues: Vec::new(),
        }
    }

    pub(super) fn fork(&self) -> Self {
        Self {
            api: self.api.clone(),
            spotlight: self.spotlight.clone(),
            entries: self.entries.clone(),
            windows: BTreeMap::new(),
            issues: Vec::new(),
        }
    }
}

impl State {
    /// Only matching running processes incur signature/hash IO. No Spotlight
    /// bundle is opened, even when an installed app has the same display name.
    pub(super) fn resolve_running(&mut self, query: &AppQuery) -> Result<Option<AppRef>> {
        MacGate::engine(self.api.engine_euid()?)?;
        let running_apps = self.api.running_apps()?;
        let mut running = Vec::new();
        for app in &running_apps {
            if !apps::matches_running(app, &query.0) {
                continue;
            }
            let Some(path) = fact_path(&app.bundle_path, &app.executable_path) else {
                continue;
            };
            running.push((app.clone(), self.api.bundle_facts(path)?));
        }
        if running.is_empty() {
            return Ok(None);
        }
        // Preserve ambiguity using cached hints, without opening unrelated code.
        for hit in self.spotlight.hits()? {
            if hit_could_match(&hit, &query.0)
                && crate::spotlight::within_recent(hit.last_used, SystemTime::now())
                && !running.iter().any(|(app, _)| {
                    app.bundle_path.eq_ignore_ascii_case(&hit.path)
                        || app.bundle_id.as_ref().is_some_and(|id| {
                            hit.bundle_id
                                .as_ref()
                                .is_some_and(|other| id.eq_ignore_ascii_case(other))
                        })
                })
            {
                return Err(DriverError::AmbiguousApp);
            }
        }
        self.bind(resolve_loaded(
            &running,
            &[],
            None,
            &query.0,
            SystemTime::now(),
        )?)
        .map(Some)
    }

    /// Installed/direct-path lookup has its own queue, never ahead of running apps.
    pub(super) fn resolve(&mut self, query: &AppQuery) -> Result<AppRef> {
        // Recheck: an app can start between the fast path and this fallback.
        if let Some(app) = self.resolve_running(query)? {
            return Ok(app);
        }
        let hits = self.spotlight.hits()?;
        let mut recent: Vec<(SpotlightHit, BundleFacts)> = Vec::new();
        for hit in hits {
            if !hit_could_match(&hit, &query.0) {
                continue;
            }
            recent.push((hit.clone(), self.api.bundle_facts(&hit.path)?));
        }
        let direct = direct_facts(self.api.as_ref(), &query.0, &[], &recent)?;
        self.bind(resolve_loaded(
            &[],
            &recent,
            direct.as_ref(),
            &query.0,
            SystemTime::now(),
        )?)
    }

    fn bind(&mut self, outcome: ResolveOutcome) -> Result<AppRef> {
        match outcome {
            ResolveOutcome::None => Err(DriverError::AppNotFound),
            ResolveOutcome::Ambiguous(_) => Err(DriverError::AmbiguousApp),
            ResolveOutcome::One(app) => {
                let app = *app;
                let info = app.info.clone();
                self.entries
                    .lock()
                    .map_err(|_| DriverError::Crashed {
                        maybe_executed: false,
                    })?
                    .insert(info.id.clone(), app);
                Ok(AppRef { info })
            }
        }
    }

    pub(super) fn launch(&mut self, app: &AppRef) -> Result<AppRef> {
        let resolved = self.validate_app(app)?;
        self.observation_gate()?;
        if resolved.pid.is_some() {
            return Ok(app.clone());
        }
        let front = self.api.frontmost_pid()?;
        self.api.launch(&resolved.identity.bundle_path)?;
        if self.api.frontmost_pid()? != front {
            return Err(DriverError::Native {
                message: "launch changed the frontmost app".into(),
                maybe_executed: true,
            });
        }
        Ok(app.clone())
    }

    pub(super) fn list_windows(&mut self, app: Option<&AppRef>) -> Result<Vec<WindowInfo>> {
        self.observation_gate()?;
        if let Some(app) = app {
            let resolved = self.validate_app(app)?;
            let Some(pid) = resolved.pid else {
                return Ok(Vec::new());
            };
            return Ok(self
                .windows_for(pid)?
                .into_iter()
                .map(|window| window.info())
                .collect());
        }
        Ok(self
            .collect_windows(None)?
            .into_iter()
            .map(|window| window.info())
            .collect())
    }

    pub(super) fn key_window(&mut self, app: &AppRef) -> Result<WindowInfo> {
        self.observation_gate()?;
        let resolved = self.validate_app(app)?;
        let pid = resolved.pid.ok_or(DriverError::WindowGone)?;
        let front = self.api.frontmost_pid()?;
        let mut rows = self.windows_for(pid)?;
        if rows.is_empty() {
            self.api.unhide(pid)?;
            rows = self.windows_for(pid)?;
        }
        let Some(mut chosen) = pick_key(&rows).cloned() else {
            return Err(DriverError::WindowGone);
        };
        if chosen.minimized {
            let index = chosen.ax_index.ok_or(DriverError::WindowMinimized)?;
            self.api.ax_raise(pid, index)?;
            rows = self.windows_for(pid)?;
            chosen = rows
                .into_iter()
                .find(|window| window.cg_id == chosen.cg_id)
                .ok_or(DriverError::WindowGone)?;
            if chosen.minimized {
                return Err(DriverError::WindowMinimized);
            }
        }
        if self.api.frontmost_pid()? != front {
            return Err(DriverError::Native {
                message: "window restore changed the frontmost app".into(),
                maybe_executed: true,
            });
        }
        Ok(chosen.info())
    }

    pub(super) fn validate_app(&mut self, app: &AppRef) -> Result<ResolvedApp> {
        MacGate::engine(self.api.engine_euid()?)?;
        let entry = self
            .entries
            .lock()
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?
            .get(&app.info.id)
            .cloned()
            .ok_or(DriverError::AppNotFound)?;
        if app.info.pid != entry.pid
            || app.info.identity.as_ref() != Some(&entry.identity.app_identity())
        {
            return Err(DriverError::AppNotFound);
        }
        let fresh = identity_from_facts(self.api.bundle_facts(&entry.identity.bundle_path)?)?;
        if needs_reauth(&entry.identity, &fresh)
            || fresh.app_identity() != entry.identity.app_identity()
        {
            self.entries
                .lock()
                .map_err(|_| DriverError::Crashed {
                    maybe_executed: false,
                })?
                .remove(&app.info.id);
            return Err(DriverError::AppNotFound);
        }
        if let Some(pid) = app.info.pid {
            let running = self.api.running_apps()?;
            let process = running
                .iter()
                .find(|item| item.pid == pid)
                .ok_or(DriverError::AppNotFound)?;
            let same_bundle = process.bundle_path.eq_ignore_ascii_case(&fresh.bundle_path);
            let same_exe = process
                .executable_path
                .eq_ignore_ascii_case(&fresh.executable_path);
            if !same_bundle && !same_exe {
                return Err(DriverError::AppNotFound);
            }
        }
        Ok(entry)
    }

    pub(super) fn validate_window(&mut self, win: &WindowRef) -> Result<()> {
        self.observation_gate()?;
        self.confirm_window(win, true)?;
        Ok(())
    }

    /// Window gate for a screenshot. Minimized windows are not restored.
    pub(super) fn prepare_capture(&mut self, win: &WindowRef) -> Result<SourceWindow> {
        let window = self.capture_window(win)?;
        if window.minimized {
            return Err(DriverError::WindowMinimized);
        }
        Ok(source_window(&window))
    }

    /// Window identity for targeted input. Minimized windows are not restored.
    pub(super) fn prepare_input(&mut self, win: &WindowRef) -> Result<crate::input::InputTarget> {
        self.observation_gate()?;
        let window = self.confirm_window(win, true)?;
        if window.minimized {
            return Err(DriverError::WindowMinimized);
        }
        if window.pid == 0 || window.cg_id == 0 {
            return Err(DriverError::WindowGone);
        }
        if !window.scale.is_finite() || window.scale <= 0.0 {
            return Err(DriverError::Native {
                message: "window scale is not usable".into(),
                maybe_executed: false,
            });
        }
        let pid = i32::try_from(window.pid).map_err(|_| DriverError::InvalidRequest)?;
        Ok(crate::input::InputTarget {
            pid,
            window_id: window.cg_id,
            bundle_id: win.app.info.id.clone(),
            app_name: win.app.info.name.clone(),
            origin_x: window.frame.x,
            origin_y: window.frame.y,
            width_px: window.frame.w * window.scale,
            height_px: window.frame.h * window.scale,
            scale: window.scale,
        })
    }

    /// Physical geometry, including a minimized window whose frame is known.
    pub(super) fn prepare_geometry(&mut self, win: &WindowRef) -> Result<Geometry> {
        let window = self.capture_window(win)?;
        let (width, height) = capture::physical_size(window.frame.w, window.frame.h, window.scale)
            .ok_or(DriverError::Native {
                message: "window frame is not a physical size".into(),
                maybe_executed: false,
            })?;
        Ok(capture::physical_geometry(
            window.frame,
            window.scale,
            width,
            height,
        ))
    }

    /// Accessibility-only gate plus the window identity. Screen Recording is
    /// not required to read the tree.
    pub(super) fn prepare_tree(&mut self, win: &WindowRef) -> Result<TreeTarget> {
        self.tree_gate()?;
        let window = self.confirm_window(win, false)?;
        Ok(TreeTarget {
            pid: window.pid,
            ax_index: window.ax_index,
            scale: window.scale,
            bundle_id: win.app.info.id.clone(),
            app_name: win.app.info.name.clone(),
        })
    }

    pub(super) fn validate_focus(&mut self, app: &AppRef) -> Result<()> {
        self.observation_gate()?;
        self.validate_app(app)?;
        Ok(())
    }

    pub(super) fn doctor_issues(&self) -> Result<(bool, Vec<String>)> {
        MacGate::engine(self.api.engine_euid()?)?;
        let accessibility = self.api.accessibility_trusted()?;
        let screen = self.api.screen_recording_ok()?;
        let mut issues = self.spotlight.issues();
        issues.extend(self.issues.clone());
        if !accessibility {
            issues.push("permission missing: accessibility".into());
        }
        if !screen {
            issues.push("permission missing: screen_recording".into());
        }
        Ok((accessibility && screen, issues))
    }

    fn capture_window(&mut self, win: &WindowRef) -> Result<crate::windows::AlignedWindow> {
        self.observation_gate()?;
        let resolved = self.validate_app(&win.app)?;
        check_window_pid(win, resolved.pid)?;
        let prior = self.windows.get(&win.id).cloned();
        if prior.is_none() {
            return Err(DriverError::WindowGone);
        }
        let current = self
            .collect_windows(resolved.pid)?
            .into_iter()
            .find(|window| window.id == win.id);
        if let Some(window) = current {
            if resolved.pid.is_some_and(|pid| pid != window.pid) {
                return Err(DriverError::AppNotFound);
            }
            if let Some(old) = prior {
                if window.pid != old.pid || window.cg_id != old.cg_id {
                    return Err(DriverError::WindowGone);
                }
            }
            return Ok(window);
        }
        let Some(old) = prior else {
            return Err(DriverError::WindowGone);
        };
        if resolved.pid.is_some_and(|pid| pid != old.pid) {
            return Err(DriverError::AppNotFound);
        }
        self.minimized_prior(old)
    }

    fn minimized_prior(
        &self,
        old: crate::windows::AlignedWindow,
    ) -> Result<crate::windows::AlignedWindow> {
        let Some(index) = old.ax_index else {
            return Err(DriverError::WindowGone);
        };
        let ax = self.api.ax_windows(old.pid)?;
        if ax.iter().any(|row| row.index == index && row.minimized) {
            let mut window = old;
            window.minimized = true;
            return Ok(window);
        }
        Err(DriverError::WindowGone)
    }

    fn confirm_window(
        &mut self,
        win: &WindowRef,
        require_prior: bool,
    ) -> Result<crate::windows::AlignedWindow> {
        let resolved = self.validate_app(&win.app)?;
        check_window_pid(win, resolved.pid)?;
        let prior = self.windows.get(&win.id).cloned();
        if require_prior && prior.is_none() {
            return Err(DriverError::WindowGone);
        }
        let current = self
            .collect_windows(resolved.pid)?
            .into_iter()
            .find(|window| window.id == win.id)
            .ok_or(DriverError::WindowGone)?;
        if resolved.pid.is_some_and(|pid| pid != current.pid) {
            return Err(DriverError::AppNotFound);
        }
        if let Some(old) = prior {
            if current.pid != old.pid || current.cg_id != old.cg_id {
                return Err(DriverError::WindowGone);
            }
        }
        Ok(current)
    }

    fn observation_gate(&self) -> Result<()> {
        MacGate::engine(self.api.engine_euid()?)?;
        MacGate::permissions(
            self.api.accessibility_trusted()?,
            self.api.screen_recording_ok()?,
        )?;
        self.prompt_gate()
    }

    fn tree_gate(&self) -> Result<()> {
        MacGate::engine(self.api.engine_euid()?)?;
        if !self.api.accessibility_trusted()? {
            return Err(DriverError::PermissionMissing {
                permission: "accessibility".into(),
            });
        }
        self.prompt_gate()
    }

    /// Semantic actions need the same Accessibility grant as a tree read.
    pub(super) fn action_gate(&self) -> Result<()> {
        self.tree_gate()
    }

    fn prompt_gate(&self) -> Result<()> {
        if gate::security_prompt(&self.api.running_apps()?, &self.api.windows()?) {
            return Err(DriverError::SecureDesktop);
        }
        Ok(())
    }

    fn collect_windows(&mut self, target_pid: Option<u32>) -> Result<Vec<AlignedWindow>> {
        let cg = self.api.windows()?;
        let mut by_pid: BTreeMap<u32, Vec<_>> = BTreeMap::new();
        for window in cg
            .into_iter()
            .filter(windows::is_listed)
            .filter(|window| target_pid.is_none_or(|pid| window.pid == pid))
        {
            by_pid.entry(window.pid).or_default().push(window);
        }
        let mut aligned = Vec::new();
        for (pid, rows) in by_pid {
            let ax = match self.api.ax_windows(pid) {
                Ok(ax) => ax,
                Err(DriverError::PermissionMissing { permission }) => {
                    return Err(DriverError::PermissionMissing { permission });
                }
                Err(error) => {
                    self.note(&error);
                    Vec::new()
                }
            };
            aligned.extend(align_pid(pid, &rows, &ax));
        }
        aligned.sort_by(|left, right| left.id.cmp(&right.id));
        self.windows
            .retain(|_, window| target_pid.is_some_and(|pid| window.pid != pid));
        self.windows.extend(
            aligned
                .iter()
                .map(|window| (window.id.clone(), window.clone())),
        );
        Ok(aligned)
    }

    fn windows_for(&mut self, pid: u32) -> Result<Vec<AlignedWindow>> {
        self.collect_windows(Some(pid))
    }

    fn note(&mut self, error: &DriverError) {
        self.issues.push(error.to_string());
        self.issues.truncate(8);
    }
}

fn check_window_pid(win: &WindowRef, pid: Option<u32>) -> Result<()> {
    let pid = pid.ok_or(DriverError::WindowGone)?;
    let window_pid = crate::ax::parse_window_pid(&win.id).ok_or(DriverError::WindowGone)?;
    if pid != window_pid {
        return Err(DriverError::AppNotFound);
    }
    Ok(())
}

fn direct_facts(
    api: &dyn MacApi,
    query: &str,
    running: &[(RunningApp, BundleFacts)],
    recent: &[(SpotlightHit, BundleFacts)],
) -> Result<Option<BundleFacts>> {
    let Ok(path) = crate::identity::normalize_path(query.trim()) else {
        return Ok(None);
    };
    let known = |facts: &BundleFacts| {
        facts.bundle_path.eq_ignore_ascii_case(&path)
            || facts.executable_path.eq_ignore_ascii_case(&path)
    };
    if running.iter().any(|(_, facts)| known(facts)) || recent.iter().any(|(_, facts)| known(facts))
    {
        return Ok(None);
    }
    match api.bundle_facts(&path) {
        Ok(facts) => Ok(Some(facts)),
        Err(DriverError::AppNotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Window selected for an accessibility walk.
///
/// Pid, AX index, and app identity are read by the macOS walk. Portable tests
/// only need `scale`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) struct TreeTarget {
    pub(super) pid: u32,
    pub(super) ax_index: Option<u32>,
    pub(super) scale: f64,
    pub(super) bundle_id: String,
    pub(super) app_name: String,
}

fn source_window(window: &crate::windows::AlignedWindow) -> SourceWindow {
    SourceWindow {
        cg_id: window.cg_id,
        frame: window.frame,
        scale: window.scale,
    }
}

fn fact_path<'a>(bundle: &'a str, executable: &'a str) -> Option<&'a str> {
    if !bundle.is_empty() {
        Some(bundle)
    } else if !executable.is_empty() {
        Some(executable)
    } else {
        None
    }
}

fn pick_key(windows: &[AlignedWindow]) -> Option<&AlignedWindow> {
    windows.iter().find(|window| window.main).or_else(|| {
        windows.iter().max_by(|left, right| {
            let left_area = left.frame.w * left.frame.h;
            let right_area = right.frame.w * right.frame.h;
            left_area
                .total_cmp(&right_area)
                .then(right.cg_id.cmp(&left.cg_id))
        })
    })
}
