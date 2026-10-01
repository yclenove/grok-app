//! Host-only clipboard transaction. Bytes never enter observations or logs.
#![cfg(target_os = "windows")]

mod data;
mod native;
#[cfg(feature = "computer-use-probe")]
pub(super) mod probe;
#[cfg(feature = "computer-use-probe")]
mod publication_probe;
mod text_formats;

use native::{ClipboardLock, ClipboardWindow, Snapshot};
use std::time::Duration;
use windows::Win32::System::DataExchange::{GetClipboardOwner, GetClipboardSequenceNumber};

fn publish_task(task: &mut [data::OwnedFormat], locked: &ClipboardLock) -> Result<(), String> {
    for entry in task {
        entry.publish(locked)?;
    }
    Ok(())
}

pub fn sequence() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

#[cfg(feature = "computer-use-probe")]
pub fn get_text() -> Option<String> {
    clipboard_win::get_clipboard(clipboard_win::formats::Unicode).ok()
}

/// Preserve all materializable formats before mutation. Unsupported ownership
/// and allocation failures reject before modification. Never replay an action.
pub fn with_task_text(
    text: &str,
    before_write: impl FnOnce() -> Result<(), String>,
    op: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let mut transaction = Transaction::begin(text, before_write)?;
    if !transaction.owns_receipt() {
        return Err("clipboard changed before paste; action was not dispatched".into());
    }
    let result = op();
    let restored = transaction.restore();
    match (result, restored) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(format!("clipboard cleanup failed: {error}")),
        (Err(primary), Err(cleanup)) => Err(format!(
            "{primary}; clipboard cleanup also failed: {cleanup}"
        )),
    }
}

struct Transaction {
    window: ClipboardWindow,
    saved: Option<Snapshot>,
    owned_sequence: u32,
}

impl Transaction {
    fn begin(
        text: &str,
        before_write: impl FnOnce() -> Result<(), String>,
    ) -> Result<Self, String> {
        let window = ClipboardWindow::new()?;
        let mut task = text_formats::prepare(text)?;
        let locked = ClipboardLock::open(&window)?;
        let saved = Snapshot::capture(&locked)?;
        before_write()?;
        locked.empty()?;
        let mut transaction = Self {
            window,
            saved: Some(saved),
            owned_sequence: sequence(),
        };
        let published = publish_task(&mut task, &locked);
        transaction.owned_sequence = sequence();
        // Undo a failed initial publication while this lock still prevents any
        // external copy. The action has not been called.
        if let Err(error) = published {
            transaction.restore_locked(&locked);
            drop(locked);
            return Err(error);
        }
        drop(locked);
        // The receipt was captured under the lock. Never adopt a later sequence
        // just because the clipboard text or the owner HWND still looks familiar.
        Ok(transaction)
    }

    fn owns_window(&self) -> bool {
        unsafe { GetClipboardOwner() }.ok() == Some(self.window.hwnd())
    }

    fn owns_receipt(&self) -> bool {
        self.owns_window() && sequence() == self.owned_sequence
    }

    fn restore_locked(&mut self, locked: &ClipboardLock) {
        // A partially restored text set lets CloseClipboard synthesize formats
        // and bump the sequence. Releasing here would make our own partial
        // recovery look like an external copy and discard the remaining backup.
        // Retain this exact OS lock through recovery. Already transferred
        // handles are skipped; no second EmptyClipboard or action dispatch.
        while let Some(saved) = self.saved.as_mut() {
            if saved.restore(locked).is_ok() {
                self.saved.take();
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn restore(&mut self) -> Result<(), String> {
        if self.saved.is_none() {
            return Ok(());
        }
        // A temporary reader must not discard the backup or claim native idle.
        // The action guard remains held through this cleanup; Stop still fences
        // all new input. This retries only OS lock acquisition, never the paste.
        loop {
            if !self.owns_receipt() {
                self.saved.take();
                return Ok(());
            }
            if let Ok(locked) = ClipboardLock::open(&self.window) {
                if !self.owns_receipt() {
                    self.saved.take();
                    return Ok(());
                }
                self.restore_locked(&locked);
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            tracing::error!("Computer Use clipboard cleanup: {error}");
        }
    }
}
