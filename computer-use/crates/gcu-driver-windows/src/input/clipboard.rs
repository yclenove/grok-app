//! Private clipboard transactions. Clipboard bytes never enter observations.
use crate::uia::patterns::ActionContext;
use async_trait::async_trait;
use gcu_core::driver::{DriverError, Result};
use std::sync::Arc;

/// Live proof run after the clipboard wait and before bytes become visible.
pub type PublishProof = Arc<dyn Fn() -> Result<()> + Send + Sync>;

/// Injectable clipboard OS boundary. Every method is bounded by the caller.
/// `stage` serializes the ENTIRE transaction across sessions and processes and
/// saves all supported formats before mutation; unsupported formats refuse.
/// The receipt is captured while the OS clipboard lock is held, never adopted
/// from a subsequent read. Dropped staging must remain recoverable by `restore`.
#[async_trait]
pub trait ClipboardApi: Send + Sync + 'static {
    /// Save then publish once. Cancellation before publication has no effect.
    async fn stage(&self, text: String, context: ActionContext) -> Result<u32>;
    /// Publish only when `proof` succeeds after the wait and before the bytes are visible.
    ///
    /// The default refuses and does not publish. Unscoped [`ClipboardApi::stage`]
    /// is unchanged.
    async fn stage_proved(
        &self,
        _text: String,
        _context: ActionContext,
        _proof: PublishProof,
    ) -> Result<u32> {
        Err(DriverError::ScopeDenied)
    }
    /// Under one OS clipboard lock compare BOTH exact receipt and text.
    async fn matches(&self, sequence: u32, text: String) -> Result<bool>;
    /// Restore only while sequence AND content still belong to this transaction.
    /// Ignore cancelled action tokens. External copies are always preserved.
    async fn restore(&self) -> Result<()>;
    /// Failed recovery remains pending and must prevent another transaction.
    fn pending(&self) -> bool;
}

#[cfg(windows)]
pub use super::ffi::clipboard::NativeClipboard;
