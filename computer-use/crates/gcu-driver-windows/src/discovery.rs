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
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub(super) struct Entry {
    pub(super) info: AppInfo,
    pub(super) identity: WindowsIdentity,
    pub(super) shortcut: Option<Shortcut>,
}
pub(super) struct State {
    pub(super) _thread_context: Box<dyn std::any::Any>,
    pub(super) api: Arc<dyn WinApi>,
    pub(super) index: Arc<AppIndex>,
    pub(super) entries: Arc<Mutex<BTreeMap<String, Entry>>>,
    pub(super) issues: Vec<String>,
    pub(super) windows: BTreeMap<String, (NativeWindow, u32, u64, WindowsIdentity)>,
}
impl State {
    pub(super) fn check(&self) -> Result<()> {
        check_engine(&self.api.engine_security()?)
    }
    /// Window enumeration never verifies unrelated executables. Bound entries
    /// are hints here; observation_window still revalidates the exact image.
    pub(super) fn enumerate(
        &mut self,
        target: Option<&AppRef>,
    ) -> Result<Vec<(WindowInfo, String)>> {
        let _timing = crate::timing::Timing::start("windows_metadata");
        self.check()?;
        let entries = self.entries.lock().map_err(|_| crashed())?.clone();
        let mut result = Vec::new();
        for window in visible_windows(self.api.enum_windows()?) {
            let found = (|| {
                let pid = window_process(self.api.as_ref(), &window)?;
                let path = crate::identity::normalize_path(&self.api.process_path(pid)?)?;
                Ok((pid, path))
            })();
            let (pid, path) = match found {
                Ok(found) => found,
                Err(error) => {
                    self.note(error);
                    continue;
                }
            };
            if target.is_some_and(|app| app.info.id != path || app.info.pid != Some(pid)) {
                continue;
            }
            let id = HwndStamp::from(&window).id();
            result.push((
                WindowInfo {
                    id: id.clone(),
                    title: window.title.clone(),
                    frame: window.frame,
                    scale: window.scale,
                    on_current_desktop: self.api.on_current_desktop(window.hwnd)?,
                },
                path.clone(),
            ));
            if let Some(entry) = entries.get(&path) {
                let target_created = self.api.process_security(pid)?.created;
                self.windows
                    .insert(id, (window, pid, target_created, entry.identity.clone()));
            }
        }
        self.windows.retain(|id, (_, pid, _, identity)| {
            // A filtered enumeration must not invalidate other apps' windows.
            target.is_some_and(|app| app.info.id != identity.path || app.info.pid != Some(*pid))
                || result.iter().any(|(window, _)| &window.id == id)
        });
        Ok(result)
    }
    /// Resolve only matching live images. Start Menu/registry scans and unrelated
    /// signatures never occur on this queue; cached shortcuts only detect ambiguity.
    pub(super) fn resolve_running(&mut self, query: &AppQuery) -> Result<Option<AppRef>> {
        let _timing = crate::timing::Timing::start("resolve_running");
        self.check()?;
        let query = query.0.to_lowercase().replace('/', "\\");
        let mut candidates = BTreeMap::new();
        let matching = crate::timing::Timing::start("resolve_process_match");
        for window in visible_windows(self.api.enum_windows()?) {
            let candidate = (|| {
                let pid = window_process(self.api.as_ref(), &window)?;
                let path = crate::identity::normalize_path(&self.api.process_path(pid)?)?;
                let matches = matches_path(&path, &query)
                    || self
                        .api
                        .process_aumid(pid)?
                        .is_some_and(|id| id.eq_ignore_ascii_case(&query));
                Ok((pid, path, matches))
            })();
            match candidate {
                Ok((pid, path, true)) => {
                    candidates.insert(path, pid);
                }
                Ok(_) => (),
                Err(error) => self.note(error),
            }
        }
        drop(matching);
        if candidates.is_empty() {
            return Ok(None);
        }
        // Cached hints retain ambiguity without opening unrelated files/links.
        for shortcut in self.index.shortcuts()? {
            if matches_shortcut(&shortcut, &query)
                && !candidates
                    .keys()
                    .any(|p| p.eq_ignore_ascii_case(&shortcut.target))
            {
                return Err(DriverError::AmbiguousApp);
            }
        }
        if candidates.len() != 1 {
            return Err(DriverError::AmbiguousApp);
        }
        let (path, pid) = candidates
            .into_iter()
            .next()
            .ok_or(DriverError::AppNotFound)?;
        // Refuse a visible High target before opening its High-labelled image.
        // File verification can report NotFound/AccessDenied for that image;
        // those errors must never hide the known integrity refusal.
        let before = WinGate.check(self.api.as_ref(), pid, false, None)?;
        let identity = {
            let _timing = crate::timing::Timing::start("resolve_identity");
            identity_for_path(self.api.as_ref(), &path)?
        };
        let _binding = crate::timing::Timing::start("resolve_process_binding");
        // Bind a process instance only if it still names the verified image.
        if self.api.process_security(pid)?.created != before.created
            || crate::identity::normalize_path(&self.api.process_path(pid)?)? != identity.path
        {
            return Err(DriverError::AppNotFound);
        }
        self.bind(Entry {
            info: AppInfo {
                id: identity.path.clone(),
                name: executable_name(&identity.path),
                pid: Some(pid),
                identity: Some(identity.app_identity()),
            },
            identity,
            shortcut: None,
        })
        .map(Some)
    }
    /// Installed/direct-path verification has a separate queue from live lookup.
    pub(super) fn resolve(&mut self, query: &AppQuery) -> Result<AppRef> {
        let _timing = crate::timing::Timing::start("resolve_installed");
        let query = query.0.to_lowercase().replace('/', "\\");
        self.check()?;
        let mut matches = BTreeMap::new();
        for shortcut in self.index.shortcuts()? {
            if matches_shortcut(&shortcut, &query) {
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
            1 => self.bind(
                matches
                    .into_values()
                    .next()
                    .ok_or(DriverError::AppNotFound)?,
            ),
            _ => Err(DriverError::AmbiguousApp),
        }
    }
    fn bind(&self, entry: Entry) -> Result<AppRef> {
        self.entries
            .lock()
            .map_err(|_| crashed())?
            .insert(entry.info.id.clone(), entry.clone());
        Ok(AppRef { info: entry.info })
    }
    fn note(&mut self, error: DriverError) {
        super::discovery_workers::note_window(&mut self.issues, error);
    }
    pub(super) fn validate_app(&self, app: &AppRef) -> Result<Entry> {
        let _timing = crate::timing::Timing::start("validate_app");
        self.check()?;
        let entry = self
            .entries
            .lock()
            .map_err(|_| crashed())?
            .get(&app.info.id)
            .cloned()
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
            // The exact canonical file was just verified above. Only an alias
            // needs another canonical-file comparison; never hash the same path twice.
            let process = crate::identity::normalize_path(&self.api.process_path(pid)?)?;
            if process != fresh.path && identity_for_path(self.api.as_ref(), &process)? != fresh {
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
        let current = self.metadata_window(win, foreground)?;
        let (_, pid, _, identity) = self.windows.get(&win.id).ok_or(DriverError::WindowGone)?;
        let fresh = identity_for_path(self.api.as_ref(), &self.api.process_path(*pid)?)?;
        if fresh != *identity {
            return Err(DriverError::AppNotFound);
        }
        Ok(current)
    }
    // Win32 metadata only. Used after this call's validate_app or as the first
    // half of full observation; never an authorization gate for content/input.
    pub(super) fn metadata_window(
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
        let path = crate::identity::normalize_path(&self.api.process_path(*pid)?)?;
        if path != identity.path && identity_for_path(self.api.as_ref(), &path)? != *identity {
            return Err(DriverError::AppNotFound);
        }
        Ok(current)
    }
    /// Re-prove the bound fixture instance immediately before content is returned
    /// or input is delivered. Call only after [`Self::observation_window`] or
    /// [`Self::metadata_window`], so elevation and secure-desktop errors keep
    /// their priority. A miss is [`DriverError::ScopeDenied`] and does not select
    /// another window. The image path is not an input.
    pub(super) fn scope_close(&self, win: &WindowRef) -> Result<()> {
        let (old, pid, target_created, _) =
            self.windows.get(&win.id).ok_or(DriverError::ScopeDenied)?;
        let boot = self.api.boot_identity()?;
        let current = self
            .api
            .enum_windows()?
            .into_iter()
            .find(|item| item.hwnd == old.hwnd)
            .ok_or(DriverError::ScopeDenied)?;
        let owner = match window_process(self.api.as_ref(), &current) {
            Ok(owner) => owner,
            Err(
                DriverError::WindowGone
                | DriverError::AppNotFound
                | DriverError::AmbiguousApp
                | DriverError::PermissionMissing { .. },
            ) => return Err(DriverError::ScopeDenied),
            Err(error) => return Err(error),
        };
        let created = match self.api.process_security(owner) {
            Ok(security) => security.created,
            Err(DriverError::WindowGone | DriverError::PermissionMissing { .. }) => {
                return Err(DriverError::ScopeDenied);
            }
            Err(error) => return Err(error),
        };
        let stored = crate::scope_identity::WindowClaim {
            hwnd: old.hwnd,
            incarnation: old.created,
            owner: crate::scope_identity::ProcessClaim {
                pid: *pid,
                created: *target_created,
                boot: boot.clone(),
            },
        };
        let live = crate::scope_identity::WindowClaim {
            hwnd: current.hwnd,
            incarnation: current.created,
            owner: crate::scope_identity::ProcessClaim {
                pid: owner,
                created,
                boot,
            },
        };
        crate::scope_identity::prove_window(&stored, &live)
    }
}
pub(super) fn executable_name(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .trim_end_matches(".exe")
        .to_owned()
}
fn matches_path(path: &str, query: &str) -> bool {
    let name = executable_name(path);
    path.eq_ignore_ascii_case(query)
        || name.eq_ignore_ascii_case(query)
        || path.rsplit('\\').next().is_some_and(|file| file.eq_ignore_ascii_case(query))
        // A short executable-name alias (e.g. fixture -> gcu-fixture). Ambiguous
        // aliases fail closed; verified path/signature remain the grant identity.
        || name.rsplit('-').next().is_some_and(|part| part.eq_ignore_ascii_case(query))
}
fn matches_shortcut(shortcut: &Shortcut, query: &str) -> bool {
    shortcut.name.eq_ignore_ascii_case(query)
        || matches_path(&shortcut.target.to_lowercase(), query)
        || shortcut
            .aumid
            .as_ref()
            .is_some_and(|id| id.eq_ignore_ascii_case(query))
}
fn crashed() -> DriverError {
    DriverError::Crashed {
        maybe_executed: false,
    }
}
