//! Pure discovery registry; native calls are injected through WinApi.
use crate::{
    api::{NativeWindow, Shortcut, WinApi},
    apps::AppIndex,
    desktop::{visible_windows, window_process, HwndStamp},
    gate::{check_engine, WinGate},
    identity::{identity_for_path, WindowsIdentity},
};
use gcu_core::driver::*;
use gcu_protocol::{AppInfo, WindowInfo};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone)]
pub(super) struct Entry {
    pub(super) info: AppInfo,
    pub(super) identity: WindowsIdentity,
    pub(super) shortcut: Option<Shortcut>,
}
pub(super) struct State {
    pub(super) _thread_context: Box<dyn std::any::Any>,
    pub(super) api: Arc<dyn WinApi>,
    pub(super) index: AppIndex,
    pub(super) entries: BTreeMap<String, Entry>,
    pub(super) issues: Vec<String>,
    pub(super) windows: BTreeMap<String, (NativeWindow, u32, u64, WindowsIdentity)>,
}
impl State {
    pub(super) fn check(&self) -> Result<()> {
        check_engine(&self.api.engine_security()?)
    }
    pub(super) fn enumerate(&mut self) -> Result<Vec<(WindowInfo, String)>> {
        self.check()?;
        let mut result = Vec::new();
        for window in visible_windows(self.api.enum_windows()?) {
            let pid = window_process(self.api.as_ref(), &window)?;
            let identity = identity_for_path(self.api.as_ref(), &self.api.process_path(pid)?)?;
            let info = AppInfo {
                id: identity.path.clone(),
                name: executable_name(&identity.path),
                pid: Some(pid),
                identity: Some(identity.app_identity()),
            };
            self.entries.insert(
                info.id.clone(),
                Entry {
                    info: info.clone(),
                    identity: identity.clone(),
                    shortcut: None,
                },
            );
            let target_created = self.api.process_security(pid)?.created;
            let id = HwndStamp::from(&window).id();
            result.push((
                WindowInfo {
                    id: id.clone(),
                    title: window.title.clone(),
                    frame: window.frame,
                    scale: window.scale,
                    on_current_desktop: self.api.on_current_desktop(window.hwnd)?,
                },
                info.id,
            ));
            self.windows
                .insert(id, (window, pid, target_created, identity));
        }
        // Keep only live stamps; an absent exact ID is never re-resolved by title.
        self.windows
            .retain(|id, _| result.iter().any(|(w, _)| &w.id == id));
        Ok(result)
    }
    pub(super) fn list(&mut self) -> Result<Vec<AppInfo>> {
        self.check()?;
        let mut apps = BTreeMap::new();
        for (_, pid, _, id) in self.windows.values() {
            if let Some(entry) = self.entries.get(&id.path) {
                let mut info = entry.info.clone();
                info.pid = Some(*pid);
                apps.insert(info.id.clone(), info);
            }
        }
        for shortcut in self.index.shortcuts()? {
            let id = shortcut
                .aumid
                .clone()
                .unwrap_or_else(|| shortcut.target.to_lowercase());
            apps.entry(id.clone()).or_insert(AppInfo {
                id,
                name: shortcut.name,
                pid: None,
                identity: None,
            });
        }
        Ok(apps.into_values().collect())
    }
    pub(super) fn resolve(&mut self, query: &AppQuery) -> Result<AppRef> {
        self.enumerate()?;
        let query = query.0.to_lowercase().replace('/', "\\");
        let mut matches = BTreeMap::new();
        for (_, pid, _, identity) in self.windows.values() {
            let entry = self
                .entries
                .get(&identity.path)
                .ok_or(DriverError::AppNotFound)?;
            if matches_query(&entry.info, &query) {
                let mut entry = entry.clone();
                entry.info.pid = Some(*pid);
                matches.insert(identity.path.clone(), entry);
            }
        }
        for shortcut in self.index.shortcuts()? {
            if shortcut.name.to_lowercase() == query
                || shortcut.target.to_lowercase() == query
                || shortcut
                    .aumid
                    .as_ref()
                    .is_some_and(|id| id.to_lowercase() == query)
                || executable_name(&shortcut.target).to_lowercase() == query
            {
                let actual = self.api.read_link(&shortcut.link)?;
                let identity = identity_for_path(self.api.as_ref(), &actual.target)?;
                let info = AppInfo {
                    id: identity.path.clone(),
                    name: actual.name.clone(),
                    pid: None,
                    identity: Some(identity.app_identity()),
                };
                matches.entry(identity.path.clone()).or_insert(Entry {
                    info,
                    identity,
                    shortcut: Some(actual),
                });
            }
        }
        if crate::identity::normalize_path(&query).is_ok() && query.ends_with(".exe") {
            let identity = identity_for_path(self.api.as_ref(), &query)?;
            let info = AppInfo {
                id: identity.path.clone(),
                name: executable_name(&identity.path),
                pid: None,
                identity: Some(identity.app_identity()),
            };
            matches.entry(identity.path.clone()).or_insert(Entry {
                info: info.clone(),
                identity,
                shortcut: Some(Shortcut {
                    link: String::new(),
                    name: info.name,
                    target: info.id,
                    aumid: None,
                }),
            });
        }
        match matches.len() {
            0 => Err(DriverError::AppNotFound),
            1 => {
                let entry = matches
                    .into_values()
                    .next()
                    .ok_or(DriverError::AppNotFound)?;
                self.entries.insert(entry.info.id.clone(), entry.clone());
                Ok(AppRef { info: entry.info })
            }
            _ => Err(DriverError::AmbiguousApp),
        }
    }
    pub(super) fn validate_app(&self, app: &AppRef) -> Result<Entry> {
        self.check()?;
        let entry = self
            .entries
            .get(&app.info.id)
            .ok_or(DriverError::AppNotFound)?;
        if app.info.identity.as_ref() != Some(&entry.identity.app_identity()) {
            return Err(DriverError::AppNotFound);
        }
        let fresh = identity_for_path(self.api.as_ref(), &entry.identity.path)?;
        if fresh.app_identity() != entry.identity.app_identity() {
            return Err(DriverError::AppNotFound);
        }
        if let Some(pid) = app.info.pid {
            WinGate.check(self.api.as_ref(), pid, false, None)?;
            let process = identity_for_path(self.api.as_ref(), &self.api.process_path(pid)?)?;
            if process != fresh {
                return Err(DriverError::AppNotFound);
            }
        }
        Ok(entry.clone())
    }
    pub(super) fn validate_window(&self, win: &WindowRef, foreground: bool) -> Result<()> {
        self.observation_window(win, foreground).map(|_| ())
    }
    pub(super) fn observation_window(
        &self,
        win: &WindowRef,
        foreground: bool,
    ) -> Result<NativeWindow> {
        self.check()?;
        let (old, pid, target_created, identity) =
            self.windows.get(&win.id).ok_or(DriverError::WindowGone)?;
        if win.app.info.id != identity.path
            || win.app.info.identity.as_ref() != Some(&identity.app_identity())
        {
            return Err(DriverError::AppNotFound);
        }
        let current = self
            .api
            .enum_windows()?
            .into_iter()
            .find(|w| w.hwnd == old.hwnd)
            .ok_or(DriverError::WindowGone)?;
        HwndStamp::from(old).validate(&current)?;
        if window_process(self.api.as_ref(), &current)? != *pid {
            return Err(DriverError::WindowGone);
        }
        let desktop = self.api.on_current_desktop(old.hwnd)?;
        if *pid != old.pid {
            WinGate.check(self.api.as_ref(), old.pid, foreground, desktop)?;
        }
        let security = WinGate.check(self.api.as_ref(), *pid, foreground, desktop)?;
        if security.created != *target_created {
            return Err(DriverError::WindowGone);
        }
        let fresh = identity_for_path(self.api.as_ref(), &self.api.process_path(*pid)?)?;
        if fresh != *identity {
            return Err(DriverError::AppNotFound);
        }
        Ok(current)
    }
}
fn executable_name(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .trim_end_matches(".exe")
        .to_owned()
}
fn matches_query(info: &AppInfo, query: &str) -> bool {
    info.id.to_lowercase() == query
        || info.name.to_lowercase() == query
        || info
            .identity
            .as_ref()
            .and_then(|i| i.executable.as_ref())
            .is_some_and(|p| p.rsplit('\\').next().is_some_and(|p| p == query))
}
