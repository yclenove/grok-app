//! Native bindings and owned Win32 resources; compiled only on Windows.
mod packages;
pub(crate) mod security;
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
/// Boot-environment record. `NtQuerySystemInformation` class 90.
///
/// Layout is the documented 32-byte structure: a 16-byte GUID, a firmware
/// type, padding, and boot flags. A nil GUID is not a session id.
#[repr(C)]
struct BootEnvironment {
    identifier: [u8; 16],
    _firmware_type: u32,
    _pad: u32,
    _flags: u64,
}
const _: () = assert!(std::mem::size_of::<BootEnvironment>() == 32);

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQuerySystemInformation(
        class: u32,
        buffer: *mut core::ffi::c_void,
        length: u32,
        returned: *mut u32,
    ) -> i32;
}

/// OS boot-session GUID, or `None` when the query fails or the GUID is nil.
///
/// This is not a wall-clock estimate. `scope_identity::boot_token` rejects a
/// numeric session. One stack query. No process handle and no retry.
pub(crate) fn read_boot_session() -> Option<String> {
    let mut info = BootEnvironment {
        identifier: [0; 16],
        _firmware_type: 0,
        _pad: 0,
        _flags: 0,
    };
    let mut returned = 0u32;
    // SAFETY: writable stack buffer of the asserted size. The call does not
    // retain the pointer. A negative status or a short result is unproved.
    let status = unsafe {
        NtQuerySystemInformation(
            90,
            (&mut info as *mut BootEnvironment).cast(),
            32,
            &mut returned,
        )
    };
    if status < 0 || returned < 16 || info.identifier == [0; 16] {
        return None;
    }
    let bytes = info.identifier;
    let data1 = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let data2 = u16::from_le_bytes([bytes[4], bytes[5]]);
    let data3 = u16::from_le_bytes([bytes[6], bytes[7]]);
    let [b8, b9, b10, b11, b12, b13, b14, b15] = [
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ];
    Some(format!(
        "{data1:08x}-{data2:04x}-{data3:04x}-{b8:02x}{b9:02x}-{b10:02x}{b11:02x}{b12:02x}{b13:02x}{b14:02x}{b15:02x}"
    ))
}

/// Live check for one scoped clipboard publication owned by the isolated parent.
///
/// Does not enumerate and does not call `SetPropW`. The generation property is
/// the worker namespace stamp (`GrokComputerUse.WindowGeneration.{namespace}`).
/// The HWND's process must be the admitted owner. An ApplicationFrameHost window
/// whose process is not that owner cannot be unwrapped here and is
/// [`DriverError::ScopeDenied`]. One stack query and one process-times read.
/// No retry and no clipboard mutation.
pub(crate) fn prove_parent_publication(
    namespace: &str,
    hwnd: u64,
    incarnation: u64,
    pid: u32,
    created: u64,
    boot: &str,
) -> Result<()> {
    use ::windows::{
        core::PCWSTR,
        Win32::{
            Foundation::HWND,
            UI::WindowsAndMessaging::{GetPropW, GetWindowThreadProcessId},
        },
    };
    if namespace.is_empty()
        || namespace.len() > 128
        || !namespace
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
        || hwnd == 0
        || incarnation == 0
        || pid == 0
        || created == 0
        || boot.is_empty()
    {
        return Err(DriverError::ScopeDenied);
    }
    let session = read_boot_session().ok_or(DriverError::ScopeDenied)?;
    if crate::scope_identity::boot_token(Some(&session), None)? != boot {
        return Err(DriverError::ScopeDenied);
    }
    let window = HWND(hwnd as usize as *mut _);
    let mut live_pid = 0u32;
    // SAFETY: numeric HWND and a writable pid slot. No process handle is kept.
    // A destroyed window returns a zero thread id and is not published.
    let tid = unsafe { GetWindowThreadProcessId(window, Some(&mut live_pid)) };
    if tid == 0 || live_pid != pid {
        return Err(DriverError::ScopeDenied);
    }
    let live_created = match security::process_created(pid) {
        Ok(live_created) => live_created,
        Err(_) => return Err(DriverError::ScopeDenied),
    };
    if live_created != created {
        return Err(DriverError::ScopeDenied);
    }
    let property = wide(&crate::scope_identity::generation_property_name(namespace));
    // SAFETY: the name is NUL-terminated and borrowed for this call. GetPropW
    // reads the property list. It does not send a message or set a property.
    let marker = unsafe { GetPropW(window, PCWSTR(property.as_ptr())) };
    if marker.is_invalid() || marker.0 as usize as u64 != incarnation {
        return Err(DriverError::ScopeDenied);
    }
    Ok(())
}

/// Real Windows discovery API. All methods are called on dedicated native workers.
/// Owns no COM interfaces across threads; apartments/resources are scoped per call.
pub struct RealWinApi {
    property: Vec<u16>,
    generation: AtomicU64,
    cache: PathBuf,
    watcher: std::sync::Mutex<Option<shell::WatchSet>>,
    enumeration: std::sync::Mutex<()>,
    /// Cached at process start so later reads in this process agree.
    boot: String,
}
impl RealWinApi {
    /// Allocate adapter metadata; does not enumerate or launch any application.
    /// Startup token validation is performed by WindowsDriver before indexing.
    pub fn new() -> Result<Self> {
        // SAFETY: GetCurrentProcessId takes no pointers and cannot block.
        let pid = unsafe { GetCurrentProcessId() };
        let created = security::process_created(pid)?;
        let namespace = format!("{pid}.{created}");
        let root = std::env::var_os("LOCALAPPDATA").ok_or(DriverError::PermissionMissing {
            permission: "LOCALAPPDATA".into(),
        })?;
        Ok(Self {
            property: wide(&crate::scope_identity::generation_property_name(&namespace)),
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
            // The wall-clock argument is ignored. A missing session stays empty.
            boot: crate::scope_identity::boot_token(
                read_boot_session().as_deref(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok()),
            )
            .unwrap_or_default(),
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
        api.property = wide(&crate::scope_identity::generation_property_name(namespace));
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
    fn generation_property(&self) -> &[u16] {
        &self.property
    }
    fn engine_security(&self) -> Result<ProcessSecurity> {
        // SAFETY: no arguments or handles, returns the current process ID.
        security::security(unsafe { GetCurrentProcessId() })
    }
    fn process_security(&self, pid: u32) -> Result<ProcessSecurity> {
        security::security(pid)
    }
    fn boot_identity(&self) -> Result<String> {
        if self.boot.is_empty() || self.boot.len() > 256 {
            return Err(DriverError::ScopeDenied);
        }
        Ok(self.boot.clone())
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
