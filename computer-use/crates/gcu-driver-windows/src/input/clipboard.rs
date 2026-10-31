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

/// Recheck immediately before each publication.
///
/// A failed recheck does not publish that format or any later one. When no
/// format has been set, the original error is returned. When an earlier format
/// was set, or a later publish fails after one succeeded, the error reports
/// mutation uncertainty. The caller rolls the clipboard back, or leaves it
/// empty when rollback fails. Unscoped staging does not call this function.
#[cfg(any(windows, test))]
pub(crate) fn publish_formats(
    count: usize,
    mut recheck: impl FnMut() -> Result<()>,
    mut publish: impl FnMut(usize) -> Result<()>,
) -> Result<()> {
    let mut published = 0usize;
    for index in 0..count {
        if let Err(error) = recheck() {
            return Err(publication_error(published, error));
        }
        if let Err(error) = publish(index) {
            return Err(publication_error(published, error));
        }
        published = published.saturating_add(1);
    }
    Ok(())
}

/// Preserve a refusal that happened before any format was set. A later failure
/// can no longer prove the payload stayed unpublished.
#[cfg(any(windows, test))]
fn publication_error(published: usize, error: DriverError) -> DriverError {
    if published == 0 {
        error
    } else {
        match error {
            DriverError::Cancelled { .. } => DriverError::Cancelled {
                maybe_executed: true,
            },
            DriverError::Timeout { .. } => DriverError::Timeout {
                maybe_executed: true,
            },
            DriverError::Crashed { .. } => DriverError::Crashed {
                maybe_executed: true,
            },
            DriverError::NotSettable { .. } => DriverError::NotSettable {
                maybe_executed: true,
            },
            DriverError::Native { message, .. } => DriverError::Native {
                message,
                maybe_executed: true,
            },
            _ => DriverError::Native {
                message: "clipboard publication stopped after a format was set".into(),
                maybe_executed: true,
            },
        }
    }
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
    use super::{authorize_publish, bind_admitted_proof, publish_formats, PublishProof};
    use crate::input::api::AdmittedTarget;
    use crate::uia::patterns::ActionContext;
    use gcu_core::driver::{Cancel, DriverError, Result};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

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

    fn context() -> ActionContext {
        ActionContext::until(
            &Cancel::default(),
            std::time::Instant::now() + std::time::Duration::from_secs(5),
        )
    }

    fn count_publishes(recheck: impl FnMut() -> Result<()>) -> (usize, Result<()>) {
        let published = AtomicUsize::new(0);
        let result = publish_formats(4, recheck, |_| {
            published.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        (published.load(Ordering::SeqCst), result)
    }

    #[test]
    fn publish_formats_does_not_publish_when_cancelled_after_empty() {
        let cancel = Cancel::default();
        let context = ActionContext::until(
            &cancel,
            std::time::Instant::now() + std::time::Duration::from_secs(5),
        );
        let proof: PublishProof = Arc::new(|| Ok(()));
        authorize_publish(&context, &proof).unwrap();
        cancel.cancel();
        let (published, result) = count_publishes(|| authorize_publish(&context, &proof));
        let error = result.unwrap_err();
        assert!(matches!(
            error,
            DriverError::Cancelled {
                maybe_executed: false
            }
        ));
        assert_eq!(published, 0);
    }

    #[test]
    fn publish_formats_does_not_publish_when_the_post_empty_proof_closes() {
        let context = context();
        let closing = context.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        let proof: PublishProof = Arc::new(move || {
            if counted.fetch_add(1, Ordering::SeqCst) >= 1 {
                closing.close();
            }
            Ok(())
        });
        authorize_publish(&context, &proof).unwrap();
        let (published, result) = count_publishes(|| authorize_publish(&context, &proof));
        let error = result.unwrap_err();
        assert!(matches!(
            error,
            DriverError::Timeout {
                maybe_executed: false
            }
        ));
        assert_eq!(published, 0);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn publish_formats_does_not_publish_when_the_bound_target_changes() {
        let live = Arc::new(Mutex::new(target(8)));
        let current = Arc::clone(&live);
        let proof = bind_admitted_proof(
            target(8),
            Arc::new(move |admitted| {
                let live = current.lock().unwrap_or_else(|poison| poison.into_inner());
                if live.hwnd != admitted.hwnd
                    || live.incarnation != admitted.incarnation
                    || live.pid != admitted.pid
                    || live.created != admitted.created
                    || live.boot != admitted.boot
                {
                    return Err(DriverError::ScopeDenied);
                }
                Ok(())
            }),
        );
        let context = context();
        authorize_publish(&context, &proof).unwrap();
        *live.lock().unwrap_or_else(|poison| poison.into_inner()) = target(99);
        let (published, result) = count_publishes(|| authorize_publish(&context, &proof));
        let error = result.unwrap_err();
        assert_eq!(error, DriverError::ScopeDenied);
        assert!(!error.maybe_executed());
        assert_eq!(published, 0);
    }

    #[test]
    fn publish_formats_stops_after_one_format_when_the_next_recheck_fails() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        let proof: PublishProof = Arc::new(move || {
            if counted.fetch_add(1, Ordering::SeqCst) >= 2 {
                Err(DriverError::ScopeDenied)
            } else {
                Ok(())
            }
        });
        let context = context();
        authorize_publish(&context, &proof).unwrap();
        let (published, result) = count_publishes(|| authorize_publish(&context, &proof));
        let error = result.unwrap_err();
        assert!(matches!(
            error,
            DriverError::Native {
                maybe_executed: true,
                ..
            }
        ));
        assert_eq!(published, 1);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn publish_formats_publishes_every_format_when_rechecks_pass() {
        let context = context();
        let proof: PublishProof = Arc::new(|| Ok(()));
        authorize_publish(&context, &proof).unwrap();
        let (published, result) = count_publishes(|| authorize_publish(&context, &proof));
        result.unwrap();
        assert_eq!(published, 4);
    }

    #[test]
    fn publish_formats_keeps_the_original_error_when_the_first_publish_fails() {
        let error = publish_formats(2, || Ok(()), |_| {
            Err(DriverError::Native {
                message: "Windows clipboard operation failed".into(),
                maybe_executed: false,
            })
        })
        .unwrap_err();
        assert_eq!(
            error,
            DriverError::Native {
                message: "Windows clipboard operation failed".into(),
                maybe_executed: false,
            }
        );
    }

    #[test]
    fn publish_formats_reports_uncertainty_when_a_later_publish_fails() {
        let published = AtomicUsize::new(0);
        let error = publish_formats(
            4,
            || Ok(()),
            |index| {
                if index == 0 {
                    published.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                } else {
                    Err(DriverError::Native {
                        message: "Windows clipboard operation failed".into(),
                        maybe_executed: false,
                    })
                }
            },
        )
        .unwrap_err();
        assert!(matches!(
            error,
            DriverError::Native {
                maybe_executed: true,
                ..
            }
        ));
        assert_eq!(published.load(Ordering::SeqCst), 1);
    }
}
