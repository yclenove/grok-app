//! Start Menu/AppsFolder indexing, shell-link parsing and explicit launching.
use super::{allocated_string, io, native, packages, string, wide, Com, Handle};
use crate::api::{Shortcut, WinApi};
use ::windows::{
    core::{Interface, PCWSTR},
    Win32::{
        Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
        Storage::FileSystem::*,
        System::{
            Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ},
            Threading::*,
        },
        UI::{
            Shell::{PropertiesSystem::PSGetPropertyKeyFromName, *},
            WindowsAndMessaging::SW_SHOWNOACTIVATE,
        },
    },
};
use gcu_core::driver::{DriverError, Result};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

fn roots() -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    for folder in [&FOLDERID_Programs, &FOLDERID_CommonPrograms] {
        // SAFETY: returned string is CoTaskMem owned; copied and freed immediately.
        let path = unsafe { SHGetKnownFolderPath(folder, KF_FLAG_DEFAULT, None).map_err(native)? };
        result.push(PathBuf::from(allocated_string(path)?));
    }
    Ok(result)
}
fn property(item: &IShellItem2, name: &str) -> Result<Option<String>> {
    let name = wide(name);
    // SAFETY: property name is null-terminated; COM apartment initialized by caller.
    let mut key = ::windows::Win32::Foundation::PROPERTYKEY::default();
    // SAFETY: writable property key output and owned name buffer.
    unsafe {
        PSGetPropertyKeyFromName(PCWSTR(name.as_ptr()), &mut key).map_err(native)?;
    }
    // SAFETY: shell returns an owned string, or an absent property.
    match unsafe { item.GetString(&key) } {
        Ok(value) => Ok(Some(allocated_string(value)?)),
        Err(_) => Ok(None), // property absence is normal for Win32 shell items
    }
}
fn app_item(aumid: &str) -> Result<IShellItem2> {
    let id = wide(aumid);
    // SAFETY: AppsFolder parses the registered identifier, not a command line.
    unsafe {
        SHCreateItemInKnownFolder(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, PCWSTR(id.as_ptr()))
            .map_err(native)
    }
}
fn packaged(aumid: &str) -> Result<Shortcut> {
    let item = app_item(aumid)?;
    if property(&item, "System.AppUserModel.ID")?.as_deref() != Some(aumid) {
        return Err(DriverError::AppNotFound);
    }
    // SAFETY: COM shell item and apartment are valid; display string is owned.
    let name =
        allocated_string(unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY).map_err(native)? })?;
    Ok(Shortcut {
        link: format!("gcu-aumid:{aumid}"),
        name,
        target: packages::executable(aumid)?,
        aumid: Some(aumid.into()),
    })
}
pub(super) fn read_link(path: &str) -> Result<Shortcut> {
    let _com = Com::new()?;
    if let Some(aumid) = path.strip_prefix("gcu-aumid:") {
        return packaged(aumid);
    }
    // Cache content is untrusted; accept .lnk files only within canonical Start
    // Menu roots, excluding paths replaced by a junction into another directory.
    let canonical = std::fs::canonicalize(path).map_err(io)?;
    let mut allowed = false;
    for root in roots()? {
        if canonical.starts_with(std::fs::canonicalize(root).map_err(io)?) {
            allowed = true;
        }
    }
    if !allowed
        || canonical
            .extension()
            .is_none_or(|e| !e.eq_ignore_ascii_case("lnk"))
    {
        return Err(DriverError::InvalidRequest);
    }
    let path_w = wide(&canonical.to_string_lossy());
    // SAFETY: all COM interfaces stay on this worker, .lnk is loaded read-only.
    unsafe {
        let link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(native)?;
        let persist: IPersistFile = link.cast().map_err(native)?;
        persist
            .Load(PCWSTR(path_w.as_ptr()), STGM_READ)
            .map_err(native)?;
        let mut buffer = vec![0u16; 32768];
        let mut find = WIN32_FIND_DATAW::default();
        link.GetPath(&mut buffer, &mut find, 0).map_err(native)?;
        let mut target = string(&buffer);
        let mut aumid = None;
        let pidl = link.GetIDList().map_err(native)?;
        if !pidl.is_null() {
            let item: ::windows::core::Result<IShellItem2> = SHCreateItemFromIDList(pidl);
            ::windows::Win32::System::Com::CoTaskMemFree(Some(pidl.cast()));
            if let Ok(item) = item {
                aumid = property(&item, "System.AppUserModel.ID")?.filter(|id| id.contains('!'));
            }
        }
        if let Some(id) = &aumid {
            target = packages::executable(id)?;
        }
        if target.is_empty() || !target.to_lowercase().ends_with(".exe") {
            return Err(DriverError::AppNotFound);
        }
        Ok(Shortcut {
            link: canonical.to_string_lossy().into_owned(),
            name: canonical
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            target,
            aumid,
        })
    }
}
fn walk(root: &Path, depth: usize, result: &mut Vec<String>) -> Result<()> {
    if depth > 32 || result.len() >= 10_000 {
        return Err(DriverError::InvalidRequest);
    }
    for entry in std::fs::read_dir(root).map_err(io)? {
        let entry = entry.map_err(io)?;
        let kind = entry.file_type().map_err(io)?;
        if kind.is_dir() {
            walk(&entry.path(), depth + 1, result)?;
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
        {
            result.push(entry.path().to_string_lossy().into_owned());
        }
    }
    Ok(())
}
pub(super) fn links() -> Result<Vec<String>> {
    let _com = Com::new()?;
    let mut result = Vec::new();
    for root in roots()? {
        walk(&root, 0, &mut result)?;
    }
    // Settings and modern Notepad may only be registered in AppsFolder.
    // SAFETY: enumeration and all shell objects remain on the index apartment.
    unsafe {
        let folder: IShellItem =
            SHCreateItemInKnownFolder(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, PCWSTR::null())
                .map_err(native)?;
        let enumeration: IEnumShellItems = folder
            .BindToHandler(None, &BHID_EnumItems)
            .map_err(native)?;
        loop {
            let mut items = [None];
            let mut fetched = 0;
            enumeration
                .Next(&mut items, Some(&mut fetched))
                .map_err(native)?;
            if fetched == 0 {
                break;
            }
            if let Some(item) = items[0].take() {
                let item: IShellItem2 = item.cast().map_err(native)?;
                if let Some(aumid) =
                    property(&item, "System.AppUserModel.ID")?.filter(|id| id.contains('!'))
                {
                    result.push(format!("gcu-aumid:{aumid}"));
                }
            }
        }
    }
    Ok(result)
}
/// Persistent change handles represented as integers; kernel wait handles may
/// cross threads but are only waited/reset by the index worker.
pub(super) struct WatchSet(Vec<usize>);
impl Drop for WatchSet {
    fn drop(&mut self) {
        for handle in &self.0 {
            // SAFETY: owned notification handle, closed once with its matching API.
            unsafe {
                let _ = FindCloseChangeNotification(HANDLE(*handle as *mut _));
            }
        }
    }
}
pub(super) fn changed(watcher: &Mutex<Option<WatchSet>>, timeout: Duration) -> Result<bool> {
    let mut watcher = watcher.lock().map_err(|_| DriverError::Crashed {
        maybe_executed: false,
    })?;
    if watcher.is_none() {
        let mut handles = WatchSet(Vec::new());
        for root in roots()? {
            let path = wide(&root.to_string_lossy());
            // SAFETY: null-terminated root path; handle retained until adapter drop.
            let handle = unsafe {
                FindFirstChangeNotificationW(
                    PCWSTR(path.as_ptr()),
                    true,
                    FILE_NOTIFY_CHANGE_FILE_NAME
                        | FILE_NOTIFY_CHANGE_DIR_NAME
                        | FILE_NOTIFY_CHANGE_LAST_WRITE,
                )
                .map_err(native)?
            };
            handles.0.push(handle.0 as usize);
        }
        *watcher = Some(handles);
    }
    let handles: Vec<_> = watcher
        .as_ref()
        .ok_or(DriverError::InvalidRequest)?
        .0
        .iter()
        .map(|h| HANDLE(*h as *mut _))
        .collect();
    // SAFETY: persistent valid handles; wait is bounded by the requested timeout.
    let result =
        unsafe { WaitForMultipleObjects(&handles, false, timeout.as_millis().min(250) as u32) };
    if result == WAIT_TIMEOUT {
        return Ok(false);
    }
    let index = result.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
    let handle = handles
        .get(index)
        .ok_or_else(|| native("notification wait"))?;
    // SAFETY: reset only the signaled, valid persistent notification handle.
    unsafe {
        FindNextChangeNotification(*handle).map_err(native)?;
    }
    Ok(true)
}
pub(super) fn launch(
    api: &super::RealWinApi,
    shortcut: &Shortcut,
    expected: &crate::identity::WindowsIdentity,
) -> Result<()> {
    let _com = Com::new()?;
    use std::os::windows::fs::OpenOptionsExt;
    // Keep the expected entry file immutable through native launch/activation.
    let _file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(&shortcut.target)
        .map_err(io)?;
    if crate::identity::identity_for_path(api, &shortcut.target)? != *expected {
        return Err(DriverError::AppNotFound);
    }
    crate::gate::check_engine(&api.engine_security()?)?;
    if let Some(aumid) = &shortcut.aumid {
        if packages::executable(aumid)?.to_lowercase() != shortcut.target.to_lowercase() {
            return Err(DriverError::AppNotFound);
        }
        let id = wide(aumid);
        // SAFETY: authorized registered AUMID, no caller-controlled arguments/UI.
        unsafe {
            let manager: IApplicationActivationManager =
                CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_INPROC_SERVER)
                    .map_err(native)?;
            manager
                .ActivateApplication(
                    PCWSTR(id.as_ptr()),
                    PCWSTR::null(),
                    AO_NOERRORUI | AO_NOSPLASHSCREEN,
                )
                .map_err(|_| DriverError::Native {
                    message: "Packaged app launch failed".into(),
                    maybe_executed: true,
                })?;
        }
    } else {
        // Hold the verified executable against write/delete replacement through
        // CreateProcess; explicit lpApplicationName prevents path search hijacks.
        let path = wide(&expected.path);
        let startup = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            dwFlags: STARTF_USESHOWWINDOW,
            wShowWindow: SW_SHOWNOACTIVATE.0 as u16,
            ..Default::default()
        };
        let mut process = PROCESS_INFORMATION::default();
        // SAFETY: explicit verified executable, no inherited handles/command string.
        unsafe {
            CreateProcessW(
                PCWSTR(path.as_ptr()),
                None,
                None,
                None,
                false,
                PROCESS_CREATION_FLAGS(0),
                None,
                PCWSTR::null(),
                &startup,
                &mut process,
            )
            .map_err(native)?;
        }
        let _process = Handle(process.hProcess);
        let _thread = Handle(process.hThread);
    }
    Ok(())
}
