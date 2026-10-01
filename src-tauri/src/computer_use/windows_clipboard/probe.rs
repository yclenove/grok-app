//! Destructive clipboard fixtures run only in a new, noninteractive window
//! station. Never fall back to WinSta0 or seed the user's actual clipboard.
use super::{
    data::OwnedFormat,
    native::{ClipboardLock, ClipboardWindow},
    with_task_text,
};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::DataExchange::{
    CountClipboardFormats, GetClipboardData, RegisterClipboardFormatW,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::StationsAndDesktops::*;
use windows::Win32::System::Threading::GetCurrentThreadId;

#[path = "probe_races.rs"]
mod races;

pub(in crate::computer_use) struct IsolatedStation {
    original: HWINSTA,
    original_desktop: HDESK,
    owned: HWINSTA,
    desktop: Option<HDESK>,
}

impl IsolatedStation {
    pub(in crate::computer_use) fn new() -> Result<Self, String> {
        unsafe {
            let original = GetProcessWindowStation().map_err(|e| e.to_string())?;
            let original_desktop =
                GetThreadDesktop(GetCurrentThreadId()).map_err(|e| e.to_string())?;
            let name: Vec<u16> = format!("GrokCuClipboard-{}", uuid::Uuid::new_v4())
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let owned = CreateWindowStationW(PCWSTR(name.as_ptr()), 0, 0x000f037f, None)
                .map_err(|e| format!("isolated clipboard station: {e}"))?;
            let mut guard = Self {
                original,
                original_desktop,
                owned,
                desktop: None,
            };
            SetProcessWindowStation(owned).map_err(|e| e.to_string())?;
            let desktop = CreateDesktopW(
                w!("CuClipboard"),
                None,
                None,
                Default::default(),
                0x01ff,
                None,
            )
            .map_err(|e| e.to_string())?;
            guard.desktop = Some(desktop);
            SetThreadDesktop(desktop).map_err(|e| e.to_string())?;
            if GetProcessWindowStation().map_err(|e| e.to_string())? != owned {
                return Err("clipboard station isolation was not installed".into());
            }
            Ok(guard)
        }
    }
}

impl Drop for IsolatedStation {
    fn drop(&mut self) {
        unsafe {
            let _ = SetThreadDesktop(self.original_desktop);
            let _ = SetProcessWindowStation(self.original);
            if let Some(desktop) = self.desktop {
                let _ = CloseDesktop(desktop);
            }
            let _ = CloseWindowStation(self.owned);
        }
    }
}

pub(super) fn replace(window: &ClipboardWindow, entries: &[(u32, Vec<u8>)]) -> Result<(), String> {
    let mut prepared = entries
        .iter()
        .map(|(f, v)| OwnedFormat::bytes(*f, v))
        .collect::<Result<Vec<_>, _>>()?;
    let locked = ClipboardLock::open(window)?;
    locked.empty()?;
    for data in &mut prepared {
        data.publish(&locked)?;
    }
    Ok(())
}

pub(super) fn verify(window: &ClipboardWindow, entries: &[(u32, Vec<u8>)]) -> Result<(), String> {
    let _locked = ClipboardLock::open(window)?;
    for (format, expected) in entries {
        let handle = unsafe { GetClipboardData(*format) }
            .map_err(|_| format!("fixture format {format} is missing"))?;
        let memory = HGLOBAL(handle.0);
        let size = unsafe { GlobalSize(memory) };
        let ptr = unsafe { GlobalLock(memory) };
        if ptr.is_null() {
            return Err(format!("fixture format {format} is not readable"));
        }
        let equal = size >= expected.len()
            && unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), expected.len()) == expected };
        unsafe {
            let _ = GlobalUnlock(memory);
        }
        if !equal {
            return Err(format!("fixture format {format} changed"));
        }
    }
    Ok(())
}

pub(super) fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain(Some(0))
        .flat_map(|c| c.to_le_bytes())
        .collect()
}

