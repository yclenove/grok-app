use super::data::{OwnedFormat, MAX_BYTES};
use std::marker::PhantomData;
use std::rc::Rc;
use windows::core::w;
use windows::Win32::Foundation::{GetLastError, SetLastError, ERROR_SUCCESS, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, OpenClipboard,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE};

pub(super) struct ClipboardWindow {
    hwnd: HWND,
    _thread: PhantomData<Rc<()>>,
}

impl ClipboardWindow {
    pub fn new() -> Result<Self, String> {
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("Grok CU clipboard owner"),
                Default::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .map_err(|e| format!("clipboard owner window: {e}"))?;
        Ok(Self {
            hwnd,
            _thread: PhantomData,
        })
    }
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for ClipboardWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

pub(super) struct ClipboardLock {
    _thread: PhantomData<Rc<()>>,
}

impl ClipboardLock {
    pub fn open(window: &ClipboardWindow) -> Result<Self, String> {
        unsafe { OpenClipboard(Some(window.hwnd())) }
            .map_err(|e| format!("clipboard is unavailable: {e}"))?;
        Ok(Self {
            _thread: PhantomData,
        })
    }
    pub fn empty(&self) -> Result<(), String> {
        unsafe { EmptyClipboard() }.map_err(|e| format!("clipboard clear failed: {e}"))
    }
}

impl Drop for ClipboardLock {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

pub(super) struct Snapshot {
    entries: Vec<OwnedFormat>,
    restore_started: bool,
}

impl Snapshot {
    pub fn capture(_locked: &ClipboardLock) -> Result<Self, String> {
        let mut entries = Vec::new();
        let mut budget = MAX_BYTES;
        let mut current = 0;
        loop {
            unsafe {
                SetLastError(ERROR_SUCCESS);
            }
            let next = unsafe { EnumClipboardFormats(current) };
            if next == 0 {
                if unsafe { GetLastError() } != ERROR_SUCCESS {
                    return Err("clipboard formats could not be enumerated".into());
                }
                break;
            }
            if entries.len() >= 256 || entries.iter().any(|f: &OwnedFormat| f.format == next) {
                return Err("clipboard formats exceed the snapshot budget".into());
            }
            let source = unsafe { GetClipboardData(next) }
                .map_err(|_| format!("clipboard format {next} could not be materialized"))?;
            entries.push(OwnedFormat::duplicate(next, source, &mut budget)?);
            current = next;
        }
        Ok(Self {
            entries,
            restore_started: false,
        })
    }
    pub fn restore(&mut self, locked: &ClipboardLock) -> Result<(), String> {
        if !self.restore_started {
            locked.empty()?;
            self.restore_started = true;
        }
        // Keep preference order. An empty original remains genuinely empty.
        for entry in &mut self.entries {
            entry.publish(locked)?;
        }
        Ok(())
    }
}
