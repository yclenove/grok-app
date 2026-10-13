//! Capture kernel process identity before dispatch; never infer death from errors.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::PathBuf;
use std::sync::Arc;
use windows::Win32::Foundation::{
    ERROR_INVALID_PARAMETER, FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE,
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::computer_use::webview::download::windows) struct Identity {
    pub pid: u32,
    pub created: u64,
}

pub(in crate::computer_use::webview::download::windows) struct Witness {
    pub identity: Identity,
    handle: OwnedHandle,
}

impl Witness {
    pub fn capture(pid: u32) -> Result<Arc<Self>, String> {
        if pid == 0 {
            return Err("native download process identity unavailable".into());
        }
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
        }
        .map_err(|_| "native download process capture unavailable")?;
        let handle = unsafe { OwnedHandle::from_raw_handle(handle.0) };
        let raw = HANDLE(handle.as_raw_handle());
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        unsafe { GetProcessTimes(raw, &mut created, &mut exited, &mut kernel, &mut user) }
            .map_err(|_| "native download process birth unavailable")?;
        let witness = Arc::new(Self {
            identity: Identity {
                pid,
                created: (u64::from(created.dwHighDateTime) << 32)
                    | u64::from(created.dwLowDateTime),
            },
            handle,
        });
        if !witness.live() || witness.identity.created == 0 {
            return Err("native download process retired during capture".into());
        }
        Ok(witness)
    }

    pub fn live(&self) -> bool {
        unsafe { WaitForSingleObject(HANDLE(self.handle.as_raw_handle()), 0) == WAIT_TIMEOUT }
    }

    pub fn terminated(&self) -> bool {
        unsafe { WaitForSingleObject(HANDLE(self.handle.as_raw_handle()), 0) == WAIT_OBJECT_0 }
    }
}

impl Identity {
    pub fn valid(&self) -> bool {
        self.pid > 0 && self.created > 0
    }

    pub fn retired(&self) -> Result<bool, String> {
        if !self.valid() {
            return Err("native download invalid process identity".into());
        }
        let handle = match unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                self.pid,
            )
        } {
            Ok(handle) => unsafe { OwnedHandle::from_raw_handle(handle.0) },
            Err(error) if error.code() == ERROR_INVALID_PARAMETER.to_hresult() => return Ok(true),
            Err(_) => return Err("native download process query unavailable".into()),
        };
        let raw = HANDLE(handle.as_raw_handle());
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        unsafe { GetProcessTimes(raw, &mut created, &mut exited, &mut kernel, &mut user) }
            .map_err(|_| "native download process birth unavailable")?;
        let birth = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
        if birth == 0 {
            return Err("native download process birth unavailable".into());
        }
        if birth != self.created {
            return Ok(true);
        }
        match unsafe { WaitForSingleObject(raw, 0) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err("native download process query unavailable".into()),
        }
    }
}

#[derive(Clone)]
pub(in crate::computer_use::webview::download::windows) struct Owners {
    pub app_root: PathBuf,
    pub app: Arc<Witness>,
    pub browser: Arc<Witness>,
}

impl Owners {
    pub fn capture(browser_pid: u32, app_root: PathBuf) -> Result<Self, String> {
        Ok(Self {
            app_root,
            app: Witness::capture(std::process::id())?,
            browser: Witness::capture(browser_pid)?,
        })
    }

    pub fn live(&self) -> bool {
        self.app.live() && self.browser.live()
    }
}