pub fn run_isolated() -> Result<(), String> {
    let _isolation = IsolatedStation::new()?;
    let window = ClipboardWindow::new()?;
    let copied = ClipboardWindow::new()?;
    let html = unsafe { RegisterClipboardFormatW(w!("HTML Format")) };
    let rich = unsafe { RegisterClipboardFormatW(w!("Rich Text Format")) };
    if html == 0 || rich == 0 {
        return Err("fixture formats could not be registered".into());
    }
    let mut dib = vec![0u8; 44];
    dib[0..4].copy_from_slice(&40u32.to_le_bytes());
    dib[4..8].copy_from_slice(&1i32.to_le_bytes());
    dib[8..12].copy_from_slice(&1i32.to_le_bytes());
    dib[12..14].copy_from_slice(&1u16.to_le_bytes());
    dib[14..16].copy_from_slice(&32u16.to_le_bytes());
    dib[20..24].copy_from_slice(&4u32.to_le_bytes());
    dib[40..44].copy_from_slice(&[0x11, 0x55, 0x99, 0xff]);
    let mut files = vec![0u8; 20];
    files[0..4].copy_from_slice(&20u32.to_le_bytes());
    files[16..20].copy_from_slice(&1u32.to_le_bytes());
    files.extend(utf16("C:\\cu-fixture\\not-a-real-file.txt\0"));
    let baseline = vec![
        (13, utf16("original-原文")),
        (html, b"<b>fixture</b>\0".to_vec()),
        (rich, b"{\\rtf1 fixture}\0".to_vec()),
        (8, dib),
        (15, files),
    ];
    replace(&window, &baseline)?;
    let mut calls = 0;
    with_task_text(
        "task-任务",
        || Ok(()),
        || {
            calls += 1;
            verify(&window, &[(13, utf16("task-任务"))])
        },
    )?;
    verify(&window, &baseline)?;
    if calls != 1 {
        return Err("clipboard action was replayed".into());
    }
    println!("gate: isolated clipboard preserves text/HTML/RTF/bitmap/file-list and executes once");

    let error = with_task_text("error", || Ok(()), || Err("fixture action rejected".into()));
    if !error
        .as_ref()
        .is_err_and(|e| e == "fixture action rejected")
    {
        return Err("clipboard swallowed the action error".into());
    }
    verify(&window, &baseline)?;
    let rejected = with_task_text(
        "cancelled",
        || Err("fixture cancelled before write".into()),
        || Err("action must never run".into()),
    );
    if !rejected
        .as_ref()
        .is_err_and(|e| e == "fixture cancelled before write")
    {
        return Err("clipboard admission was ignored".into());
    }
    verify(&window, &baseline)?;
    println!("gate: isolated clipboard restores after failure and honors pre-write cancellation");

    let new_copy = vec![
        (13, utf16("same-text")),
        (html, b"<i>new user copy</i>\0".to_vec()),
    ];
    with_task_text("same-text", || Ok(()), || replace(&copied, &new_copy))?;
    verify(&copied, &new_copy)?;
    println!("gate: identical-text concurrent user copy is not overwritten");

    replace(&window, &[])?;
    with_task_text("empty before", || Ok(()), || Ok(()))?;
    if unsafe { CountClipboardFormats() } != 0 {
        return Err("empty clipboard became empty text".into());
    }
    println!("gate: genuinely empty clipboard remains empty");

    // Owner-dependent format must reject without mutation or running the paste.
    let opaque = vec![(0x0200, vec![1, 2, 3, 4])];
    replace(&window, &opaque)?;
    let rejected = with_task_text(
        "never written",
        || Ok(()),
        || Err("action must never run".into()),
    );
    if !rejected
        .as_ref()
        .is_err_and(|e| e.contains("requires its original owner"))
    {
        return Err("owner-dependent clipboard was not rejected before mutation".into());
    }
    verify(&window, &opaque)?;
    // Windows does not own private-format allocations; test-only cleanup is
    // handled below by retaining and freeing the original handle explicitly.
    let locked = ClipboardLock::open(&window)?;
    let handle = unsafe { GetClipboardData(0x0200) }.map_err(|e| e.to_string())?;
    locked.empty()?;
    unsafe {
        let _ = windows::Win32::Foundation::GlobalFree(Some(HGLOBAL(handle.0)));
    }
    drop(locked);
    println!("gate: unsupported owner-dependent data fails without loss");
    races::run(&window, &baseline)?;
    publication_recovery(&window, &baseline, html)?;
    Ok(())
}

fn publication_recovery(
    window: &ClipboardWindow,
    baseline: &[(u32, Vec<u8>)],
    html: u32,
) -> Result<(), String> {
    use super::publication_probe::Faults;
    replace(window, baseline)?;
    let fault = Faults::new(&[html]);
    let mut calls = 0;
    with_task_text(
        "restore failure",
        || Ok(()),
        || {
            calls += 1;
            Ok(())
        },
    )?;
    fault.verify_consumed()?;
    verify(window, baseline)?;
    if calls != 1 {
        return Err("clipboard recovery replayed the action".into());
    }
    drop(fault);
    println!("gate: partial format restoration retains every remaining format");

    let fault = Faults::new(&[16, html]);
    let rejected = with_task_text(
        "initial failure",
        || Ok(()),
        || {
            calls += 1;
            Ok(())
        },
    );
    fault.verify_consumed()?;
    if !rejected.is_err_and(|e| e == "injected clipboard publication failure") || calls != 1 {
        return Err("partial initial publication dispatched or swallowed its failure".into());
    }
    verify(window, baseline)?;
    println!("gate: failed initial publication and failed rollback preserve data without dispatch");
    Ok(())
}
