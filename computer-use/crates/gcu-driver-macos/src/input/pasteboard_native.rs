//! NSPasteboard boundary. All methods run on the clipboard blocking worker.

use super::clipboard::{Pasteboard, PasteboardContent, PasteboardSnapshot};
use gcu_core::driver::{DriverError, Result};
use objc2::{
    rc::{autoreleasepool, Retained},
    runtime::ProtocolObject,
};
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSData, NSString};

/// AppKit pasteboard adapter. Init NSApplication on the GUI main thread first.
#[derive(Default)]
pub struct NativePasteboard {
    name: Option<String>,
}
impl NativePasteboard {
    /// The system general pasteboard, used only for explicitly permitted input.
    pub fn general() -> Self {
        Self::default()
    }
    /// An isolated named pasteboard for native smoke tests.
    pub fn named(name: &str) -> Self {
        Self {
            name: Some(name.into()),
        }
    }
    fn board(&self) -> Result<Retained<NSPasteboard>> {
        if !super::ensure_appkit() {
            return Err(failure("AppKit is not initialized"));
        }
        Ok(match &self.name {
            Some(name) => NSPasteboard::pasteboardWithName(&NSString::from_str(name)),
            None => NSPasteboard::generalPasteboard(),
        })
    }
}
impl Pasteboard for NativePasteboard {
    fn read(&self) -> Result<PasteboardSnapshot> {
        autoreleasepool(|_| {
            let board = self.board()?;
            snapshot(&board)
        })
    }
    fn replace_if(
        &self,
        expected: &PasteboardSnapshot,
        content: &PasteboardContent,
    ) -> Result<Option<PasteboardSnapshot>> {
        autoreleasepool(|_| {
            let board = self.board()?;
            // Materialize all data before touching the global board.
            let prepared = objects(content)?;
            let original = objects(&expected.content)?;
            if snapshot(&board)? != *expected {
                return Ok(None);
            }
            let cleared = board.clearContents() as i64;
            if cleared != expected.change_count.saturating_add(1) {
                return Ok(None);
            }
            let accepted = content.0.is_empty() || board.writeObjects(&prepared);
            let after = snapshot(&board)?;
            // clearContents returns our ownership generation. writeObjects adds
            // data to it; never adopt a later foreign generation, even if its
            // bytes happen to match. NSPasteboard.changeCount ownership contract:
            // https://developer.apple.com/documentation/appkit/nspasteboard/changecount
            if after.change_count != cleared {
                return Ok(None);
            }
            if accepted
                && after.content == *content
                && board.changeCount() as i64 == after.change_count
            {
                return Ok(Some(after));
            }
            // A failed write may leave the board empty. Restore only that exact
            // owned empty generation, never content supplied by another owner.
            if after.change_count == cleared
                && after.content.0.is_empty()
                && !expected.content.0.is_empty()
                && board.changeCount() as i64 == cleared
                && !board.writeObjects(&original)
            {
                return Err(failure("pasteboard recovery failed"));
            }
            Err(failure("pasteboard write was not confirmed"))
        })
    }
}

fn snapshot(board: &NSPasteboard) -> Result<PasteboardSnapshot> {
    let change_count = board.changeCount() as i64;
    let mut items = Vec::new();
    let mut bytes = 0usize;
    if let Some(native) = board.pasteboardItems() {
        for index in 0..native.count() {
            let item = native.objectAtIndex(index);
            let mut formats = Vec::new();
            let types = item.types();
            for index in 0..types.count() {
                let kind = types.objectAtIndex(index);
                let data = item
                    .dataForType(&kind)
                    .ok_or_else(|| failure("pasteboard format cannot be saved"))?;
                bytes = bytes.saturating_add(data.len());
                if bytes > 16 * 1024 * 1024 {
                    return Err(failure("pasteboard snapshot exceeds limit"));
                }
                formats.push((kind.to_string(), data.to_vec()));
            }
            if formats.is_empty() {
                return Err(failure("pasteboard item cannot be saved"));
            }
            items.push(formats);
        }
    } else if board.types().is_some_and(|types| types.count() > 0) {
        return Err(failure("pasteboard items cannot be completely saved"));
    }
    if board.changeCount() as i64 != change_count {
        return Err(failure("pasteboard changed during snapshot"));
    }
    Ok(PasteboardSnapshot {
        change_count,
        content: PasteboardContent(items),
    })
}

fn objects(
    content: &PasteboardContent,
) -> Result<Retained<NSArray<ProtocolObject<dyn NSPasteboardWriting>>>> {
    let mut items = Vec::new();
    for formats in &content.0 {
        let item = NSPasteboardItem::new();
        for (kind, bytes) in formats {
            if !item.setData_forType(&NSData::with_bytes(bytes), &NSString::from_str(kind)) {
                return Err(failure("pasteboard item could not be prepared"));
            }
        }
        items.push(ProtocolObject::from_retained(item));
    }
    Ok(NSArray::from_retained_slice(&items))
}
fn failure(message: &str) -> DriverError {
    DriverError::Native {
        message: message.into(),
        maybe_executed: false,
    }
}
