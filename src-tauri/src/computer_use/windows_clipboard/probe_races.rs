use super::{replace, utf16, verify};
use crate::computer_use::windows_clipboard::{
    data::OwnedFormat,
    native::{ClipboardLock, ClipboardWindow},
    sequence, with_task_text,
};
use std::sync::mpsc;
use std::time::Duration;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Gdi::{
    CloseEnhMetaFile, CreateEnhMetaFileW, GetEnhMetaFileBits, Rectangle, HENHMETAFILE,
};
use windows::Win32::System::DataExchange::{
    GetClipboardData, RegisterClipboardFormatW, SetClipboardData,
};

pub(super) fn run(window: &ClipboardWindow, baseline: &[(u32, Vec<u8>)]) -> Result<(), String> {
    // Establish whether the exact receipt can be captured before CloseClipboard.
    let locked = ClipboardLock::open(window)?;
    locked.empty()?;
    crate::computer_use::windows_clipboard::publish_task(
        &mut crate::computer_use::windows_clipboard::text_formats::prepare("receipt")?,
        &locked,
    )?;
    let before_close = sequence();
    drop(locked);
    if sequence() != before_close {
        return Err("CloseClipboard changed the sealed task receipt".into());
    }
    println!("gate: exact text receipt is sealed before releasing the OS clipboard lock");

    replace(window, baseline)?;
    // Same-owner metadata append is a user mutation even without EmptyClipboard.
    let appended =
        unsafe { RegisterClipboardFormatW(windows::core::w!("Grok CU fixture appended format")) };
    if appended == 0 {
        return Err("fixture append format registration failed".into());
    }
    with_task_text(
        "task",
        || Ok(()),
        || {
            let other = ClipboardWindow::new()?;
            let locked = ClipboardLock::open(&other)?;
            OwnedFormat::bytes(appended, b"copy metadata")?.publish(&locked)
        },
    )?;
    verify(
        window,
        &[(13, utf16("task")), (appended, b"copy metadata".to_vec())],
    )?;
    println!("gate: same-owner clipboard mutation prevents stale restoration");

    replace(window, baseline)?;
    let unwound = std::panic::catch_unwind(|| {
        let _ = with_task_text(
            "unwind",
            || Ok(()),
            || panic!("expected isolated clipboard fixture unwind"),
        );
    });
    if unwound.is_ok() {
        return Err("clipboard unwind fixture did not unwind".into());
    }
    verify(window, baseline)?;
    println!("gate: clipboard backup survives action unwinding");

    // Create a genuine enhanced-metafile GDI handle, not an HGLOBAL lookalike.
    let dc = unsafe { CreateEnhMetaFileW(None, None, None, None) };
    if dc.is_invalid() {
        return Err("fixture enhanced metafile creation failed".into());
    }
    unsafe {
        let _ = Rectangle(dc, 0, 0, 8, 8);
    }
    let emf = unsafe { CloseEnhMetaFile(dc) };
    if emf.is_invalid() {
        return Err("fixture enhanced metafile completion failed".into());
    }
    let size = unsafe { GetEnhMetaFileBits(emf, None) } as usize;
    let mut expected = vec![0u8; size];
    unsafe {
        GetEnhMetaFileBits(emf, Some(&mut expected));
    }
    let locked = ClipboardLock::open(window)?;
    locked.empty()?;
    unsafe { SetClipboardData(14, Some(HANDLE(emf.0))) }.map_err(|e| e.to_string())?;
    drop(locked);
    with_task_text("metafile", || Ok(()), || Ok(()))?;
    let locked = ClipboardLock::open(window)?;
    let restored = unsafe { GetClipboardData(14) }.map_err(|e| e.to_string())?;
    let mut actual = vec![0u8; size];
    let restored_size =
        unsafe { GetEnhMetaFileBits(HENHMETAFILE(restored.0), Some(&mut actual)) } as usize;
    if restored_size != size || actual != expected {
        return Err("enhanced metafile bytes changed".into());
    }
    drop(locked);
    println!("gate: enhanced-metafile handle and contents restored");

    replace(window, baseline)?;
    busy_reader()?;
    verify(window, baseline)?;
    println!("gate: cleanup waits for native clipboard reader, without replay or early return");
    Ok(())
}

fn busy_reader() -> Result<(), String> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = with_task_text(
            "reader",
            || Ok(()),
            || {
                ready_tx.send(()).map_err(|e| e.to_string())?;
                go_rx
                    .recv_timeout(Duration::from_secs(2))
                    .map_err(|e| e.to_string())
            },
        );
        let _ = done_tx.send(result);
    });
    recv_pumping(&ready_rx, Duration::from_secs(2))?;
    let reader = ClipboardWindow::new()?;
    let locked = ClipboardLock::open(&reader)?;
    go_tx.send(()).map_err(|e| e.to_string())?;
    let pending = matches!(
        done_rx.recv_timeout(Duration::from_millis(80)),
        Err(mpsc::RecvTimeoutError::Timeout)
    );
    drop(locked); // Always release before evaluating the assertion / joining.
    let result = recv_pumping(&done_rx, Duration::from_secs(2))?;
    worker
        .join()
        .map_err(|_| "clipboard reader fixture worker panicked")?;
    if !pending {
        return Err("clipboard transaction returned before native cleanup".into());
    }
    result?;
    Ok(())
}

fn recv_pumping<T>(receiver: &mpsc::Receiver<T>, timeout: Duration) -> Result<T, String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };
    let start = std::time::Instant::now();
    loop {
        match receiver.try_recv() {
            Ok(value) => return Ok(value),
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("clipboard fixture channel closed".into())
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if start.elapsed() >= timeout {
            return Err("clipboard fixture phase timed out".into());
        }
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
