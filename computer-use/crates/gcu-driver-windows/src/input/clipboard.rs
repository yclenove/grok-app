//! Private clipboard transactions. Clipboard bytes never enter observations.
use crate::uia::patterns::ActionContext;
use async_trait::async_trait;
use gcu_core::driver::{DriverError, Result};
use std::sync::Arc;

/// Live proof run after the clipboard wait and before bytes become visible.
pub type PublishProof = Arc<dyn Fn() -> Result<()> + Send + Sync>;

/// Check of one admitted target. [`bind_admitted_proof`] copies that target in.
#[cfg(any(windows, test))]
pub(crate) type AdmittedProver =
    Arc<dyn Fn(&crate::input::api::AdmittedTarget) -> Result<()> + Send + Sync>;

/// Run `proof`, then require the operation to still be current.
///
/// Call this after the clipboard wait and backup, and before `EmptyClipboard`.
/// A close, Stop, or deadline during `proof` refuses and does not publish.
/// The caller must not mark mutation dispatch before this returns.
/// Memory only besides `proof`. No input-ledger lock.
#[cfg(any(windows, test))]
pub(crate) fn authorize_publish(context: &ActionContext, proof: &PublishProof) -> Result<()> {
    context.check()?;
    proof()?;
    context.check()
}

/// Proof closed over one admitted target copied at stage time.
///
/// Later writes to a shared claim slot are not visible to `prove`. Memory only
/// besides `prove`.
#[cfg(any(windows, test))]
pub(crate) fn bind_admitted_proof(
    admitted: crate::input::api::AdmittedTarget,
    prove: AdmittedProver,
) -> PublishProof {
    Arc::new(move || prove(&admitted))
}

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

#[cfg(test)]
mod tests {
    use super::{authorize_publish, bind_admitted_proof, PublishProof};
    use crate::input::api::AdmittedTarget;
    use crate::uia::patterns::ActionContext;
    use gcu_core::driver::{Cancel, DriverError};
    use std::sync::{Arc, Mutex};

    fn target(hwnd: u64) -> AdmittedTarget {
        AdmittedTarget {
            hwnd,
            incarnation: 3,
            pid: 7,
            created: 42,
            boot: "boot-1".into(),
        }
    }

    #[test]
    fn bind_admitted_proof_ignores_a_later_shared_slot() {
        let slot = Arc::new(Mutex::new(target(99)));
        let seen = Arc::new(Mutex::new(0u64));
        let seen_proof = Arc::clone(&seen);
        let proof = bind_admitted_proof(
            target(8),
            Arc::new(move |admitted| {
                *seen_proof
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner()) = admitted.hwnd;
                Ok(())
            }),
        );
        *slot.lock().unwrap_or_else(|poison| poison.into_inner()) = target(77);
        proof().unwrap();
        assert_eq!(*seen.lock().unwrap_or_else(|poison| poison.into_inner()), 8);
        assert_eq!(
            slot.lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .hwnd,
            77
        );
    }

    #[test]
    fn authorize_publish_refuses_when_the_context_closes_during_the_proof() {
        let context = ActionContext::until(
            &Cancel::default(),
            std::time::Instant::now() + std::time::Duration::from_secs(5),
        );
        let closing = context.clone();
        let proof: PublishProof = Arc::new(move || {
            closing.close();
            Ok(())
        });
        let error = authorize_publish(&context, &proof).unwrap_err();
        assert!(matches!(
            error,
            DriverError::Timeout {
                maybe_executed: false
            }
        ));
        assert!(!context.maybe_executed());
    }
}
