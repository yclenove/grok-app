//! Discovery worker state. OS calls go through [`crate::api::MacApi`].

use crate::{
    api::{BundleFacts, MacApi, RunningApp, SpotlightHit},
    apps::{self, hit_could_match, list_catalog, resolve_loaded, ResolveOutcome, ResolvedApp},
    gate::{self, MacGate},
    identity::{identity_from_facts, needs_reauth},
    spotlight::SpotlightIndex,
    windows::{self, align_pid, AlignedWindow},
};
use gcu_core::driver::{AppQuery, AppRef, DriverError, Result, WindowRef};
use gcu_protocol::{AppInfo, WindowInfo};
use std::{collections::BTreeMap, sync::Arc, time::SystemTime};

pub(super) struct State {
    pub(super) api: Arc<dyn MacApi>,
    pub(super) spotlight: SpotlightIndex,
    entries: BTreeMap<String, ResolvedApp>,
    windows: BTreeMap<String, AlignedWindow>,
    pub(super) issues: Vec<String>,
}

impl State {
    pub(super) fn new(api: Arc<dyn MacApi>, spotlight: SpotlightIndex) -> Self {
        Self {
            api,
            spotlight,
            entries: BTreeMap::new(),
            windows: BTreeMap::new(),
            issues: Vec::new(),
        }
    }
}

impl State {
    pub(super) fn list(&mut self) -> Result<Vec<AppInfo>> {
        MacGate::engine(self.api.engine_euid()?)?;
        let running = self.running_resolved()?;
        let hits = self.spotlight.hits()?;
        Ok(list_catalog(&running, &hits, SystemTime::now()))
    }

    pub(super) fn resolve(&mut self, query: &AppQuery) -> Result<AppRef> {
        MacGate::engine(self.api.engine_euid()?)?;
        let now = SystemTime::now();
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
        let hits = self.spotlight.hits()?;
        let mut recent: Vec<(SpotlightHit, BundleFacts)> = Vec::new();
        for hit in hits {
            if running.iter().any(|(app, _)| {
                app.bundle_id.as_ref().is_some_and(|id| {
                    hit.bundle_id
                        .as_ref()
                        .is_some_and(|other| id.eq_ignore_ascii_case(other))
                }) || app.bundle_path.eq_ignore_ascii_case(&hit.path)
            }) {
                continue;
            }
            if !hit_could_match(&hit, &query.0) {
                continue;
            }
            recent.push((hit.clone(), self.api.bundle_facts(&hit.path)?));
        }
        let direct = direct_facts(self.api.as_ref(), &query.0, &running, &recent)?;
        match resolve_loaded(&running, &recent, direct.as_ref(), &query.0, now)? {
            ResolveOutcome::None => Err(DriverError::AppNotFound),
            ResolveOutcome::Ambiguous(_) => Err(DriverError::AmbiguousApp),
            ResolveOutcome::One(app) => {
                let app = *app;
                let info = app.info.clone();
                self.entries.insert(info.id.clone(), app);
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
            .collect_windows()?
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
            .get(&app.info.id)
            .cloned()
            .ok_or(DriverError::AppNotFound)?;
        if app.info.identity.as_ref() != Some(&entry.identity.app_identity()) {
            return Err(DriverError::AppNotFound);
        }
        let fresh = identity_from_facts(self.api.bundle_facts(&entry.identity.bundle_path)?)?;
        if needs_reauth(&entry.identity, &fresh)
            || fresh.app_identity() != entry.identity.app_identity()
        {
            self.entries.remove(&app.info.id);
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
        let resolved = self.validate_app(&win.app)?;
        let old = self
            .windows
            .get(&win.id)
            .cloned()
            .ok_or(DriverError::WindowGone)?;
        if resolved.pid.is_some_and(|pid| pid != old.pid) {
            return Err(DriverError::AppNotFound);
        }
        let current = self
            .collect_windows()?
            .into_iter()
            .find(|window| window.id == win.id)
            .ok_or(DriverError::WindowGone)?;
        if current.pid != old.pid || current.cg_id != old.cg_id {
            return Err(DriverError::WindowGone);
        }
        Ok(())
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

    fn running_resolved(&mut self) -> Result<Vec<ResolvedApp>> {
        let mut resolved = Vec::new();
        for app in self.api.running_apps()? {
            let Some(path) = fact_path(&app.bundle_path, &app.executable_path) else {
                continue;
            };
            let facts = match self.api.bundle_facts(path) {
                Ok(facts) => facts,
                Err(error) => {
                    self.note(&error);
                    continue;
                }
            };
            match apps::resolved_from(facts, Some(app.pid), app.hidden) {
                Ok(row) => {
                    self.entries.insert(row.info.id.clone(), row.clone());
                    resolved.push(row);
                }
                Err(error) => self.note(&error),
            }
        }
        Ok(resolved)
    }

    fn observation_gate(&self) -> Result<()> {
        MacGate::engine(self.api.engine_euid()?)?;
        MacGate::permissions(
            self.api.accessibility_trusted()?,
            self.api.screen_recording_ok()?,
        )?;
        if gate::security_prompt(&self.api.running_apps()?, &self.api.windows()?) {
            return Err(DriverError::SecureDesktop);
        }
        Ok(())
    }

    fn collect_windows(&mut self) -> Result<Vec<AlignedWindow>> {
        let cg = self.api.windows()?;
        let mut by_pid: BTreeMap<u32, Vec<_>> = BTreeMap::new();
        for window in cg.into_iter().filter(windows::is_listed) {
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
        self.windows = aligned
            .iter()
            .map(|window| (window.id.clone(), window.clone()))
            .collect();
        Ok(aligned)
    }

    fn windows_for(&mut self, pid: u32) -> Result<Vec<AlignedWindow>> {
        Ok(self
            .collect_windows()?
            .into_iter()
            .filter(|window| window.pid == pid)
            .collect())
    }

    fn note(&mut self, error: &DriverError) {
        self.issues.push(error.to_string());
        self.issues.truncate(8);
    }
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
