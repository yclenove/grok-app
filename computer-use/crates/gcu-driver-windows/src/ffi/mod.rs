//! Native bindings and owned Win32 resources; compiled only on Windows.
mod packages;
mod security;
mod shell;
mod signature;
mod windows;
use crate::{
    api::{ImageInfo, NativeWindow, Shortcut, WinApi},
    gate::ProcessSecurity,
};
use ::windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED},
            Threading::GetCurrentProcessId,
        },
    },
};
use gcu_core::driver::{DriverError, Result};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

pub(super) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
pub(super) fn string(value: &[u16]) -> String {
    String::from_utf16_lossy(&value[..value.iter().position(|x| *x == 0).unwrap_or(value.len())])
}
pub(super) fn native(_: impl std::fmt::Display) -> DriverError {
    DriverError::Native {
        message: "Windows discovery API failed".into(),
        maybe_executed: false,
    }
}
pub(super) fn io(error: std::io::Error) -> DriverError {
    native(error.kind())
}
pub(super) struct Handle(pub HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: constructed only from an owned valid kernel handle, closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
pub(super) struct Com {
    _dpi: DpiContext,
}
impl Com {
    pub fn new() -> Result<Self> {
        let dpi = DpiContext::new()?;
        // SAFETY: worker thread initializes its own apartment; balanced in Drop.
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(native)?;
        }
        Ok(Self { _dpi: dpi })
    }
}
pub(crate) struct DpiContext(::windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT);
impl DpiContext {
    pub(crate) fn new() -> Result<Self> {
        use ::windows::Win32::UI::HiDpi::{
            SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        // SAFETY: affects only the dedicated caller thread; previous context is
        // retained and restored on that same thread in Drop.
        let previous =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        if previous.0.is_null() {
            return Err(native("DPI context unavailable"));
        }
        Ok(Self(previous))
    }
}
impl Drop for DpiContext {
    fn drop(&mut self) {
        // SAFETY: restores the context returned by this thread's successful set.
        unsafe {
            ::windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(self.0);
        }
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        // SAFETY: balances exactly one successful CoInitializeEx on this thread.
        unsafe {
            CoUninitialize();
        }
    }
}
pub(super) fn allocated_string(value: PWSTR) -> Result<String> {
    // SAFETY: shell API returned a null-terminated CoTaskMemAlloc string.
    let text = unsafe { value.to_string() }.map_err(native);
    // SAFETY: the shell allocated this pointer; no references remain after copying.
    unsafe {
        ::windows::Win32::System::Com::CoTaskMemFree(Some(value.0.cast()));
    }
    text
}
/// Real Windows discovery API. All methods are called on dedicated native workers.
/// Owns no COM interfaces across threads; apartments/resources are scoped per call.
pub struct RealWinApi {
    property: Vec<u16>,
    generation: AtomicU64,
    cache: PathBuf,
    watcher: std::sync::Mutex<Option<shell::WatchSet>>,
    enumeration: std::sync::Mutex<()>,
}
impl RealWinApi {
    /// Allocate adapter metadata; does not enumerate or launch any application.
    /// Startup token validation is performed by WindowsDriver before indexing.
    pub fn new() -> Result<Self> {
        // SAFETY: GetCurrentProcessId takes no pointers and cannot block.
        let pid = unsafe { GetCurrentProcessId() };
        let created = security::process_created(pid)?;
        let root = std::env::var_os("LOCALAPPDATA").ok_or(DriverError::PermissionMissing {
            permission: "LOCALAPPDATA".into(),
        })?;
        Ok(Self {
            property: wide(&format!("GrokComputerUse.WindowGeneration.{pid}.{created}")),
            generation: AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(native)?
                    .as_micros() as u64,
            ),
            cache: PathBuf::from(root)
                .join("grok-computer-use")
                .join("apps.json"),
            watcher: std::sync::Mutex::new(None),
            enumeration: std::sync::Mutex::new(()),
        })
    }
    // Stable across child restarts, private to one parent process instance.
    pub(crate) fn for_worker(namespace: &str) -> Result<Self> {
        if namespace.is_empty()
            || namespace.len() > 128
            || !namespace.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        {
            return Err(DriverError::InvalidRequest);
        }
        let mut api = Self::new()?;
        api.property = wide(&format!("GrokComputerUse.WindowGeneration.{namespace}"));
        Ok(api)
    }
    fn generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::Relaxed)
    }
}
impl WinApi for RealWinApi {
    fn enter_thread(&self) -> Result<Box<dyn std::any::Any>> {
        Ok(Box::new(Com::new()?))
    }
    fn engine_security(&self) -> Result<ProcessSecurity> {
        // SAFETY: no arguments or handles, returns the current process ID.
        security::security(unsafe { GetCurrentProcessId() })
    }
    fn process_security(&self, pid: u32) -> Result<ProcessSecurity> {
        security::security(pid)
    }
    fn enum_windows(&self) -> Result<Vec<NativeWindow>> {
        let timing = crate::timing::Timing::start("enum_windows_lock");
        // Only HWND metadata is serialized, never hashing or signature IO. This
        // keeps concurrent discovery threads from racing GetProp/SetProp stamps.
        let _enumeration = self
            .enumeration
            .lock()
            .map_err(|_| native("enumeration lock"))?;
        drop(timing);
        let _timing = crate::timing::Timing::start("enum_windows");
        windows::enumerate(self)
    }
    fn process_path(&self, pid: u32) -> Result<String> {
        security::process_path(pid)
    }
    fn process_aumid(&self, pid: u32) -> Result<Option<String>> {
        security::process_aumid(pid)
    }
    fn image(&self, path: &str) -> Result<ImageInfo> {
        signature::image(path)
    }
    fn on_current_desktop(&self, hwnd: u64) -> Result<Option<bool>> {
        windows::desktop(hwnd)
    }
    fn read_link(&self, path: &str) -> Result<Shortcut> {
        shell::read_link(path)
    }
    fn start_menu_links(&self) -> Result<Vec<String>> {
        shell::links()
    }
    fn read_cache(&self) -> Result<Option<Vec<u8>>> {
        use std::io::Read;
        let file = match std::fs::File::open(&self.cache) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io(e)),
        };
        let mut bytes = Vec::new();
        file.take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        Ok(Some(bytes))
    }
    fn write_cache(&self, bytes: &[u8]) -> Result<()> {
        use std::io::Write;
        let parent = self.cache.parent().ok_or(DriverError::InvalidRequest)?;
        std::fs::create_dir_all(parent).map_err(io)?;
        let temp = self
            .cache
            .with_extension(format!("json.{}.tmp", self.generation()));
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(io)?;
        file.write_all(bytes).map_err(io)?;
        file.sync_all().map_err(io)?;
        drop(file);
        let from = wide(&temp.to_string_lossy());
        let to = wide(&self.cache.to_string_lossy());
        // SAFETY: both paths are owned null-terminated buffers; atomic replacement.
        let result = unsafe {
            ::windows::Win32::Storage::FileSystem::MoveFileExW(
                PCWSTR(from.as_ptr()),
                PCWSTR(to.as_ptr()),
                ::windows::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
                    | ::windows::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
            )
            .map_err(native)
        };
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    }
    fn links_changed(&self, timeout: Duration) -> Result<bool> {
        shell::changed(&self.watcher, timeout)
    }
    fn launch(
        &self,
        shortcut: &Shortcut,
        expected: &crate::identity::WindowsIdentity,
    ) -> Result<()> {
        shell::launch(self, shortcut, expected)
    }
    fn restore(&self, expected: &NativeWindow) -> Result<()> {
        windows::restore(self, expected)
    }
}
