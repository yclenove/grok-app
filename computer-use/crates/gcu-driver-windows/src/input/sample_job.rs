//! Scoped sample contract. One text read, proved immediately before the read
//! and again before the hash leaves the job. Unscoped sampling does not use this.
use gcu_core::driver::Result;
use std::hash::{Hash, Hasher};

/// Process proofs in one scoped sample: before the text read, then before return.
#[cfg(test)]
pub(crate) const SCOPED_PROOFS: usize = 2;

/// Read window text once. Drop the hash when `prove` fails. No later read runs here.
pub(crate) fn scoped(
    mut prove: impl FnMut() -> Result<()>,
    mut read_text: impl FnMut() -> Result<Vec<u16>>,
) -> Result<u64> {
    prove()?;
    let text = read_text()?;
    prove()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    Ok(hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gcu_core::driver::DriverError;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn exit_proof_failure_drops_the_text_and_does_not_read_again() {
        let reads = AtomicUsize::new(0);
        let proofs = AtomicUsize::new(0);
        let error = scoped(
            || {
                let n = proofs.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    Ok(())
                } else {
                    Err(DriverError::ScopeDenied)
                }
            },
            || {
                reads.fetch_add(1, Ordering::SeqCst);
                Ok(vec![u16::from(b'C')])
            },
        )
        .unwrap_err();
        assert_eq!(error, DriverError::ScopeDenied);
        assert!(!error.maybe_executed());
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(proofs.load(Ordering::SeqCst), SCOPED_PROOFS);
    }

    #[test]
    fn a_failed_text_read_returns_no_hash() {
        let proofs = AtomicUsize::new(0);
        let error = scoped(
            || {
                proofs.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
            || {
                Err(DriverError::Timeout {
                    maybe_executed: false,
                })
            },
        )
        .unwrap_err();
        assert_eq!(
            error,
            DriverError::Timeout {
                maybe_executed: false
            }
        );
        assert_eq!(proofs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_passing_sample_returns_only_after_the_exit_proof() {
        let proofs = AtomicUsize::new(0);
        let hash = scoped(
            || {
                proofs.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
            || Ok(vec![1, 2]),
        )
        .unwrap();
        assert_ne!(hash, 0);
        assert_eq!(proofs.load(Ordering::SeqCst), SCOPED_PROOFS);
    }
}
